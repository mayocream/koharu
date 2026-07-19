# /// script
# requires-python = ">=3.11,<3.15"
# dependencies = [
#   "numpy>=2.0",
#   "pillow>=11.0",
#   "pycocotools>=2.0.10",
#   "tqdm>=4.67",
# ]
# ///
"""Join COO, MangaSeg, and Manga109 annotations for verifier training.

COO and MangaSeg annotate the same Manga109 pages. They must be joined rather
than concatenated: COO supplies polygons, transcriptions, links, and the
official book split, while MangaSeg supplies tight RLE masks and dialogue hard
negatives. The generated JSONL manifests reference the existing Manga109 images
and do not copy or redistribute them.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
import shutil
import sys
import xml.etree.ElementTree as ET
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from pycocotools import mask as mask_utils
from tqdm import tqdm


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANGASEG_ROOT = REPOSITORY_ROOT / "data" / "datasets" / "mangaseg"
DEFAULT_COO_ROOT = REPOSITORY_ROOT / "data" / "datasets" / "coo"
DEFAULT_MANGA109_ROOT = REPOSITORY_ROOT / "data" / "Manga109_released_2021_12_30"
DEFAULT_OUTPUT = REPOSITORY_ROOT / "data" / "datasets" / "comic-onomatopoeia-verifier"
SPLITS = ("train", "val", "test")
SCHEMA_VERSION = 1
POSITIVE_CATEGORY = 6
NEGATIVE_CATEGORY = 2
OUTPUT_MARKER = ".comic-onomatopoeia-verifier-dataset"


@dataclass(frozen=True)
class SourceObject:
    source_id: str
    text: str
    bbox: tuple[float, float, float, float]
    polygon: tuple[tuple[float, float], ...] | None = None
    links: tuple[dict[str, Any], ...] = ()


@dataclass
class MatchStats:
    source_count: int = 0
    mask_count: int = 0
    matched: int = 0
    unmatched_source: int = 0
    unmatched_mask: int = 0
    containment_sum: float = 0.0
    containment_min: float = 1.0
    iou_sum: float = 0.0
    iou_min: float = 1.0
    low_quality: int = 0

    def add_match(self, containment: float, iou: float) -> None:
        self.matched += 1
        self.containment_sum += containment
        self.containment_min = min(self.containment_min, containment)
        self.iou_sum += iou
        self.iou_min = min(self.iou_min, iou)
        if containment < 0.8:
            self.low_quality += 1

    def as_dict(self) -> dict[str, Any]:
        return {
            "source_count": self.source_count,
            "mask_count": self.mask_count,
            "matched": self.matched,
            "unmatched_source": self.unmatched_source,
            "unmatched_mask": self.unmatched_mask,
            "coverage_of_source": self.matched / self.source_count
            if self.source_count
            else 0.0,
            "coverage_of_masks": self.matched / self.mask_count
            if self.mask_count
            else 0.0,
            "mean_mask_bbox_containment": self.containment_sum / self.matched
            if self.matched
            else 0.0,
            "minimum_mask_bbox_containment": self.containment_min
            if self.matched
            else 0.0,
            "mean_bbox_iou": self.iou_sum / self.matched if self.matched else 0.0,
            "minimum_bbox_iou": self.iou_min if self.matched else 0.0,
            "matches_below_0_8_containment": self.low_quality,
        }


@dataclass
class BookResult:
    book: str
    split: str
    records: list[dict[str, Any]] = field(default_factory=list)
    positive_matches: MatchStats = field(default_factory=MatchStats)
    negative_matches: MatchStats = field(default_factory=MatchStats)
    annotated_pages: set[int] = field(default_factory=set)
    dropped_invalid_records: Counter = field(default_factory=Counter)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mangaseg-root", type=Path, default=DEFAULT_MANGASEG_ROOT)
    parser.add_argument("--coo-root", type=Path, default=DEFAULT_COO_ROOT)
    parser.add_argument("--manga109-root", type=Path, default=DEFAULT_MANGA109_ROOT)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--seed", type=int, default=20260720)
    parser.add_argument(
        "--visual-samples",
        type=int,
        default=18,
        help="Number of deterministic joined records to render for geometry review.",
    )
    parser.add_argument(
        "--books",
        nargs="*",
        help="Optional book subset for debugging. Production runs should omit this.",
    )
    parser.add_argument(
        "--overwrite",
        action="store_true",
        help="Replace an existing generated dataset directory.",
    )
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def stable_random_key(seed: int, sample_id: str) -> str:
    return hashlib.sha256(f"{seed}:{sample_id}".encode()).hexdigest()


def load_book_splits(coo_root: Path) -> dict[str, str]:
    split_by_book: dict[str, str] = {}
    for split in SPLITS:
        path = coo_root / f"books_natsorted_{split}.txt"
        if not path.is_file():
            raise FileNotFoundError(f"missing COO split file: {path}")
        for book in path.read_text(encoding="utf-8-sig").splitlines():
            book = book.strip()
            if not book:
                continue
            if book in split_by_book:
                raise ValueError(f"COO split contains duplicate book {book!r}")
            split_by_book[book] = split
    return split_by_book


def validate_inputs(
    mangaseg_root: Path,
    coo_root: Path,
    manga109_root: Path,
    split_by_book: dict[str, str],
    selected_books: set[str] | None,
) -> list[str]:
    mangaseg_books = {path.stem for path in (mangaseg_root / "jsons").glob("*.json")}
    coo_books = {path.stem for path in (coo_root / "annotations").glob("*.xml")}
    manga109_books = {
        path.stem for path in (manga109_root / "annotations.v2018.05.31").glob("*.xml")
    }
    image_books = {
        path.name for path in (manga109_root / "images").iterdir() if path.is_dir()
    }
    split_books = set(split_by_book)
    common = mangaseg_books & coo_books & manga109_books & image_books & split_books
    if not common:
        raise ValueError("no common books across COO, MangaSeg, and Manga109")
    differences = {
        "MangaSeg": split_books - mangaseg_books,
        "COO": split_books - coo_books,
        "Manga109 annotations": split_books - manga109_books,
        "Manga109 images": split_books - image_books,
    }
    missing = {name: sorted(values) for name, values in differences.items() if values}
    if missing:
        raise ValueError(f"COO books are missing from inputs: {missing}")
    books = sorted(split_books)
    if selected_books is not None:
        unknown = selected_books - set(books)
        if unknown:
            raise ValueError(f"unknown --books values: {sorted(unknown)}")
        books = [book for book in books if book in selected_books]
    return books


def xywh_to_xyxy(values: Iterable[float]) -> tuple[float, float, float, float]:
    x, y, width, height = (float(value) for value in values)
    return x, y, x + width, y + height


def bbox_area(box: tuple[float, float, float, float]) -> float:
    return max(0.0, box[2] - box[0]) * max(0.0, box[3] - box[1])


def bbox_intersection(
    first: tuple[float, float, float, float],
    second: tuple[float, float, float, float],
) -> float:
    return max(0.0, min(first[2], second[2]) - max(first[0], second[0])) * max(
        0.0, min(first[3], second[3]) - max(first[1], second[1])
    )


def bbox_metrics(
    source: tuple[float, float, float, float],
    mask: tuple[float, float, float, float],
) -> tuple[float, float, float]:
    intersection = bbox_intersection(source, mask)
    mask_area = bbox_area(mask)
    union = bbox_area(source) + mask_area - intersection
    containment = intersection / mask_area if mask_area else 0.0
    iou = intersection / union if union else 0.0
    source_center = ((source[0] + source[2]) / 2, (source[1] + source[3]) / 2)
    mask_center = ((mask[0] + mask[2]) / 2, (mask[1] + mask[3]) / 2)
    diagonal = math.hypot(source[2] - source[0], source[3] - source[1]) or 1.0
    distance = math.dist(source_center, mask_center) / diagonal
    return containment, iou, distance


def point_in_polygon(
    point: tuple[float, float], polygon: Iterable[tuple[float, float]]
) -> bool:
    x, y = point
    points = tuple(polygon)
    inside = False
    previous = points[-1]
    for current in points:
        x0, y0 = previous
        x1, y1 = current
        crosses = (y0 > y) != (y1 > y)
        if crosses and x < (x1 - x0) * (y - y0) / (y1 - y0) + x0:
            inside = not inside
        previous = current
    return inside


def match_sources_to_masks(
    sources: list[SourceObject],
    masks: list[dict[str, Any]],
) -> tuple[dict[int, tuple[int, float, float]], set[int], MatchStats]:
    stats = MatchStats(source_count=len(sources), mask_count=len(masks))
    candidates: list[tuple[float, int, int, float, float]] = []
    for source_index, source in enumerate(sources):
        for mask_index, mask in enumerate(masks):
            mask_box = xywh_to_xyxy(mask["bbox"])
            containment, iou, distance = bbox_metrics(source.bbox, mask_box)
            center = (
                (mask_box[0] + mask_box[2]) / 2,
                (mask_box[1] + mask_box[3]) / 2,
            )
            center_inside = (
                point_in_polygon(center, source.polygon)
                if source.polygon is not None
                else source.bbox[0] <= center[0] <= source.bbox[2]
                and source.bbox[1] <= center[1] <= source.bbox[3]
            )
            if containment < 0.25 and not center_inside:
                continue
            score = 4.0 * containment + iou + (1.0 if center_inside else 0.0) - distance
            candidates.append((score, source_index, mask_index, containment, iou))

    matches: dict[int, tuple[int, float, float]] = {}
    used_masks: set[int] = set()
    for _, source_index, mask_index, containment, iou in sorted(
        candidates, reverse=True
    ):
        if source_index in matches or mask_index in used_masks:
            continue
        matches[source_index] = (mask_index, containment, iou)
        used_masks.add(mask_index)
        stats.add_match(containment, iou)

    stats.unmatched_source = len(sources) - len(matches)
    stats.unmatched_mask = len(masks) - len(used_masks)
    return matches, used_masks, stats


def parse_polygon(element: ET.Element) -> tuple[tuple[float, float], ...]:
    indices = sorted(
        int(key[1:])
        for key in element.attrib
        if key.startswith("x") and key[1:].isdigit()
    )
    return tuple(
        (float(element.attrib[f"x{index}"]), float(element.attrib[f"y{index}"]))
        for index in indices
    )


def polygon_bbox(
    polygon: tuple[tuple[float, float], ...],
) -> tuple[float, float, float, float]:
    return (
        min(point[0] for point in polygon),
        min(point[1] for point in polygon),
        max(point[0] for point in polygon),
        max(point[1] for point in polygon),
    )


def load_coo_pages(path: Path) -> dict[int, list[SourceObject]]:
    root = ET.parse(path).getroot()
    pages: dict[int, list[SourceObject]] = {}
    pages_element = root.find("pages")
    if pages_element is None:
        return pages
    for page in pages_element:
        page_index = int(page.attrib["index"])
        objects: list[SourceObject] = []
        links_by_object: dict[str, list[dict[str, Any]]] = defaultdict(list)
        for child in page:
            if not child.tag.startswith("onomatopoeia_link"):
                continue
            endpoints = [child.attrib["link0"], child.attrib["link1"]]
            for position, object_id in enumerate(endpoints):
                links_by_object[object_id].append(
                    {
                        "link_id": child.attrib["id"],
                        "link_type": child.tag,
                        "position": position,
                        "other_id": endpoints[1 - position],
                    }
                )
        for child in page.findall("onomatopoeia"):
            polygon = parse_polygon(child)
            objects.append(
                SourceObject(
                    source_id=child.attrib["id"],
                    text=child.text or "",
                    bbox=polygon_bbox(polygon),
                    polygon=polygon,
                    links=tuple(links_by_object.get(child.attrib["id"], ())),
                )
            )
        pages[page_index] = objects
    return pages


def load_manga109_text_pages(path: Path) -> dict[int, list[SourceObject]]:
    root = ET.parse(path).getroot()
    pages: dict[int, list[SourceObject]] = {}
    pages_element = root.find("pages")
    if pages_element is None:
        return pages
    for page in pages_element:
        page_index = int(page.attrib["index"])
        pages[page_index] = [
            SourceObject(
                source_id=element.attrib["id"],
                text=element.text or "",
                bbox=(
                    float(element.attrib["xmin"]),
                    float(element.attrib["ymin"]),
                    float(element.attrib["xmax"]),
                    float(element.attrib["ymax"]),
                ),
            )
            for element in page.findall("text")
        ]
    return pages


def clipped_crop_box(
    box: tuple[float, float, float, float], width: int, height: int
) -> list[int]:
    left = max(0, min(width, math.floor(box[0])))
    top = max(0, min(height, math.floor(box[1])))
    right = max(0, min(width, math.ceil(box[2])))
    bottom = max(0, min(height, math.ceil(box[3])))
    return [left, top, right, bottom]


def mask_payload(annotation: dict[str, Any] | None) -> dict[str, Any] | None:
    if annotation is None:
        return None
    return {
        "annotation_id": int(annotation["id"]),
        "bbox_xywh": [float(value) for value in annotation["bbox"]],
        "area": int(annotation["area"]),
        "rle": annotation["segmentation"],
    }


def merge_stats(target: MatchStats, source: MatchStats) -> None:
    target.source_count += source.source_count
    target.mask_count += source.mask_count
    target.matched += source.matched
    target.unmatched_source += source.unmatched_source
    target.unmatched_mask += source.unmatched_mask
    target.containment_sum += source.containment_sum
    target.iou_sum += source.iou_sum
    target.low_quality += source.low_quality
    if source.matched:
        target.containment_min = min(target.containment_min, source.containment_min)
        target.iou_min = min(target.iou_min, source.iou_min)


def prepare_book(
    book: str,
    split: str,
    mangaseg_path: Path,
    coo_path: Path,
    manga109_path: Path,
    image_root: Path,
) -> BookResult:
    document = json.loads(mangaseg_path.read_text(encoding="utf-8"))
    categories = {
        int(category["id"]): category["name"] for category in document["categories"]
    }
    if categories.get(POSITIVE_CATEGORY) != "onomatopoeia":
        raise ValueError(f"{book}: MangaSeg category 6 is not onomatopoeia")
    if categories.get(NEGATIVE_CATEGORY) != "text":
        raise ValueError(f"{book}: MangaSeg category 2 is not text")

    image_by_page: dict[int, dict[str, Any]] = {}
    image_id_to_page: dict[int, int] = {}
    for image in document["images"]:
        relative = Path(image["file_name"])
        if not relative.parts or relative.parts[0] != book:
            continue
        page_index = int(relative.stem)
        image_by_page[page_index] = image
        image_id_to_page[int(image["id"])] = page_index

    masks_by_page: dict[int, dict[int, list[dict[str, Any]]]] = defaultdict(
        lambda: defaultdict(list)
    )
    for annotation in document["annotations"]:
        category_id = int(annotation["category_id"])
        if category_id not in {POSITIVE_CATEGORY, NEGATIVE_CATEGORY}:
            continue
        page_index = image_id_to_page.get(int(annotation["image_id"]))
        if page_index is not None:
            masks_by_page[page_index][category_id].append(annotation)

    coo_pages = load_coo_pages(coo_path)
    text_pages = load_manga109_text_pages(manga109_path)
    result = BookResult(book=book, split=split)
    page_indices = sorted(set(coo_pages) | set(text_pages) | set(masks_by_page))
    for page_index in page_indices:
        image_info = image_by_page.get(page_index)
        if image_info is None:
            if coo_pages.get(page_index) or text_pages.get(page_index):
                raise ValueError(
                    f"{book}: annotated page {page_index} has no image record"
                )
            continue
        relative_image = Path(image_info["file_name"])
        image_path = image_root / relative_image
        if not image_path.is_file():
            if coo_pages.get(page_index) or text_pages.get(page_index):
                raise FileNotFoundError(
                    f"annotated source image is missing: {image_path}"
                )
            continue
        width = int(image_info["width"])
        height = int(image_info["height"])
        if coo_pages.get(page_index) or text_pages.get(page_index):
            result.annotated_pages.add(page_index)

        positive_sources = coo_pages.get(page_index, [])
        positive_masks = masks_by_page[page_index][POSITIVE_CATEGORY]
        positive_matches, used_positive_masks, positive_stats = match_sources_to_masks(
            positive_sources, positive_masks
        )
        merge_stats(result.positive_matches, positive_stats)
        for source_index, source in enumerate(positive_sources):
            match = positive_matches.get(source_index)
            mask = positive_masks[match[0]] if match is not None else None
            crop_box = clipped_crop_box(source.bbox, width, height)
            if crop_box[2] <= crop_box[0] or crop_box[3] <= crop_box[1]:
                result.dropped_invalid_records["coo"] += 1
                continue
            result.records.append(
                {
                    "schema_version": SCHEMA_VERSION,
                    "sample_id": f"coo:{book}:{page_index:03d}:{source.source_id}",
                    "split": split,
                    "book": book,
                    "page_index": page_index,
                    "image": relative_image.as_posix(),
                    "image_size": [width, height],
                    "crop_box_xyxy": crop_box,
                    "label_is_onomatopoeia": True,
                    "target_text": source.text,
                    "is_truncated": bool(source.links),
                    "coo": {
                        "annotation_id": source.source_id,
                        "polygon": [[x, y] for x, y in source.polygon or ()],
                        "links": list(source.links),
                    },
                    "manga109": None,
                    "mangaseg": mask_payload(mask),
                    "join": {
                        "status": "matched" if match else "coo_only",
                        "mask_bbox_containment": match[1] if match else None,
                        "bbox_iou": match[2] if match else None,
                    },
                }
            )
        for mask_index, mask in enumerate(positive_masks):
            if mask_index in used_positive_masks:
                continue
            crop_box = clipped_crop_box(xywh_to_xyxy(mask["bbox"]), width, height)
            if crop_box[2] <= crop_box[0] or crop_box[3] <= crop_box[1]:
                result.dropped_invalid_records["mangaseg_onomatopoeia"] += 1
                continue
            result.records.append(
                {
                    "schema_version": SCHEMA_VERSION,
                    "sample_id": (
                        f"mangaseg-onomatopoeia:{book}:{page_index:03d}:"
                        f"{int(mask['id'])}"
                    ),
                    "split": split,
                    "book": book,
                    "page_index": page_index,
                    "image": relative_image.as_posix(),
                    "image_size": [width, height],
                    "crop_box_xyxy": crop_box,
                    "label_is_onomatopoeia": True,
                    "target_text": None,
                    "is_truncated": False,
                    "coo": None,
                    "manga109": None,
                    "mangaseg": mask_payload(mask),
                    "join": {
                        "status": "mangaseg_only",
                        "mask_bbox_containment": None,
                        "bbox_iou": None,
                    },
                }
            )

        negative_sources = text_pages.get(page_index, [])
        negative_masks = masks_by_page[page_index][NEGATIVE_CATEGORY]
        negative_matches, _, negative_stats = match_sources_to_masks(
            negative_sources, negative_masks
        )
        merge_stats(result.negative_matches, negative_stats)
        for source_index, source in enumerate(negative_sources):
            match = negative_matches.get(source_index)
            mask = negative_masks[match[0]] if match is not None else None
            crop_source = (
                xywh_to_xyxy(mask["bbox"]) if mask is not None else source.bbox
            )
            crop_box = clipped_crop_box(crop_source, width, height)
            if crop_box[2] <= crop_box[0] or crop_box[3] <= crop_box[1]:
                result.dropped_invalid_records["manga109_text"] += 1
                continue
            result.records.append(
                {
                    "schema_version": SCHEMA_VERSION,
                    "sample_id": f"manga109-text:{book}:{page_index:03d}:{source.source_id}",
                    "split": split,
                    "book": book,
                    "page_index": page_index,
                    "image": relative_image.as_posix(),
                    "image_size": [width, height],
                    "crop_box_xyxy": crop_box,
                    "label_is_onomatopoeia": False,
                    "target_text": source.text,
                    "is_truncated": False,
                    "coo": None,
                    "manga109": {
                        "annotation_id": source.source_id,
                        "bbox_xyxy": list(source.bbox),
                    },
                    "mangaseg": mask_payload(mask),
                    "join": {
                        "status": "matched" if match else "manga109_only",
                        "mask_bbox_containment": match[1] if match else None,
                        "bbox_iou": match[2] if match else None,
                    },
                }
            )
    result.records.sort(key=lambda record: record["sample_id"])
    return result


def write_jsonl(path: Path, records: Iterable[dict[str, Any]]) -> int:
    count = 0
    with path.open("w", encoding="utf-8", newline="\n") as file:
        for record in records:
            file.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")))
            file.write("\n")
            count += 1
    return count


def compact_training_record(record: dict[str, Any]) -> dict[str, Any]:
    compact = dict(record)
    if record["mangaseg"] is not None:
        compact["mangaseg"] = {
            key: value for key, value in record["mangaseg"].items() if key != "rle"
        }
    return compact


def balanced_records(
    results: list[BookResult], split: str, seed: int
) -> list[dict[str, Any]]:
    selected: list[dict[str, Any]] = []
    for result in results:
        if result.split != split:
            continue
        positives = [
            record for record in result.records if record["label_is_onomatopoeia"]
        ]
        negatives = [
            record for record in result.records if not record["label_is_onomatopoeia"]
        ]
        negatives.sort(key=lambda record: stable_random_key(seed, record["sample_id"]))
        selected.extend(positives)
        selected.extend(negatives[: len(positives)])
    selected.sort(key=lambda record: stable_random_key(seed, record["sample_id"]))
    return selected


def decode_mask(record: dict[str, Any]) -> np.ndarray | None:
    mangaseg = record["mangaseg"]
    if mangaseg is None:
        return None
    rle = dict(mangaseg["rle"])
    if isinstance(rle.get("counts"), str):
        rle["counts"] = rle["counts"].encode("ascii")
    mask = np.asarray(mask_utils.decode(rle), dtype=np.uint8)
    if mask.ndim == 3:
        mask = np.any(mask, axis=2).astype(np.uint8)
    return mask


def render_audit_samples(
    output: Path,
    image_root: Path,
    records: list[dict[str, Any]],
    count: int,
    seed: int,
) -> None:
    if count <= 0 or not records:
        return
    randomizer = random.Random(seed)
    positives = [record for record in records if record["label_is_onomatopoeia"]]
    negatives = [record for record in records if not record["label_is_onomatopoeia"]]
    half = count // 2
    sampled = randomizer.sample(positives, min(half, len(positives)))
    sampled += randomizer.sample(negatives, min(count - len(sampled), len(negatives)))
    sample_dir = output / "audit_samples"
    sample_dir.mkdir(parents=True, exist_ok=True)
    panels: list[Image.Image] = []
    font = ImageFont.load_default()
    for font_path in (
        Path(r"C:\Windows\Fonts\NotoSansJP-VF.ttf"),
        Path("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
    ):
        if font_path.is_file():
            font = ImageFont.truetype(font_path, 13)
            break
    for index, record in enumerate(sampled):
        image = Image.open(image_root / record["image"]).convert("RGB")
        overlay = np.asarray(image).copy()
        mask = decode_mask(record)
        if mask is not None:
            color = (
                np.array([0, 220, 255], dtype=np.float32)
                if record["label_is_onomatopoeia"]
                else np.array([255, 80, 90], dtype=np.float32)
            )
            selected = mask > 0
            overlay[selected] = (
                overlay[selected].astype(np.float32) * 0.45 + color * 0.55
            ).astype(np.uint8)
        marked = Image.fromarray(overlay)
        draw = ImageDraw.Draw(marked)
        if record["coo"] is not None:
            polygon = [tuple(point) for point in record["coo"]["polygon"]]
            if polygon:
                draw.line(polygon + [polygon[0]], fill=(255, 40, 40), width=3)
        crop_box = record["crop_box_xyxy"]
        padding = 16
        expanded = (
            max(0, crop_box[0] - padding),
            max(0, crop_box[1] - padding),
            min(marked.width, crop_box[2] + padding),
            min(marked.height, crop_box[3] + padding),
        )
        crop = marked.crop(expanded)
        crop.thumbnail((360, 260), Image.Resampling.LANCZOS)
        panel = Image.new("RGB", (380, 310), "white")
        panel.paste(crop, ((380 - crop.width) // 2, 38))
        label = "positive" if record["label_is_onomatopoeia"] else "dialogue negative"
        caption = f"{label} | {record['book']} p{record['page_index']} | {record['target_text']}"
        ImageDraw.Draw(panel).text((8, 8), caption[:58], fill="black", font=font)
        panels.append(panel)
        panel.save(
            sample_dir / f"{index:02d}_{label.replace(' ', '_')}.jpg", quality=94
        )

    columns = 3
    rows = math.ceil(len(panels) / columns)
    sheet = Image.new("RGB", (columns * 380, rows * 310), "white")
    for index, panel in enumerate(panels):
        sheet.paste(panel, ((index % columns) * 380, (index // columns) * 310))
    sheet.save(output / "audit_contact_sheet.jpg", quality=94)


def aggregate_match_stats(results: list[BookResult], field_name: str) -> MatchStats:
    aggregate = MatchStats()
    for result in results:
        merge_stats(aggregate, getattr(result, field_name))
    return aggregate


def write_readme(output: Path) -> None:
    readme = """# Comic onomatopoeia verifier dataset

Generated by `scripts/prepare_comic_onomatopoeia_verifier_dataset.py`.

This is a joined annotation manifest, not a concatenation of duplicate images:

- COO supplies onomatopoeia polygons, transcription, truncated-text links, and
  the official 89/10/10 book split.
- MangaSeg supplies tight RLE instance masks and ordinary dialogue masks.
- Manga109 v2018 annotations supply dialogue transcription.

`all/{train,val,test}.jsonl` retains every COO onomatopoeia and Manga109 text
record, including matched MangaSeg RLE. `balanced/` retains every positive,
deterministically samples the same number of dialogue negatives per book, and
omits the large RLE payload because verifier training only needs the crop and
labels. Images are referenced relative to the Manga109 image root recorded in
`audit.json`; they are not copied.

For verifier training, use `label_is_onomatopoeia` as the visual classification
target. Compare recognizer output with `target_text` to derive OCR-correctness
labels. `is_truncated` must not be treated as incorrect OCR: it marks a valid COO
region whose intended reading requires another linked region.

Do not move books between splits. MangaSeg and COO cover the same Manga109
pages, so using MangaSeg validation/test records during training leaks the COO
evaluation set.

Generated data is local and must not be committed. COO annotations are CC BY
4.0; MangaSeg has its own attribution terms; Manga109 image-use and derived-model
terms apply separately. Review all current licenses before publishing data or a
checkpoint.
"""
    (output / "README.md").write_text(readme, encoding="utf-8")


def main() -> None:
    args = parse_args()
    if args.visual_samples < 0:
        raise ValueError("--visual-samples must be non-negative")
    mangaseg_root = args.mangaseg_root.resolve()
    coo_root = args.coo_root.resolve()
    manga109_root = args.manga109_root.resolve()
    image_root = manga109_root / "images"
    output = args.output.resolve()

    split_by_book = load_book_splits(coo_root)
    selected_books = set(args.books) if args.books else None
    books = validate_inputs(
        mangaseg_root,
        coo_root,
        manga109_root,
        split_by_book,
        selected_books,
    )
    if output.exists():
        if not args.overwrite:
            raise FileExistsError(f"{output} exists; pass --overwrite to replace it")
        if not (output / OUTPUT_MARKER).is_file():
            raise ValueError(
                f"refusing to replace {output}: generated-dataset marker is missing"
            )
        shutil.rmtree(output)
    (output / "all").mkdir(parents=True)
    (output / "balanced").mkdir(parents=True)
    (output / OUTPUT_MARKER).write_text(
        "generated; safe to replace\n", encoding="utf-8"
    )

    results: list[BookResult] = []
    for book in tqdm(books, desc="Joining COO and MangaSeg", unit="book"):
        results.append(
            prepare_book(
                book,
                split_by_book[book],
                mangaseg_root / "jsons" / f"{book}.json",
                coo_root / "annotations" / f"{book}.xml",
                manga109_root / "annotations.v2018.05.31" / f"{book}.xml",
                image_root,
            )
        )

    all_records: list[dict[str, Any]] = []
    split_counts: dict[str, dict[str, int]] = {}
    balanced_counts: dict[str, dict[str, int]] = {}
    for split in SPLITS:
        records = [
            record
            for result in results
            if result.split == split
            for record in result.records
        ]
        records.sort(key=lambda record: record["sample_id"])
        write_jsonl(output / "all" / f"{split}.jsonl", records)
        all_records.extend(records)
        split_counts[split] = {
            "books": sum(result.split == split for result in results),
            "records": len(records),
            "positive": sum(record["label_is_onomatopoeia"] for record in records),
            "negative": sum(not record["label_is_onomatopoeia"] for record in records),
            "truncated_positive": sum(
                record["label_is_onomatopoeia"] and record["is_truncated"]
                for record in records
            ),
        }
        balanced = balanced_records(results, split, args.seed)
        write_jsonl(
            output / "balanced" / f"{split}.jsonl",
            (compact_training_record(record) for record in balanced),
        )
        balanced_counts[split] = {
            "records": len(balanced),
            "positive": sum(record["label_is_onomatopoeia"] for record in balanced),
            "negative": sum(not record["label_is_onomatopoeia"] for record in balanced),
        }

    positive_matches = aggregate_match_stats(results, "positive_matches")
    negative_matches = aggregate_match_stats(results, "negative_matches")
    dropped_invalid_records: Counter = Counter()
    for result in results:
        dropped_invalid_records.update(result.dropped_invalid_records)
    split_hashes = {
        split: sha256(coo_root / f"books_natsorted_{split}.txt") for split in SPLITS
    }
    audit = {
        "schema_version": SCHEMA_VERSION,
        "seed": args.seed,
        "sources": {
            "mangaseg_root": str(mangaseg_root),
            "coo_root": str(coo_root),
            "manga109_root": str(manga109_root),
            "image_root": str(image_root),
            "manga109_annotation_version": "v2018.05.31",
            "coo_split_sha256": split_hashes,
        },
        "selected_books": books,
        "splits": split_counts,
        "balanced_splits": balanced_counts,
        "joins": {
            "coo_to_mangaseg_onomatopoeia": positive_matches.as_dict(),
            "manga109_text_to_mangaseg_text": negative_matches.as_dict(),
        },
        "dropped_invalid_records": dict(dropped_invalid_records),
        "notes": [
            "COO and MangaSeg positives are joined, never duplicated.",
            "COO book splits are preserved to prevent same-page leakage.",
            "MangaSeg RLE is retained verbatim in each matched record.",
            "The manifests reference Manga109 images and do not copy them.",
        ],
    }
    (output / "audit.json").write_text(
        json.dumps(audit, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    write_readme(output)
    render_audit_samples(
        output, image_root, all_records, args.visual_samples, args.seed
    )

    print(json.dumps({"splits": split_counts, "joins": audit["joins"]}, indent=2))
    print(f"Dataset written to {output}")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise
