use loscope::analysis::fresnel_zone::FresnelZone;

pub struct Snapshot {
    pub height: usize,
    pub max_width: usize,
    pub x_base_offset: i64,
    pub y_base_offset: i64,
    pub widths: Vec<u32>,
    pub offsets: Vec<u32>,
    pub top: Vec<u16>,    // row-major, height × max_width
    pub bottom: Vec<u16>, // row-major, height × max_width
}

pub fn load_snapshot(name: &str) -> Snapshot {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.bin"));
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read snapshot {path:?}: {e}"));
    let mut pos = 0;

    macro_rules! read_u32 {
        () => {{
            let v = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            pos += 4;
            v
        }};
    }
    macro_rules! read_i64 {
        () => {{
            let v = i64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap());
            pos += 8;
            v
        }};
    }

    let height = read_u32!() as usize;
    let max_width = read_u32!() as usize;
    let x_base_offset = read_i64!();
    let y_base_offset = read_i64!();

    let read_u32_vec = |bytes: &[u8], pos: &mut usize, n: usize| -> Vec<u32> {
        (0..n)
            .map(|_| {
                let v = u32::from_le_bytes(bytes[*pos..*pos + 4].try_into().unwrap());
                *pos += 4;
                v
            })
            .collect()
    };
    let read_u16_vec = |bytes: &[u8], pos: &mut usize, n: usize| -> Vec<u16> {
        (0..n)
            .map(|_| {
                let v = u16::from_le_bytes(bytes[*pos..*pos + 2].try_into().unwrap());
                *pos += 2;
                v
            })
            .collect()
    };

    let widths = read_u32_vec(&bytes, &mut pos, height);
    let offsets = read_u32_vec(&bytes, &mut pos, height);
    let top = read_u16_vec(&bytes, &mut pos, height * max_width);
    let bottom = read_u16_vec(&bytes, &mut pos, height * max_width);

    Snapshot {
        height,
        max_width,
        x_base_offset,
        y_base_offset,
        widths,
        offsets,
        top,
        bottom,
    }
}

pub fn assert_matches_snapshot(zone: &FresnelZone, snap: &Snapshot) {
    let base = zone.base_offset();
    assert_eq!(
        *base.easting() as i64,
        snap.x_base_offset,
        "x_base_offset mismatch"
    );
    assert_eq!(
        *base.northing() as i64,
        snap.y_base_offset,
        "y_base_offset mismatch"
    );

    let h = zone.widths().len();
    assert_eq!(h, snap.height, "height mismatch");

    for i in 0..h {
        assert_eq!(
            zone.widths()[i] as u32,
            snap.widths[i],
            "widths[{i}] mismatch"
        );
        assert_eq!(
            zone.offsets()[i] as u32,
            snap.offsets[i],
            "offsets[{i}] mismatch"
        );

        let w = zone.widths()[i];
        for j in 0..w {
            let cell = &zone.values()[[i, j]];
            let snap_idx = i * snap.max_width + j;
            let exp_top = snap.top[snap_idx];
            let exp_bottom = snap.bottom[snap_idx];
            // ±1 inch tolerance: uint16 inches derived from float via *12 + clip + cast
            assert!(
                cell.top().abs_diff(exp_top) <= 1,
                "top mismatch at [{i},{j}]: got {}, expected {exp_top}",
                cell.top()
            );
            assert!(
                cell.bottom().abs_diff(exp_bottom) <= 1,
                "bottom mismatch at [{i},{j}]: got {}, expected {exp_bottom}",
                cell.bottom()
            );
        }
    }
}
