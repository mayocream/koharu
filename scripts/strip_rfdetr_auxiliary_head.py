#!/usr/bin/env python3
"""Remove an auxiliary module from an RF-DETR Lightning checkpoint."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

import torch


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--module", default="typography_head")
    return parser.parse_args()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    args = parse_args()
    source = args.source.resolve()
    destination = args.destination.resolve()
    checkpoint = torch.load(source, map_location="cpu", weights_only=False)
    removed: dict[str, list[str]] = {}
    for state_name in ("model", "state_dict"):
        state = checkpoint.get(state_name)
        if not isinstance(state, dict):
            continue
        keys = [key for key in state if args.module in key]
        removed[state_name] = keys
        for key in keys:
            del state[key]
    if not removed.get("model") or not removed.get("state_dict"):
        raise RuntimeError(
            f"expected {args.module!r} tensors in both model and state_dict"
        )
    destination.parent.mkdir(parents=True, exist_ok=True)
    torch.save(checkpoint, destination)
    reloaded = torch.load(destination, map_location="cpu", weights_only=False)
    leftovers = [
        f"{state_name}:{key}"
        for state_name in ("model", "state_dict")
        for key in reloaded.get(state_name, {})
        if args.module in key
    ]
    if leftovers:
        raise RuntimeError(f"auxiliary tensors remain: {leftovers}")
    print(f"source_sha256={sha256_file(source)}")
    print(f"destination_sha256={sha256_file(destination)}")
    print(f"removed_model_tensors={len(removed['model'])}")
    print(f"removed_state_dict_tensors={len(removed['state_dict'])}")
    print(f"destination={destination}")


if __name__ == "__main__":
    main()
