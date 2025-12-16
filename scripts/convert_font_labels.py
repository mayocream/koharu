#!/usr/bin/env python3
"""
Convert font_demo_cache.bin (from YuzuMarker.FontDetection) to a JSON list that Rust can read.

Usage:
    python scripts/convert_font_labels.py \
        --input font_demo_cache.bin \
        --output yuzumarker-font-labels.json
"""

import argparse
import json
import sys
import types
from pathlib import Path


def parse_args():
    parser = argparse.ArgumentParser(description="Convert font_demo_cache.bin to JSON")
    parser.add_argument(
        "-i",
        "--input",
        type=Path,
        default=Path("font_demo_cache.bin"),
        help="Input pickle file (font_demo_cache.bin from the original repo)",
    )
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        default=Path("yuzumarker-font-labels.json"),
        help="Output JSON path (default: yuzumarker-font-labels.json)",
    )
    return parser.parse_args()


def main():
    args = parse_args()
    if not args.input.exists():
        sys.exit(f"Input file not found: {args.input}")

    # Stub module/classes so pickle can load without the original code
    font_dataset_mod = types.ModuleType("font_dataset")
    font_mod = types.ModuleType("font_dataset.font")

    class DSFont:
        def __init__(self, path=None, language=None):
            self.path = path
            self.language = language

    font_mod.DSFont = DSFont
    sys.modules["font_dataset"] = font_dataset_mod
    sys.modules["font_dataset.font"] = font_mod
    font_dataset_mod.font = font_mod

    import pickle  # noqa: E402

    with open(args.input, "rb") as f:
        data = pickle.load(f)

    entries = []
    for item in data:
        path = getattr(item, "path", None)
        language = getattr(item, "language", None)
        if path is None:
            continue
        entries.append({"path": path, "language": language})

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(entries, f, ensure_ascii=False, indent=2)

    print(f"Wrote {len(entries)} labels to {args.output}")


if __name__ == "__main__":
    main()
