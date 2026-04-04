#!/usr/bin/env python3
"""
Convert manga-image-translator AOT inpainting weights to safetensors for Candle.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from urllib.request import urlopen

from huggingface_hub import HfApi
from safetensors.torch import save_file
import torch


SOURCE_CHECKPOINT_URL = (
    "https://github.com/zyddnys/manga-image-translator/releases/download/beta-0.3/inpainting.ckpt"
)
TARGET_REPO = "mayocream/manga-image-translator-inpainting-aot"


def parse_args() -> argparse.Namespace:
    default_artifacts = Path("temp") / "aot-inpainting" / "artifacts"
    default_output = Path("temp") / "aot-inpainting" / "export"
    parser = argparse.ArgumentParser(
        description="Convert manga-image-translator AOT inpainting weights to safetensors."
    )
    parser.add_argument(
        "--checkpoint",
        type=Path,
        default=default_artifacts / "inpainting.ckpt",
        help=f"Local checkpoint path (default: {default_artifacts / 'inpainting.ckpt'})",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=default_output,
        help=f"Output directory (default: {default_output})",
    )
    parser.add_argument(
        "--repo-id",
        default=TARGET_REPO,
        help=f"Target Hugging Face repo for --upload (default: {TARGET_REPO})",
    )
    parser.add_argument(
        "--upload",
        action="store_true",
        help="Upload the converted bundle to Hugging Face after conversion.",
    )
    parser.add_argument(
        "--private",
        action="store_true",
        help="Create the target Hugging Face repo as private when used with --upload.",
    )
    return parser.parse_args()


def ensure_checkpoint(path: Path) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        return path

    print(f"Downloading {SOURCE_CHECKPOINT_URL} -> {path}")
    with urlopen(SOURCE_CHECKPOINT_URL) as response:
        path.write_bytes(response.read())
    return path


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_model_card(repo_id: str, checkpoint_sha256: str, tensor_count: int) -> str:
    tags = [
        "candle",
        "image-inpainting",
        "manga",
        "comic",
        "aot",
    ]
    tags_block = "\n".join(f"- {tag}" for tag in tags)
    return f"""---
license: mit
library_name: candle
tags:
{tags_block}
---

# {repo_id}

This repository contains a Candle-compatible `safetensors` conversion of the
`inpainting.ckpt` AOT generator released by
[`zyddnys/manga-image-translator`](https://github.com/zyddnys/manga-image-translator).

Files:

- `model.safetensors`: converted floating-point checkpoint using the original tensor names
- `config.json`: loader metadata for `koharu-ml`

Metadata:

- Source checkpoint: `{SOURCE_CHECKPOINT_URL}`
- Source checkpoint SHA256: `{checkpoint_sha256}`
- Tensor count: `{tensor_count}`
- Architecture: `AOTGenerator`
- Input channels: `4` (`mask + RGB image`)
- Output channels: `3`
- Base channels: `32`
- AOT blocks: `10`
- Dilation rates: `[2, 4, 8, 16]`
- Default max side: `1024`
"""


def main() -> None:
    args = parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    checkpoint_path = ensure_checkpoint(args.checkpoint)
    checkpoint = torch.load(checkpoint_path, map_location="cpu")
    state_dict = checkpoint["model"] if isinstance(checkpoint, dict) and "model" in checkpoint else checkpoint
    if not isinstance(state_dict, dict):
        raise RuntimeError(f"Unexpected checkpoint payload: {type(state_dict)!r}")

    tensor_map: dict[str, torch.Tensor] = {}
    for key, value in state_dict.items():
        if not isinstance(value, torch.Tensor) or not value.is_floating_point():
            continue
        tensor_map[key] = value.detach().cpu().contiguous().clone()

    config = {
        "model_type": "manga-image-translator-aot",
        "input_channels": 4,
        "output_channels": 3,
        "base_channels": 32,
        "num_blocks": 10,
        "dilation_rates": [2, 4, 8, 16],
        "pad_multiple": 8,
        "default_max_side": 1024,
        "source_checkpoint_url": SOURCE_CHECKPOINT_URL,
        "source_checkpoint_sha256": sha256_file(checkpoint_path),
    }

    save_file(tensor_map, str(args.output_dir / "model.safetensors"))
    with (args.output_dir / "config.json").open("w", encoding="utf-8") as handle:
        json.dump(config, handle, indent=2)
        handle.write("\n")

    with (args.output_dir / "README.md").open("w", encoding="utf-8") as handle:
        handle.write(
            build_model_card(
                args.repo_id,
                config["source_checkpoint_sha256"],
                len(tensor_map),
            )
        )
        handle.write("\n")

    print(f"Saved {len(tensor_map)} tensors to {args.output_dir / 'model.safetensors'}")
    print(f"Saved config to {args.output_dir / 'config.json'}")
    print(f"Saved README to {args.output_dir / 'README.md'}")

    if args.upload:
        api = HfApi()
        api.create_repo(repo_id=args.repo_id, repo_type="model", private=args.private, exist_ok=True)
        api.upload_folder(
            folder_path=str(args.output_dir),
            repo_id=args.repo_id,
            repo_type="model",
            commit_message="Add Candle safetensors conversion for manga-image-translator AOT inpainting",
        )
        print(f"Uploaded converted bundle to https://huggingface.co/{args.repo_id}")


if __name__ == "__main__":
    main()
