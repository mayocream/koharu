# /// script
# requires-python = ">=3.11,<3.15"
# dependencies = [
#   "safetensors>=0.5,<1",
#   "ultralytics==8.4.43",
# ]
# ///
"""Export and verify the Comic Layout YOLO26s checkpoint as SafeTensors."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any

import torch
import ultralytics
import yaml
from safetensors.torch import load_file, save_file
from ultralytics import YOLO


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("output", type=Path)
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def flatten_tensors(value: Any) -> list[torch.Tensor]:
    if isinstance(value, torch.Tensor):
        return [value]
    if isinstance(value, dict):
        tensors: list[torch.Tensor] = []
        for key in sorted(value):
            tensors.extend(flatten_tensors(value[key]))
        return tensors
    if isinstance(value, (list, tuple)):
        tensors = []
        for item in value:
            tensors.extend(flatten_tensors(item))
        return tensors
    return []


def main() -> None:
    args = parse_args()
    args.output.mkdir(parents=True, exist_ok=True)

    source = YOLO(args.checkpoint)
    source.model.eval()
    source_sha256 = sha256(args.checkpoint)
    state = {
        name: tensor.detach().cpu().contiguous()
        for name, tensor in source.model.state_dict().items()
    }
    weights_path = args.output / "model.safetensors"
    save_file(
        state,
        weights_path,
        metadata={
            "format": "pt",
            "architecture": "YOLO26s-seg",
            "classes": json.dumps(source.names, separators=(",", ":")),
            "source_checkpoint_sha256": source_sha256,
            "ultralytics": ultralytics.__version__,
        },
    )

    model_yaml = dict(source.model.yaml)
    model_yaml.pop("yaml_file", None)
    model_yaml.pop("channels", None)
    # Ultralytics selects the compound scale from the architecture filename.
    yaml_path = args.output / "yolo26s-seg.yaml"
    yaml_path.write_text(
        yaml.safe_dump(model_yaml, sort_keys=False, allow_unicode=True),
        encoding="utf-8",
    )
    config = {
        "architectures": ["YOLO26s-seg"],
        "library_name": "ultralytics",
        "task": "instance-segmentation",
        "image_size": 1280,
        "num_classes": len(source.names),
        "names": {str(key): value for key, value in source.names.items()},
        "weights": "model.safetensors",
        "model_config": "yolo26s-seg.yaml",
        "ultralytics_version": ultralytics.__version__,
    }
    (args.output / "config.json").write_text(
        json.dumps(config, indent=2) + "\n", encoding="utf-8"
    )

    restored_state = load_file(weights_path, device="cpu")
    if set(restored_state) != set(state):
        raise RuntimeError("SafeTensors keys do not match the source checkpoint")
    for name, source_tensor in state.items():
        if not torch.equal(source_tensor, restored_state[name]):
            raise RuntimeError(f"SafeTensors value mismatch: {name}")

    restored = YOLO(yaml_path, task="segment")
    restored.model.load_state_dict(restored_state, strict=True)
    restored.model.eval()
    torch.manual_seed(42)
    image = torch.rand(1, 3, 256, 256)
    with torch.inference_mode():
        expected = flatten_tensors(source.model(image))
        actual = flatten_tensors(restored.model(image))
    if len(expected) != len(actual):
        raise RuntimeError("Forward outputs have different structures")
    max_absolute_difference = max(
        (left - right).abs().max().item()
        for left, right in zip(expected, actual, strict=True)
    )
    if max_absolute_difference != 0.0:
        raise RuntimeError(
            f"Forward output mismatch: max absolute difference {max_absolute_difference}"
        )

    manifest = {
        "source_checkpoint": args.checkpoint.name,
        "source_checkpoint_sha256": source_sha256,
        "architecture": "YOLO26s-seg",
        "parameters": sum(parameter.numel() for parameter in source.model.parameters()),
        "tensors": len(state),
        "classes": source.names,
        "model_safetensors": {
            "bytes": weights_path.stat().st_size,
            "sha256": sha256(weights_path),
        },
        "validation": {
            "strict_state_dict": True,
            "forward_input_shape": list(image.shape),
            "forward_tensors": len(expected),
            "max_absolute_difference": max_absolute_difference,
        },
    }
    (args.output / "export-manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
