"""
Preprocess all .las files in data/nys_raw/.

Usage:
    python tools/preprocess_all.py [out_dir]

Default out_dir: data/preprocessed
"""
import sys
from pathlib import Path

from tqdm import tqdm

sys.path.insert(0, str(Path(__file__).parent.parent))
from src.los_analyzer.preprocessing.preprocess import run_preprocessing


def main():
    raw_dir = Path("data/nys_raw")
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("data/preprocessed")

    las_files = sorted(raw_dir.glob("*.las"))
    if not las_files:
        print(f"No .las files found in {raw_dir}")
        sys.exit(1)

    real_stdout = sys.stdout

    class TqdmWriter:
        def write(self, msg):
            if msg.strip():
                tqdm.write(msg.rstrip(), file=real_stdout)
        def flush(self):
            pass

    tqdm_writer = TqdmWriter()
    tqdm.write(f"Found {len(las_files)} file(s). Output -> {out_dir}", file=real_stdout)
    with tqdm(las_files, desc="Preprocessing", unit="file", file=real_stdout) as bar:
        for las_file in bar:
            bar.set_postfix(file=las_file.name)
            sys.stdout = tqdm_writer
            try:
                run_preprocessing(las_file, out_dir)
            finally:
                sys.stdout = real_stdout


if __name__ == "__main__":
    main()
