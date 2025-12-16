#!/usr/bin/env python3
"""
Convert YuzuMarker.FontDetection checkpoints (.ckpt) to safetensors for Candle.

Example:
    python scripts/convert_font_detection.py \
        --checkpoint name=4x-epoch=84-step=1649340.ckpt
"""

import argparse
from pathlib import Path

from huggingface_hub import hf_hub_download
import torch
from safetensors.torch import save_file


DEFAULT_CKPT = "name=4x-epoch=84-step=1649340.ckpt"
REPO_ID = "gyrojeff/YuzuMarker.FontDetection"


def parse_args() -> argparse.Namespace:
    cache_dir = (
        Path.home() / ".cache" / "Koharu" / "models" / "yuzumarker-font-detection.safetensors"
    )
    parser = argparse.ArgumentParser(description="Convert YuzuMarker.FontDetection checkpoint.")
    parser.add_argument(
        "-c",
        "--checkpoint",
        default=DEFAULT_CKPT,
        help=f"Checkpoint filename from {REPO_ID} (default: {DEFAULT_CKPT})",
    )
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        default=cache_dir,
        help=f"Output safetensors path (default: {cache_dir})",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)

    print(f"Downloading {args.checkpoint} from {REPO_ID} ...")
    ckpt_path = hf_hub_download(repo_id=REPO_ID, filename=args.checkpoint)
    print(f"Loaded checkpoint at {ckpt_path}")

    state = torch.load(ckpt_path, map_location="cpu")
    if "state_dict" not in state:
        raise RuntimeError("Unexpected checkpoint format: missing state_dict")
    state_dict = state["state_dict"]
    print(f"Saving {len(state_dict)} tensors to {args.output}")
    save_file(state_dict, str(args.output))
    print("Done.")


if __name__ == "__main__":
    main()
