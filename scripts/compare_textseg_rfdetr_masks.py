#!/usr/bin/env python3
"""Run KoharuLayout RF-DETR and compare its typography masks with TextSeg."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import time
from collections import Counter
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image, ImageDraw, ImageOps


ROOT = Path(__file__).resolve().parents[1]
MODEL_ID = "mayocream/koharu-layout-rfdetr-seg-2xl-1152"
MODEL_REVISION = "2b564b5189965423578212285dffa5e770819caf"
TEXT_CLASS = 0
COO_CLASS = 1
TEXT_THRESHOLD = 0.25
COO_THRESHOLD = 0.40


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--textseg-root",
        type=Path,
        default=ROOT / "runs" / "koharu-text-sam-ts-l-domain-evaluation",
    )
    parser.add_argument(
        "--model-root",
        type=Path,
        default=ROOT / "runs" / "hf-koharu-layout-rfdetr-seg-2xl-1152",
    )
    parser.add_argument(
        "--checkpoint",
        type=Path,
        help="Load an RF-DETR Seg 2XL Lightning checkpoint instead of model-root.",
    )
    parser.add_argument(
        "--output-root",
        type=Path,
        default=(
            ROOT
            / "runs"
            / "koharu-text-sam-ts-l-vs-layout-rfdetr-seg-2xl-1152"
        ),
    )
    parser.add_argument("--batch-size", type=int, default=2)
    parser.add_argument("--review-pages", type=int, default=24)
    parser.add_argument("--text-threshold", type=float, default=TEXT_THRESHOLD)
    parser.add_argument(
        "--onomatopoeia-threshold", type=float, default=COO_THRESHOLD
    )
    parser.add_argument(
        "--top-level-only",
        action="store_true",
        help="Exclude images stored in subfolders of each source dataset.",
    )
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_model(model_root: Path, checkpoint: Path | None = None) -> Any:
    if checkpoint is not None:
        from rfdetr import RFDETRSeg2XLarge

        return RFDETRSeg2XLarge(
            pretrain_weights=str(checkpoint),
            resolution=1152,
            num_select=160,
        )
    loader_path = model_root / "load_model.py"
    weights_path = model_root / "model.safetensors"
    if not loader_path.is_file() or not weights_path.is_file():
        raise FileNotFoundError(f"incomplete model directory: {model_root}")
    spec = importlib.util.spec_from_file_location("koharu_layout_loader", loader_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load {loader_path}")
    loader = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loader)
    return loader.load_model(weights_path)


def comparison_counts(
    sam: np.ndarray, rfdetr: np.ndarray
) -> dict[str, int | float | None]:
    if sam.shape != rfdetr.shape:
        raise ValueError(f"mask shape mismatch: {sam.shape} != {rfdetr.shape}")
    both = int(np.count_nonzero(sam & rfdetr))
    sam_only = int(np.count_nonzero(sam & ~rfdetr))
    rfdetr_only = int(np.count_nonzero(~sam & rfdetr))
    neither = int(sam.size - both - sam_only - rfdetr_only)
    union = both + sam_only + rfdetr_only
    sam_pixels = both + sam_only
    rfdetr_pixels = both + rfdetr_only
    return {
        "pixels": int(sam.size),
        "sam_pixels": sam_pixels,
        "rfdetr_pixels": rfdetr_pixels,
        "intersection_pixels": both,
        "union_pixels": union,
        "sam_only_pixels": sam_only,
        "rfdetr_only_pixels": rfdetr_only,
        "neither_pixels": neither,
        "iou": float(both / union) if union else 1.0,
        "dice": float(2 * both / (sam_pixels + rfdetr_pixels))
        if sam_pixels + rfdetr_pixels
        else 1.0,
        "pixel_agreement": float((both + neither) / sam.size),
        "sam_ratio": float(sam_pixels / sam.size),
        "rfdetr_ratio": float(rfdetr_pixels / sam.size),
        "sam_only_ratio": float(sam_only / sam.size),
        "rfdetr_only_ratio": float(rfdetr_only / sam.size),
        "rfdetr_to_sam_area_ratio": float(rfdetr_pixels / sam_pixels)
        if sam_pixels
        else (1.0 if not rfdetr_pixels else None),
    }


def save_binary(mask: np.ndarray, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(mask.astype(np.uint8) * 255, mode="L").save(destination)


def save_comparison_map(
    sam: np.ndarray, rfdetr: np.ndarray, destination: Path
) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    result = np.zeros((*sam.shape, 3), dtype=np.uint8)
    result[sam & rfdetr] = (255, 255, 255)
    result[sam & ~rfdetr] = (255, 40, 40)
    result[~sam & rfdetr] = (0, 220, 255)
    Image.fromarray(result, mode="RGB").save(destination)


def summarize_values(values: list[float]) -> dict[str, float]:
    array = np.asarray(values, dtype=np.float64)
    return {
        "minimum": float(array.min()),
        "p10": float(np.quantile(array, 0.10)),
        "median": float(np.median(array)),
        "mean": float(array.mean()),
        "p90": float(np.quantile(array, 0.90)),
        "maximum": float(array.max()),
    }


def aggregate(records: list[dict[str, Any]], key: str) -> dict[str, Any]:
    fields = (
        "pixels",
        "sam_pixels",
        "rfdetr_pixels",
        "intersection_pixels",
        "union_pixels",
        "sam_only_pixels",
        "rfdetr_only_pixels",
        "neither_pixels",
    )
    totals = {
        field: sum(int(record[key][field]) for record in records) for field in fields
    }
    sam_pixels = totals["sam_pixels"]
    rfdetr_pixels = totals["rfdetr_pixels"]
    union = totals["union_pixels"]
    both = totals["intersection_pixels"]
    pixels = totals["pixels"]
    totals.update(
        {
            "iou": float(both / union) if union else 1.0,
            "dice": float(2 * both / (sam_pixels + rfdetr_pixels))
            if sam_pixels + rfdetr_pixels
            else 1.0,
            "pixel_agreement": float(
                (both + totals["neither_pixels"]) / pixels
            ),
            "sam_ratio": float(sam_pixels / pixels),
            "rfdetr_ratio": float(rfdetr_pixels / pixels),
            "sam_only_ratio": float(totals["sam_only_pixels"] / pixels),
            "rfdetr_only_ratio": float(totals["rfdetr_only_pixels"] / pixels),
            "rfdetr_to_sam_area_ratio": float(rfdetr_pixels / sam_pixels)
            if sam_pixels
            else (1.0 if not rfdetr_pixels else None),
        }
    )
    return {
        "pages": len(records),
        "micro_pixel_counts": totals,
        "per_page_iou": summarize_values([record[key]["iou"] for record in records]),
        "per_page_dice": summarize_values(
            [record[key]["dice"] for record in records]
        ),
        "per_page_pixel_agreement": summarize_values(
            [record[key]["pixel_agreement"] for record in records]
        ),
    }


def subgroup(image: str) -> str:
    parts = Path(image).parts
    return "source_pages" if len(parts) == 1 else parts[0]


def select_review_records(
    records: list[dict[str, Any]], count: int
) -> list[dict[str, Any]]:
    source_records = [record for record in records if subgroup(record["image"]) == "source_pages"]
    selected: dict[str, tuple[dict[str, Any], set[str]]] = {}

    def add(record: dict[str, Any], reason: str) -> None:
        if record["image"] not in selected:
            selected[record["image"]] = (record, set())
        selected[record["image"]][1].add(reason)

    for index in np.linspace(0, len(source_records) - 1, max(1, count // 2), dtype=int):
        add(source_records[int(index)], "evenly_spaced")
    rankings = (
        (lambda item: item["typography"]["iou"], "lowest_iou"),
        (lambda item: -item["typography"]["sam_only_ratio"], "most_sam_only"),
        (lambda item: -item["typography"]["rfdetr_only_ratio"], "most_rfdetr_only"),
    )
    for ranking, reason in rankings:
        for record in sorted(source_records, key=ranking)[: max(2, count // 6)]:
            add(record, reason)
    return [
        {**record, "review_reason": sorted(reasons)}
        for record, reasons in list(selected.values())[:count]
    ]


def comparison_overlay(
    source: Image.Image, sam: np.ndarray, rfdetr: np.ndarray
) -> Image.Image:
    image = np.asarray(source.convert("RGB"), dtype=np.float32).copy()
    colors = (
        (sam & rfdetr, np.asarray((60, 255, 80), dtype=np.float32)),
        (sam & ~rfdetr, np.asarray((255, 35, 35), dtype=np.float32)),
        (~sam & rfdetr, np.asarray((0, 225, 255), dtype=np.float32)),
    )
    for mask, color in colors:
        image[mask] = image[mask] * 0.25 + color * 0.75
    return Image.fromarray(np.clip(image, 0, 255).astype(np.uint8), mode="RGB")


def fit(image: Image.Image, width: int, height: int) -> Image.Image:
    result = Image.new("RGB", (width, height), "white")
    contained = ImageOps.contain(
        image.convert("RGB"), (width, height), Image.Resampling.LANCZOS
    )
    result.paste(
        contained,
        ((width - contained.width) // 2, (height - contained.height) // 2),
    )
    return result


def render_review(
    source_root: Path,
    textseg_dataset_root: Path,
    output: Path,
    selected: list[dict[str, Any]],
) -> None:
    overlays_root = output / "review_overlays"
    overlays_root.mkdir(parents=True, exist_ok=True)
    cell_width, image_height, label_height = 420, 500, 68
    columns = 4
    rows = math.ceil(len(selected) / columns)
    sheet = Image.new(
        "RGB", (columns * cell_width, rows * (image_height + label_height)), (225, 225, 225)
    )
    draw = ImageDraw.Draw(sheet)
    for index, record in enumerate(selected):
        image_path = source_root / record["image"]
        sam_path = textseg_dataset_root / record["sam_mask"]
        rfdetr_path = output / record["rfdetr_typography_mask"]
        with (
            Image.open(image_path) as raw_source,
            Image.open(sam_path) as raw_sam,
            Image.open(rfdetr_path) as raw_rfdetr,
        ):
            source = raw_source.convert("RGB")
            sam = np.asarray(raw_sam.convert("L")) > 127
            rfdetr = np.asarray(raw_rfdetr.convert("L")) > 127
            rendered = comparison_overlay(source, sam, rfdetr)
        safe_name = record["image"].replace("/", "__").replace("\\", "__")
        overlay_path = overlays_root / f"{Path(safe_name).stem}.jpg"
        ImageOps.contain(rendered, (1800, 1800), Image.Resampling.LANCZOS).save(
            overlay_path, quality=94, subsampling=0
        )
        record["review_overlay"] = overlay_path.relative_to(output).as_posix()
        x = (index % columns) * cell_width
        y = (index // columns) * (image_height + label_height)
        sheet.paste(fit(source, cell_width // 2, image_height), (x, y + label_height))
        sheet.paste(
            fit(rendered, cell_width // 2, image_height),
            (x + cell_width // 2, y + label_height),
        )
        metrics = record["typography"]
        draw.text(
            (x + 5, y + 4),
            f"{record['image']}  IoU={metrics['iou']:.3f}  Dice={metrics['dice']:.3f}",
            fill="black",
        )
        draw.text(
            (x + 5, y + 23),
            f"SAM-only={metrics['sam_only_ratio']:.2%}  RF-only={metrics['rfdetr_only_ratio']:.2%}",
            fill=(35, 35, 35),
        )
        draw.text(
            (x + 5, y + 42),
            "green=both  red=SAM only  cyan=RF-DETR only",
            fill=(35, 35, 35),
        )
    sheet.save(output / "review_contact_sheet.jpg", quality=94, subsampling=0)
    (output / "review_selection.json").write_text(
        json.dumps(selected, indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )


def infer_dataset(
    model: Any,
    dataset_name: str,
    textseg_root: Path,
    output_root: Path,
    batch_size: int,
    review_pages: int,
    top_level_only: bool,
    text_threshold: float,
    onomatopoeia_threshold: float,
) -> dict[str, Any]:
    textseg_dataset_root = textseg_root / dataset_name
    textseg_metrics_path = textseg_dataset_root / "metrics.json"
    metrics = json.loads(textseg_metrics_path.read_text(encoding="utf-8"))
    source_root = Path(metrics["source"])
    manifest = metrics["records"]
    if top_level_only:
        manifest = [
            item for item in manifest if Path(item["image"]).parent == Path(".")
        ]
    output = output_root / dataset_name
    output.mkdir(parents=True, exist_ok=True)
    records: list[dict[str, Any]] = []
    started = time.perf_counter()

    for offset in range(0, len(manifest), batch_size):
        batch_manifest = manifest[offset : offset + batch_size]
        images: list[Image.Image] = []
        for item in batch_manifest:
            with Image.open(source_root / item["image"]) as image:
                images.append(image.convert("RGB"))
        detections_batch = model.predict(
            images,
            threshold=min(text_threshold, onomatopoeia_threshold),
            shape=(1152, 1152),
            include_source_image=False,
        )
        if not isinstance(detections_batch, list):
            detections_batch = [detections_batch]

        for item, source, detections in zip(
            batch_manifest, images, detections_batch, strict=True
        ):
            width, height = source.size
            text_mask = np.zeros((height, width), dtype=bool)
            coo_mask = np.zeros((height, width), dtype=bool)
            counts: Counter[int] = Counter()
            for class_id, score, instance_mask in zip(
                detections.class_id,
                detections.confidence,
                detections.mask,
                strict=True,
            ):
                class_id = int(class_id)
                score = float(score)
                counts[class_id] += 1
                if class_id == TEXT_CLASS and score >= text_threshold:
                    text_mask |= instance_mask
                elif class_id == COO_CLASS and score >= onomatopoeia_threshold:
                    coo_mask |= instance_mask
            typography_mask = text_mask | coo_mask
            relative = Path(item["image"]).with_suffix(".png")
            text_destination = Path("rfdetr_text_masks") / relative
            coo_destination = Path("rfdetr_onomatopoeia_masks") / relative
            typography_destination = Path("rfdetr_typography_masks") / relative
            comparison_destination = Path("comparison_maps") / relative
            save_binary(text_mask, output / text_destination)
            save_binary(coo_mask, output / coo_destination)
            save_binary(typography_mask, output / typography_destination)

            with Image.open(textseg_dataset_root / item["mask"]) as sam_image:
                sam = np.asarray(sam_image.convert("L")) > 127
            save_comparison_map(sam, typography_mask, output / comparison_destination)
            records.append(
                {
                    "image": item["image"],
                    "sam_mask": item["mask"],
                    "rfdetr_text_mask": text_destination.as_posix(),
                    "rfdetr_onomatopoeia_mask": coo_destination.as_posix(),
                    "rfdetr_typography_mask": typography_destination.as_posix(),
                    "comparison_map": comparison_destination.as_posix(),
                    "width": width,
                    "height": height,
                    "detections": {
                        "text": counts[TEXT_CLASS],
                        "onomatopoeia": counts[COO_CLASS],
                        "all_classes_at_predict_threshold": len(detections),
                    },
                    "text_only": comparison_counts(sam, text_mask),
                    "typography": comparison_counts(sam, typography_mask),
                }
            )
        completed = min(offset + batch_size, len(manifest))
        if completed == len(manifest) or completed % 20 < batch_size:
            elapsed = time.perf_counter() - started
            print(
                json.dumps(
                    {
                        "event": "progress",
                        "dataset": dataset_name,
                        "completed": completed,
                        "total": len(manifest),
                        "pages_per_second": completed / elapsed,
                    }
                ),
                flush=True,
            )

    groups: dict[str, list[dict[str, Any]]] = {}
    for record in records:
        groups.setdefault(subgroup(record["image"]), []).append(record)
    summary = {
        "name": dataset_name,
        "source": str(source_root),
        "pages": len(records),
        "top_level_only": top_level_only,
        "seconds": time.perf_counter() - started,
        "thresholds": {
            "text": text_threshold,
            "onomatopoeia": onomatopoeia_threshold,
        },
        "comparison_target": "TextSeg vs RF-DETR union(text, onomatopoeia)",
        "all_pages": {
            "text_only": aggregate(records, "text_only"),
            "typography": aggregate(records, "typography"),
        },
        "groups": {
            name: {
                "text_only": aggregate(group_records, "text_only"),
                "typography": aggregate(group_records, "typography"),
            }
            for name, group_records in groups.items()
        },
        "records": records,
    }
    summary["pages_per_second"] = summary["pages"] / summary["seconds"]
    selected = select_review_records(records, review_pages)
    render_review(source_root, textseg_dataset_root, output, selected)
    summary["review_pages"] = len(selected)
    (output / "metrics.json").write_text(
        json.dumps(summary, indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )
    return summary


def main() -> None:
    args = parse_args()
    textseg_root = args.textseg_root.resolve()
    model_root = args.model_root.resolve()
    output_root = args.output_root.resolve()
    if output_root.exists():
        raise FileExistsError(output_root)
    if args.batch_size < 1 or args.review_pages < 1:
        raise ValueError("batch size and review pages must be positive")
    if not 0.0 <= args.text_threshold <= 1.0:
        raise ValueError("text threshold must be between 0 and 1")
    if not 0.0 <= args.onomatopoeia_threshold <= 1.0:
        raise ValueError("onomatopoeia threshold must be between 0 and 1")
    output_root.mkdir(parents=True)

    weights = (
        args.checkpoint.resolve()
        if args.checkpoint is not None
        else model_root / "model.safetensors"
    )
    if not weights.is_file():
        raise FileNotFoundError(weights)
    weight_hash = sha256(weights)
    model = load_model(model_root, weights if args.checkpoint is not None else None)
    summaries = {}
    for dataset_name in ("bluearchive_comics", "marriagetoxin-chapter-1"):
        summary = infer_dataset(
            model,
            dataset_name,
            textseg_root,
            output_root,
            args.batch_size,
            args.review_pages,
            args.top_level_only,
            args.text_threshold,
            args.onomatopoeia_threshold,
        )
        summaries[dataset_name] = {
            key: value for key, value in summary.items() if key != "records"
        }

    report = {
        "status": "complete",
        "models": {
            "sam": "mayocream/koharu-text-sam-ts-l",
            "rfdetr": MODEL_ID if args.checkpoint is None else "local RF-DETR Seg 2XL checkpoint",
            "rfdetr_revision": MODEL_REVISION if args.checkpoint is None else None,
            "rfdetr_weights": str(weights),
            "rfdetr_weight_sha256": weight_hash,
        },
        "comparison": {
            "primary": (
                "SAM mask vs union of RF-DETR "
                f"text@{args.text_threshold:g} and "
                f"onomatopoeia@{args.onomatopoeia_threshold:g}"
            ),
            "secondary": f"SAM mask vs RF-DETR text@{args.text_threshold:g} only",
            "map_legend": {
                "black": "neither model",
                "white": "both models",
                "red": "SAM only",
                "cyan": "RF-DETR only",
            },
            "ground_truth": False,
            "top_level_only": args.top_level_only,
        },
        "datasets": summaries,
    }
    (output_root / "summary.json").write_text(
        json.dumps(report, indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )
    (output_root / "INFERENCE_COMPLETE.json").write_text(
        json.dumps(
            {
                "status": "complete",
                "pages": sum(summary["pages"] for summary in summaries.values()),
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    print(json.dumps({"event": "complete", **report}, allow_nan=False), flush=True)


if __name__ == "__main__":
    main()
