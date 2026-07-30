#!/usr/bin/env python3
"""Rebuild Manga109 typography masks with Zenodo/TextSeg pixel supervision.

Human text boxes, human COO polygons, bubbles, panels, IDs, and relations remain
authoritative.  PP-DocLayoutV3 is used only as a rectangular proposal source;
its polygons never become masks.  A PP proposal mask is TextSeg/Zenodo ink
clipped by that rectangle, while its COCO bbox is recomputed from the ink.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import math
import os
import shutil
import time
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import cv2
import numpy as np
from PIL import Image

from build_manga109_segmentation import (
    decode_rle,
    encode_rle,
    load_coo_records,
    mask_geometry,
)
from prepare_manga109_pp_doclayout_distillation import box_gold_coverage


ROOT = Path(__file__).resolve().parents[1]
MARKER = ".manga109-textseg-refinement"
SPLITS = ("train", "validation", "test")
MINIMUM_TEACHER_PIXELS = 4
PP_MINIMUM_MASK_PIXELS = 16
DEFAULT_EMPTY_TARGET_TEACHER_PIXELS = 512
DEFAULT_LOW_COVERAGE_TEACHER_PIXELS = 10_000
DEFAULT_LOW_COVERAGE_RECALL = 0.20


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dataset",
        type=Path,
        default=ROOT / "data" / "manga109-segmentation",
    )
    parser.add_argument(
        "--images-root",
        type=Path,
        default=ROOT / "data" / "Manga109_released_2026_05_21" / "images",
    )
    parser.add_argument(
        "--coo-annotations",
        type=Path,
        default=ROOT / "data" / "Manga109_released_2026_05_21" / "annotations_COO",
    )
    parser.add_argument(
        "--textseg-root",
        type=Path,
        default=ROOT / "data" / "manga109-textseg-teacher",
    )
    parser.add_argument(
        "--zenodo-root",
        type=Path,
        default=ROOT / "data" / "manga109-zenodo-sam-ts",
    )
    parser.add_argument(
        "--pp-root",
        type=Path,
        default=ROOT / "data" / "manga109-segmentation-pp-doclayoutv3",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "data" / "manga109-segmentation-textseg",
    )
    parser.add_argument("--pp-positive-score", type=float, default=0.70)
    parser.add_argument("--pp-max-gold-box-coverage", type=float, default=0.05)
    parser.add_argument(
        "--drop-empty-target-teacher-pixels",
        type=int,
        default=DEFAULT_EMPTY_TARGET_TEACHER_PIXELS,
        help="Drop an empty-target page when the teacher finds at least this many pixels.",
    )
    parser.add_argument(
        "--drop-low-coverage-teacher-pixels",
        type=int,
        default=DEFAULT_LOW_COVERAGE_TEACHER_PIXELS,
        help="Minimum teacher foreground used by the low-coverage page filter.",
    )
    parser.add_argument(
        "--drop-low-coverage-recall",
        type=float,
        default=DEFAULT_LOW_COVERAGE_RECALL,
        help="Drop pages whose final mask covers less than this fraction of a substantial teacher mask.",
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=min(12, os.cpu_count() or 1),
        help="Page workers. Each worker decodes full-resolution RLE masks, so 8-12 is normally optimal.",
    )
    parser.add_argument(
        "--only-available-pages",
        action="store_true",
        help="Pilot mode: emit only pages with a Zenodo or available TextSeg mask.",
    )
    parser.add_argument(
        "--audit-artifacts",
        action="store_true",
        help="Save per-page old/new/teacher/pseudo union masks for visual review.",
    )
    parser.add_argument("--overwrite", action="store_true")
    return parser.parse_args()


def prepare_output(output: Path, overwrite: bool) -> None:
    if output.exists() and any(output.iterdir()):
        if not overwrite:
            raise FileExistsError(f"output exists; pass --overwrite: {output}")
        if not (output / MARKER).is_file():
            raise ValueError(f"refusing to replace unmarked output: {output}")
        if ROOT.resolve() not in output.resolve().parents:
            raise ValueError(f"refusing to replace output outside repository: {output}")
        shutil.rmtree(output)
    output.mkdir(parents=True, exist_ok=True)
    (output / MARKER).write_text("generated Manga109 TextSeg refinement\n", encoding="utf-8")


def load_zenodo(root: Path) -> dict[str, Path]:
    result: dict[str, Path] = {}
    for split in ("train", "val", "test"):
        with (root / "manifests" / f"{split}.jsonl").open(encoding="utf-8") as stream:
            for line in stream:
                record = json.loads(line)
                result[str(record["source_image"])] = root / str(record["mask"])
    return result


def load_textseg(root: Path) -> dict[str, Path]:
    result: dict[str, Path] = {}
    path = root / "records.jsonl"
    if not path.is_file():
        return result
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            record = json.loads(line)
            mask = root / str(record["mask"])
            if mask.is_file():
                result[str(record["image"])] = mask
    return result


def load_mask(path: Path, width: int, height: int) -> np.ndarray:
    with Image.open(path) as image:
        mask = np.asarray(image.convert("L")) > 127
    if mask.shape != (height, width):
        raise ValueError(f"mask shape mismatch for {path}: {mask.shape} != {(height, width)}")
    return mask


def pair_counts(reference: np.ndarray, prediction: np.ndarray) -> dict[str, Any]:
    intersection = int((reference & prediction).sum())
    reference_pixels = int(reference.sum())
    prediction_pixels = int(prediction.sum())
    union = reference_pixels + prediction_pixels - intersection
    return {
        "reference_pixels": reference_pixels,
        "prediction_pixels": prediction_pixels,
        "intersection_pixels": intersection,
        "union_pixels": union,
        "iou": float(intersection / union) if union else 1.0,
        "precision": float(intersection / prediction_pixels) if prediction_pixels else float(reference_pixels == 0),
        "recall": float(intersection / reference_pixels) if reference_pixels else float(prediction_pixels == 0),
    }


def save_mask(mask: np.ndarray, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(mask.astype(np.uint8) * 255, mode="L").save(path)


def bbox_from_polygon(prediction: dict[str, Any], width: int, height: int) -> list[int] | None:
    polygon = prediction.get("polygon")
    if not isinstance(polygon, list) or len(polygon) < 3:
        return None
    try:
        points = np.asarray(polygon, dtype=np.float64)
    except (TypeError, ValueError):
        return None
    if points.ndim != 2 or points.shape[1] != 2 or not np.isfinite(points).all():
        return None
    x0 = max(0, min(width, int(math.floor(points[:, 0].min()))))
    y0 = max(0, min(height, int(math.floor(points[:, 1].min()))))
    x1 = max(0, min(width, int(math.ceil(points[:, 0].max()))))
    y1 = max(0, min(height, int(math.ceil(points[:, 1].max()))))
    if x1 <= x0 or y1 <= y0:
        return None
    return [x0, y0, x1 - x0, y1 - y0]


def remove_tiny_components(mask: np.ndarray, minimum_area: int = 2) -> np.ndarray:
    count, labels, stats, _ = cv2.connectedComponentsWithStats(
        mask.astype(np.uint8), connectivity=8
    )
    keep = np.zeros_like(mask)
    for label in range(1, count):
        if int(stats[label, cv2.CC_STAT_AREA]) >= minimum_area:
            keep[labels == label] = True
    return keep


def rectangle_roi(
    box: list[float] | tuple[float, ...], width: int, height: int, padding: int = 0
) -> tuple[tuple[int, int, int, int], np.ndarray]:
    x, y, box_width, box_height = (int(round(value)) for value in box)
    x0, y0 = max(0, x - padding), max(0, y - padding)
    x1 = min(width, x + box_width + padding)
    y1 = min(height, y + box_height + padding)
    if x1 <= x0 or y1 <= y0:
        return (0, 0, 0, 0), np.zeros((0, 0), dtype=bool)
    return (x0, y0, x1, y1), np.ones((y1 - y0, x1 - x0), dtype=bool)


def coo_envelope_roi(
    annotation: dict[str, Any],
    image_name: str,
    coo_records: list[dict[str, Any]],
    width: int,
    height: int,
) -> tuple[tuple[int, int, int, int], np.ndarray, str]:
    identifiers = {
        str(value)
        for value in annotation.get("attributes", {}).get("coo_annotation_ids", [])
    }
    if identifiers:
        polygons: list[np.ndarray] = []
        for record in coo_records:
            source = record.get("coo")
            if source is not None and str(source["annotation_id"]) in identifiers:
                polygons.append(np.asarray(source["polygon"], dtype=np.float64))
        if len(polygons) == len(identifiers):
            points = np.concatenate(polygons, axis=0)
            x0 = max(0, int(math.floor(points[:, 0].min())))
            y0 = max(0, int(math.floor(points[:, 1].min())))
            x1 = min(width, int(math.ceil(points[:, 0].max())) + 1)
            y1 = min(height, int(math.ceil(points[:, 1].max())) + 1)
            if x1 > x0 and y1 > y0:
                envelope = np.zeros((y1 - y0, x1 - x0), dtype=np.uint8)
                shifted = []
                for polygon in polygons:
                    local = polygon.copy()
                    local[:, 0] = np.clip(np.round(local[:, 0] - x0), 0, x1 - x0 - 1)
                    local[:, 1] = np.clip(np.round(local[:, 1] - y0), 0, y1 - y0 - 1)
                    shifted.append(local.astype(np.int32))
                cv2.fillPoly(envelope, shifted, 1)
                return (x0, y0, x1, y1), envelope.astype(bool), "human_coo_polygon"
    roi, envelope = rectangle_roi(annotation["bbox"], width, height, padding=1)
    return roi, envelope, "existing_mask_bbox_fallback"


def annotation_envelope_roi(
    annotation: dict[str, Any],
    image_name: str,
    coo_records: list[dict[str, Any]],
    width: int,
    height: int,
) -> tuple[tuple[int, int, int, int], np.ndarray, str]:
    if int(annotation["category_id"]) == 1:
        box = annotation.get("v2026_bbox") or annotation["bbox"]
        roi, envelope = rectangle_roi(box, width, height, padding=0)
        return roi, envelope, "human_v2026_bbox" if annotation.get("v2026_bbox") else "existing_text_bbox"
    return coo_envelope_roi(annotation, image_name, coo_records, width, height)


def refinement_policy(annotation: dict[str, Any], teacher_source: str) -> str:
    if teacher_source == "zenodo":
        return "replace_with_zenodo"
    tier = str(annotation.get("attributes", {}).get("quality_tier", ""))
    if "fallback" in tier or tier == "gold_polygon" or tier.startswith("silver_"):
        return "replace_fallback_with_textseg"
    return "union_existing_with_textseg"


def refine_annotation(
    annotation: dict[str, Any],
    teacher: np.ndarray,
    teacher_source: str,
    image_name: str,
    coo_records: list[dict[str, Any]],
    width: int,
    height: int,
) -> tuple[dict[str, Any], np.ndarray, dict[str, Any]]:
    old_full = decode_rle(annotation["segmentation"])
    roi, envelope, envelope_source = annotation_envelope_roi(
        annotation, image_name, coo_records, width, height
    )
    x0, y0, x1, y1 = roi
    if x1 <= x0 or y1 <= y0:
        return annotation, old_full, {
            "teacher_source": teacher_source,
            "envelope_source": envelope_source,
            "policy": "retain_existing_invalid_envelope",
            "minimum_teacher_pixels": MINIMUM_TEACHER_PIXELS,
            "teacher_pixels_clipped": 0,
            "old_pixels_clipped": int(old_full.sum()),
            "final_pixels": int(old_full.sum()),
            "added_pixels": 0,
            "removed_pixels": 0,
            "old_final_iou": 1.0,
        }
    old = old_full[y0:y1, x0:x1] & envelope
    clipped = teacher[y0:y1, x0:x1] & envelope
    policy = refinement_policy(annotation, teacher_source)
    minimum = max(MINIMUM_TEACHER_PIXELS, int(math.ceil(envelope.sum() * 0.001)))
    if int(clipped.sum()) < minimum:
        refined = old
        applied = "retain_existing_teacher_too_sparse"
    elif policy.startswith("replace"):
        refined = clipped
        applied = policy
    else:
        refined = old | clipped
        applied = policy
    if not refined.any():
        refined = old
        applied = "retain_existing_empty_refinement"
    final_mask = np.zeros((height, width), dtype=bool)
    final_mask[y0:y1, x0:x1] = refined
    if not final_mask.any():
        final_mask = old_full
    bbox, area = mask_geometry(final_mask)
    attributes = dict(annotation.get("attributes", {}))
    previous_quality = str(attributes.get("quality_tier", "unspecified"))
    comparison = pair_counts(old, refined)
    metadata = {
        "teacher_source": teacher_source,
        "envelope_source": envelope_source,
        "policy": applied,
        "minimum_teacher_pixels": minimum,
        "teacher_pixels_clipped": int(clipped.sum()),
        "old_pixels_clipped": int(old.sum()),
        "final_pixels": area,
        "added_pixels": int((refined & ~old).sum()),
        "removed_pixels": int((old & ~refined).sum()),
        "old_final_iou": comparison["iou"],
    }
    attributes["previous_quality_tier"] = previous_quality
    attributes["quality_tier"] = {
        "zenodo": "gold_zenodo_refined",
        "textseg": "silver_textseg_refined",
    }[teacher_source]
    attributes["textseg_refinement"] = metadata
    updated = {
        **annotation,
        "bbox": bbox,
        "area": area,
        "segmentation": encode_rle(final_mask),
        "attributes": attributes,
        "source_dataset": str(annotation.get("source_dataset", "Manga109"))
        + ("+Zenodo4511796" if teacher_source == "zenodo" else "+KoharuTextSAMTSL"),
    }
    return updated, final_mask, metadata


def pp_pseudo_annotations(
    image: dict[str, Any],
    teacher: np.ndarray,
    teacher_source: str,
    existing_annotations: list[dict[str, Any]],
    assigned: np.ndarray,
    pp_root: Path,
    positive_score: float,
    max_gold_coverage: float,
) -> tuple[list[dict[str, Any]], np.ndarray, list[dict[str, Any]], Counter[str]]:
    width, height = int(image["width"]), int(image["height"])
    image_name = str(image["file_name"])
    record_path = pp_root / "records" / Path(image_name).with_suffix(".json")
    empty = np.zeros((height, width), dtype=bool)
    if not record_path.is_file():
        return [], empty, [], Counter(pp_record_missing=1)
    record = json.loads(record_path.read_text(encoding="utf-8"))
    predictions = record.get("predictions", [])
    candidates: list[tuple[float, list[int], dict[str, Any]]] = []
    stats: Counter[str] = Counter()
    for prediction in predictions:
        score = float(prediction.get("score", 0.0))
        if score < positive_score:
            stats["below_score"] += 1
            continue
        box = bbox_from_polygon(prediction, width, height)
        if box is None:
            stats["invalid_box"] += 1
            continue
        candidates.append((score, box, prediction))
    candidates.sort(key=lambda value: value[0], reverse=True)

    gold_boxes = []
    for annotation in existing_annotations:
        x, y, box_width, box_height = map(float, annotation["bbox"])
        gold_boxes.append((x, y, x + box_width, y + box_height))
    pseudo_claimed = empty.copy()
    annotations: list[dict[str, Any]] = []
    audit_boxes: list[dict[str, Any]] = []
    for score, box, prediction in candidates:
        x, y, box_width, box_height = box
        xyxy = (float(x), float(y), float(x + box_width), float(y + box_height))
        coverage = box_gold_coverage(xyxy, gold_boxes)
        if coverage >= max_gold_coverage:
            stats["gold_overlap"] += 1
            continue
        mask_roi = teacher[y : y + box_height, x : x + box_width].copy()
        mask_roi &= ~assigned[y : y + box_height, x : x + box_width]
        mask_roi &= ~pseudo_claimed[y : y + box_height, x : x + box_width]
        mask_roi = remove_tiny_components(mask_roi, minimum_area=2)
        if int(mask_roi.sum()) < PP_MINIMUM_MASK_PIXELS:
            stats["insufficient_textseg_ink"] += 1
            continue
        rows, columns = np.where(mask_roi)
        tight_box = [
            x + int(columns.min()),
            y + int(rows.min()),
            int(columns.max() - columns.min() + 1),
            int(rows.max() - rows.min() + 1),
        ]
        area = int(mask_roi.sum())
        mask = np.zeros_like(teacher)
        mask[y : y + box_height, x : x + box_width] = mask_roi
        annotation = {
            "id": None,
            "image_id": int(image["id"]),
            "category_id": 1,
            "bbox": tight_box,
            "segmentation": encode_rle(mask),
            "area": area,
            # Stock RF-DETR keeps only iscrowd=0 annotations.  This is a
            # positive pseudo-instance, not a custom supervision marker.
            "iscrowd": 0,
            "source_dataset": "PP-DocLayoutV3-box+"
            + ("Zenodo4511796" if teacher_source == "zenodo" else "KoharuTextSAMTSL"),
            "attributes": {
                "quality_tier": "silver_pp_bbox_textseg_mask",
                "class_authority": "PP-DocLayoutV3 normal-text bbox",
                "mask_authority": teacher_source,
                "proposal_bbox": box,
                "proposal_score": score,
                "proposal_label": prediction.get("label"),
                "gold_box_coverage": coverage,
            },
        }
        annotations.append(annotation)
        audit_boxes.append(
            {
                "proposal_bbox": box,
                "tight_mask_bbox": tight_box,
                "score": score,
                "mask_pixels": area,
            }
        )
        pseudo_claimed[y : y + box_height, x : x + box_width] |= mask_roi
        stats["accepted"] += 1
    return annotations, pseudo_claimed, audit_boxes, stats


def initialize_worker() -> None:
    cv2.setNumThreads(1)


def process_page_task(
    task: tuple[Any, ...]
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], dict[str, Any], dict[str, int]]:
    (
        split,
        image,
        page_annotations,
        teacher_source,
        teacher_path_raw,
        coo_records,
        pp_root_raw,
        positive_score,
        max_gold_coverage,
        output_raw,
        images_root_raw,
        audit_artifacts,
        drop_empty_target_teacher_pixels,
        drop_low_coverage_teacher_pixels,
        drop_low_coverage_recall,
    ) = task
    teacher_path = Path(teacher_path_raw)
    pp_root = Path(pp_root_raw)
    output = Path(output_raw)
    images_root = Path(images_root_raw)
    image_name = str(image["file_name"])
    width, height = int(image["width"]), int(image["height"])
    teacher = load_mask(teacher_path, width, height)
    updated_page: list[dict[str, Any]] = []
    old_union = np.zeros((height, width), dtype=bool)
    new_gold_union = np.zeros((height, width), dtype=bool)
    page_policy: Counter[str] = Counter()
    counts: Counter[str] = Counter()
    typography_count = 0
    for annotation in page_annotations:
        if int(annotation["category_id"]) not in (1, 2):
            updated_page.append(annotation)
            continue
        typography_count += 1
        old_union |= decode_rle(annotation["segmentation"])
        updated, mask, metadata = refine_annotation(
            annotation,
            teacher,
            teacher_source,
            image_name,
            coo_records,
            width,
            height,
        )
        updated_page.append(updated)
        new_gold_union |= mask
        page_policy[metadata["policy"]] += 1
        counts[f"refinement_{metadata['policy']}"] += 1

    pseudo: list[dict[str, Any]] = []
    pseudo_union = np.zeros((height, width), dtype=bool)
    audit_boxes: list[dict[str, Any]] = []
    if split == "train":
        typography_annotations = [
            annotation
            for annotation in updated_page
            if int(annotation["category_id"]) in (1, 2)
        ]
        pseudo, pseudo_union, audit_boxes, pp_counts = pp_pseudo_annotations(
            image,
            teacher,
            teacher_source,
            typography_annotations,
            new_gold_union,
            pp_root,
            positive_score,
            max_gold_coverage,
        )
        counts.update({f"pp_{key}": value for key, value in pp_counts.items()})

    training_union = new_gold_union | pseudo_union
    old_vs_new = pair_counts(old_union, training_union)
    teacher_vs_old = pair_counts(teacher, old_union)
    teacher_vs_new = pair_counts(teacher, training_union)
    teacher_pixels = teacher_vs_new["reference_pixels"]
    final_pixels = teacher_vs_new["prediction_pixels"]
    exclusion_reasons: list[str] = []
    if final_pixels == 0 and teacher_pixels >= drop_empty_target_teacher_pixels:
        exclusion_reasons.append("empty_target_with_teacher_foreground")
    if (
        teacher_pixels >= drop_low_coverage_teacher_pixels
        and teacher_vs_new["recall"] < drop_low_coverage_recall
    ):
        exclusion_reasons.append("low_teacher_coverage")
    page_area = max(1, width * height)
    added_pixels = int((training_union & ~old_union).sum())
    removed_pixels = int((old_union & ~training_union).sum())
    review_flags: list[str] = []
    if final_pixels / page_area >= 0.10:
        review_flags.append("final_mask_ratio_at_least_10pct")
    if added_pixels / page_area >= 0.08:
        review_flags.append("added_mask_ratio_at_least_8pct")
    relative = Path(image_name).with_suffix(".png")
    audit_paths: dict[str, str] = {}
    if audit_artifacts:
        for name, mask in (
            ("old_union", old_union),
            ("new_union", training_union),
            ("teacher", teacher),
            ("pp_pseudo", pseudo_union),
        ):
            destination = Path("audit") / name / relative
            save_mask(mask, output / destination)
            audit_paths[name] = destination.as_posix()
    audit_record = {
        "split": split,
        "image": image_name,
        "source_path": str(images_root / image_name),
        "teacher_source": teacher_source,
        "teacher_path": str(teacher_path),
        "width": width,
        "height": height,
        "existing_typography_instances": typography_count,
        "pp_pseudo_instances": len(pseudo),
        "pp_boxes": audit_boxes,
        "refinement_policies": dict(page_policy),
        "old_vs_new": old_vs_new,
        "teacher_vs_old": teacher_vs_old,
        "teacher_vs_new": teacher_vs_new,
        "added_pixels": added_pixels,
        "removed_pixels": removed_pixels,
        "excluded_from_dataset": bool(exclusion_reasons),
        "exclusion_reasons": exclusion_reasons,
        "review_flags": review_flags,
        "audit_masks": audit_paths,
    }
    counts[f"pages_{teacher_source}"] += 1
    if exclusion_reasons:
        counts["pages_excluded"] += 1
        for reason in exclusion_reasons:
            counts[f"excluded_{reason}"] += 1
    else:
        counts["pages_retained"] += 1
    for flag in review_flags:
        counts[f"review_{flag}"] += 1
    return updated_page, pseudo, audit_record, dict(counts)


def process_split(
    split: str,
    dataset_root: Path,
    output: Path,
    images_root: Path,
    teacher_paths: dict[str, tuple[str, Path]],
    coo_by_image: dict[str, list[dict[str, Any]]],
    pp_root: Path,
    positive_score: float,
    max_gold_coverage: float,
    only_available: bool,
    audit_artifacts: bool,
    workers: int,
    drop_empty_target_teacher_pixels: int,
    drop_low_coverage_teacher_pixels: int,
    drop_low_coverage_recall: float,
) -> dict[str, Any]:
    source_path = dataset_root / "annotations" / f"{split}.coco.json"
    source = json.loads(source_path.read_text(encoding="utf-8"))
    annotations_by_image: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for annotation in source["annotations"]:
        annotations_by_image[int(annotation["image_id"])].append(annotation)
    candidate_images = [
        image
        for image in source["images"]
        if not only_available or str(image["file_name"]) in teacher_paths
    ]
    maximum_id = max((int(annotation["id"]) for annotation in source["annotations"]), default=0)
    next_id = maximum_id + 1
    output_annotations: list[dict[str, Any]] = []
    selected_images: list[dict[str, Any]] = []
    audit_records: list[dict[str, Any]] = []
    counts: Counter[str] = Counter()
    started = time.perf_counter()

    tasks = []
    task_images: list[dict[str, Any]] = []
    for image in candidate_images:
        image_name = str(image["file_name"])
        teacher_info = teacher_paths.get(image_name)
        if teacher_info is None:
            selected_images.append(image)
            output_annotations.extend(annotations_by_image[int(image["id"])])
            counts["pages_without_teacher"] += 1
            continue
        teacher_source, teacher_path = teacher_info
        tasks.append(
            (
                split,
                image,
                annotations_by_image[int(image["id"])],
                teacher_source,
                str(teacher_path),
                coo_by_image.get(image_name, []),
                str(pp_root),
                positive_score,
                max_gold_coverage,
                str(output),
                str(images_root),
                audit_artifacts,
                drop_empty_target_teacher_pixels,
                drop_low_coverage_teacher_pixels,
                drop_low_coverage_recall,
            )
        )
        task_images.append(image)
    # Release the original annotation list before worker results accumulate.
    source["annotations"] = []
    with concurrent.futures.ProcessPoolExecutor(
        max_workers=workers, initializer=initialize_worker
    ) as executor:
        results = executor.map(process_page_task, tasks, chunksize=2)
        for index, (updated_page, pseudo, audit_record, page_counts) in enumerate(results, 1):
            audit_records.append(audit_record)
            counts.update(page_counts)
            if index == len(tasks) or index % 100 == 0:
                print(
                    json.dumps(
                        {
                            "event": "build_progress",
                            "split": split,
                            "completed": index,
                            "total": len(tasks),
                            "workers": workers,
                        }
                    ),
                    flush=True,
                )
            if audit_record["excluded_from_dataset"]:
                continue
            selected_images.append(task_images[index - 1])
            for annotation in pseudo:
                annotation["id"] = next_id
                next_id += 1
            output_annotations.extend(updated_page)
            output_annotations.extend(pseudo)

    source["info"] = {
        **source.get("info", {}),
        "version": "1.4.0-textseg-filtered",
        "date_created": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "annotation_policy": (
            "Human text/COO geometry is authoritative; Zenodo or Koharu TextSeg supplies ink; "
            "PP-DocLayoutV3 supplies rectangular proposals only."
        ),
    }
    source["images"] = selected_images
    source["annotations"] = output_annotations
    valid_annotation_ids = {int(annotation["id"]) for annotation in output_annotations}
    source["relations"] = [
        relation
        for relation in source.get("relations", [])
        if int(relation["from"]) in valid_annotation_ids
        and int(relation["to"]) in valid_annotation_ids
    ]
    annotation_output = output / "annotations" / f"{split}.coco.json"
    annotation_output.parent.mkdir(parents=True, exist_ok=True)
    annotation_output.write_text(
        json.dumps(source, ensure_ascii=False, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    audit_output = output / "audit" / f"{split}.jsonl"
    audit_output.parent.mkdir(parents=True, exist_ok=True)
    with audit_output.open("w", encoding="utf-8", newline="\n") as stream:
        for record in audit_records:
            stream.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
    return {
        "split": split,
        "candidate_images": len(candidate_images),
        "images": len(selected_images),
        "excluded_images": len(candidate_images) - len(selected_images),
        "annotations": len(output_annotations),
        "relations": len(source["relations"]),
        "audit_records": len(audit_records),
        "counts": dict(counts),
        "seconds": time.perf_counter() - started,
    }


def main() -> None:
    args = parse_args()
    dataset_root = args.dataset.resolve()
    images_root = args.images_root.resolve()
    textseg_root = args.textseg_root.resolve()
    zenodo_root = args.zenodo_root.resolve()
    pp_root = args.pp_root.resolve()
    output = args.output.resolve()
    if not 0 <= args.pp_max_gold_box_coverage <= 1:
        raise ValueError("PP gold coverage must be in [0, 1]")
    if args.drop_empty_target_teacher_pixels < 0:
        raise ValueError("--drop-empty-target-teacher-pixels must be non-negative")
    if args.drop_low_coverage_teacher_pixels < 0:
        raise ValueError("--drop-low-coverage-teacher-pixels must be non-negative")
    if not 0 <= args.drop_low_coverage_recall <= 1:
        raise ValueError("--drop-low-coverage-recall must be in [0, 1]")
    if args.workers < 1:
        raise ValueError("--workers must be at least 1")
    prepare_output(output, args.overwrite)
    zenodo = load_zenodo(zenodo_root)
    textseg = load_textseg(textseg_root)
    overlap = set(zenodo) & set(textseg)
    if overlap:
        raise ValueError(f"TextSeg cache must exclude Zenodo pages; overlap={len(overlap)}")
    teacher_paths = {
        **{image: ("textseg", path) for image, path in textseg.items()},
        **{image: ("zenodo", path) for image, path in zenodo.items()},
    }
    if not args.only_available_pages and len(teacher_paths) != 10602:
        raise RuntimeError(
            f"full build requires 10,602 teacher masks, found {len(teacher_paths)}"
        )
    coo_by_image = load_coo_records(args.coo_annotations.resolve())
    summaries = []
    for split in SPLITS:
        summaries.append(
            process_split(
                split,
                dataset_root,
                output,
                images_root,
                teacher_paths,
                coo_by_image,
                pp_root,
                args.pp_positive_score,
                args.pp_max_gold_box_coverage,
                args.only_available_pages,
                args.audit_artifacts,
                args.workers,
                args.drop_empty_target_teacher_pixels,
                args.drop_low_coverage_teacher_pixels,
                args.drop_low_coverage_recall,
            )
        )
    manifest = {
        "schema_version": 1,
        "name": "manga109-segmentation-textseg",
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "sources": {
            "base_dataset": str(dataset_root),
            "images": str(images_root),
            "zenodo_masks": str(zenodo_root),
            "textseg_masks": str(textseg_root),
            "pp_boxes": str(pp_root),
        },
        "teacher_inventory": {
            "zenodo": len(zenodo),
            "textseg": len(textseg),
            "combined": len(teacher_paths),
        },
        "policy": {
            "human_geometry": "authoritative text boxes and COO polygons",
            "good_existing_masks": "union with clipped TextSeg ink",
            "geometry_fallbacks": "replace with clipped TextSeg ink when supported",
            "zenodo": "replace with clipped manually painted ink when supported",
            "pp_doclayoutv3": "bbox proposal only; mask is teacher ink clipped to bbox; output bbox is tight mask geometry",
            "pp_positive_score": args.pp_positive_score,
            "pp_max_gold_box_coverage": args.pp_max_gold_box_coverage,
            "page_filter": {
                "empty_target_teacher_pixels": args.drop_empty_target_teacher_pixels,
                "low_coverage_teacher_pixels": args.drop_low_coverage_teacher_pixels,
                "low_coverage_recall": args.drop_low_coverage_recall,
                "purpose": "remove incomplete pages that would become false-negative COCO background",
            },
            "negative_supervision": (
                "standard RF-DETR implicit background only; no synthetic negative mask class"
            ),
        },
        "pilot_only_available_pages": args.only_available_pages,
        "audit_artifacts": args.audit_artifacts,
        "workers": args.workers,
        "splits": {summary["split"]: summary for summary in summaries},
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps({"event": "complete", "output": str(output)}), flush=True)


if __name__ == "__main__":
    main()
