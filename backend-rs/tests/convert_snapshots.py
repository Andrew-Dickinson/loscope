"""
Convert Python .npz fresnel zone snapshots to a flat binary format for Rust tests.

Binary layout (all little-endian):
  u32  height
  u32  max_width
  i64  x_base_offset
  i64  y_base_offset
  u32[height]            widths
  u32[height]            offsets
  u16[height*max_width]  top    (row-major)
  u16[height*max_width]  bottom (row-major)

Usage:
  python3 tests/convert_snapshots.py
"""

from pathlib import Path
import struct
import numpy as np

NPZ_DIR  = Path("/Users/ally/PycharmProjects/los-analyzer-4/tests/fresnel/snapshots")
OUT_DIR  = Path(__file__).parent / "snapshots"
ALPHAS   = [1.0, 0.6]
CASES    = ["ns_link_24ghz", "diag_link_60ghz", "ew_link_5ghz"]

def alpha_tag(a: float) -> str:
    return f"alpha{a:.1f}".replace(".", "_")

OUT_DIR.mkdir(exist_ok=True)

for case in CASES:
    for alpha in ALPHAS:
        tag  = alpha_tag(alpha)
        src  = NPZ_DIR / f"{case}_{tag}.npz"
        dst  = OUT_DIR / f"{case}_{tag}.bin"

        snap = np.load(src)
        top    = snap["top"].astype("<u2")     # uint16, row-major
        bottom = snap["bottom"].astype("<u2")
        widths = snap["widths"].astype("<u4")  # uint32
        offsets = snap["offsets"].astype("<u4")
        x_base = int(snap["x_base_offset"])
        y_base = int(snap["y_base_offset"])

        height, max_width = top.shape

        with open(dst, "wb") as f:
            f.write(struct.pack("<II", height, max_width))
            f.write(struct.pack("<qq", x_base, y_base))
            f.write(widths.tobytes())
            f.write(offsets.tobytes())
            f.write(top.tobytes())
            f.write(bottom.tobytes())

        print(f"wrote {dst.name}  ({height}×{max_width}, x_base={x_base}, y_base={y_base})")