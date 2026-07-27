"""Run Koharu Text SAM-TS-L over comic folders and build review sheets."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import sys
import time
from collections import Counter
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import numpy as np
import torch
import torch.utils.checkpoint
from PIL import Image, ImageDraw, ImageOps
from safetensors.torch import load_file

ROOT = Path(__file__).resolve().parents[1]
IMAGE_SIZE = 1024
IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp", ".bmp", ".tif", ".tiff"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--input",
        action="append",
        required=True,
        metavar="NAME=PATH",
        help="Dataset name and image root; repeat for multiple folders",
    )
    parser.add_argument(
        "--output-root",
        type=Path,
        default=ROOT / "runs" / "koharu-text-sam-ts-l-domain-evaluation",
    )
    parser.add_argument(
        "--weights",
        type=Path,
        default=(
            ROOT
            / "runs"
            / "huggingface"
            / "koharu-text-sam-ts-l"
            / "model.safetensors"
        ),
    )
    parser.add_argument("--hi-sam-root", type=Path, default=ROOT / "temp" / "Hi-SAM")
    parser.add_argument("--batch-size", type=int, default=4)
    parser.add_argument("--review-pages", type=int, default=24)
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def natural_key(path: Path) -> list[str | int]:
    return [int(part) if part.isdigit() else part.casefold() for part in re.split(r"(\d+)", path.as_posix())]


def parse_inputs(values: list[str]) -> dict[str, Path]:
    result: dict[str, Path] = {}
    for value in values:
        if "=" not in value:
            raise ValueError(f"input must be NAME=PATH: {value}")
        name, raw_path = value.split("=", 1)
        if not name or name in result:
            raise ValueError(f"invalid or duplicate input name: {name}")
        path = Path(raw_path).resolve()
        if not path.is_dir():
            raise FileNotFoundError(path)
        result[name] = path
    return result


def install_checkpoint_compatibility() -> None:
    original = torch.utils.checkpoint.checkpoint

    def checkpoint(function: Any, *args: Any, **kwargs: Any) -> Any:
        kwargs.setdefault("use_reentrant", False)
        return original(function, *args, **kwargs)

    torch.utils.checkpoint.checkpoint = checkpoint


def prepare(path: Path) -> tuple[torch.Tensor, tuple[int, int], tuple[int, int]]:
    with Image.open(path) as source:
        image = source.convert("RGB")
        original_size = image.size
        scale = IMAGE_SIZE / max(image.size)
        resized_size = tuple(max(1, round(axis * scale)) for axis in image.size)
        resized = image.resize(resized_size, Image.Resampling.BILINEAR)
    canvas = Image.new("RGB", (IMAGE_SIZE, IMAGE_SIZE), (128, 128, 128))
    canvas.paste(resized, (0, 0))
    array = np.asarray(canvas, dtype=np.float32).copy()
    return torch.from_numpy(array).permute(2, 0, 1), resized_size, original_size


def mask_iou(left: np.ndarray, right: np.ndarray) -> float:
    union = np.logical_or(left, right).sum()
    return float(np.logical_and(left, right).sum() / max(union, 1))


def infer_dataset(
    model: torch.nn.Module,
    device: torch.device,
    name: str,
    source_root: Path,
    output_root: Path,
    batch_size: int,
) -> dict[str, Any]:
    files = sorted(
        (
            path
            for path in source_root.rglob("*")
            if path.is_file() and path.suffix.lower() in IMAGE_EXTENSIONS
        ),
        key=lambda path: natural_key(path.relative_to(source_root)),
    )
    if not files:
        raise ValueError(f"no images found under {source_root}")
    relative_masks = [path.relative_to(source_root).with_suffix(".png") for path in files]
    if len(set(relative_masks)) != len(relative_masks):
        raise ValueError(f"mask output collision under {source_root}")

    dataset_output = output_root / name
    masks_root = dataset_output / "masks"
    masks_root.mkdir(parents=True)
    records: list[dict[str, Any]] = []
    started = time.perf_counter()
    for offset in range(0, len(files), batch_size):
        batch_paths = files[offset : offset + batch_size]
        prepared = [prepare(path) for path in batch_paths]
        images = torch.stack([item[0] for item in prepared]).to(device)
        inputs = [
            {"image": image.contiguous(), "original_size": (IMAGE_SIZE, IMAGE_SIZE)}
            for image in images
        ]
        with torch.inference_mode(), torch.autocast(
            device_type="cuda", dtype=torch.bfloat16
        ):
            _, low, _, _, high, _ = model(inputs, multimask_output=False)
        low = low.bool().cpu().numpy()
        high = high.bool().cpu().numpy()
        for index, path in enumerate(batch_paths):
            resized_size = prepared[index][1]
            original_size = prepared[index][2]
            resized_width, resized_height = resized_size
            low_mask = low[index, 0, :resized_height, :resized_width]
            high_mask = high[index, 0, :resized_height, :resized_width]
            mask_image = Image.fromarray(high_mask.astype(np.uint8) * 255, mode="L")
            mask_image = mask_image.resize(original_size, Image.Resampling.NEAREST)
            relative = path.relative_to(source_root)
            relative_mask = relative.with_suffix(".png")
            destination = masks_root / relative_mask
            destination.parent.mkdir(parents=True, exist_ok=True)
            mask_image.save(destination)
            original_mask = np.asarray(mask_image) > 127
            records.append(
                {
                    "image": relative.as_posix(),
                    "mask": (Path("masks") / relative_mask).as_posix(),
                    "width": original_size[0],
                    "height": original_size[1],
                    "predicted_pixels": int(original_mask.sum()),
                    "predicted_ratio": float(original_mask.mean()),
                    "low_hr_iou_1024": mask_iou(low_mask, high_mask),
                }
            )
        completed = min(offset + batch_size, len(files))
        if completed == len(files) or completed % 100 < batch_size:
            elapsed = time.perf_counter() - started
            print(
                json.dumps(
                    {
                        "event": "progress",
                        "dataset": name,
                        "completed": completed,
                        "total": len(files),
                        "pages_per_second": completed / elapsed,
                    }
                ),
                flush=True,
            )

    elapsed = time.perf_counter() - started
    ratios = np.asarray([record["predicted_ratio"] for record in records])
    agreements = np.asarray([record["low_hr_iou_1024"] for record in records])
    summary = {
        "name": name,
        "source": str(source_root),
        "pages": len(records),
        "seconds": elapsed,
        "pages_per_second": len(records) / elapsed,
        "predicted_ratio": {
            "minimum": float(ratios.min()),
            "p10": float(np.quantile(ratios, 0.10)),
            "median": float(np.median(ratios)),
            "p90": float(np.quantile(ratios, 0.90)),
            "maximum": float(ratios.max()),
            "mean": float(ratios.mean()),
        },
        "low_hr_iou_1024": {
            "minimum": float(agreements.min()),
            "p10": float(np.quantile(agreements, 0.10)),
            "median": float(np.median(agreements)),
            "p90": float(np.quantile(agreements, 0.90)),
            "maximum": float(agreements.max()),
            "mean": float(agreements.mean()),
        },
        "records": records,
    }
    (dataset_output / "metrics.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    return summary


def select_review_records(records: list[dict[str, Any]], count: int) -> list[dict[str, Any]]:
    selected: dict[str, tuple[dict[str, Any], set[str]]] = {}

    def add(record: dict[str, Any], reason: str) -> None:
        image = record["image"]
        if image not in selected:
            selected[image] = (record, set())
        selected[image][1].add(reason)

    even_count = max(1, count // 2)
    for index in np.linspace(0, len(records) - 1, even_count, dtype=int):
        add(records[int(index)], "evenly_spaced")
    for record in sorted(records, key=lambda item: item["low_hr_iou_1024"]):
        add(record, "low_branch_agreement")
        if len(selected) >= count:
            break
    for record in sorted(records, key=lambda item: item["predicted_ratio"], reverse=True):
        add(record, "high_predicted_ratio")
        if len(selected) >= count + 2:
            break
    for record in sorted(records, key=lambda item: item["predicted_ratio"]):
        add(record, "low_predicted_ratio")
        if len(selected) >= count + 4:
            break

    values = list(selected.values())[:count]
    return [
        {**record, "review_reason": sorted(reasons)} for record, reasons in values
    ]


def overlay(source: Image.Image, mask: Image.Image) -> Image.Image:
    image = np.asarray(source.convert("RGB"), dtype=np.float32).copy()
    predicted = np.asarray(mask.convert("L")) > 127
    image[predicted] = image[predicted] * 0.30 + np.asarray((255, 30, 30)) * 0.70
    return Image.fromarray(np.clip(image, 0, 255).astype(np.uint8))


def fit_panel(image: Image.Image, width: int, height: int) -> Image.Image:
    contained = ImageOps.contain(image.convert("RGB"), (width, height), Image.Resampling.LANCZOS)
    panel = Image.new("RGB", (width, height), "white")
    panel.paste(contained, ((width - contained.width) // 2, (height - contained.height) // 2))
    return panel


def render_review(
    source_root: Path,
    dataset_output: Path,
    records: list[dict[str, Any]],
    count: int,
) -> list[dict[str, Any]]:
    selected = select_review_records(records, count)
    overlays_root = dataset_output / "review_overlays"
    overlays_root.mkdir()
    tile_width, tile_height, label_height = 480, 360, 48
    columns = 3
    rows = math.ceil(len(selected) / columns)
    sheet = Image.new(
        "RGB", (tile_width * columns, (tile_height + label_height) * rows), (225, 225, 225)
    )
    draw = ImageDraw.Draw(sheet)
    for index, record in enumerate(selected):
        source_path = source_root / record["image"]
        mask_path = dataset_output / record["mask"]
        with Image.open(source_path) as raw_source, Image.open(mask_path) as raw_mask:
            source = raw_source.convert("RGB")
            mask = raw_mask.convert("L")
            rendered = overlay(source, mask)
        safe_name = record["image"].replace("/", "__").replace("\\", "__")
        overlay_path = overlays_root / f"{Path(safe_name).stem}.jpg"
        review_copy = ImageOps.contain(rendered, (1600, 1600), Image.Resampling.LANCZOS)
        review_copy.save(overlay_path, quality=92)
        record["overlay"] = overlay_path.relative_to(dataset_output).as_posix()

        x = (index % columns) * tile_width
        y = (index // columns) * (tile_height + label_height)
        half = tile_width // 2
        sheet.paste(fit_panel(source, half, tile_height), (x, y + label_height))
        sheet.paste(fit_panel(rendered, half, tile_height), (x + half, y + label_height))
        label = (
            f"{record['image']}  mask={record['predicted_ratio']:.1%}  "
            f"agreement={record['low_hr_iou_1024']:.3f}"
        )
        draw.text((x + 6, y + 7), label, fill="black")
        draw.text((x + 6, y + 25), ", ".join(record["review_reason"]), fill=(60, 60, 60))
    sheet.save(dataset_output / "review_contact_sheet.jpg", quality=92)
    (dataset_output / "review_selection.json").write_text(
        json.dumps(selected, indent=2) + "\n", encoding="utf-8"
    )
    return selected


def main() -> None:
    args = parse_args()
    inputs = parse_inputs(args.input)
    args.output_root = args.output_root.resolve()
    args.weights = args.weights.resolve()
    args.hi_sam_root = args.hi_sam_root.resolve()
    if args.output_root.exists():
        raise FileExistsError(args.output_root)
    if not args.weights.is_file():
        raise FileNotFoundError(args.weights)
    if not (args.hi_sam_root / "hi_sam" / "modeling" / "build.py").is_file():
        raise FileNotFoundError(args.hi_sam_root)
    if args.batch_size < 1 or args.review_pages < 1:
        raise ValueError("batch size and review pages must be positive")
    if not torch.cuda.is_available():
        raise RuntimeError("CUDA is required")

    args.output_root.mkdir(parents=True)
    install_checkpoint_compatibility()
    sys.path.insert(0, str(args.hi_sam_root))
    from hi_sam.modeling.build import model_registry

    device = torch.device("cuda:0")
    model_args = SimpleNamespace(
        checkpoint=None,
        model_type="vit_l",
        attn_layers=1,
        prompt_len=12,
        hier_det=False,
    )
    model = model_registry["vit_l"](args=model_args)
    state = load_file(args.weights, device="cpu")
    model.load_state_dict(state, strict=True)
    model = model.eval().to(device)
    print(
        json.dumps(
            {
                "event": "model_ready",
                "device": torch.cuda.get_device_name(0),
                "weight_sha256": sha256(args.weights),
                "state_tensors": len(state),
            }
        ),
        flush=True,
    )

    summaries = {}
    for name, source_root in inputs.items():
        summary = infer_dataset(
            model,
            device,
            name,
            source_root,
            args.output_root,
            args.batch_size,
        )
        selected = render_review(
            source_root,
            args.output_root / name,
            summary["records"],
            args.review_pages,
        )
        summaries[name] = {
            key: value for key, value in summary.items() if key != "records"
        }
        summaries[name]["review_pages"] = len(selected)

    report = {
        "status": "complete",
        "model": "mayocream/koharu-text-sam-ts-l",
        "weights": str(args.weights),
        "weight_sha256": sha256(args.weights),
        "runtime": {
            "device": torch.cuda.get_device_name(0),
            "torch": torch.__version__,
            "cuda": torch.version.cuda,
            "amp_dtype": "bfloat16",
            "batch_size": args.batch_size,
        },
        "datasets": summaries,
    }
    (args.output_root / "summary.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    (args.output_root / "INFERENCE_COMPLETE.json").write_text(
        json.dumps({"status": "complete", "pages": sum(item["pages"] for item in summaries.values())}, indent=2)
        + "\n",
        encoding="utf-8",
    )
    print(json.dumps({"event": "complete", **report}), flush=True)


if __name__ == "__main__":
    main()
