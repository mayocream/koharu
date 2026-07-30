# /// script
# requires-python = ">=3.11,<3.15"
# dependencies = [
#   "numpy>=2.0",
#   "opencv-python-headless>=4.10",
#   "pillow>=11.0",
#   "pycocotools>=2.0.10",
#   "pyyaml>=6.0",
#   "tqdm>=4.67",
# ]
# ///
"""Prepare MangaSeg as a book-disjoint YOLO instance-segmentation dataset.

The generated class order preserves the first three classes in the ShadowB
YOLO26s checkpoint and adds MangaSeg's onomatopoeia masks as class 3:

    0 frame
    1 dialogue_text
    2 balloon
    3 onomatopoeia_text

MangaSeg stores masks as COCO RLE. YOLO's text label format accepts one polygon
per instance, so disconnected RLE components are joined with the same thin-line
strategy used by Ultralytics' COCO converter.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import shutil
import sys
from collections import Counter, defaultdict
from concurrent.futures import ProcessPoolExecutor, as_completed
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import cv2
import numpy as np
import yaml
from PIL import Image, ImageDraw, ImageFont
from pycocotools import mask as mask_utils
from tqdm import tqdm


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANGASEG_ROOT = REPOSITORY_ROOT / "data" / "datasets" / "mangaseg"
DEFAULT_MANGA109_IMAGES = (
    REPOSITORY_ROOT / "data" / "Manga109_released_2021_12_30" / "images"
)
DEFAULT_SPLIT_AUDIT = (
    REPOSITORY_ROOT
    / "data"
    / "models"
    / "shadowb-comic-mask-yolo26s"
    / "book_split_audit.csv"
)
DEFAULT_OUTPUT = REPOSITORY_ROOT / "data" / "datasets" / "comic-mask-yolo26s"

SCHEMA_VERSION = 2
SOURCE_TO_TARGET = {
    1: (0, "frame"),
    2: (1, "dialogue_text"),
    5: (2, "balloon"),
    6: (3, "onomatopoeia_text"),
}
CLASS_NAMES = {target_id: name for _, (target_id, name) in SOURCE_TO_TARGET.items()}
SPLITS = ("train", "val", "test")
SPLIT_BOOK_ALIASES = {
    # The checkpoint audit sanitized two apostrophes and shortened BEMADER_P.
    "BEMADER": "BEMADER_P",
    "That_sIzumiko": "That'sIzumiko",
    "UchiNoNyan_sDiary": "UchiNoNyan'sDiary",
}
VISUAL_COLORS = {
    0: (64, 164, 255),
    1: (255, 70, 92),
    2: (58, 214, 116),
    3: (255, 190, 45),
}


@dataclass
class BookStats:
    book: str
    split: str
    images: int = 0
    empty_images: int = 0
    missing_source_images: int = 0
    source_annotations: int = 0
    instances: Counter = field(default_factory=Counter)
    dropped_annotations: Counter = field(default_factory=Counter)
    duplicate_annotations: Counter = field(default_factory=Counter)
    polygon_points: Counter = field(default_factory=Counter)
    source_pixels: Counter = field(default_factory=Counter)
    polygon_iou_sum: Counter = field(default_factory=Counter)
    polygon_iou_count: Counter = field(default_factory=Counter)
    polygon_iou_min: dict[str, float] = field(default_factory=dict)
    manifest_rows: list[dict[str, Any]] = field(default_factory=list)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mangaseg-root", type=Path, default=DEFAULT_MANGASEG_ROOT)
    parser.add_argument("--image-root", type=Path, default=DEFAULT_MANGA109_IMAGES)
    parser.add_argument("--split-audit", type=Path, default=DEFAULT_SPLIT_AUDIT)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--workers", type=int, default=min(8, os.cpu_count() or 1))
    parser.add_argument(
        "--link-mode",
        choices=("hardlink", "copy"),
        default="hardlink",
        help="How source images are materialized under the generated dataset.",
    )
    parser.add_argument(
        "--simplify-pixels",
        type=float,
        default=0.5,
        help="Douglas-Peucker contour tolerance in source-image pixels; use 0 to disable.",
    )
    parser.add_argument(
        "--visual-samples",
        type=int,
        default=12,
        help="Number of deterministic labeled previews to render.",
    )
    parser.add_argument(
        "--books",
        nargs="*",
        help="Optional subset for conversion debugging. The production run should omit this.",
    )
    parser.add_argument(
        "--overwrite",
        action="store_true",
        help="Permit replacing labels and metadata in an existing generated dataset.",
    )
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_splits(path: Path) -> tuple[dict[str, str], dict[str, int]]:
    split_by_book: dict[str, str] = {}
    expected_images: dict[str, int] = {}
    with path.open(newline="", encoding="utf-8-sig") as file:
        for row in csv.DictReader(file):
            book = SPLIT_BOOK_ALIASES.get(row["group_id"], row["group_id"])
            split = row["split"]
            if split not in SPLITS:
                raise ValueError(f"unsupported split {split!r} for {book}")
            if book in split_by_book:
                raise ValueError(f"duplicate book in split audit: {book}")
            split_by_book[book] = split
            expected_images[book] = int(row["image_count"])
    return split_by_book, expected_images


def validate_inputs(
    annotation_root: Path,
    image_root: Path,
    split_by_book: dict[str, str],
    selected_books: set[str] | None,
) -> list[tuple[Path, str]]:
    json_paths = sorted((annotation_root / "jsons").glob("*.json"))
    if not json_paths:
        raise FileNotFoundError(
            f"no MangaSeg JSON files found under {annotation_root / 'jsons'}"
        )

    annotation_books = {path.stem for path in json_paths}
    image_books = {path.name for path in image_root.iterdir() if path.is_dir()}
    split_books = set(split_by_book)
    if annotation_books != image_books or annotation_books != split_books:
        raise ValueError(
            "book sets differ:\n"
            f"  annotations only: {sorted(annotation_books - image_books - split_books)}\n"
            f"  images only: {sorted(image_books - annotation_books)}\n"
            f"  split only: {sorted(split_books - annotation_books)}"
        )

    if selected_books is not None:
        unknown = selected_books - annotation_books
        if unknown:
            raise ValueError(f"unknown --books values: {sorted(unknown)}")
        json_paths = [path for path in json_paths if path.stem in selected_books]

    return [(path, split_by_book[path.stem]) for path in json_paths]


# Ported from Ultralytics 8.4.43. The thin, backtracked connections let a single
# YOLO polygon retain disconnected RLE components without filling their gaps.
# https://github.com/ultralytics/ultralytics/blob/v8.4.43/ultralytics/data/converter.py#L532-L574
def min_index(first: np.ndarray, second: np.ndarray) -> tuple[int, int]:
    distances = ((first[:, None, :] - second[None, :, :]) ** 2).sum(-1)
    return np.unravel_index(np.argmin(distances, axis=None), distances.shape)


def merge_multi_segment(segments: list[np.ndarray]) -> list[np.ndarray]:
    merged: list[np.ndarray] = []
    segments = [np.asarray(segment).reshape(-1, 2) for segment in segments]
    index_list: list[list[int]] = [[] for _ in segments]

    for index in range(1, len(segments)):
        first_index, second_index = min_index(segments[index - 1], segments[index])
        index_list[index - 1].append(int(first_index))
        index_list[index].append(int(second_index))

    for pass_index in range(2):
        if pass_index == 0:
            for index, connection_indices in enumerate(index_list):
                if (
                    len(connection_indices) == 2
                    and connection_indices[0] > connection_indices[1]
                ):
                    connection_indices = connection_indices[::-1]
                    segments[index] = segments[index][::-1, :]

                segments[index] = np.roll(
                    segments[index], -connection_indices[0], axis=0
                )
                segments[index] = np.concatenate([segments[index], segments[index][:1]])
                if index in {0, len(index_list) - 1}:
                    merged.append(segments[index])
                else:
                    start, end = 0, connection_indices[1] - connection_indices[0]
                    merged.append(segments[index][start : end + 1])
        else:
            for index in range(len(index_list) - 1, -1, -1):
                if index not in {0, len(index_list) - 1}:
                    connection_indices = index_list[index]
                    connection_length = abs(
                        connection_indices[1] - connection_indices[0]
                    )
                    merged.append(segments[index][connection_length:])
    return merged


def clean_contour(contour: np.ndarray, simplify_pixels: float) -> np.ndarray | None:
    contour = np.asarray(contour, dtype=np.int32).reshape(-1, 1, 2)
    if simplify_pixels > 0 and len(contour) >= 4:
        contour = cv2.approxPolyDP(contour, simplify_pixels, closed=True)
    points = contour.reshape(-1, 2)
    if len(points) < 3:
        x, y, width, height = cv2.boundingRect(contour)
        if width <= 1 or height <= 1:
            return None
        points = np.array(
            [
                [x, y],
                [x + width - 1, y],
                [x + width - 1, y + height - 1],
                [x, y + height - 1],
            ],
            dtype=np.int32,
        )
    return points


def mask_to_polygon(mask: np.ndarray, simplify_pixels: float) -> np.ndarray | None:
    contours, _ = cv2.findContours(
        mask.astype(np.uint8), cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE
    )
    segments = [
        segment
        for contour in contours
        if (segment := clean_contour(contour, simplify_pixels)) is not None
    ]
    if not segments:
        return None

    # Stable spatial order makes output deterministic. The upstream connector
    # then chooses the closest point between adjacent components.
    segments.sort(key=lambda points: (int(points[:, 0].min()), int(points[:, 1].min())))
    if len(segments) == 1:
        polygon = segments[0]
    else:
        polygon = np.concatenate(merge_multi_segment(segments), axis=0)

    if len(polygon) < 3:
        return None
    keep = np.ones(len(polygon), dtype=bool)
    keep[1:] = np.any(polygon[1:] != polygon[:-1], axis=1)
    polygon = polygon[keep]
    return polygon if len(polygon) >= 3 else None


def polygon_iou(mask: np.ndarray, polygon: np.ndarray) -> float:
    rendered = np.zeros(mask.shape, dtype=np.uint8)
    cv2.fillPoly(rendered, [polygon.astype(np.int32)], 1)
    source = mask > 0
    rendered = rendered > 0
    union = np.logical_or(source, rendered).sum()
    return 1.0 if union == 0 else float(np.logical_and(source, rendered).sum() / union)


def materialize_image(source: Path, destination: Path, link_mode: str) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        if destination.stat().st_size != source.stat().st_size:
            raise FileExistsError(f"existing image differs from source: {destination}")
        return
    if link_mode == "hardlink":
        try:
            os.link(source, destination)
            return
        except OSError:
            pass
    shutil.copy2(source, destination)


def process_book(
    json_path_text: str,
    split: str,
    image_root_text: str,
    output_text: str,
    link_mode: str,
    simplify_pixels: float,
) -> BookStats:
    json_path = Path(json_path_text)
    image_root = Path(image_root_text)
    output = Path(output_text)
    book = json_path.stem
    stats = BookStats(book=book, split=split)

    with json_path.open(encoding="utf-8") as file:
        document = json.load(file)

    category_names = {
        int(category["id"]): category["name"] for category in document["categories"]
    }
    expected_categories = {1: "frame", 2: "text", 5: "balloon", 6: "onomatopoeia"}
    for category_id, expected_name in expected_categories.items():
        if category_names.get(category_id) != expected_name:
            raise ValueError(f"{book}: category {category_id} is not {expected_name!r}")

    book_images = [
        image_info
        for image_info in document["images"]
        if Path(image_info["file_name"]).parts[0] == book
    ]
    if not book_images:
        raise ValueError(
            f"{book}: the cumulative MangaSeg image index contains no images for this book"
        )
    book_image_ids = {int(image_info["id"]) for image_info in book_images}

    annotations_by_image: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for annotation in document["annotations"]:
        image_id = int(annotation["image_id"])
        if image_id in book_image_ids:
            annotations_by_image[image_id].append(annotation)
    stats.source_annotations = sum(
        len(annotations) for annotations in annotations_by_image.values()
    )

    for image_info in book_images:
        image_id = int(image_info["id"])
        relative_image = Path(image_info["file_name"])
        source_image = image_root / relative_image
        if not source_image.is_file():
            mapped_annotations = [
                annotation
                for annotation in annotations_by_image.get(image_id, [])
                if int(annotation["category_id"]) in SOURCE_TO_TARGET
            ]
            if mapped_annotations:
                raise FileNotFoundError(
                    f"{source_image} is missing but has target annotations"
                )
            # MangaSeg contains a small number of trailing image records that
            # are absent from the released Manga109 archive and have no target
            # masks. They cannot be materialized and are safe to omit.
            stats.missing_source_images += 1
            continue

        with Image.open(source_image) as image:
            actual_size = image.size
        expected_size = (int(image_info["width"]), int(image_info["height"]))
        if actual_size != expected_size:
            raise ValueError(
                f"{source_image}: image size {actual_size} != annotation size {expected_size}"
            )

        destination_image = output / "images" / split / relative_image
        destination_label = (
            output / "labels" / split / relative_image.with_suffix(".txt")
        )
        materialize_image(source_image, destination_image, link_mode)
        destination_label.parent.mkdir(parents=True, exist_ok=True)

        lines: list[str] = []
        seen_lines: set[str] = set()
        image_counts: Counter = Counter()
        annotations = annotations_by_image.get(image_id, [])
        for annotation_index, annotation in enumerate(annotations):
            source_category_id = int(annotation["category_id"])
            mapping = SOURCE_TO_TARGET.get(source_category_id)
            if mapping is None:
                continue
            target_class_id, target_class_name = mapping

            segmentation = dict(annotation["segmentation"])
            if isinstance(segmentation.get("counts"), str):
                segmentation["counts"] = segmentation["counts"].encode("ascii")
            mask = np.asarray(mask_utils.decode(segmentation), dtype=np.uint8)
            if mask.ndim == 3:
                mask = np.any(mask, axis=2).astype(np.uint8)
            if mask.shape != (expected_size[1], expected_size[0]):
                raise ValueError(
                    f"{book} image {image_id}: decoded RLE has shape {mask.shape}"
                )

            polygon = mask_to_polygon(mask, simplify_pixels)
            if polygon is None:
                stats.dropped_annotations[target_class_name] += 1
                continue

            width, height = expected_size
            normalized = polygon.astype(np.float64) / np.array(
                [width, height], dtype=np.float64
            )
            normalized = np.clip(normalized, 0.0, 1.0)
            coordinates = " ".join(
                f"{coordinate:.6f}" for coordinate in normalized.reshape(-1)
            )
            label_line = f"{target_class_id} {coordinates}"
            if label_line in seen_lines:
                stats.duplicate_annotations[target_class_name] += 1
                continue
            seen_lines.add(label_line)
            lines.append(label_line)

            image_counts[target_class_name] += 1
            stats.instances[target_class_name] += 1
            stats.polygon_points[target_class_name] += len(polygon)
            stats.source_pixels[target_class_name] += int(mask.sum())

            # Two deterministic geometry checks per class and book keep the
            # full run inexpensive while still auditing every annotation file.
            if stats.polygon_iou_count[target_class_name] < 2:
                iou = polygon_iou(mask, polygon)
                stats.polygon_iou_sum[target_class_name] += iou
                stats.polygon_iou_count[target_class_name] += 1
                stats.polygon_iou_min[target_class_name] = min(
                    stats.polygon_iou_min.get(target_class_name, 1.0), iou
                )

        label_text = "\n".join(lines)
        if label_text:
            label_text += "\n"
        temporary_label = destination_label.with_suffix(".txt.tmp")
        temporary_label.write_text(label_text, encoding="utf-8")
        temporary_label.replace(destination_label)

        if not lines:
            stats.empty_images += 1
        stats.images += 1
        stats.manifest_rows.append(
            {
                "split": split,
                "book": book,
                "image_id": image_id,
                "image": destination_image.relative_to(output).as_posix(),
                "label": destination_label.relative_to(output).as_posix(),
                **{name: image_counts[name] for name in CLASS_NAMES.values()},
            }
        )

    return stats


def aggregate_stats(results: list[BookStats]) -> dict[str, Any]:
    split_books: Counter = Counter()
    split_images: Counter = Counter()
    split_empty_images: Counter = Counter()
    split_missing_source_images: Counter = Counter()
    class_instances: Counter = Counter()
    dropped_annotations: Counter = Counter()
    duplicate_annotations: Counter = Counter()
    polygon_points: Counter = Counter()
    source_pixels: Counter = Counter()
    iou_sum: Counter = Counter()
    iou_count: Counter = Counter()
    iou_min: dict[str, float] = {}

    for result in results:
        split_books[result.split] += 1
        split_images[result.split] += result.images
        split_empty_images[result.split] += result.empty_images
        split_missing_source_images[result.split] += result.missing_source_images
        class_instances.update(result.instances)
        dropped_annotations.update(result.dropped_annotations)
        duplicate_annotations.update(result.duplicate_annotations)
        polygon_points.update(result.polygon_points)
        source_pixels.update(result.source_pixels)
        iou_sum.update(result.polygon_iou_sum)
        iou_count.update(result.polygon_iou_count)
        for class_name, value in result.polygon_iou_min.items():
            iou_min[class_name] = min(iou_min.get(class_name, 1.0), value)

    geometry = {}
    for class_name in CLASS_NAMES.values():
        instances = class_instances[class_name]
        geometry[class_name] = {
            "instances": instances,
            "dropped_annotations": dropped_annotations[class_name],
            "duplicate_annotations": duplicate_annotations[class_name],
            "mean_polygon_points": polygon_points[class_name] / instances
            if instances
            else 0.0,
            "source_pixels": source_pixels[class_name],
            "sample_polygon_iou_mean": iou_sum[class_name] / iou_count[class_name]
            if iou_count[class_name]
            else 0.0,
            "sample_polygon_iou_min": iou_min.get(class_name, 0.0),
            "sample_polygon_iou_count": iou_count[class_name],
        }

    return {
        "books": {split: split_books[split] for split in SPLITS},
        "images": {split: split_images[split] for split in SPLITS},
        "empty_images": {split: split_empty_images[split] for split in SPLITS},
        "missing_source_images": {
            split: split_missing_source_images[split] for split in SPLITS
        },
        "classes": geometry,
    }


def write_metadata(
    output: Path,
    split_audit: Path,
    mangaseg_root: Path,
    image_root: Path,
    simplify_pixels: float,
    results: list[BookStats],
    aggregate: dict[str, Any],
    expected_images: dict[str, int],
) -> None:
    manifest_rows = sorted(
        (row for result in results for row in result.manifest_rows),
        key=lambda row: (SPLITS.index(row["split"]), row["book"], int(row["image_id"])),
    )
    with (output / "manifest.csv").open("w", newline="", encoding="utf-8") as file:
        writer = csv.DictWriter(file, fieldnames=list(manifest_rows[0]))
        writer.writeheader()
        writer.writerows(manifest_rows)

    for split in SPLITS:
        image_paths = [
            str((output / row["image"]).resolve())
            for row in manifest_rows
            if row["split"] == split
        ]
        (output / f"{split}.txt").write_text(
            "\n".join(image_paths) + ("\n" if image_paths else ""), encoding="utf-8"
        )

    dataset_yaml = {
        "train": "images/train",
        "val": "images/val",
        "test": "images/test",
        "names": CLASS_NAMES,
    }
    (output / "dataset.yaml").write_text(
        yaml.safe_dump(dataset_yaml, sort_keys=False, allow_unicode=True),
        encoding="utf-8",
    )

    audit = {
        "schema_version": SCHEMA_VERSION,
        "class_mapping": {
            str(source_id): {"target_id": target_id, "name": name}
            for source_id, (target_id, name) in SOURCE_TO_TARGET.items()
        },
        "sources": {
            "mangaseg_root": str(mangaseg_root.resolve()),
            "manga109_images": str(image_root.resolve()),
            "split_audit": str(split_audit.resolve()),
            "split_audit_sha256": sha256(split_audit),
        },
        "conversion": {
            "simplify_pixels": simplify_pixels,
            "disconnected_components": "Ultralytics 8.4.43 merge_multi_segment thin-line connection",
            "training_overrides": {"overlap_mask": False, "mask_ratio": 4},
        },
        "checkpoint_split_audit": {
            "expected_images": {
                split: sum(
                    expected_images[result.book]
                    for result in results
                    if result.split == split
                )
                for split in SPLITS
            },
            "note": "The checkpoint audit excludes some MangaSeg pages. This dataset retains every listed image, including fully labeled negative pages.",
        },
        **aggregate,
    }
    (output / "audit.json").write_text(json.dumps(audit, indent=2), encoding="utf-8")

    readme = """# Comic mask YOLO26s experiment dataset

Generated from `MS92/MangaSegmentation` and locally licensed Manga109 images.
The split is copied from the published ShadowB YOLO26s checkpoint audit so the
fine-tuned model remains directly comparable with that baseline.

Classes:

0. `frame`
1. `dialogue_text`
2. `balloon`
3. `onomatopoeia_text`

COO is not concatenated as another image dataset. MangaSeg already used COO for
its onomatopoeia instances; adding the raw COO pages would duplicate Manga109
images and create inconsistent partial labels.

Train with `overlap_mask=False` because dialogue text is nested inside balloon
masks. Keep the upstream `mask_ratio=4`, which matches the model's prototype
mask resolution at an input size of 1280.

Generated files are local experiment data and must not be committed. Review the
Manga109, MangaSeg, and COO terms before publishing images, annotations, or
derived artifacts. MangaSeg attribution and citation requirements remain in
force.
"""
    (output / "README.md").write_text(readme, encoding="utf-8")


def read_yolo_polygons(
    label_path: Path, width: int, height: int
) -> list[tuple[int, np.ndarray]]:
    polygons = []
    for line in label_path.read_text(encoding="utf-8").splitlines():
        values = line.split()
        class_id = int(values[0])
        points = np.asarray(
            [float(value) for value in values[1:]], dtype=np.float32
        ).reshape(-1, 2)
        points *= np.array([width, height], dtype=np.float32)
        polygons.append((class_id, points.astype(np.int32)))
    return polygons


def render_samples(
    output: Path, manifest_rows: list[dict[str, str]], count: int
) -> None:
    if count <= 0 or not manifest_rows:
        return
    sample_dir = output / "audit_samples"
    sample_dir.mkdir(parents=True, exist_ok=True)

    indices = np.linspace(
        0, len(manifest_rows) - 1, min(count, len(manifest_rows)), dtype=int
    )
    thumbnails: list[Image.Image] = []
    for sample_number, index in enumerate(indices):
        row = manifest_rows[int(index)]
        image_path = output / row["image"]
        label_path = output / row["label"]
        image = np.asarray(Image.open(image_path).convert("RGB"))
        height, width = image.shape[:2]
        overlay = image.astype(np.float32)
        polygons = read_yolo_polygons(label_path, width, height)
        for class_id in (0, 2, 1, 3):
            class_mask = np.zeros((height, width), dtype=np.uint8)
            class_polygons = [
                polygon
                for candidate_id, polygon in polygons
                if candidate_id == class_id
            ]
            if class_polygons:
                cv2.fillPoly(class_mask, class_polygons, 1)
                color = np.asarray(VISUAL_COLORS[class_id], dtype=np.float32)
                alpha = 0.12 if class_id == 0 else 0.38
                selected = class_mask > 0
                overlay[selected] = overlay[selected] * (1.0 - alpha) + color * alpha
                cv2.polylines(
                    overlay,
                    class_polygons,
                    True,
                    tuple(int(value) for value in color),
                    1,
                    cv2.LINE_AA,
                )

        preview = Image.fromarray(np.clip(overlay, 0, 255).astype(np.uint8))
        preview_path = (
            sample_dir
            / f"{sample_number:02d}_{row['split']}_{row['book']}_{int(row['image_id']):03d}.jpg"
        )
        preview.save(preview_path, quality=92)

        thumbnail_width = 360
        thumbnail_height = round(preview.height * thumbnail_width / preview.width)
        thumbnail = preview.resize(
            (thumbnail_width, thumbnail_height), Image.Resampling.LANCZOS
        )
        panel = Image.new("RGB", (thumbnail_width, thumbnail_height + 26), "white")
        panel.paste(thumbnail, (0, 26))
        ImageDraw.Draw(panel).text(
            (6, 7),
            f"{row['split']} / {row['book']} / {int(row['image_id']):03d}",
            fill="black",
            font=ImageFont.load_default(),
        )
        thumbnails.append(panel)

    columns = 2
    rows = (len(thumbnails) + columns - 1) // columns
    gap = 8
    panel_width = max(panel.width for panel in thumbnails)
    panel_height = max(panel.height for panel in thumbnails)
    sheet = Image.new(
        "RGB",
        (
            columns * panel_width + (columns - 1) * gap,
            rows * panel_height + (rows - 1) * gap,
        ),
        "white",
    )
    for index, panel in enumerate(thumbnails):
        x = (index % columns) * (panel_width + gap)
        y = (index // columns) * (panel_height + gap)
        sheet.paste(panel, (x, y))
    sheet.save(output / "audit_contact_sheet.jpg", quality=92)


def main() -> None:
    args = parse_args()
    if args.workers < 1:
        raise ValueError("--workers must be positive")
    if args.simplify_pixels < 0:
        raise ValueError("--simplify-pixels must be non-negative")

    mangaseg_root = args.mangaseg_root.resolve()
    image_root = args.image_root.resolve()
    split_audit = args.split_audit.resolve()
    output = args.output.resolve()

    split_by_book, expected_images = load_splits(split_audit)
    selected_books = set(args.books) if args.books else None
    jobs = validate_inputs(mangaseg_root, image_root, split_by_book, selected_books)

    if output.exists() and any(output.iterdir()) and not args.overwrite:
        raise FileExistsError(
            f"{output} is not empty; pass --overwrite to resume or replace generated labels"
        )
    output.mkdir(parents=True, exist_ok=True)
    for split in SPLITS:
        (output / "images" / split).mkdir(parents=True, exist_ok=True)
        (output / "labels" / split).mkdir(parents=True, exist_ok=True)

    results: list[BookStats] = []
    with ProcessPoolExecutor(max_workers=args.workers) as executor:
        futures = {
            executor.submit(
                process_book,
                str(json_path),
                split,
                str(image_root),
                str(output),
                args.link_mode,
                args.simplify_pixels,
            ): json_path.stem
            for json_path, split in jobs
        }
        with tqdm(
            total=len(futures), desc="Converting MangaSeg books", unit="book"
        ) as progress:
            for future in as_completed(futures):
                book = futures[future]
                try:
                    result = future.result()
                except Exception as error:
                    for candidate in futures:
                        candidate.cancel()
                    raise RuntimeError(f"failed to convert {book}") from error
                results.append(result)
                progress.update(1)

    results.sort(key=lambda result: result.book)
    aggregate = aggregate_stats(results)
    write_metadata(
        output,
        split_audit,
        mangaseg_root,
        image_root,
        args.simplify_pixels,
        results,
        aggregate,
        expected_images,
    )
    with (output / "manifest.csv").open(newline="", encoding="utf-8") as file:
        manifest_rows = list(csv.DictReader(file))
    render_samples(output, manifest_rows, args.visual_samples)

    print(json.dumps(aggregate, indent=2))
    print(f"Dataset written to {output}")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise
