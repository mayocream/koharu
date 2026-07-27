#!/usr/bin/env python3
"""Diagnose large TextSeg components missed by an RF-DETR checkpoint."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import cv2
import numpy as np
from PIL import Image
from rfdetr import RFDETRSeg2XLarge


ROOT = Path(__file__).resolve().parents[1]
DATASETS = ("bluearchive_comics", "marriagetoxin-chapter-1")
CLASS_NAMES = {0: "text", 1: "onomatopoeia", 2: "bubble", 3: "panel"}
COO_THRESHOLDS = (0.05, 0.10, 0.20, 0.30, 0.40)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--textseg-root",
        type=Path,
        default=ROOT / "runs" / "koharu-text-sam-ts-l-domain-evaluation",
    )
    parser.add_argument("--batch-size", type=int, default=2)
    parser.add_argument("--minimum-component-area", type=int, default=1500)
    parser.add_argument("--minimum-component-span", type=int, default=80)
    parser.add_argument("--maximum-current-coverage", type=float, default=0.60)
    return parser.parse_args()


def load_mask(path: Path) -> np.ndarray:
    with Image.open(path) as image:
        return np.asarray(image.convert("L")) > 127


def classify_reason(best: dict[str, Any] | None) -> str:
    if best is None or best["component_coverage"] < 0.20:
        return "no_matching_typography_detection_at_0.05"
    class_id = best["class_id"]
    score = best["score"]
    if class_id == 1 and score < 0.40:
        return "onomatopoeia_below_0.40_threshold"
    if class_id == 0 and score < 0.25:
        return "text_below_0.25_threshold"
    return "predicted_mask_under_covers_component"


def main() -> None:
    args = parse_args()
    checkpoint = args.checkpoint.resolve()
    output = args.output.resolve()
    textseg_root = args.textseg_root.resolve()
    if not checkpoint.is_file():
        raise FileNotFoundError(checkpoint)
    if output.exists():
        raise FileExistsError(output)
    output.mkdir(parents=True)

    model = RFDETRSeg2XLarge(
        pretrain_weights=str(checkpoint), resolution=1152, num_select=160
    )
    report: dict[str, Any] = {
        "checkpoint": str(checkpoint),
        "inference_threshold": 0.05,
        "text_threshold": 0.25,
        "onomatopoeia_thresholds": list(COO_THRESHOLDS),
        "component_filter": {
            "minimum_area": args.minimum_component_area,
            "minimum_span": args.minimum_component_span,
            "maximum_current_coverage": args.maximum_current_coverage,
        },
        "datasets": {},
    }

    for dataset in DATASETS:
        textseg_data = json.loads(
            (textseg_root / dataset / "metrics.json").read_text(encoding="utf-8")
        )
        source_root = Path(textseg_data["source"])
        manifest = [
            item
            for item in textseg_data["records"]
            if Path(item["image"]).parent == Path(".")
        ]
        totals = {
            threshold: Counter(tp=0, fp=0, fn=0) for threshold in COO_THRESHOLDS
        }
        missed_components: list[dict[str, Any]] = []
        detection_scores: dict[int, list[float]] = {class_id: [] for class_id in CLASS_NAMES}

        for offset in range(0, len(manifest), args.batch_size):
            batch = manifest[offset : offset + args.batch_size]
            images: list[Image.Image] = []
            for item in batch:
                with Image.open(source_root / item["image"]) as image:
                    images.append(image.convert("RGB"))
            prediction_batch = model.predict(
                images,
                threshold=0.05,
                shape=(1152, 1152),
                include_source_image=False,
            )
            if not isinstance(prediction_batch, list):
                prediction_batch = [prediction_batch]

            for item, detections in zip(batch, prediction_batch, strict=True):
                sam = load_mask(textseg_root / dataset / item["mask"])
                instances: list[dict[str, Any]] = []
                for class_id, score, mask in zip(
                    detections.class_id,
                    detections.confidence,
                    detections.mask,
                    strict=True,
                ):
                    class_id = int(class_id)
                    score = float(score)
                    detection_scores.setdefault(class_id, []).append(score)
                    instances.append(
                        {
                            "class_id": class_id,
                            "score": score,
                            "mask": mask.astype(bool),
                        }
                    )

                threshold_masks: dict[float, np.ndarray] = {}
                for threshold in COO_THRESHOLDS:
                    prediction = np.zeros_like(sam)
                    for instance in instances:
                        if (
                            instance["class_id"] == 0
                            and instance["score"] >= 0.25
                        ) or (
                            instance["class_id"] == 1
                            and instance["score"] >= threshold
                        ):
                            prediction |= instance["mask"]
                    threshold_masks[threshold] = prediction
                    totals[threshold]["tp"] += int((sam & prediction).sum())
                    totals[threshold]["fp"] += int((~sam & prediction).sum())
                    totals[threshold]["fn"] += int((sam & ~prediction).sum())

                current = threshold_masks[0.40]
                component_count, labels, stats, _ = cv2.connectedComponentsWithStats(
                    sam.astype(np.uint8), connectivity=8
                )
                for component_id in range(1, component_count):
                    x, y, width, height, area = map(int, stats[component_id])
                    if area < args.minimum_component_area:
                        continue
                    if max(width, height) < args.minimum_component_span:
                        continue
                    component_roi = labels[y : y + height, x : x + width] == component_id
                    current_coverage = float(
                        (
                            component_roi
                            & current[y : y + height, x : x + width]
                        ).sum()
                        / area
                    )
                    if current_coverage >= args.maximum_current_coverage:
                        continue
                    best: dict[str, Any] | None = None
                    for instance in instances:
                        if instance["class_id"] not in (0, 1):
                            continue
                        intersection = int(
                            (
                                component_roi
                                & instance["mask"][y : y + height, x : x + width]
                            ).sum()
                        )
                        if not intersection:
                            continue
                        candidate = {
                            "class_id": instance["class_id"],
                            "class_name": CLASS_NAMES.get(
                                instance["class_id"], str(instance["class_id"])
                            ),
                            "score": instance["score"],
                            "component_coverage": float(intersection / area),
                            "intersection_pixels": intersection,
                        }
                        if best is None or candidate["component_coverage"] > best["component_coverage"]:
                            best = candidate
                    missed_components.append(
                        {
                            "image": item["image"],
                            "bbox_xywh": [x, y, width, height],
                            "component_area": area,
                            "current_coverage": current_coverage,
                            "missed_pixels": int(
                                (
                                    component_roi
                                    & ~current[y : y + height, x : x + width]
                                ).sum()
                            ),
                            "best_typography_detection": best,
                            "reason": classify_reason(best),
                        }
                    )

            completed = min(offset + args.batch_size, len(manifest))
            if completed == len(manifest) or completed % 40 < args.batch_size:
                print(f"{dataset}: {completed}/{len(manifest)}", flush=True)

        sweep = {}
        for threshold, counts in totals.items():
            tp, fp, fn = counts["tp"], counts["fp"], counts["fn"]
            sweep[str(threshold)] = {
                "tp": tp,
                "fp": fp,
                "fn": fn,
                "precision": float(tp / (tp + fp)) if tp + fp else 1.0,
                "recall": float(tp / (tp + fn)) if tp + fn else 1.0,
                "iou": float(tp / (tp + fp + fn)) if tp + fp + fn else 1.0,
            }
        reasons = Counter(item["reason"] for item in missed_components)
        missed_components.sort(
            key=lambda item: item["missed_pixels"], reverse=True
        )
        dataset_report = {
            "pages": len(manifest),
            "threshold_sweep": sweep,
            "large_undercovered_components": len(missed_components),
            "reason_counts": dict(reasons),
            "top_missed_components": missed_components[:100],
            "onomatopoeia_score_distribution": {
                "count_at_or_above_0.05": len(detection_scores[1]),
                "below_0.40": sum(score < 0.40 for score in detection_scores[1]),
                "at_or_above_0.40": sum(score >= 0.40 for score in detection_scores[1]),
            },
        }
        report["datasets"][dataset] = dataset_report
        (output / f"{dataset}.json").write_text(
            json.dumps(dataset_report, indent=2, allow_nan=False) + "\n",
            encoding="utf-8",
        )

    (output / "summary.json").write_text(
        json.dumps(report, indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )
    print(json.dumps({"status": "complete", "output": str(output)}))


if __name__ == "__main__":
    main()
