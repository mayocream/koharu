#!/usr/bin/env python3
"""Run RF-DETR segmentation on image folders and render review overlays."""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image, ImageDraw, ImageOps
from rfdetr import RFDETRSeg2XLarge

from render_manga109_rfdetr_predictions import load_font, render_overlay


NAMES = {0: "text", 1: "COO", 2: "bubble", 3: "panel"}
IMAGE_SUFFIXES = frozenset({".jpg", ".jpeg", ".png", ".webp"})


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("inputs", nargs="+", type=Path)
    parser.add_argument("--threshold", type=float, default=0.25)
    parser.add_argument("--resolution", type=int, default=1152)
    parser.add_argument("--batch-size", type=int, default=2)
    parser.add_argument("--max-detections", type=int, default=160)
    parser.add_argument("--review-pages", type=int, default=20)
    return parser.parse_args()


def natural_key(path: Path) -> list[int | str]:
    return [
        int(part) if part.isdigit() else part.casefold()
        for part in re.split(r"(\d+)", path.name)
    ]


def list_images(folder: Path) -> list[Path]:
    images = sorted(
        (
            path
            for path in folder.iterdir()
            if path.is_file() and path.suffix.casefold() in IMAGE_SUFFIXES
        ),
        key=natural_key,
    )
    if not images:
        raise FileNotFoundError(f"no top-level images in {folder}")
    return images


def make_review_sheet(
    overlay_paths: list[Path], destination: Path, count: int
) -> list[str]:
    if count <= 0:
        return []
    if len(overlay_paths) <= count:
        selected = overlay_paths
    else:
        indexes = [index * len(overlay_paths) // count for index in range(count)]
        selected = [overlay_paths[index] for index in indexes]
    columns = 5
    cell_width, cell_height, header = 360, 540, 30
    rows = (len(selected) + columns - 1) // columns
    sheet = Image.new("RGB", (columns * cell_width, rows * cell_height), (225, 225, 225))
    font = load_font(18)
    for index, path in enumerate(selected):
        with Image.open(path) as source:
            fitted = ImageOps.contain(
                source.convert("RGB"),
                (cell_width, cell_height - header),
                Image.Resampling.LANCZOS,
            )
        card = Image.new("RGB", (cell_width, cell_height), "white")
        card.paste(
            fitted,
            ((cell_width - fitted.width) // 2, header + (cell_height - header - fitted.height) // 2),
        )
        draw = ImageDraw.Draw(card)
        draw.text((6, 5), path.stem, fill="black", font=font)
        sheet.paste(card, ((index % columns) * cell_width, (index // columns) * cell_height))
    sheet.save(destination, quality=92, subsampling=0)
    return [path.stem for path in selected]


def main() -> None:
    args = parse_args()
    checkpoint = args.checkpoint.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    folders = [folder.resolve() for folder in args.inputs]
    folder_images = {folder: list_images(folder) for folder in folders}

    model = RFDETRSeg2XLarge(
        pretrain_weights=str(checkpoint),
        resolution=args.resolution,
        num_select=args.max_detections,
    )
    report: dict[str, Any] = {
        "checkpoint": str(checkpoint),
        "threshold": args.threshold,
        "resolution": args.resolution,
        "sources": {},
    }

    for folder, image_paths in folder_images.items():
        source_output = output / folder.name
        overlays = source_output / "overlays"
        overlays.mkdir(parents=True, exist_ok=True)
        records: list[dict[str, Any]] = []
        total_counts: Counter[int] = Counter()
        overlay_paths: list[Path] = []
        for start in range(0, len(image_paths), args.batch_size):
            batch_paths = image_paths[start : start + args.batch_size]
            source_images: list[Image.Image] = []
            for path in batch_paths:
                with Image.open(path) as image:
                    source_images.append(image.convert("RGB"))
            detections_batch = model.predict(
                source_images,
                threshold=args.threshold,
                shape=(args.resolution, args.resolution),
                include_source_image=False,
            )
            if not isinstance(detections_batch, list):
                detections_batch = [detections_batch]

            for path, source_image, detections in zip(
                batch_paths, source_images, detections_batch, strict=True
            ):
                order = np.argsort(-detections.confidence)[: args.max_detections]
                detections = detections[order]
                instances: list[dict[str, Any]] = []
                counts: Counter[int] = Counter()
                confidences: list[float] = []
                serialized: list[dict[str, Any]] = []
                for index in range(len(detections)):
                    class_id = int(detections.class_id[index])
                    score = float(detections.confidence[index])
                    box = [float(value) for value in detections.xyxy[index].tolist()]
                    mask = (
                        detections.mask[index].astype(bool)
                        if detections.mask is not None
                        else None
                    )
                    area = float(max(0, box[2] - box[0]) * max(0, box[3] - box[1]))
                    instances.append(
                        {
                            "class_id": class_id,
                            "xyxy": box,
                            "mask": mask,
                            "score": score,
                            "area": area,
                        }
                    )
                    serialized.append(
                        {"class": NAMES[class_id], "score": score, "xyxy": box}
                    )
                    counts[class_id] += 1
                    total_counts[class_id] += 1
                    confidences.append(score)

                overlay = render_overlay(source_image, instances, NAMES, path.name)
                overlay_path = overlays / f"{path.stem}.jpg"
                overlay.save(overlay_path, quality=92, subsampling=0)
                overlay_paths.append(overlay_path)
                records.append(
                    {
                        "file_name": path.name,
                        "width": source_image.width,
                        "height": source_image.height,
                        "predictions": len(instances),
                        "counts": {NAMES[key]: counts[key] for key in sorted(counts)},
                        "mean_confidence": (
                            float(np.mean(confidences)) if confidences else None
                        ),
                        "detections": serialized,
                    }
                )
            completed = min(start + args.batch_size, len(image_paths))
            if completed % 10 == 0 or completed == len(image_paths):
                print(f"{folder.name}: {completed}/{len(image_paths)}", flush=True)

        review_selection = make_review_sheet(
            overlay_paths, source_output / "review_sheet.jpg", args.review_pages
        )
        source_report = {
            "input": str(folder),
            "pages": len(image_paths),
            "prediction_counts": {
                NAMES[key]: total_counts[key] for key in sorted(total_counts)
            },
            "mean_predictions_per_page": float(
                np.mean([record["predictions"] for record in records])
            ),
            "zero_prediction_pages": [
                record["file_name"] for record in records if not record["predictions"]
            ],
            "review_selection": review_selection,
            "records": records,
        }
        report["sources"][folder.name] = source_report
        (source_output / "predictions.json").write_text(
            json.dumps(source_report, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )

    (output / "summary.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({"output": str(output), "sources": {
        key: {
            "pages": value["pages"],
            "prediction_counts": value["prediction_counts"],
            "zero_prediction_pages": value["zero_prediction_pages"],
        }
        for key, value in report["sources"].items()
    }}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
