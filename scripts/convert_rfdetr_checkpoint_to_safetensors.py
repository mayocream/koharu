#!/usr/bin/env python3
"""Convert an RF-DETR checkpoint's inference weights to SafeTensors."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import torch
from safetensors import safe_open
from safetensors.torch import save_file


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--architecture", default="RFDETRSeg2XLarge")
    parser.add_argument("--resolution", type=int, default=1152)
    parser.add_argument("--num-select", type=int, default=160)
    parser.add_argument("--class-names", nargs="+", required=True)
    args = parser.parse_args()

    checkpoint = torch.load(args.source, map_location="cpu", weights_only=False)
    state = checkpoint.get("model")
    if not isinstance(state, dict) or not state:
        raise ValueError("Expected a non-empty top-level 'model' state dict")
    if not all(isinstance(name, str) and isinstance(tensor, torch.Tensor) for name, tensor in state.items()):
        raise TypeError("The model state dict must contain only named tensors")

    tensors = {name: tensor.detach().cpu().contiguous() for name, tensor in state.items()}
    metadata = {
        "format": "pt",
        "architecture": args.architecture,
        "resolution": str(args.resolution),
        "num_select": str(args.num_select),
        "class_names": json.dumps(args.class_names, ensure_ascii=False),
        "source_sha256": sha256(args.source),
    }
    args.destination.parent.mkdir(parents=True, exist_ok=True)
    save_file(tensors, args.destination, metadata=metadata)

    with safe_open(args.destination, framework="pt", device="cpu") as stored:
        stored_names = list(stored.keys())
        stored_metadata = stored.metadata()
        for name, expected in tensors.items():
            actual = stored.get_tensor(name)
            if not torch.equal(actual, expected):
                raise RuntimeError(f"Tensor changed during conversion: {name}")

    if set(stored_names) != set(tensors):
        raise RuntimeError("SafeTensors keys differ from the source model state dict")
    print(f"tensors={len(tensors)}")
    print(f"metadata={json.dumps(stored_metadata, sort_keys=True)}")
    print(f"sha256={sha256(args.destination)}")


if __name__ == "__main__":
    main()
