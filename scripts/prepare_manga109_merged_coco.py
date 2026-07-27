# /// script
# requires-python = ">=3.11,<3.15"
# dependencies = [
#   "numpy>=2.0",
#   "opencv-python-headless>=4.10",
#   "pillow>=11.0",
#   "pycocotools>=2.0.10",
#   "tqdm>=4.67",
# ]
# ///
"""Prepare a merged Manga109-v2026 + MangaSeg COCO segmentation dataset.

The source datasets annotate the same Manga109 pages and must be joined rather
than concatenated. Manga109-v2026 is authoritative for dialogue-text instance
boxes. MangaSeg supplies exact masks for panel, balloon, onomatopoeia, and the
legacy dialogue text. Legacy text pixels are repartitioned into the revised
v2026 text boxes at connected-component granularity.

The generated class order is:

    1 text
    2 onomatopoeia
    3 bubble
    4 panel

Masks remain compressed COCO RLE, preserving disconnected character strokes
without the lossy polygon connections needed by YOLO segmentation labels.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import shutil
import sys
import xml.etree.ElementTree as ET
from collections import Counter, defaultdict
from concurrent.futures import ProcessPoolExecutor, as_completed
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageFont
from pycocotools import mask as mask_utils
from tqdm import tqdm


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANGASEG_ROOT = REPOSITORY_ROOT / "data" / "MangaSegmentation"
DEFAULT_MANGA109_ROOT = (
    REPOSITORY_ROOT / "data" / "Manga109_released_2026_05_21"
)
DEFAULT_OUTPUT = REPOSITORY_ROOT / "data" / "manga109-v2026-mangaseg-coco"

SCHEMA_VERSION = 1
SPLITS = ("train", "valid", "test")
CATEGORIES = (
    {"id": 1, "name": "text", "supercategory": "text"},
    {"id": 2, "name": "onomatopoeia", "supercategory": "text"},
    {"id": 3, "name": "bubble", "supercategory": "layout"},
    {"id": 4, "name": "panel", "supercategory": "layout"},
)
DIRECT_CATEGORY_MAP = {
    1: 4,  # MangaSeg frame -> panel
    5: 3,  # MangaSeg balloon -> bubble
    6: 2,  # MangaSeg onomatopoeia
}
CATEGORY_NAMES = {category["id"]: category["name"] for category in CATEGORIES}
VISUAL_COLORS = {
    1: (38, 120, 255),
    2: (255, 174, 35),
    3: (42, 205, 105),
    4: (230, 65, 83),
}


@dataclass(frozen=True)
class TextTarget:
    source_id: str
    bbox: tuple[int, int, int, int]
    transcription: str


@dataclass
class BookResult:
    book: str
    split: str
    images: list[dict[str, Any]] = field(default_factory=list)
    annotations: list[dict[str, Any]] = field(default_factory=list)
    reconciliation: list[dict[str, Any]] = field(default_factory=list)
    stats: Counter = field(default_factory=Counter)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mangaseg-root", type=Path, default=DEFAULT_MANGASEG_ROOT)
    parser.add_argument("--manga109-root", type=Path, default=DEFAULT_MANGA109_ROOT)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--workers", type=int, default=min(8, os.cpu_count() or 1))
    parser.add_argument(
        "--split-seed",
        type=int,
        default=1092026,
        help="Seed for the deterministic 87/11/11 book split.",
    )
    parser.add_argument(
        "--assignment-margin",
        type=int,
        default=4,
        help="Pixel margin used only when assigning legacy ink components to revised boxes.",
    )
    parser.add_argument(
        "--link-mode",
        choices=("hardlink", "copy"),
        default="hardlink",
        help="How source images are materialized in each split directory.",
    )
    parser.add_argument(
        "--visual-samples",
        type=int,
        default=12,
        help="Number of deterministic annotation previews to render.",
    )
    parser.add_argument(
        "--books",
        nargs="*",
        help="Optional subset used for conversion debugging; split assignment remains global.",
    )
    parser.add_argument(
        "--overwrite",
        action="store_true",
        help="Permit replacing JSON and metadata in an existing generated directory.",
    )
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def normalize_rle(segmentation: dict[str, Any]) -> dict[str, Any]:
    counts = segmentation["counts"]
    if isinstance(counts, bytes):
        counts = counts.decode("ascii")
    if not isinstance(counts, str):
        raise TypeError("only compressed COCO RLE masks are supported")
    return {
        "size": [int(value) for value in segmentation["size"]],
        "counts": counts,
    }


def decode_rle(segmentation: dict[str, Any]) -> np.ndarray:
    encoded = dict(segmentation)
    if isinstance(encoded["counts"], str):
        encoded["counts"] = encoded["counts"].encode("ascii")
    mask = np.asarray(mask_utils.decode(encoded), dtype=np.uint8)
    if mask.ndim == 3:
        mask = np.any(mask, axis=2).astype(np.uint8)
    return mask


def decode_rle_at_size(
    segmentation: dict[str, Any], width: int, height: int
) -> tuple[np.ndarray, bool]:
    mask = decode_rle(segmentation)
    if mask.shape == (height, width):
        return mask, False
    resized = cv2.resize(mask, (width, height), interpolation=cv2.INTER_NEAREST)
    return (resized > 0).astype(np.uint8), True


def encode_mask(mask: np.ndarray) -> dict[str, Any]:
    encoded = mask_utils.encode(np.asfortranarray(mask.astype(np.uint8)))
    return normalize_rle(encoded)


def rle_key(segmentation: dict[str, Any]) -> tuple[int, int, str]:
    size = segmentation["size"]
    return int(size[0]), int(size[1]), str(segmentation["counts"])


def clamp_box(
    box: tuple[int, int, int, int], width: int, height: int
) -> tuple[int, int, int, int]:
    x, y, box_width, box_height = box
    x0 = max(0, min(width, x))
    y0 = max(0, min(height, y))
    x1 = max(x0, min(width, x + box_width))
    y1 = max(y0, min(height, y + box_height))
    return x0, y0, x1 - x0, y1 - y0


def enclosing_box(
    first: tuple[int, int, int, int], second: tuple[int, int, int, int]
) -> list[int]:
    x0 = min(first[0], second[0])
    y0 = min(first[1], second[1])
    x1 = max(first[0] + first[2], second[0] + second[2])
    y1 = max(first[1] + first[3], second[1] + second[3])
    return [x0, y0, x1 - x0, y1 - y0]


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


def load_text_targets(xml_path: Path) -> dict[int, list[TextTarget]]:
    pages: dict[int, list[TextTarget]] = defaultdict(list)
    root = ET.parse(xml_path).getroot()
    for page in root.find("pages").findall("page"):
        page_index = int(page.attrib["index"])
        for text in page.findall("text"):
            attributes = text.attrib
            xmin = int(attributes["xmin"])
            ymin = int(attributes["ymin"])
            xmax = int(attributes["xmax"])
            ymax = int(attributes["ymax"])
            pages[page_index].append(
                TextTarget(
                    source_id=attributes["id"],
                    bbox=(xmin, ymin, xmax - xmin, ymax - ymin),
                    transcription=text.text or "",
                )
            )
    return pages


def assign_book_splits(books: list[str], seed: int) -> dict[str, str]:
    if len(books) != 109:
        raise ValueError(f"expected 109 Manga109 books, found {len(books)}")
    shuffled = sorted(books)
    random.Random(seed).shuffle(shuffled)
    split_by_book = {book: "test" for book in shuffled[:11]}
    split_by_book.update({book: "valid" for book in shuffled[11:22]})
    split_by_book.update({book: "train" for book in shuffled[22:]})
    return split_by_book


def rectangle_intersection(
    first: tuple[int, int, int, int], second: tuple[int, int, int, int]
) -> tuple[int, int, int, int] | None:
    x0 = max(first[0], second[0])
    y0 = max(first[1], second[1])
    x1 = min(first[0] + first[2], second[0] + second[2])
    y1 = min(first[1] + first[3], second[1] + second[3])
    if x1 <= x0 or y1 <= y0:
        return None
    return x0, y0, x1 - x0, y1 - y0


def expand_box(
    box: tuple[int, int, int, int], margin: int, width: int, height: int
) -> tuple[int, int, int, int]:
    return clamp_box(
        (box[0] - margin, box[1] - margin, box[2] + 2 * margin, box[3] + 2 * margin),
        width,
        height,
    )


def component_overlap(
    labels: np.ndarray,
    component: int,
    component_box: tuple[int, int, int, int],
    target_box: tuple[int, int, int, int],
) -> int:
    intersection = rectangle_intersection(component_box, target_box)
    if intersection is None:
        return 0
    x, y, width, height = intersection
    return int(np.count_nonzero(labels[y : y + height, x : x + width] == component))


def reconcile_text(
    source_annotations: list[dict[str, Any]],
    targets: list[TextTarget],
    width: int,
    height: int,
    assignment_margin: int,
) -> tuple[list[tuple[TextTarget, np.ndarray, int]], dict[str, int]]:
    union = np.zeros((height, width), dtype=np.uint8)
    seen_masks: set[tuple[int, int, str]] = set()
    duplicate_masks = 0
    rescaled_masks = 0
    for annotation in source_annotations:
        segmentation = normalize_rle(annotation["segmentation"])
        key = rle_key(segmentation)
        if key in seen_masks:
            duplicate_masks += 1
            continue
        seen_masks.add(key)
        mask, rescaled = decode_rle_at_size(segmentation, width, height)
        if rescaled:
            rescaled_masks += 1
        union |= mask

    target_boxes = [clamp_box(target.bbox, width, height) for target in targets]
    expanded_boxes = [
        expand_box(box, assignment_margin, width, height) for box in target_boxes
    ]
    component_count, labels, statistics, centroids = cv2.connectedComponentsWithStats(
        union, connectivity=8
    )
    owners = np.zeros(component_count, dtype=np.int32)
    assigned_components = 0
    unassigned_components = 0

    for component in range(1, component_count):
        x, y, component_width, component_height, _ = statistics[component]
        component_box = (
            int(x),
            int(y),
            int(component_width),
            int(component_height),
        )
        overlaps = [
            component_overlap(labels, component, component_box, box)
            for box in target_boxes
        ]
        best_overlap = max(overlaps, default=0)
        if best_overlap == 0 and assignment_margin:
            overlaps = [
                component_overlap(labels, component, component_box, box)
                for box in expanded_boxes
            ]
            best_overlap = max(overlaps, default=0)
        if best_overlap == 0:
            unassigned_components += 1
            continue

        candidates = [
            index for index, overlap in enumerate(overlaps) if overlap == best_overlap
        ]
        if len(candidates) == 1:
            owner = candidates[0]
        else:
            center_x, center_y = centroids[component]

            def center_distance(index: int) -> float:
                box = target_boxes[index]
                target_x = box[0] + box[2] / 2
                target_y = box[1] + box[3] / 2
                return (center_x - target_x) ** 2 + (center_y - target_y) ** 2

            owner = min(candidates, key=center_distance)
        owners[component] = owner + 1
        assigned_components += 1

    owner_map = owners[labels]
    reconciled = []
    for index, target in enumerate(targets):
        mask = (owner_map == index + 1).astype(np.uint8)
        components = int(np.count_nonzero(owners == index + 1))
        reconciled.append((target, mask, components))

    return reconciled, {
        "legacy_text_masks": len(seen_masks),
        "duplicate_text_masks": duplicate_masks,
        "rescaled_text_masks": rescaled_masks,
        "assigned_text_components": assigned_components,
        "unassigned_text_components": unassigned_components,
    }


def process_book(
    json_path_text: str,
    xml_path_text: str,
    image_root_text: str,
    output_text: str,
    split: str,
    link_mode: str,
    assignment_margin: int,
) -> BookResult:
    json_path = Path(json_path_text)
    xml_path = Path(xml_path_text)
    image_root = Path(image_root_text)
    output = Path(output_text)
    book = json_path.stem
    result = BookResult(book=book, split=split)

    with json_path.open(encoding="utf-8") as file:
        document = json.load(file)
    category_names = {
        int(category["id"]): category["name"] for category in document["categories"]
    }
    expected_categories = {1: "frame", 2: "text", 5: "balloon", 6: "onomatopoeia"}
    for category_id, expected_name in expected_categories.items():
        if category_names.get(category_id) != expected_name:
            raise ValueError(f"{book}: category {category_id} is not {expected_name!r}")

    text_targets = load_text_targets(xml_path)
    book_images = [
        image_info
        for image_info in document["images"]
        if Path(image_info["file_name"]).parts[0] == book
    ]
    if not book_images:
        raise ValueError(f"{book}: cumulative MangaSeg image index has no book images")
    book_image_ids = {int(image_info["id"]) for image_info in book_images}
    annotations_by_image: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for annotation in document["annotations"]:
        image_id = int(annotation["image_id"])
        if image_id in book_image_ids:
            annotations_by_image[image_id].append(annotation)

    for image_info in book_images:
        image_id = int(image_info["id"])
        relative_source = Path(image_info["file_name"])
        source_image = image_root / relative_source
        if not source_image.is_file():
            if annotations_by_image.get(image_id) or text_targets.get(
                int(relative_source.stem)
            ):
                raise FileNotFoundError(f"missing annotated source image: {source_image}")
            result.stats["missing_source_images"] += 1
            continue

        with Image.open(source_image) as source:
            width, height = source.size
        source_width = int(image_info["width"])
        source_height = int(image_info["height"])
        if (width, height) != (source_width, source_height):
            result.stats["rescaled_images"] += 1
        destination = output / split / relative_source
        materialize_image(source_image, destination, link_mode)
        result.images.append(
            {
                "id": image_id,
                "width": width,
                "height": height,
                "file_name": relative_source.as_posix(),
                "license": 1,
            }
        )
        result.stats["images"] += 1

        source_annotations = annotations_by_image.get(image_id, [])
        direct_seen: set[tuple[int, int, int, str]] = set()
        for source_annotation in source_annotations:
            target_category = DIRECT_CATEGORY_MAP.get(
                int(source_annotation["category_id"])
            )
            if target_category is None:
                continue
            segmentation = normalize_rle(source_annotation["segmentation"])
            if tuple(segmentation["size"]) != (height, width):
                mask, _ = decode_rle_at_size(segmentation, width, height)
                segmentation = encode_mask(mask)
                result.stats["rescaled_direct_masks"] += 1
            key = (target_category, *rle_key(segmentation))
            if key in direct_seen:
                result.stats[f"duplicate_{CATEGORY_NAMES[target_category]}"] += 1
                continue
            direct_seen.add(key)
            encoded = {
                "size": segmentation["size"],
                "counts": segmentation["counts"].encode("ascii"),
            }
            result.annotations.append(
                {
                    "image_id": image_id,
                    "category_id": target_category,
                    "segmentation": segmentation,
                    "area": int(mask_utils.area(encoded)),
                    "bbox": [
                        int(round(value))
                        for value in mask_utils.toBbox(encoded).tolist()
                    ],
                    "iscrowd": 0,
                    "source_dataset": "MangaSegmentation",
                    "source_annotation_id": int(source_annotation["id"]),
                }
            )
            result.stats[CATEGORY_NAMES[target_category]] += 1

        page_index = int(relative_source.stem)
        page_targets = text_targets.get(page_index, [])
        source_text = [
            annotation
            for annotation in source_annotations
            if int(annotation["category_id"]) == 2
        ]
        reconciled, text_stats = reconcile_text(
            source_text,
            page_targets,
            width,
            height,
            assignment_margin,
        )
        result.stats.update(text_stats)
        result.stats["v2026_text_targets"] += len(page_targets)

        for target, mask, components in reconciled:
            area = int(mask.sum())
            record = {
                "split": split,
                "book": book,
                "image_id": image_id,
                "image": relative_source.as_posix(),
                "source_text_id": target.source_id,
                "v2026_bbox": list(target.bbox),
                "transcription": target.transcription,
                "assigned_components": components,
                "mask_area": area,
            }
            if area == 0:
                record["status"] = "missing_mask"
                result.reconciliation.append(record)
                result.stats["unresolved_text"] += 1
                continue

            segmentation = encode_mask(mask)
            mask_box_array = mask_utils.toBbox(
                {
                    "size": segmentation["size"],
                    "counts": segmentation["counts"].encode("ascii"),
                }
            )
            mask_box = tuple(int(round(value)) for value in mask_box_array.tolist())
            revised_box = clamp_box(target.bbox, width, height)
            bbox = enclosing_box(revised_box, mask_box)
            result.annotations.append(
                {
                    "image_id": image_id,
                    "category_id": 1,
                    "segmentation": segmentation,
                    "area": area,
                    "bbox": bbox,
                    "iscrowd": 0,
                    "source_dataset": "Manga109-v2026+MangaSegmentation",
                    "source_text_id": target.source_id,
                    "v2026_bbox": list(revised_box),
                    "transcription": target.transcription,
                    "mask_components": components,
                }
            )
            record["status"] = "resolved"
            result.reconciliation.append(record)
            result.stats["text"] += 1

    return result


def coco_document(
    split: str, images: list[dict[str, Any]], annotations: list[dict[str, Any]]
) -> dict[str, Any]:
    return {
        "info": {
            "description": "Manga109-v2026 text instances merged with MangaSeg masks",
            "version": str(SCHEMA_VERSION),
            "year": 2026,
            "split": split,
        },
        "licenses": [
            {
                "id": 1,
                "name": "See Manga109, MangaSegmentation, and COO source terms",
                "url": "https://manga109.github.io/manga109-project-website/en/",
            }
        ],
        "images": images,
        "annotations": annotations,
        "categories": list(CATEGORIES),
    }


def validate_coco(split_dir: Path) -> dict[str, Any]:
    annotation_path = split_dir / "_annotations.coco.json"
    with annotation_path.open(encoding="utf-8") as file:
        document = json.load(file)
    images = document["images"]
    annotations = document["annotations"]
    image_ids = {int(image["id"]) for image in images}
    if len(image_ids) != len(images):
        raise ValueError(f"{split_dir.name}: duplicate image ids")
    annotation_ids = {int(annotation["id"]) for annotation in annotations}
    if len(annotation_ids) != len(annotations):
        raise ValueError(f"{split_dir.name}: duplicate annotation ids")

    books = set()
    for image in images:
        relative = Path(image["file_name"])
        books.add(relative.parts[0])
        if not (split_dir / relative).is_file():
            raise FileNotFoundError(split_dir / relative)

    category_counts: Counter = Counter()
    for annotation in annotations:
        if int(annotation["image_id"]) not in image_ids:
            raise ValueError(f"{split_dir.name}: annotation references unknown image")
        category_id = int(annotation["category_id"])
        if category_id not in CATEGORY_NAMES:
            raise ValueError(f"{split_dir.name}: unknown category {category_id}")
        encoded = normalize_rle(annotation["segmentation"])
        rle = {
            "size": encoded["size"],
            "counts": encoded["counts"].encode("ascii"),
        }
        area = int(mask_utils.area(rle))
        if area != int(annotation["area"]) or area <= 0:
            raise ValueError(
                f"{split_dir.name}: invalid area for annotation {annotation['id']}"
            )
        mask_box = [float(value) for value in mask_utils.toBbox(rle).tolist()]
        box = [float(value) for value in annotation["bbox"]]
        if (
            box[0] > mask_box[0]
            or box[1] > mask_box[1]
            or box[0] + box[2] < mask_box[0] + mask_box[2]
            or box[1] + box[3] < mask_box[1] + mask_box[3]
        ):
            raise ValueError(
                f"{split_dir.name}: bbox does not enclose mask for {annotation['id']}"
            )
        category_counts[CATEGORY_NAMES[category_id]] += 1

    return {
        "books": len(books),
        "images": len(images),
        "annotations": len(annotations),
        "categories": dict(sorted(category_counts.items())),
    }


def render_samples(output: Path, count: int) -> None:
    if count <= 0:
        return
    candidates = []
    for split in SPLITS:
        path = output / split / "_annotations.coco.json"
        if not path.is_file():
            continue
        document = json.loads(path.read_text(encoding="utf-8"))
        annotations_by_image: dict[int, list[dict[str, Any]]] = defaultdict(list)
        for annotation in document["annotations"]:
            annotations_by_image[int(annotation["image_id"])].append(annotation)
        for image in document["images"]:
            if annotations_by_image.get(int(image["id"])):
                candidates.append((split, image, annotations_by_image[int(image["id"])]))
    if not candidates:
        return

    sample_dir = output / "audit_samples"
    sample_dir.mkdir(parents=True, exist_ok=True)
    indices = np.linspace(0, len(candidates) - 1, min(count, len(candidates)), dtype=int)
    thumbnails = []
    for sample_number, index in enumerate(indices):
        split, image_info, annotations = candidates[int(index)]
        image_path = output / split / image_info["file_name"]
        image = np.asarray(Image.open(image_path).convert("RGB"))
        overlay = image.astype(np.float32)
        for category_id in (4, 3, 1, 2):
            color = np.asarray(VISUAL_COLORS[category_id], dtype=np.float32)
            alpha = 0.12 if category_id == 4 else 0.38
            for annotation in annotations:
                if int(annotation["category_id"]) != category_id:
                    continue
                mask = decode_rle(annotation["segmentation"]).astype(bool)
                overlay[mask] = overlay[mask] * (1.0 - alpha) + color * alpha
        preview = Image.fromarray(np.clip(overlay, 0, 255).astype(np.uint8))
        name = f"{sample_number:02d}_{split}_{Path(image_info['file_name']).stem}.jpg"
        preview.save(sample_dir / name, quality=92)
        thumbnail_width = 360
        thumbnail_height = round(preview.height * thumbnail_width / preview.width)
        thumbnail = preview.resize(
            (thumbnail_width, thumbnail_height), Image.Resampling.LANCZOS
        )
        panel = Image.new("RGB", (thumbnail_width, thumbnail_height + 26), "white")
        panel.paste(thumbnail, (0, 26))
        ImageDraw.Draw(panel).text(
            (6, 7),
            f"{split} / {image_info['file_name']}",
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


def write_outputs(
    output: Path,
    mangaseg_root: Path,
    manga109_root: Path,
    split_seed: int,
    assignment_margin: int,
    split_by_book: dict[str, str],
    results: list[BookResult],
    visual_samples: int,
) -> dict[str, Any]:
    annotations_by_split: dict[str, list[dict[str, Any]]] = defaultdict(list)
    images_by_split: dict[str, list[dict[str, Any]]] = defaultdict(list)
    reconciliation = []
    aggregate_stats: Counter = Counter()
    for result in results:
        images_by_split[result.split].extend(result.images)
        annotations_by_split[result.split].extend(result.annotations)
        reconciliation.extend(result.reconciliation)
        aggregate_stats.update(result.stats)

    for split in SPLITS:
        images = sorted(images_by_split[split], key=lambda image: int(image["id"]))
        annotations = sorted(
            annotations_by_split[split],
            key=lambda annotation: (
                int(annotation["image_id"]),
                int(annotation["category_id"]),
                str(annotation.get("source_text_id", "")),
                int(annotation.get("source_annotation_id", -1)),
            ),
        )
        for annotation_id, annotation in enumerate(annotations, start=1):
            annotation["id"] = annotation_id
        document = coco_document(split, images, annotations)
        (output / split / "_annotations.coco.json").write_text(
            # pycocotools.COCO opens paths with the Windows locale encoding.
            # ASCII escapes keep Japanese transcription portable there while
            # json.load still restores the original Unicode strings.
            json.dumps(document, ensure_ascii=True, separators=(",", ":")),
            encoding="utf-8",
        )

    reconciliation.sort(
        key=lambda row: (
            SPLITS.index(row["split"]),
            row["book"],
            int(row["image_id"]),
            row["source_text_id"],
        )
    )
    with (output / "text_reconciliation.jsonl").open("w", encoding="utf-8") as file:
        for row in reconciliation:
            file.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")

    selected_books = {result.book for result in results}
    split_manifest = {
        split: sorted(
            book
            for book, assigned_split in split_by_book.items()
            if assigned_split == split and book in selected_books
        )
        for split in SPLITS
    }
    validation = {split: validate_coco(output / split) for split in SPLITS}
    all_split_books = [set(split_manifest[split]) for split in SPLITS]
    if any(
        all_split_books[first] & all_split_books[second]
        for first in range(len(SPLITS))
        for second in range(first + 1, len(SPLITS))
    ):
        raise ValueError("book leakage detected across splits")

    audit = {
        "schema_version": SCHEMA_VERSION,
        "sources": {
            "mangaseg_root": str(mangaseg_root.resolve()),
            "manga109_root": str(manga109_root.resolve()),
            "manga109_annotation_version": "v2026.05.21",
            "mangaseg_readme_sha256": sha256(mangaseg_root / "README.md"),
        },
        "categories": list(CATEGORIES),
        "split": {
            "strategy": "book-disjoint deterministic seeded shuffle",
            "seed": split_seed,
            "target_book_counts": {"train": 87, "valid": 11, "test": 11},
            "books": split_manifest,
        },
        "text_reconciliation": {
            "method": "connected MangaSeg ink components assigned to Manga109-v2026 boxes by pixel overlap",
            "assignment_margin": assignment_margin,
            "dimension_mismatch": "MangaSeg RLE is resized with nearest-neighbor sampling to the current image grid",
            "missing_masks": "excluded from the standard segmentation COCO annotations and listed in text_reconciliation.jsonl",
        },
        "source_stats": dict(sorted(aggregate_stats.items())),
        "validation": validation,
    }
    (output / "audit.json").write_text(
        json.dumps(audit, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    readme = """# Manga109-v2026 + MangaSeg COCO instance segmentation

This dataset joins annotations on the same Manga109 pages; it does not duplicate
the images as independent examples. Manga109-v2026 defines dialogue-text boxes
and transcription. MangaSeg supplies RLE masks for panel, bubble,
onomatopoeia, and legacy text ink. Legacy text components are assigned to the
revised v2026 text instances by pixel overlap.

Classes are `text`, `onomatopoeia`, `bubble`, and `panel`. Each split contains
`_annotations.coco.json` plus hardlinked (or copied) `book/page.jpg` images.
Splits are book-disjoint. Exact RLE masks are retained, including disconnected
glyph strokes and overlapping text/bubble masks.

If a MangaSeg RLE grid differs from the current v2026 image dimensions, masks
are resized with nearest-neighbor sampling. The affected image and mask counts
are recorded in `audit.json`.

Revised v2026 text boxes for which MangaSeg has no ink mask are excluded from
the standard instance-segmentation JSON. They are recorded as `missing_mask` in
`text_reconciliation.jsonl`; never replace them with rectangular fake masks.

Generated data must not be committed. Review the Manga109, MangaSegmentation,
and COO licenses and attribution requirements before publishing data or derived
artifacts.
"""
    (output / "README.md").write_text(readme, encoding="utf-8")
    render_samples(output, visual_samples)
    return audit


def main() -> None:
    args = parse_args()
    if args.workers < 1:
        raise ValueError("--workers must be positive")
    if args.assignment_margin < 0:
        raise ValueError("--assignment-margin must be non-negative")

    mangaseg_root = args.mangaseg_root.resolve()
    manga109_root = args.manga109_root.resolve()
    output = args.output.resolve()
    json_paths = sorted((mangaseg_root / "jsons").glob("*.json"))
    books = [path.stem for path in json_paths]
    split_by_book = assign_book_splits(books, args.split_seed)
    selected = set(args.books) if args.books else None

    source_json_books = set(books)
    source_xml_books = {
        path.stem for path in (manga109_root / "annotations").glob("*.xml")
    }
    source_image_books = {
        path.name
        for path in (manga109_root / "images").iterdir()
        if path.is_dir()
    }
    if source_json_books != source_xml_books or source_json_books != source_image_books:
        raise ValueError("MangaSeg, Manga109-v2026 XML, and image book sets differ")
    if selected is not None:
        unknown = selected - source_json_books
        if unknown:
            raise ValueError(f"unknown --books values: {sorted(unknown)}")
        json_paths = [path for path in json_paths if path.stem in selected]

    if output.exists() and any(output.iterdir()) and not args.overwrite:
        raise FileExistsError(
            f"{output} is not empty; pass --overwrite to resume or replace metadata"
        )
    output.mkdir(parents=True, exist_ok=True)
    for split in SPLITS:
        (output / split).mkdir(parents=True, exist_ok=True)

    results = []
    with ProcessPoolExecutor(max_workers=args.workers) as executor:
        futures = {
            executor.submit(
                process_book,
                str(json_path),
                str(manga109_root / "annotations" / f"{json_path.stem}.xml"),
                str(manga109_root / "images"),
                str(output),
                split_by_book[json_path.stem],
                args.link_mode,
                args.assignment_margin,
            ): json_path.stem
            for json_path in json_paths
        }
        with tqdm(total=len(futures), desc="Merging Manga109 books", unit="book") as progress:
            for future in as_completed(futures):
                book = futures[future]
                try:
                    results.append(future.result())
                except Exception as error:
                    for candidate in futures:
                        candidate.cancel()
                    raise RuntimeError(f"failed to process {book}") from error
                progress.update(1)

    results.sort(key=lambda result: result.book)
    audit = write_outputs(
        output,
        mangaseg_root,
        manga109_root,
        args.split_seed,
        args.assignment_margin,
        split_by_book,
        results,
        args.visual_samples,
    )
    print(json.dumps(audit["validation"], indent=2))
    print(
        "text reconciliation:",
        audit["source_stats"].get("text", 0),
        "resolved /",
        audit["source_stats"].get("unresolved_text", 0),
        "missing masks",
    )
    print(f"Dataset written to {output}")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise
