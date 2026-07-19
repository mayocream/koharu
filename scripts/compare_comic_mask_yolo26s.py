# /// script
# requires-python = ">=3.11,<3.15"
# dependencies = [
#   "pillow>=11,<13",
#   "ultralytics==8.4.43",
# ]
# ///
"""Render and compare upstream and fine-tuned comic-mask YOLO26s predictions."""

from __future__ import annotations

import argparse
import json
import statistics
import time
from pathlib import Path
from typing import Any

import numpy as np
import torch
from PIL import Image, ImageDraw, ImageFont
from ultralytics import YOLO


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASELINE = (
    REPOSITORY_ROOT / "data" / "models" / "shadowb-comic-mask-yolo26s" / "best.pt"
)
DEFAULT_INPUT = REPOSITORY_ROOT / "data" / "bluearchive_comics"
DEFAULT_OUTPUT = REPOSITORY_ROOT / "runs" / "comic-mask-yolo26s-comparison"
IMAGE_SUFFIXES = {".jpg", ".jpeg", ".png", ".webp", ".bmp", ".tif", ".tiff"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    parser.add_argument("--fine-tuned", type=Path, required=True)
    parser.add_argument("--input", type=Path, default=DEFAULT_INPUT)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--imgsz", type=int, default=1280)
    parser.add_argument("--conf", type=float, default=0.25)
    parser.add_argument("--iou", type=float, default=0.7)
    parser.add_argument("--match-iou", type=float, default=0.5)
    parser.add_argument("--device", default="0")
    parser.add_argument("--half", action="store_true")
    parser.add_argument("--contact-sheet-pages", type=int, default=8)
    return parser.parse_args()


def synchronize(device: str) -> None:
    if str(device).lower() != "cpu" and torch.cuda.is_available():
        torch.cuda.synchronize()


def run_model(
    model_path: Path,
    image_paths: list[Path],
    output_dir: Path,
    args: argparse.Namespace,
) -> tuple[list[Path], dict[str, Any]]:
    output_dir.mkdir(parents=True, exist_ok=True)
    detections_dir = output_dir / "detections"
    detections_dir.mkdir(parents=True, exist_ok=True)
    model = YOLO(model_path)
    predict_args = {
        "imgsz": args.imgsz,
        "conf": args.conf,
        "iou": args.iou,
        "retina_masks": True,
        "device": args.device,
        "half": args.half,
        "verbose": False,
    }
    model.predict(str(image_paths[0]), **predict_args)
    synchronize(args.device)

    rendered: list[Path] = []
    pages: list[dict[str, Any]] = []
    timings: list[float] = []
    aggregate_counts = {name: 0 for name in model.names.values()}
    aggregate_scores = {name: [] for name in model.names.values()}
    for index, image_path in enumerate(image_paths, start=1):
        synchronize(args.device)
        started = time.perf_counter()
        result = model.predict(str(image_path), **predict_args)[0]
        synchronize(args.device)
        timings.append((time.perf_counter() - started) * 1000.0)

        boxes = (
            result.boxes.xyxy.detach().float().cpu().numpy()
            if result.boxes is not None
            else np.empty((0, 4), dtype=np.float32)
        )
        classes = (
            result.boxes.cls.detach().cpu().numpy().astype(np.int64)
            if result.boxes is not None
            else np.empty(0, dtype=np.int64)
        )
        scores = (
            result.boxes.conf.detach().float().cpu().numpy()
            if result.boxes is not None
            else np.empty(0, dtype=np.float32)
        )
        masks = (
            result.masks.data.detach().cpu().numpy()
            if result.masks is not None
            else np.empty((0, *result.orig_shape), dtype=np.uint8)
        )
        detections = []
        for detection_index, (box, class_id, score) in enumerate(
            zip(boxes, classes, scores)
        ):
            label = model.names[int(class_id)]
            aggregate_counts[label] += 1
            aggregate_scores[label].append(float(score))
            detections.append(
                {
                    "label_id": int(class_id),
                    "label": label,
                    "score": float(score),
                    "bbox": [float(value) for value in box],
                    "area": int(np.count_nonzero(masks[detection_index])),
                }
            )

        page = {
            "page": image_path.stem,
            "counts": {
                name: int(np.sum(classes == class_id))
                for class_id, name in model.names.items()
            },
            "mean_confidence": float(scores.mean()) if len(scores) else 0.0,
            "detections": detections,
        }
        pages.append(page)
        (detections_dir / f"{image_path.stem}.json").write_text(
            json.dumps(detections, indent=2), encoding="utf-8"
        )

        rendered_path = output_dir / f"{image_path.stem}.png"
        Image.fromarray(result.plot()[..., ::-1]).save(rendered_path)
        rendered.append(rendered_path)
        print(
            f"[{index}/{len(image_paths)}] {model_path.name} {image_path.name}: "
            f"{len(detections)} instances",
            flush=True,
        )

    return rendered, {
        "checkpoint": str(model_path.resolve()),
        "classes": model.names,
        "aggregate_counts": aggregate_counts,
        "mean_confidence_by_class": {
            name: statistics.mean(values) if values else 0.0
            for name, values in aggregate_scores.items()
        },
        "wall_mean_ms": statistics.mean(timings),
        "wall_median_ms": statistics.median(timings),
        "pages_without_detections": sum(not page["detections"] for page in pages),
        "pages": pages,
    }


def box_iou(left: list[float], right: list[float]) -> float:
    intersection_width = max(0.0, min(left[2], right[2]) - max(left[0], right[0]))
    intersection_height = max(0.0, min(left[3], right[3]) - max(left[1], right[1]))
    intersection = intersection_width * intersection_height
    left_area = max(0.0, left[2] - left[0]) * max(0.0, left[3] - left[1])
    right_area = max(0.0, right[2] - right[0]) * max(0.0, right[3] - right[1])
    union = left_area + right_area - intersection
    return intersection / union if union > 0.0 else 0.0


def match_reports(
    baseline: dict[str, Any],
    fine_tuned: dict[str, Any],
    mapping: dict[str, set[str]],
    threshold: float,
) -> dict[str, Any]:
    fine_pages = {page["page"]: page for page in fine_tuned["pages"]}
    per_class = {
        label: {
            "baseline": 0,
            "fine_tuned": 0,
            "matches": 0,
            "ious": [],
        }
        for label in mapping
    }
    for baseline_page in baseline["pages"]:
        fine_page = fine_pages[baseline_page["page"]]
        for label, allowed in mapping.items():
            baseline_detections = [
                detection
                for detection in baseline_page["detections"]
                if detection["label"] == label
            ]
            fine_detections = [
                detection
                for detection in fine_page["detections"]
                if detection["label"] in allowed
            ]
            stats = per_class[label]
            stats["baseline"] += len(baseline_detections)
            stats["fine_tuned"] += len(fine_detections)
            pairs = sorted(
                (
                    (box_iou(left["bbox"], right["bbox"]), left_index, right_index)
                    for left_index, left in enumerate(baseline_detections)
                    for right_index, right in enumerate(fine_detections)
                ),
                reverse=True,
            )
            used_baseline: set[int] = set()
            used_fine: set[int] = set()
            for iou, baseline_index, fine_index in pairs:
                if iou < threshold:
                    break
                if baseline_index in used_baseline or fine_index in used_fine:
                    continue
                used_baseline.add(baseline_index)
                used_fine.add(fine_index)
                stats["matches"] += 1
                stats["ious"].append(iou)

    def summarize(stats: dict[str, Any]) -> dict[str, Any]:
        baseline_count = stats["baseline"]
        fine_count = stats["fine_tuned"]
        matches = stats["matches"]
        return {
            "baseline_count": baseline_count,
            "fine_tuned_count": fine_count,
            "matches": matches,
            "baseline_match_rate": matches / baseline_count if baseline_count else 0.0,
            "fine_tuned_match_rate": matches / fine_count if fine_count else 0.0,
            "mean_matched_box_iou": (
                statistics.mean(stats["ious"]) if stats["ious"] else 0.0
            ),
        }

    aggregate = {
        "baseline": sum(stats["baseline"] for stats in per_class.values()),
        "fine_tuned": sum(stats["fine_tuned"] for stats in per_class.values()),
        "matches": sum(stats["matches"] for stats in per_class.values()),
        "ious": [iou for stats in per_class.values() for iou in stats["ious"]],
    }
    return {
        "match_iou_threshold": threshold,
        "overall": summarize(aggregate),
        "per_upstream_class": {
            label: summarize(stats) for label, stats in per_class.items()
        },
    }


def make_contact_sheet(
    originals: list[Path],
    baseline: list[Path],
    fine_tuned: list[Path],
    output: Path,
) -> None:
    column_width = 360
    gap = 8
    header_height = 34
    font = ImageFont.load_default()
    rows: list[Image.Image] = []
    for original_path, baseline_path, fine_path in zip(
        originals, baseline, fine_tuned
    ):
        cells = []
        for path in (original_path, baseline_path, fine_path):
            with Image.open(path) as source:
                image = source.convert("RGB")
                height = round(image.height * column_width / image.width)
                cells.append(
                    image.resize((column_width, height), Image.Resampling.LANCZOS)
                )
        row_height = max(cell.height for cell in cells)
        row = Image.new("RGB", (column_width * 3 + gap * 2, row_height + 22), "white")
        draw = ImageDraw.Draw(row)
        draw.text((5, 5), f"page {original_path.stem}", fill="black", font=font)
        x = 0
        for cell in cells:
            row.paste(cell, (x, 22))
            x += column_width + gap
        rows.append(row)

    width = rows[0].width
    height = header_height + sum(row.height for row in rows) + gap * (len(rows) - 1)
    sheet = Image.new("RGB", (width, height), "white")
    draw = ImageDraw.Draw(sheet)
    for index, title in enumerate(("original", "upstream baseline", "fine-tuned")):
        draw.text(
            (index * (column_width + gap) + 5, 10), title, fill="black", font=font
        )
    y = header_height
    for row in rows:
        sheet.paste(row, (0, y))
        y += row.height + gap
    sheet.save(output)


def main() -> None:
    args = parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    image_paths = sorted(
        path
        for path in args.input.iterdir()
        if path.is_file() and path.suffix.lower() in IMAGE_SUFFIXES
    )
    if not image_paths:
        raise FileNotFoundError(f"no supported images found in {args.input}")

    baseline_images, baseline_report = run_model(
        args.baseline, image_paths, args.output / "upstream", args
    )
    fine_images, fine_report = run_model(
        args.fine_tuned, image_paths, args.output / "fine-tuned", args
    )
    for start in range(0, len(image_paths), args.contact_sheet_pages):
        end = min(start + args.contact_sheet_pages, len(image_paths))
        make_contact_sheet(
            image_paths[start:end],
            baseline_images[start:end],
            fine_images[start:end],
            args.output
            / f"contact-sheet-{image_paths[start].stem}-{image_paths[end - 1].stem}.png",
        )

    report = {
        "device": torch.cuda.get_device_name(0) if torch.cuda.is_available() else "CPU",
        "ultralytics_version": "8.4.43",
        "imgsz": args.imgsz,
        "confidence": args.conf,
        "iou": args.iou,
        "half": args.half,
        "images": len(image_paths),
        "baseline": baseline_report,
        "fine_tuned": fine_report,
        "comparison_core_classes": match_reports(
            baseline_report,
            fine_report,
            {"frame": {"frame"}, "text": {"dialogue_text"}, "balloon": {"balloon"}},
            args.match_iou,
        ),
        "comparison_text_any": match_reports(
            baseline_report,
            fine_report,
            {
                "frame": {"frame"},
                "text": {"dialogue_text", "onomatopoeia_text"},
                "balloon": {"balloon"},
            },
            args.match_iou,
        ),
    }
    (args.output / "report.json").write_text(
        json.dumps(report, indent=2), encoding="utf-8"
    )
    print(json.dumps({key: value for key, value in report.items() if key not in {"baseline", "fine_tuned"}}, indent=2))


if __name__ == "__main__":
    main()
