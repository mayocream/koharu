#!/usr/bin/env python3
"""Compare old and newly fine-tuned RF-DETR typography masks pixel by pixel."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image, ImageDraw, ImageOps


ROOT = Path(__file__).resolve().parents[1]
DATASETS = ("bluearchive_comics", "marriagetoxin-chapter-1")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old-root", type=Path, required=True)
    parser.add_argument("--new-root", type=Path, required=True)
    parser.add_argument("--textseg-root", type=Path, required=True)
    parser.add_argument("--old-post-root", type=Path, required=True)
    parser.add_argument("--new-post-root", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--review-pages", type=int, default=12)
    return parser.parse_args()


def load_mask(path: Path) -> np.ndarray:
    with Image.open(path) as image:
        return np.asarray(image.convert("L")) > 127


def pixel_metrics(reference: np.ndarray, prediction: np.ndarray) -> dict[str, Any]:
    if reference.shape != prediction.shape:
        raise ValueError(f"mask shape mismatch: {reference.shape} != {prediction.shape}")
    both = int(np.count_nonzero(reference & prediction))
    reference_only = int(np.count_nonzero(reference & ~prediction))
    prediction_only = int(np.count_nonzero(~reference & prediction))
    neither = int(reference.size - both - reference_only - prediction_only)
    union = both + reference_only + prediction_only
    reference_pixels = both + reference_only
    prediction_pixels = both + prediction_only
    return {
        "pixels": int(reference.size),
        "reference_pixels": reference_pixels,
        "prediction_pixels": prediction_pixels,
        "intersection_pixels": both,
        "union_pixels": union,
        "reference_only_pixels": reference_only,
        "prediction_only_pixels": prediction_only,
        "neither_pixels": neither,
        "iou": float(both / union) if union else 1.0,
        "dice": float(2 * both / (reference_pixels + prediction_pixels))
        if reference_pixels + prediction_pixels
        else 1.0,
        "pixel_agreement": float((both + neither) / reference.size),
        "prediction_to_reference_area_ratio": float(
            prediction_pixels / reference_pixels
        )
        if reference_pixels
        else (1.0 if not prediction_pixels else None),
    }


def aggregate(records: list[dict[str, Any]], key: str) -> dict[str, Any]:
    count_fields = (
        "pixels",
        "reference_pixels",
        "prediction_pixels",
        "intersection_pixels",
        "union_pixels",
        "reference_only_pixels",
        "prediction_only_pixels",
        "neither_pixels",
    )
    totals = {
        field: sum(int(record[key][field]) for record in records)
        for field in count_fields
    }
    reference_pixels = totals["reference_pixels"]
    prediction_pixels = totals["prediction_pixels"]
    intersection = totals["intersection_pixels"]
    totals.update(
        {
            "iou": float(intersection / totals["union_pixels"])
            if totals["union_pixels"]
            else 1.0,
            "dice": float(2 * intersection / (reference_pixels + prediction_pixels))
            if reference_pixels + prediction_pixels
            else 1.0,
            "pixel_agreement": float(
                (intersection + totals["neither_pixels"]) / totals["pixels"]
            ),
            "prediction_to_reference_area_ratio": float(
                prediction_pixels / reference_pixels
            )
            if reference_pixels
            else (1.0 if not prediction_pixels else None),
        }
    )
    return totals


def summarize(values: list[float]) -> dict[str, float]:
    array = np.asarray(values, dtype=np.float64)
    return {
        "minimum": float(array.min()),
        "p10": float(np.quantile(array, 0.10)),
        "median": float(np.median(array)),
        "mean": float(array.mean()),
        "p90": float(np.quantile(array, 0.90)),
        "maximum": float(array.max()),
    }


def fit(image: Image.Image, width: int, height: int) -> Image.Image:
    canvas = Image.new("RGB", (width, height), (245, 245, 245))
    fitted = ImageOps.contain(image.convert("RGB"), (width, height), Image.Resampling.LANCZOS)
    canvas.paste(fitted, ((width - fitted.width) // 2, (height - fitted.height) // 2))
    return canvas


def mask_preview(mask: np.ndarray) -> Image.Image:
    result = np.zeros((*mask.shape, 3), dtype=np.uint8)
    result[mask] = 255
    return Image.fromarray(result, mode="RGB")


def version_map(old: np.ndarray, new: np.ndarray) -> Image.Image:
    result = np.zeros((*old.shape, 3), dtype=np.uint8)
    result[old & new] = (255, 255, 255)
    result[old & ~new] = (255, 150, 0)
    result[~old & new] = (0, 220, 255)
    return Image.fromarray(result, mode="RGB")


def render_review(
    dataset: str,
    source_root: Path,
    output: Path,
    selected: list[dict[str, Any]],
) -> None:
    review_root = output / dataset / "review_pages"
    review_root.mkdir(parents=True, exist_ok=True)
    columns = 5
    cell_width, image_height, header = 300, 440, 70
    sheet = Image.new(
        "RGB",
        (columns * cell_width, len(selected) * (image_height + header)),
        (225, 225, 225),
    )
    sheet_draw = ImageDraw.Draw(sheet)
    for row, record in enumerate(selected):
        with Image.open(source_root / record["image"]) as source_image:
            source = source_image.convert("RGB")
        sam = load_mask(Path(record["sam_path"]))
        old = load_mask(Path(record["old_path"]))
        new = load_mask(Path(record["new_path"]))
        panels = [source, mask_preview(sam), mask_preview(old), mask_preview(new), version_map(old, new)]
        labels = ["source", "TextSeg", "old RF-DETR", "new RF-DETR", "old/new diff"]
        y = row * (image_height + header)
        for column, (panel, label) in enumerate(zip(panels, labels, strict=True)):
            x = column * cell_width
            sheet.paste(fit(panel, cell_width, image_height), (x, y + header))
            sheet_draw.text((x + 6, y + 48), label, fill=(20, 20, 20))
        sheet_draw.text(
            (6, y + 5),
            f"{record['image']}  TextSeg IoU old={record['old_vs_textseg']['iou']:.3f} "
            f"new={record['new_vs_textseg']['iou']:.3f}  delta={record['iou_delta']:+.3f}",
            fill=(20, 20, 20),
        )
        sheet_draw.text(
            (6, y + 25),
            "diff: white=both  orange=old only  cyan=new only",
            fill=(45, 45, 45),
        )

        page = Image.new("RGB", (columns * 500, 820), (235, 235, 235))
        page_draw = ImageDraw.Draw(page)
        page_draw.text(
            (10, 8),
            f"{dataset}/{record['image']}  old IoU={record['old_vs_textseg']['iou']:.3f}  "
            f"new IoU={record['new_vs_textseg']['iou']:.3f}  delta={record['iou_delta']:+.3f}",
            fill=(20, 20, 20),
        )
        page_draw.text(
            (10, 30),
            "source | TextSeg | old RF-DETR | new RF-DETR | white=both, orange=old only, cyan=new only",
            fill=(45, 45, 45),
        )
        for column, panel in enumerate(panels):
            page.paste(fit(panel, 500, 760), (column * 500, 60))
        destination = review_root / f"{Path(record['image']).stem}.jpg"
        page.save(destination, quality=94, subsampling=0)
        record["review_image"] = destination.relative_to(output).as_posix()
    sheet.save(output / dataset / "review_contact_sheet.jpg", quality=94, subsampling=0)


def select_reviews(records: list[dict[str, Any]], count: int) -> list[dict[str, Any]]:
    selected: dict[str, dict[str, Any]] = {}
    group = max(2, count // 3)
    for record in sorted(records, key=lambda item: item["iou_delta"], reverse=True)[:group]:
        selected[record["image"]] = record
    for record in sorted(records, key=lambda item: item["iou_delta"])[:group]:
        selected[record["image"]] = record
    for record in sorted(
        records, key=lambda item: item["new_vs_old"]["iou"]
    )[:group]:
        selected[record["image"]] = record
    return list(selected.values())[:count]


def process_dataset(args: argparse.Namespace, dataset: str) -> dict[str, Any]:
    old_metrics = json.loads((args.old_root / dataset / "metrics.json").read_text(encoding="utf-8"))
    new_metrics = json.loads((args.new_root / dataset / "metrics.json").read_text(encoding="utf-8"))
    old_by_image = {
        item["image"]: item
        for item in old_metrics["records"]
        if Path(item["image"]).parent == Path(".")
    }
    new_by_image = {item["image"]: item for item in new_metrics["records"]}
    if old_by_image.keys() != new_by_image.keys():
        raise ValueError(f"image-set mismatch for {dataset}")
    source_root = Path(new_metrics["source"])
    records: list[dict[str, Any]] = []
    for image_name in sorted(new_by_image):
        old_item = old_by_image[image_name]
        new_item = new_by_image[image_name]
        sam_path = args.textseg_root / dataset / new_item["sam_mask"]
        old_path = args.old_root / dataset / old_item["rfdetr_typography_mask"]
        new_path = args.new_root / dataset / new_item["rfdetr_typography_mask"]
        relative = Path(image_name).with_suffix(".png")
        old_post_path = args.old_post_root / dataset / "rfdetr_processed_masks" / relative
        new_post_path = args.new_post_root / dataset / "rfdetr_processed_masks" / relative
        sam = load_mask(sam_path)
        old = load_mask(old_path)
        new = load_mask(new_path)
        old_vs_textseg = pixel_metrics(sam, old)
        new_vs_textseg = pixel_metrics(sam, new)
        records.append(
            {
                "image": image_name,
                "sam_path": str(sam_path),
                "old_path": str(old_path),
                "new_path": str(new_path),
                "old_vs_textseg": old_vs_textseg,
                "new_vs_textseg": new_vs_textseg,
                "new_vs_old": pixel_metrics(old, new),
                "processed_new_vs_old": pixel_metrics(
                    load_mask(old_post_path), load_mask(new_post_path)
                ),
                "iou_delta": new_vs_textseg["iou"] - old_vs_textseg["iou"],
                "dice_delta": new_vs_textseg["dice"] - old_vs_textseg["dice"],
            }
        )
    deltas = [record["iou_delta"] for record in records]
    epsilon = 1e-12
    selected = select_reviews(records, args.review_pages)
    dataset_output = args.output_root / dataset
    dataset_output.mkdir(parents=True, exist_ok=True)
    render_review(dataset, source_root, args.output_root, selected)
    summary = {
        "dataset": dataset,
        "pages": len(records),
        "old_vs_textseg": aggregate(records, "old_vs_textseg"),
        "new_vs_textseg": aggregate(records, "new_vs_textseg"),
        "new_vs_old": aggregate(records, "new_vs_old"),
        "processed_new_vs_old": aggregate(records, "processed_new_vs_old"),
        "per_page_iou_delta": summarize(deltas),
        "improved_pages": sum(delta > epsilon for delta in deltas),
        "regressed_pages": sum(delta < -epsilon for delta in deltas),
        "tied_pages": sum(abs(delta) <= epsilon for delta in deltas),
        "review_pages": [record["image"] for record in selected],
        "records": records,
    }
    (dataset_output / "metrics.json").write_text(
        json.dumps(summary, indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )
    return summary


def main() -> None:
    args = parse_args()
    for name in (
        "old_root",
        "new_root",
        "textseg_root",
        "old_post_root",
        "new_post_root",
        "output_root",
    ):
        setattr(args, name, getattr(args, name).resolve())
    if args.output_root.exists():
        raise FileExistsError(args.output_root)
    if args.review_pages < 1:
        raise ValueError("review-pages must be positive")
    args.output_root.mkdir(parents=True)
    summaries = {dataset: process_dataset(args, dataset) for dataset in DATASETS}
    compact = {
        dataset: {key: value for key, value in summary.items() if key != "records"}
        for dataset, summary in summaries.items()
    }
    report = {
        "status": "complete",
        "scope": "top-level source images only",
        "legend": {
            "white": "old and new RF-DETR",
            "orange": "old RF-DETR only",
            "cyan": "new RF-DETR only",
            "black": "neither RF-DETR",
        },
        "datasets": compact,
    }
    (args.output_root / "summary.json").write_text(
        json.dumps(report, indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )
    print(json.dumps(report, allow_nan=False))


if __name__ == "__main__":
    main()
