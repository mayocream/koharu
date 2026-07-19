# /// script
# requires-python = ">=3.11,<3.15"
# dependencies = [
#   "ultralytics==8.4.43",
# ]
# ///
"""Fine-tune the ShadowB YOLO26s comic-mask checkpoint.

The recipe follows the hyperparameters recorded in the upstream checkpoint,
with only throughput settings (batch size, workers, and cache) exposed for the
target machine.
"""

from __future__ import annotations

import argparse
from pathlib import Path

from ultralytics import YOLO


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MODEL = (
    REPOSITORY_ROOT / "data" / "models" / "shadowb-comic-mask-yolo26s" / "best.pt"
)
DEFAULT_DATA = (
    REPOSITORY_ROOT / "data" / "datasets" / "comic-mask-yolo26s" / "dataset.yaml"
)
DEFAULT_PROJECT = REPOSITORY_ROOT / "runs" / "comic-mask-yolo26s-finetune"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, default=DEFAULT_MODEL)
    parser.add_argument("--data", type=Path, default=DEFAULT_DATA)
    parser.add_argument("--project", type=Path, default=DEFAULT_PROJECT)
    parser.add_argument("--name", default="yolo26s-mangaseg-sfx")
    parser.add_argument("--epochs", type=int, default=41)
    parser.add_argument("--imgsz", type=int, default=1280)
    parser.add_argument(
        "--batch",
        type=float,
        default=0.8,
        help="Fixed integer-valued batch or fraction of GPU memory in (0, 1).",
    )
    parser.add_argument("--workers", type=int, default=16)
    parser.add_argument("--device", default="0")
    parser.add_argument("--cache", choices=("ram", "disk", "none"), default="ram")
    parser.add_argument("--fraction", type=float, default=1.0)
    parser.add_argument("--resume", type=Path)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.resume:
        YOLO(args.resume).train(resume=True)
        return

    cache: str | bool = False if args.cache == "none" else args.cache
    batch = (
        int(args.batch) if args.batch >= 1 and args.batch.is_integer() else args.batch
    )
    model = YOLO(args.model)
    model.train(
        data=args.data,
        project=args.project,
        name=args.name,
        exist_ok=False,
        epochs=args.epochs,
        patience=30,
        imgsz=args.imgsz,
        batch=batch,
        device=args.device,
        workers=args.workers,
        cache=cache,
        fraction=args.fraction,
        optimizer="MuSGD",
        lr0=0.01,
        lrf=0.01,
        momentum=0.937,
        weight_decay=0.0005,
        warmup_epochs=3.0,
        warmup_momentum=0.8,
        warmup_bias_lr=0.1,
        cos_lr=True,
        close_mosaic=10,
        mosaic=0.3,
        mixup=0.0,
        copy_paste=0.1,
        copy_paste_mode="flip",
        hsv_h=0.0,
        hsv_s=0.0,
        hsv_v=0.04,
        degrees=0.0,
        translate=0.03,
        scale=0.15,
        shear=1.0,
        perspective=0.0002,
        flipud=0.0,
        fliplr=0.0,
        bgr=0.0,
        multi_scale=0.0,
        rect=False,
        amp=True,
        deterministic=True,
        seed=42,
        overlap_mask=False,
        mask_ratio=4,
        save=True,
        save_period=5,
        plots=True,
    )


if __name__ == "__main__":
    main()
