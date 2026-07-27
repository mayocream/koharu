# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "numpy>=2.0",
#   "opencv-python-headless>=4.10",
#   "safetensors>=0.5",
#   "segmentation-models-pytorch>=0.5",
#   "torch>=2.6",
# ]
# ///
"""Run mayocream/manga-text-segmentation-2025 with PyTorch."""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import cv2
import numpy as np
import segmentation_models_pytorch as smp
import torch
import torch.nn as nn
import torch.nn.functional as F
from safetensors.torch import load_file


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--threshold", type=float, default=0.5)
    parser.add_argument("--tta", action="store_true", help="Average original, horizontal, and vertical flips.")
    return parser.parse_args()


def convert_batchnorm_to_groupnorm(module: nn.Module) -> None:
    for name, child in module.named_children():
        if isinstance(child, nn.BatchNorm2d):
            groups = next(
                (
                    value
                    for value in range(min(child.num_features, 8), 1, -1)
                    if child.num_features % value == 0
                ),
                1,
            )
            setattr(module, name, nn.GroupNorm(groups, child.num_features))
        else:
            convert_batchnorm_to_groupnorm(child)


def load_model(weights: Path, device: torch.device) -> nn.Module:
    model = smp.UnetPlusPlus(
        encoder_name="tu-efficientnetv2_rw_m",
        encoder_weights=None,
        in_channels=3,
        classes=1,
        activation=None,
        decoder_attention_type="scse",
    )
    convert_batchnorm_to_groupnorm(model.decoder)
    model.load_state_dict(load_file(str(weights)), strict=True)
    return model.eval().to(device)


def main() -> None:
    args = parse_args()
    if not 0.0 < args.threshold < 1.0:
        raise ValueError("--threshold must be between zero and one")
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    image_bgr = cv2.imread(str(args.input), cv2.IMREAD_COLOR)
    if image_bgr is None:
        raise FileNotFoundError(args.input)
    image_rgb = cv2.cvtColor(image_bgr, cv2.COLOR_BGR2RGB)
    height, width = image_rgb.shape[:2]
    tensor = torch.from_numpy(image_rgb.copy()).permute(2, 0, 1).float().div_(255.0)
    mean = torch.tensor((0.485, 0.456, 0.406))[:, None, None]
    std = torch.tensor((0.229, 0.224, 0.225))[:, None, None]
    tensor = ((tensor - mean) / std).unsqueeze(0).to(device)
    tensor = F.pad(
        tensor,
        (0, (32 - width % 32) % 32, 0, (32 - height % 32) % 32),
    )

    model = load_model(args.model, device)
    variants = [(tensor, ())]
    if args.tta:
        variants.extend(((torch.flip(tensor, (3,)), (3,)), (torch.flip(tensor, (2,)), (2,))))
    if device.type == "cuda":
        torch.cuda.reset_peak_memory_stats()
        torch.cuda.synchronize()
    started = time.perf_counter()
    probabilities = None
    with torch.inference_mode():
        for value, flip_dims in variants:
            with torch.autocast(device_type=device.type, enabled=device.type == "cuda"):
                prediction = model(value).sigmoid()
            if flip_dims:
                prediction = torch.flip(prediction, flip_dims)
            probabilities = prediction if probabilities is None else probabilities + prediction
    probabilities = probabilities / len(variants)
    if device.type == "cuda":
        torch.cuda.synchronize()
    elapsed = time.perf_counter() - started
    probability = probabilities[0, 0, :height, :width].float().cpu().numpy()
    mask = probability >= args.threshold

    overlay = image_rgb.copy()
    overlay[mask] = (
        overlay[mask].astype(np.float32) * 0.35
        + np.array((255, 32, 32), dtype=np.float32) * 0.65
    ).astype(np.uint8)
    probability_image = np.clip(probability * 255.0, 0, 255).astype(np.uint8)
    binary_image = mask.astype(np.uint8) * 255
    args.output.mkdir(parents=True, exist_ok=True)
    cv2.imwrite(str(args.output / "probability.png"), probability_image)
    cv2.imwrite(str(args.output / "mask.png"), binary_image)
    cv2.imwrite(
        str(args.output / "overlay.jpg"), cv2.cvtColor(overlay, cv2.COLOR_RGB2BGR)
    )
    stats = {
        "model": "mayocream/manga-text-segmentation-2025",
        "runtime": "PyTorch",
        "device": str(device),
        "input": str(args.input),
        "shape": [height, width],
        "threshold": args.threshold,
        "tta_passes": len(variants),
        "mask_pixels": int(mask.sum()),
        "mask_fraction": round(float(mask.mean()), 6),
        "seconds": round(elapsed, 4),
        "peak_cuda_memory_gib": (
            round(torch.cuda.max_memory_allocated() / 1024**3, 3)
            if device.type == "cuda"
            else None
        ),
    }
    (args.output / "result.json").write_text(
        json.dumps(stats, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(stats, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
