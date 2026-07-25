#!/usr/bin/env python3
"""Postprocess TextSeg and RF-DETR masks and render white pre-inpainting inputs."""

from __future__ import annotations

import argparse
import json
import math
import time
from pathlib import Path
from typing import Any

import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageOps


ROOT = Path(__file__).resolve().parents[1]
REFERENCE_LONG_SIDE = 1024
DEFAULT_DILATION_RADIUS = 4


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--comparison-root",
        type=Path,
        default=(
            ROOT
            / "runs"
            / "koharu-text-sam-ts-l-vs-layout-rfdetr-seg-2xl-1152"
        ),
    )
    parser.add_argument(
        "--textseg-root",
        type=Path,
        default=ROOT / "runs" / "koharu-text-sam-ts-l-domain-evaluation",
    )
    parser.add_argument(
        "--output-root",
        type=Path,
        default=(
            ROOT
            / "runs"
            / "koharu-text-sam-ts-l-vs-layout-rfdetr-preinpainting"
        ),
    )
    parser.add_argument(
        "--dilation-radius",
        type=int,
        default=DEFAULT_DILATION_RADIUS,
        help="Elliptical dilation radius at a 1024 px image long side",
    )
    parser.add_argument("--review-pages", type=int, default=24)
    return parser.parse_args()


def fill_holes(mask: np.ndarray) -> np.ndarray:
    """Fill background components that do not touch the image border."""
    padded = np.pad(mask.astype(np.uint8) * 255, 1)
    exterior = padded.copy()
    flood_mask = np.zeros(
        (exterior.shape[0] + 2, exterior.shape[1] + 2), dtype=np.uint8
    )
    cv2.floodFill(exterior, flood_mask, (0, 0), 255)
    holes = cv2.bitwise_not(exterior)
    return (cv2.bitwise_or(padded, holes)[1:-1, 1:-1] > 0)


def postprocess(
    mask: np.ndarray, width: int, height: int, radius_at_reference: int
) -> tuple[np.ndarray, dict[str, int | float]]:
    radius = max(
        1,
        int(round(radius_at_reference * max(width, height) / REFERENCE_LONG_SIDE)),
    )
    kernel = cv2.getStructuringElement(
        cv2.MORPH_ELLIPSE, (2 * radius + 1, 2 * radius + 1)
    )
    dilated = cv2.dilate(mask.astype(np.uint8), kernel, iterations=1) > 0
    processed = fill_holes(dilated)
    raw_pixels = int(mask.sum())
    dilated_pixels = int(dilated.sum())
    processed_pixels = int(processed.sum())
    return processed, {
        "dilation_radius_pixels": radius,
        "raw_pixels": raw_pixels,
        "dilated_pixels": dilated_pixels,
        "hole_fill_added_pixels": processed_pixels - dilated_pixels,
        "processed_pixels": processed_pixels,
        "raw_ratio": float(raw_pixels / mask.size),
        "processed_ratio": float(processed_pixels / mask.size),
        "processed_to_raw_area_ratio": float(processed_pixels / raw_pixels)
        if raw_pixels
        else (1.0 if not processed_pixels else None),
    }


def save_mask(mask: np.ndarray, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(mask.astype(np.uint8) * 255, mode="L").save(destination)


def save_white_painted(
    source: np.ndarray, mask: np.ndarray, destination: Path
) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    painted = source.copy()
    painted[mask] = 255
    Image.fromarray(painted, mode="RGB").save(destination, compress_level=4)


def coverage_metrics(
    sam_raw: np.ndarray,
    rfdetr_raw: np.ndarray,
    sam_processed: np.ndarray,
    rfdetr_processed: np.ndarray,
) -> dict[str, int | float]:
    raw_union = sam_raw | rfdetr_raw
    processed_union = sam_processed | rfdetr_processed
    processed_intersection = sam_processed & rfdetr_processed
    sam_peer_covered = int((sam_processed & rfdetr_raw).sum())
    rfdetr_peer_covered = int((rfdetr_processed & sam_raw).sum())
    sam_union_covered = int((sam_processed & raw_union).sum())
    rfdetr_union_covered = int((rfdetr_processed & raw_union).sum())
    raw_union_pixels = int(raw_union.sum())
    processed_union_pixels = int(processed_union.sum())
    processed_intersection_pixels = int(processed_intersection.sum())
    sam_processed_pixels = int(sam_processed.sum())
    rfdetr_processed_pixels = int(rfdetr_processed.sum())
    return {
        "pixels": int(raw_union.size),
        "raw_union_pixels": raw_union_pixels,
        "processed_union_pixels": processed_union_pixels,
        "processed_intersection_pixels": processed_intersection_pixels,
        "processed_iou": float(
            processed_intersection_pixels / processed_union_pixels
        )
        if processed_union_pixels
        else 1.0,
        "sam_covered_rfdetr_raw_pixels": sam_peer_covered,
        "rfdetr_covered_sam_raw_pixels": rfdetr_peer_covered,
        "sam_covered_raw_union_pixels": sam_union_covered,
        "rfdetr_covered_raw_union_pixels": rfdetr_union_covered,
        "sam_coverage_of_rfdetr_raw": float(sam_peer_covered / rfdetr_raw.sum())
        if rfdetr_raw.any()
        else 1.0,
        "rfdetr_coverage_of_sam_raw": float(rfdetr_peer_covered / sam_raw.sum())
        if sam_raw.any()
        else 1.0,
        "sam_coverage_of_raw_union": float(sam_union_covered / raw_union_pixels)
        if raw_union_pixels
        else 1.0,
        "rfdetr_coverage_of_raw_union": float(rfdetr_union_covered / raw_union_pixels)
        if raw_union_pixels
        else 1.0,
        "sam_pixels_outside_raw_union": int((sam_processed & ~raw_union).sum()),
        "rfdetr_pixels_outside_raw_union": int(
            (rfdetr_processed & ~raw_union).sum()
        ),
        "sam_processed_pixels": sam_processed_pixels,
        "rfdetr_processed_pixels": rfdetr_processed_pixels,
    }


def summary_values(values: list[float]) -> dict[str, float]:
    array = np.asarray(values, dtype=np.float64)
    return {
        "minimum": float(array.min()),
        "p10": float(np.quantile(array, 0.10)),
        "median": float(np.median(array)),
        "mean": float(array.mean()),
        "p90": float(np.quantile(array, 0.90)),
        "maximum": float(array.max()),
    }


def aggregate(records: list[dict[str, Any]]) -> dict[str, Any]:
    pixels = sum(record["coverage"]["pixels"] for record in records)
    raw_union_pixels = sum(
        record["coverage"]["raw_union_pixels"] for record in records
    )
    processed_union = sum(
        record["coverage"]["processed_union_pixels"] for record in records
    )
    processed_intersection = sum(
        record["coverage"]["processed_intersection_pixels"] for record in records
    )
    sam_raw = sum(record["sam"]["raw_pixels"] for record in records)
    rfdetr_raw = sum(record["rfdetr"]["raw_pixels"] for record in records)
    sam_processed = sum(record["sam"]["processed_pixels"] for record in records)
    rfdetr_processed = sum(
        record["rfdetr"]["processed_pixels"] for record in records
    )
    sam_peer_covered = sum(
        record["coverage"]["sam_covered_rfdetr_raw_pixels"]
        for record in records
    )
    rfdetr_peer_covered = sum(
        record["coverage"]["rfdetr_covered_sam_raw_pixels"]
        for record in records
    )
    sam_union_covered = sum(
        record["coverage"]["sam_covered_raw_union_pixels"]
        for record in records
    )
    rfdetr_union_covered = sum(
        record["coverage"]["rfdetr_covered_raw_union_pixels"]
        for record in records
    )
    return {
        "pages": len(records),
        "pixels": pixels,
        "sam": {
            "raw_pixels": sam_raw,
            "processed_pixels": sam_processed,
            "processed_ratio": float(sam_processed / pixels),
            "growth_ratio": float(sam_processed / sam_raw),
            "coverage_of_rfdetr_raw": float(sam_peer_covered / rfdetr_raw),
            "coverage_of_raw_union": float(sam_union_covered / raw_union_pixels),
            "pixels_outside_raw_union": sum(
                record["coverage"]["sam_pixels_outside_raw_union"]
                for record in records
            ),
            "hole_fill_added_pixels": sum(
                record["sam"]["hole_fill_added_pixels"] for record in records
            ),
        },
        "rfdetr": {
            "raw_pixels": rfdetr_raw,
            "processed_pixels": rfdetr_processed,
            "processed_ratio": float(rfdetr_processed / pixels),
            "growth_ratio": float(rfdetr_processed / rfdetr_raw),
            "coverage_of_sam_raw": float(rfdetr_peer_covered / sam_raw),
            "coverage_of_raw_union": float(
                rfdetr_union_covered / raw_union_pixels
            ),
            "pixels_outside_raw_union": sum(
                record["coverage"]["rfdetr_pixels_outside_raw_union"]
                for record in records
            ),
            "hole_fill_added_pixels": sum(
                record["rfdetr"]["hole_fill_added_pixels"] for record in records
            ),
        },
        "processed_mask_iou": float(processed_intersection / processed_union),
        "per_page_processed_iou": summary_values(
            [record["coverage"]["processed_iou"] for record in records]
        ),
    }


def select_reviews(records: list[dict[str, Any]], count: int) -> list[dict[str, Any]]:
    selected: dict[str, tuple[dict[str, Any], set[str]]] = {}

    def add(record: dict[str, Any], reason: str) -> None:
        if record["image"] not in selected:
            selected[record["image"]] = (record, set())
        selected[record["image"]][1].add(reason)

    for index in np.linspace(0, len(records) - 1, max(1, count // 2), dtype=int):
        add(records[int(index)], "evenly_spaced")
    rankings = (
        (lambda record: record["coverage"]["processed_iou"], "lowest_iou"),
        (
            lambda record: record["coverage"]["sam_coverage_of_rfdetr_raw"],
            "sam_misses_rf",
        ),
        (
            lambda record: record["coverage"]["rfdetr_coverage_of_sam_raw"],
            "rf_misses_sam",
        ),
    )
    for ranking, reason in rankings:
        for record in sorted(records, key=ranking)[: max(2, count // 6)]:
            add(record, reason)
    return [
        {**record, "review_reason": sorted(reasons)}
        for record, reasons in list(selected.values())[:count]
    ]


def fit(image: Image.Image, width: int, height: int) -> Image.Image:
    canvas = Image.new("RGB", (width, height), "white")
    contained = ImageOps.contain(
        image.convert("RGB"), (width, height), Image.Resampling.LANCZOS
    )
    canvas.paste(
        contained,
        ((width - contained.width) // 2, (height - contained.height) // 2),
    )
    return canvas


def render_review(
    output: Path, selected: list[dict[str, Any]], count: int
) -> list[dict[str, Any]]:
    selected = selected[:count]
    triptychs = output / "review_triptychs"
    triptychs.mkdir(parents=True, exist_ok=True)
    cell_width, image_height, label_height = 480, 480, 64
    columns = 3
    rows = math.ceil(len(selected) / columns)
    sheet = Image.new(
        "RGB", (cell_width * columns, (image_height + label_height) * rows), (225, 225, 225)
    )
    draw = ImageDraw.Draw(sheet)
    for index, record in enumerate(selected):
        with (
            Image.open(record["source_path"]) as raw_source,
            Image.open(output / record["sam_white_painted"]) as raw_sam,
            Image.open(output / record["rfdetr_white_painted"]) as raw_rfdetr,
        ):
            source = raw_source.convert("RGB")
            sam = raw_sam.convert("RGB")
            rfdetr = raw_rfdetr.convert("RGB")
            panels = [source, sam, rfdetr]
            preview = Image.new("RGB", (1500, 1500), "white")
            for panel_index, panel in enumerate(panels):
                preview.paste(fit(panel, 500, 1460), (panel_index * 500, 40))
            preview_draw = ImageDraw.Draw(preview)
            preview_draw.text((5, 8), "source", fill="black")
            preview_draw.text((505, 8), "TextSeg white-painted", fill="black")
            preview_draw.text((1005, 8), "RF-DETR white-painted", fill="black")
        triptych_path = triptychs / f"{Path(record['image']).stem}.jpg"
        preview.save(triptych_path, quality=94, subsampling=0)
        record["review_triptych"] = triptych_path.relative_to(output).as_posix()

        x = (index % columns) * cell_width
        y = (index // columns) * (image_height + label_height)
        panel_width = cell_width // 3
        for panel_index, panel in enumerate(panels):
            sheet.paste(
                fit(panel, panel_width, image_height),
                (x + panel_index * panel_width, y + label_height),
            )
        draw.text(
            (x + 5, y + 4),
            f"{record['image']}  post-IoU={record['coverage']['processed_iou']:.3f}",
            fill="black",
        )
        draw.text(
            (x + 5, y + 23),
            f"white area: SAM={record['sam']['processed_ratio']:.1%}  RF={record['rfdetr']['processed_ratio']:.1%}",
            fill=(35, 35, 35),
        )
        draw.text(
            (x + 5, y + 42),
            "left=source  middle=TextSeg  right=RF-DETR",
            fill=(35, 35, 35),
        )
    sheet.save(output / "review_contact_sheet.jpg", quality=94, subsampling=0)
    (output / "review_selection.json").write_text(
        json.dumps(selected, indent=2) + "\n", encoding="utf-8"
    )
    return selected


def process_dataset(
    dataset_name: str,
    comparison_root: Path,
    textseg_root: Path,
    output_root: Path,
    dilation_radius: int,
    review_pages: int,
) -> dict[str, Any]:
    comparison_data = json.loads(
        (comparison_root / dataset_name / "metrics.json").read_text(encoding="utf-8")
    )
    source_root = Path(comparison_data["source"])
    source_records = [
        record for record in comparison_data["records"] if "/" not in record["image"]
    ]
    output = output_root / dataset_name
    output.mkdir(parents=True, exist_ok=True)
    records: list[dict[str, Any]] = []
    started = time.perf_counter()
    for index, item in enumerate(source_records, 1):
        image_path = source_root / item["image"]
        sam_path = textseg_root / dataset_name / item["sam_mask"]
        rfdetr_path = comparison_root / dataset_name / item["rfdetr_typography_mask"]
        with (
            Image.open(image_path) as raw_source,
            Image.open(sam_path) as raw_sam,
            Image.open(rfdetr_path) as raw_rfdetr,
        ):
            source = np.asarray(raw_source.convert("RGB")).copy()
            sam_raw = np.asarray(raw_sam.convert("L")) > 127
            rfdetr_raw = np.asarray(raw_rfdetr.convert("L")) > 127
        height, width = sam_raw.shape
        sam_processed, sam_metrics = postprocess(
            sam_raw, width, height, dilation_radius
        )
        rfdetr_processed, rfdetr_metrics = postprocess(
            rfdetr_raw, width, height, dilation_radius
        )
        relative = Path(item["image"]).with_suffix(".png")
        sam_mask_destination = Path("textseg_processed_masks") / relative
        rfdetr_mask_destination = Path("rfdetr_processed_masks") / relative
        sam_painted_destination = Path("textseg_white_painted") / relative
        rfdetr_painted_destination = Path("rfdetr_white_painted") / relative
        save_mask(sam_processed, output / sam_mask_destination)
        save_mask(rfdetr_processed, output / rfdetr_mask_destination)
        save_white_painted(source, sam_processed, output / sam_painted_destination)
        save_white_painted(
            source, rfdetr_processed, output / rfdetr_painted_destination
        )
        records.append(
            {
                "image": item["image"],
                "source_path": str(image_path),
                "width": width,
                "height": height,
                "sam_processed_mask": sam_mask_destination.as_posix(),
                "rfdetr_processed_mask": rfdetr_mask_destination.as_posix(),
                "sam_white_painted": sam_painted_destination.as_posix(),
                "rfdetr_white_painted": rfdetr_painted_destination.as_posix(),
                "sam": sam_metrics,
                "rfdetr": rfdetr_metrics,
                "coverage": coverage_metrics(
                    sam_raw, rfdetr_raw, sam_processed, rfdetr_processed
                ),
            }
        )
        if index == len(source_records) or index % 20 == 0:
            elapsed = time.perf_counter() - started
            print(
                json.dumps(
                    {
                        "event": "progress",
                        "dataset": dataset_name,
                        "completed": index,
                        "total": len(source_records),
                        "pages_per_second": index / elapsed,
                    }
                ),
                flush=True,
            )
    selected = render_review(output, select_reviews(records, review_pages), review_pages)
    summary = {
        "name": dataset_name,
        "source": str(source_root),
        "pages": len(records),
        "postprocessing": {
            "order": ["elliptical_dilation", "fill_enclosed_holes", "paint_white"],
            "reference_long_side": REFERENCE_LONG_SIDE,
            "dilation_radius_at_reference": dilation_radius,
            "output_format": "lossless RGB PNG",
        },
        "aggregate": aggregate(records),
        "review_pages": len(selected),
        "seconds": time.perf_counter() - started,
        "records": records,
    }
    (output / "metrics.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    return summary


def main() -> None:
    args = parse_args()
    comparison_root = args.comparison_root.resolve()
    textseg_root = args.textseg_root.resolve()
    output_root = args.output_root.resolve()
    if output_root.exists():
        raise FileExistsError(output_root)
    if args.dilation_radius < 1 or args.review_pages < 1:
        raise ValueError("dilation radius and review pages must be positive")
    output_root.mkdir(parents=True)
    summaries = {}
    for dataset_name in ("bluearchive_comics", "marriagetoxin-chapter-1"):
        summary = process_dataset(
            dataset_name,
            comparison_root,
            textseg_root,
            output_root,
            args.dilation_radius,
            args.review_pages,
        )
        summaries[dataset_name] = {
            key: value for key, value in summary.items() if key != "records"
        }
    report = {
        "status": "complete",
        "scope": "291 source pages; derivative folders excluded",
        "models": {
            "sam": "mayocream/koharu-text-sam-ts-l",
            "rfdetr": "mayocream/koharu-layout-rfdetr-seg-2xl-1152",
        },
        "postprocessing": {
            "dilation": f"elliptical radius {args.dilation_radius}px at 1024px long side, scaled to source resolution",
            "fill_holes": "all enclosed background components",
            "paint": "masked RGB pixels set exactly to (255, 255, 255)",
        },
        "ground_truth": False,
        "datasets": summaries,
    }
    (output_root / "summary.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    (output_root / "PREINPAINTING_COMPLETE.json").write_text(
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
    print(json.dumps({"event": "complete", **report}), flush=True)


if __name__ == "__main__":
    main()
