//! Custom PNG/DEFLATE encoder for sparse RGBA images from intersection_vis.
//!
//! Exploits two structural properties:
//!  1. Each source cell (sx,sy) maps to an 8×8 output block. Sub-rows 1–7 are identical
//!     to sub-row 0, so they are emitted as DEFLATE back-references to the previous row.
//!  2. Transparent source cells produce all-zero pixel bytes and are run-length encoded.
//!
//! Uses PNG filter 0 (None) on every row. Fixed Huffman DEFLATE throughout.
//!
//! Adler-32 is updated in O(1) per row using the closed-form batch formula, avoiding
//! per-byte scanning of the 64 MB pixel buffer.

// ── geometry (must match intersection_vis.rs) ────────────────────────────────
pub const TILE_SIDE: usize = 500;
pub const UPSCALE: usize = 8;
pub const OUT_SIDE: usize = TILE_SIDE * UPSCALE; // 4000

const BPP: usize = 4;
const ROW_BYTES: usize = OUT_SIDE * BPP; // 16000
const STRIDE: usize = ROW_BYTES + 1; // 16001 (filter byte + row pixels)

// ── CRC-32 (PNG chunk integrity) ─────────────────────────────────────────────

fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for i in 0u32..256 {
            let mut c = i;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            t[i as usize] = c;
        }
        t
    });
    let mut c = !0u32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

// ── Adler-32 (zlib stream integrity) ─────────────────────────────────────────
//
// Batch update formula: for a block b[0..N] with known
//   sum_b  = Σ b[i]           (mod 65521)
//   wsum_b = Σ (N−i)·b[i]     (mod 65521)   (i = 0..N)
// the state transforms as:
//   s1' = (s1 + sum_b)        % 65521
//   s2' = (s2 + N·s1 + wsum_b) % 65521
//
// For an all-zero block: sum_b = wsum_b = 0, so s1 is unchanged and
//   s2' = (s2 + N·s1) % 65521 — O(1) with no byte scanning.

const ADLER_MOD: u64 = 65521;

struct Adler32 {
    s1: u64,
    s2: u64,
}

impl Adler32 {
    fn new() -> Self {
        Self { s1: 1, s2: 0 }
    }

    /// Update for a block of `n` zero bytes in O(1).
    #[inline]
    fn zeros(&mut self, n: u64) {
        // s1 unchanged; s2 += n·s1 mod MOD.
        self.s2 = (self.s2 + n % ADLER_MOD * self.s1) % ADLER_MOD;
    }

    /// Update for a block of `n` bytes with precomputed `sum_b` and `wsum_b`.
    #[inline]
    fn batch(&mut self, n: u64, sum_b: u64, wsum_b: u64) {
        self.s2 = (self.s2 + n % ADLER_MOD * self.s1 % ADLER_MOD + wsum_b) % ADLER_MOD;
        self.s1 = (self.s1 + sum_b) % ADLER_MOD;
    }

    fn finish(self) -> u32 {
        ((self.s2 << 16) | self.s1) as u32
    }
}

// ── PNG chunk framing ─────────────────────────────────────────────────────────

fn write_chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let mut buf = Vec::with_capacity(4 + data.len());
    buf.extend_from_slice(tag);
    buf.extend_from_slice(data);
    out.extend_from_slice(&crc32(&buf).to_be_bytes());
}

// ── DEFLATE fixed-Huffman bit writer ─────────────────────────────────────────
//
// RFC 1951 §3.2.6 fixed literal/length codes:
//   0..=143   → 8-bit, base 0b00110000 (48)
//   144..=255 → 9-bit, base 0b110010000 (400 offset for sym-144)
//   256       → 7-bit  0b0000000
//   257..=279 → 7-bit, base 0b0000001 (1 + (sym-257))
//   280..=287 → 8-bit, base 0b11000000 (192 + (sym-280))

struct BitWriter {
    buf: Vec<u8>,
    acc: u32,
    nbits: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            acc: 0,
            nbits: 0,
        }
    }

    #[inline]
    fn lsb(&mut self, val: u32, n: u8) {
        self.acc |= val << self.nbits;
        self.nbits += n;
        while self.nbits >= 8 {
            self.buf.push(self.acc as u8);
            self.acc >>= 8;
            self.nbits -= 8;
        }
    }

    #[inline]
    fn msb(&mut self, val: u32, n: u8) {
        self.lsb(val.reverse_bits() >> (32 - n), n);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.buf.push(self.acc as u8);
        }
        self.buf
    }
}

#[inline]
fn emit_lit(w: &mut BitWriter, byte: u8) {
    let s = byte as u32;
    if s <= 143 {
        w.msb(48 + s, 8);
    } else {
        w.msb(400 + (s - 144), 9);
    }
}

#[inline]
fn emit_eob(w: &mut BitWriter) {
    w.msb(0, 7);
}

fn emit_backref(w: &mut BitWriter, length: usize, distance: usize) {
    let (ls, le, lb) = length_code(length);
    if ls <= 279 {
        w.msb(1 + (ls - 257), 7);
    } else {
        w.msb(192 + (ls - 280), 8);
    }
    if lb > 0 {
        w.lsb(le, lb);
    }
    let (ds, de, db) = dist_code(distance);
    w.msb(ds, 5);
    if db > 0 {
        w.lsb(de, db);
    }
}

/// Emit `total` bytes as back-references at `dist`, split into chunks of 3..=258.
/// Adjusts chunk sizes to avoid a trailing chunk < 3 bytes.
fn emit_backref_run(w: &mut BitWriter, mut total: usize, dist: usize) {
    while total >= 3 {
        let len = if total <= 258 {
            total
        } else if total - 258 >= 3 {
            258
        } else {
            total - 3 // shorten so remainder is exactly 3
        };
        emit_backref(w, len, dist);
        total -= len;
    }
    for _ in 0..total {
        emit_lit(w, 0);
    } // tail < 3 (only reachable when total was < 3)
}

/// Emit `total` zero bytes: seed with up to 3 literals, then self-replicate via dist=1.
fn emit_zero_run(w: &mut BitWriter, total: usize) {
    if total == 0 {
        return;
    }
    if total < 6 {
        for _ in 0..total {
            emit_lit(w, 0);
        }
        return;
    }
    for _ in 0..3 {
        emit_lit(w, 0);
    }
    emit_backref_run(w, total - 3, 1);
}

fn length_code(len: usize) -> (u32, u32, u8) {
    match len {
        3 => (257, 0, 0),
        4 => (258, 0, 0),
        5 => (259, 0, 0),
        6 => (260, 0, 0),
        7 => (261, 0, 0),
        8 => (262, 0, 0),
        9 => (263, 0, 0),
        10 => (264, 0, 0),
        11..=12 => (265, (len - 11) as u32, 1),
        13..=14 => (266, (len - 13) as u32, 1),
        15..=16 => (267, (len - 15) as u32, 1),
        17..=18 => (268, (len - 17) as u32, 1),
        19..=22 => (269, (len - 19) as u32, 2),
        23..=26 => (270, (len - 23) as u32, 2),
        27..=30 => (271, (len - 27) as u32, 2),
        31..=34 => (272, (len - 31) as u32, 2),
        35..=42 => (273, (len - 35) as u32, 3),
        43..=50 => (274, (len - 43) as u32, 3),
        51..=58 => (275, (len - 51) as u32, 3),
        59..=66 => (276, (len - 59) as u32, 3),
        67..=82 => (277, (len - 67) as u32, 4),
        83..=98 => (278, (len - 83) as u32, 4),
        99..=114 => (279, (len - 99) as u32, 4),
        115..=130 => (280, (len - 115) as u32, 4),
        131..=162 => (281, (len - 131) as u32, 5),
        163..=194 => (282, (len - 163) as u32, 5),
        195..=226 => (283, (len - 195) as u32, 5),
        227..=257 => (284, (len - 227) as u32, 5),
        258 => (285, 0, 0),
        _ => panic!("length {len} out of DEFLATE range 3..=258"),
    }
}

fn dist_code(dist: usize) -> (u32, u32, u8) {
    match dist {
        1 => (0, 0, 0),
        2 => (1, 0, 0),
        3 => (2, 0, 0),
        4 => (3, 0, 0),
        5..=6 => (4, (dist - 5) as u32, 1),
        7..=8 => (5, (dist - 7) as u32, 1),
        9..=12 => (6, (dist - 9) as u32, 2),
        13..=16 => (7, (dist - 13) as u32, 2),
        17..=24 => (8, (dist - 17) as u32, 3),
        25..=32 => (9, (dist - 25) as u32, 3),
        33..=48 => (10, (dist - 33) as u32, 4),
        49..=64 => (11, (dist - 49) as u32, 4),
        65..=96 => (12, (dist - 65) as u32, 5),
        97..=128 => (13, (dist - 97) as u32, 5),
        129..=192 => (14, (dist - 129) as u32, 6),
        193..=256 => (15, (dist - 193) as u32, 6),
        257..=384 => (16, (dist - 257) as u32, 7),
        385..=512 => (17, (dist - 385) as u32, 7),
        513..=768 => (18, (dist - 513) as u32, 8),
        769..=1024 => (19, (dist - 769) as u32, 8),
        1025..=1536 => (20, (dist - 1025) as u32, 9),
        1537..=2048 => (21, (dist - 1537) as u32, 9),
        2049..=3072 => (22, (dist - 2049) as u32, 10),
        3073..=4096 => (23, (dist - 3073) as u32, 10),
        4097..=6144 => (24, (dist - 4097) as u32, 11),
        6145..=8192 => (25, (dist - 6145) as u32, 11),
        8193..=12288 => (26, (dist - 8193) as u32, 12),
        12289..=16384 => (27, (dist - 12289) as u32, 12),
        16385..=24576 => (28, (dist - 16385) as u32, 13),
        24577..=32768 => (29, (dist - 24577) as u32, 13),
        _ => panic!("distance {dist} out of DEFLATE range"),
    }
}

/// 32 bytes: one source-cell column, one sub-row (8 pixels × 4 bytes).
#[inline]
fn pixel_run(color: [u8; 4]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..UPSCALE {
        out[i * 4..i * 4 + 4].copy_from_slice(&color);
    }
    out
}

// ── per-source-row precomputed metadata ──────────────────────────────────────

struct RowMeta {
    has_fill: bool,
    /// Adler `sum_b`  for the full STRIDE-byte sequence [filter=0x00 | pixel bytes].
    sum_b: u64,
    /// Adler `wsum_b` for the same sequence.
    wsum_b: u64,
}

fn precompute_row_meta(source_filled: &[bool], cell_color: &[[u8; 4]]) -> Vec<RowMeta> {
    (0..TILE_SIDE)
        .map(|sy| {
            let has_fill = (0..TILE_SIDE).any(|sx| source_filled[sy * TILE_SIDE + sx]);
            if !has_fill {
                return RowMeta {
                    has_fill: false,
                    sum_b: 0,
                    wsum_b: 0,
                };
            }
            // Block = [filter=0x00 at pos 0] ++ [pixel bytes at pos 1..STRIDE].
            // filter byte contributes 0 to both sum and wsum, so iterate pixels only.
            let mut sum_b = 0u64;
            let mut wsum_b = 0u64;
            for sx in 0..TILE_SIDE {
                let color = if source_filled[sy * TILE_SIDE + sx] {
                    cell_color[sy * TILE_SIDE + sx]
                } else {
                    [0u8; 4]
                };
                // Pixel positions in block: 1 + sx*32 .. 1 + sx*32 + 32 (exclusive)
                let base_pos = 1 + sx * UPSCALE * BPP;
                for p in 0..(UPSCALE * BPP) {
                    let byte = color[p % BPP] as u64;
                    sum_b += byte;
                    wsum_b += (STRIDE - base_pos - p) as u64 * byte;
                }
            }
            RowMeta {
                has_fill: true,
                sum_b: sum_b % ADLER_MOD,
                wsum_b: wsum_b % ADLER_MOD,
            }
        })
        .collect()
}

// ── main encoder ─────────────────────────────────────────────────────────────

/// Encode the intersection image as a PNG byte vector.
///
/// `source_filled[sy * TILE_SIDE + sx]` — true when source cell (sx,sy) has a non-zero color.
/// `cell_color[sy * TILE_SIDE + sx]`    — RGBA color; only meaningful when filled.
///
/// Image orientation: sy=TILE_SIDE-1 is the top row (north-up).
pub fn encode_png(source_filled: &[bool], cell_color: &[[u8; 4]]) -> Vec<u8> {
    let meta = precompute_row_meta(source_filled, cell_color);

    let mut w = BitWriter::new();
    let mut adler = Adler32::new();

    // Single final fixed-Huffman block: BFINAL=1, BTYPE=01.
    // LSB-first: value 0b011 (BFINAL in bit0, BTYPE=01 in bits 1-2).
    w.lsb(0b011, 3);

    for img_row in 0..OUT_SIDE {
        let sy = TILE_SIDE - 1 - (img_row / UPSCALE);
        let dr = img_row % UPSCALE;
        let m = &meta[sy];

        if dr > 0 {
            // Sub-rows 1–7 are identical to sub-row 0. Back-reference the previous row.
            emit_lit(&mut w, 0x00);
            emit_backref_run(&mut w, ROW_BYTES, STRIDE);
            adler.batch(STRIDE as u64, m.sum_b, m.wsum_b);
        } else if !m.has_fill {
            // dr==0, all-transparent row.
            emit_lit(&mut w, 0x00);
            emit_zero_run(&mut w, ROW_BYTES);
            adler.zeros(STRIDE as u64);
        } else {
            // dr==0, row has colored cells: emit source-cell by source-cell.
            emit_lit(&mut w, 0x00);
            let mut sx = 0usize;
            while sx < TILE_SIDE {
                if !source_filled[sy * TILE_SIDE + sx] {
                    let start = sx;
                    while sx < TILE_SIDE && !source_filled[sy * TILE_SIDE + sx] {
                        sx += 1;
                    }
                    emit_zero_run(&mut w, (sx - start) * UPSCALE * BPP);
                } else {
                    let run = pixel_run(cell_color[sy * TILE_SIDE + sx]);
                    for &b in &run {
                        emit_lit(&mut w, b);
                    }
                    sx += 1;
                }
            }
            adler.batch(STRIDE as u64, m.sum_b, m.wsum_b);
        }
    }

    emit_eob(&mut w);
    let deflate_bits = w.finish();
    crate::analysis::memory_paranoid::check("encode_png::deflate_bits", deflate_bits.len() as u64);

    // zlib wrapper (RFC 1950).
    let adler_val = adler.finish();
    let mut zlib = Vec::with_capacity(2 + deflate_bits.len() + 4);
    zlib.extend_from_slice(&[0x78, 0x01]);
    zlib.extend_from_slice(&deflate_bits);
    zlib.extend_from_slice(&adler_val.to_be_bytes());
    crate::analysis::memory_paranoid::check("encode_png::zlib", zlib.capacity() as u64);

    // PNG file.
    let mut png = Vec::with_capacity(8 + 25 + 12 + zlib.len() + 12);
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&(OUT_SIDE as u32).to_be_bytes());
    ihdr[4..8].copy_from_slice(&(OUT_SIDE as u32).to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6; // bit depth=8, colour type=RGBA
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &zlib);
    write_chunk(&mut png, b"IEND", b"");
    crate::analysis::memory_paranoid::check("encode_png::png", png.capacity() as u64);

    png
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> image::DynamicImage {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Png).expect("valid PNG")
    }

    fn make_inputs(fills: &[(usize, usize, [u8; 4])]) -> (Vec<bool>, Vec<[u8; 4]>) {
        let mut filled = vec![false; TILE_SIDE * TILE_SIDE];
        let mut colors = vec![[0u8; 4]; TILE_SIDE * TILE_SIDE];
        for &(sx, sy, color) in fills {
            filled[sy * TILE_SIDE + sx] = true;
            colors[sy * TILE_SIDE + sx] = color;
        }
        (filled, colors)
    }

    #[test]
    fn empty_image_is_fully_transparent() {
        let (f, c) = make_inputs(&[]);
        let img = decode(&encode_png(&f, &c));
        use image::GenericImageView;
        for y in 0..OUT_SIDE as u32 {
            for x in 0..OUT_SIDE as u32 {
                assert_eq!(
                    img.get_pixel(x, y).0[3],
                    0,
                    "pixel ({x},{y}) should be transparent"
                );
            }
        }
    }

    #[test]
    fn single_cell_color_and_position() {
        let color = [255u8, 100, 50, 255];
        let (f, c) = make_inputs(&[(0, 0, color)]);
        let img = decode(&encode_png(&f, &c));
        use image::GenericImageView;
        // sx=0, sy=0 → SW corner → image rows OUT_SIDE-UPSCALE..OUT_SIDE, cols 0..UPSCALE
        let row_start = (OUT_SIDE - UPSCALE) as u32;
        for row in row_start..OUT_SIDE as u32 {
            for col in 0..UPSCALE as u32 {
                assert_eq!(img.get_pixel(col, row).0, color, "cell pixel ({col},{row})");
            }
        }
        assert_eq!(
            img.get_pixel(0, row_start - 1).0[3],
            0,
            "above cell should be transparent"
        );
        assert_eq!(
            img.get_pixel(UPSCALE as u32, row_start).0[3],
            0,
            "right of cell should be transparent"
        );
    }

    #[test]
    fn north_cell_is_at_top_of_image() {
        let color = [0u8, 200, 255, 255];
        let (f, c) = make_inputs(&[(0, TILE_SIDE - 1, color)]);
        let img = decode(&encode_png(&f, &c));
        use image::GenericImageView;
        for row in 0..UPSCALE as u32 {
            assert_eq!(img.get_pixel(0, row).0, color, "north cell row {row}");
        }
    }

    #[test]
    fn png_dimensions_are_correct() {
        let (f, c) = make_inputs(&[]);
        let img = decode(&encode_png(&f, &c));
        assert_eq!(img.width(), OUT_SIDE as u32);
        assert_eq!(img.height(), OUT_SIDE as u32);
    }

    #[test]
    fn multiple_cells_correct_colors() {
        let red = [255u8, 0, 0, 255];
        let green = [0u8, 255, 0, 255];
        let (f, c) = make_inputs(&[(10, 10, red), (490, 490, green)]);
        let img = decode(&encode_png(&f, &c));
        use image::GenericImageView;
        // (sx=10, sy=10) → image row (TILE_SIDE-1-10)*8 = 489*8 = 3912, col 10*8=80
        assert_eq!(img.get_pixel(80, 3912).0, red, "red cell");
        // (sx=490, sy=490) → image row (9)*8=72, col 490*8=3920
        assert_eq!(img.get_pixel(3920, 72).0, green, "green cell");
    }
}
