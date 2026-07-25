"""Merge a SAM-TS fine-tune and export full plus adapter SafeTensors weights."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import torch
from safetensors import safe_open
from safetensors.torch import load_file, save_file

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from train_zenodo_sam_ts import build_model  # noqa: E402


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--checkpoint",
        type=Path,
        default=ROOT / "models" / "hi-sam" / "sam_tss_l_textseg.pth",
    )
    parser.add_argument(
        "--sam-checkpoint",
        type=Path,
        default=ROOT / "models" / "hi-sam" / "sam_vit_l_0b3195.pth",
    )
    parser.add_argument(
        "--fine-tuned",
        type=Path,
        default=(
            ROOT
            / "runs"
            / "sam-ts-l-textseg-zenodo-b300-full"
            / "sam_tss_l_zenodo_best.pth"
        ),
    )
    parser.add_argument("--hi-sam-root", type=Path, default=ROOT / "temp" / "Hi-SAM")
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "runs" / "huggingface" / "koharu-text-sam-ts-l",
    )
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tensor_report(state: dict[str, torch.Tensor]) -> dict[str, Any]:
    return {
        "tensors": len(state),
        "elements": sum(value.numel() for value in state.values()),
        "dtypes": sorted({str(value.dtype) for value in state.values()}),
    }


def save_and_verify(
    state: dict[str, torch.Tensor], destination: Path, metadata: dict[str, str]
) -> dict[str, Any]:
    contiguous = {
        name: value.detach().cpu().contiguous().clone()
        for name, value in state.items()
    }
    save_file(contiguous, destination, metadata=metadata)
    loaded = load_file(destination, device="cpu")
    if set(loaded) != set(contiguous):
        raise ValueError(f"SafeTensors key mismatch for {destination}")
    for name, expected in contiguous.items():
        actual = loaded[name]
        if actual.dtype != expected.dtype or actual.shape != expected.shape:
            raise ValueError(f"SafeTensors tensor metadata mismatch: {name}")
        if not torch.equal(actual, expected):
            raise ValueError(f"SafeTensors tensor value mismatch: {name}")
    with safe_open(destination, framework="pt", device="cpu") as file:
        stored_metadata = file.metadata() or {}
    if stored_metadata != metadata:
        raise ValueError(f"SafeTensors header metadata mismatch for {destination}")
    return {
        **tensor_report(contiguous),
        "bytes": destination.stat().st_size,
        "sha256": sha256(destination),
        "metadata": stored_metadata,
    }


def main() -> None:
    args = parse_args()
    for name in (
        "checkpoint",
        "sam_checkpoint",
        "fine_tuned",
        "hi_sam_root",
        "output",
    ):
        setattr(args, name, getattr(args, name).resolve())
    for path in (
        args.checkpoint,
        args.sam_checkpoint,
        args.fine_tuned,
        args.hi_sam_root / "hi_sam" / "modeling" / "build.py",
    ):
        if not path.is_file():
            raise FileNotFoundError(path)
    args.output.mkdir(parents=True, exist_ok=False)

    adapter = torch.load(args.fine_tuned, map_location="cpu", weights_only=True)
    if not isinstance(adapter, dict) or not adapter:
        raise ValueError("fine-tuned checkpoint is not a non-empty state dictionary")
    if any(not isinstance(value, torch.Tensor) for value in adapter.values()):
        raise ValueError("fine-tuned checkpoint contains non-tensor entries")

    model_args = SimpleNamespace(
        hi_sam_root=args.hi_sam_root,
        checkpoint=args.checkpoint,
        sam_checkpoint=args.sam_checkpoint,
    )
    with tempfile.TemporaryDirectory(
        prefix="koharu-text-sam-ts-l-build-", dir=args.output.parent
    ) as temporary:
        model = build_model(model_args, Path(temporary))
    trainable = {
        name for name, parameter in model.named_parameters() if parameter.requires_grad
    }
    if set(adapter) != trainable:
        raise ValueError(
            "adapter keys differ from trainable model keys: "
            f"missing={sorted(trainable - set(adapter))[:5]}, "
            f"unexpected={sorted(set(adapter) - trainable)[:5]}"
        )
    incompatible = model.load_state_dict(adapter, strict=False)
    if incompatible.unexpected_keys:
        raise ValueError(f"unexpected adapter keys: {incompatible.unexpected_keys[:5]}")
    full_state = model.state_dict()

    common_metadata = {
        "format": "pt",
        "architecture": "Hi-SAM SAM-TS-L TextSeg",
        "model_name": "koharu-text-sam-ts-l",
        "source_checkpoint_sha256": sha256(args.fine_tuned),
        "base_textseg_sha256": sha256(args.checkpoint),
        "sam_vit_l_sha256": sha256(args.sam_checkpoint),
    }
    adapter_report = save_and_verify(
        adapter,
        args.output / "adapter_model.safetensors",
        {**common_metadata, "weight_type": "trainable_delta"},
    )
    full_report = save_and_verify(
        full_state,
        args.output / "model.safetensors",
        {**common_metadata, "weight_type": "full_merged_state_dict"},
    )

    loaded_full = load_file(args.output / "model.safetensors", device="cpu")
    strict_result = model.load_state_dict(loaded_full, strict=True)
    if strict_result.missing_keys or strict_result.unexpected_keys:
        raise ValueError(f"strict full-state load failed: {strict_result}")
    report = {
        "status": "complete",
        "source": {
            "fine_tuned": str(args.fine_tuned),
            "sha256": sha256(args.fine_tuned),
            "best_epoch": 20,
        },
        "adapter": adapter_report,
        "full": full_report,
        "strict_full_state_load": True,
    }
    (args.output / "conversion_report.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
