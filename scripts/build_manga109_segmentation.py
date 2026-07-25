# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "numpy>=2.0",
#   "opencv-python-headless>=4.10",
#   "pycocotools>=2.0.10",
#   "tqdm>=4.67",
# ]
# ///
"""Build the annotation-only Manga109 Segmentation dataset.

Human Manga109-v2026 text boxes and local release COO polygons remain gold
instances. Comic Text Detector blocks outside that gold coverage supplement
titles, credits, prose, and back matter that Manga109 does not annotate.
MangaSeg, Comic Text Detector, and MTS-2025 provide the pixel masks.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import math
import os
import time
import xml.etree.ElementTree as ET
from collections import Counter, defaultdict
from dataclasses import dataclass
from functools import cmp_to_key
from functools import partial
from pathlib import Path
from typing import Any, Iterable

import cv2
import numpy as np
from pycocotools import mask as mask_utils
from tqdm import tqdm


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MERGED_COCO_ROOT = REPOSITORY_ROOT / "data" / "manga109-v2026-mangaseg-coco"
DEFAULT_COO_ANNOTATIONS = (
    REPOSITORY_ROOT / "data" / "Manga109_released_2026_05_21" / "annotations_COO"
)
DEFAULT_IMAGES_ROOT = REPOSITORY_ROOT / "data" / "Manga109_released_2026_05_21" / "images"
DEFAULT_TEACHER_ROOT = REPOSITORY_ROOT / "data" / "koharu-manga-layoutseg" / "teachers"
DEFAULT_MTS2025_ROOT = (
    REPOSITORY_ROOT / "data" / "koharu-manga-layoutseg" / "teachers-mts2025"
)
DEFAULT_OUTPUT = REPOSITORY_ROOT / "data" / "manga109-segmentation"

SOURCE_SPLITS = ("train", "valid", "test")
OFFICIAL_SPLITS = ("train", "validation", "test")
CATEGORIES = (
    {"id": 1, "name": "text", "supercategory": "text"},
    {"id": 2, "name": "onomatopoeia", "supercategory": "text"},
    {"id": 3, "name": "bubble", "supercategory": "layout"},
    {"id": 4, "name": "panel", "supercategory": "layout"},
)
SCHEMA_VERSION = 1
DATASET_VERSION = "1.2.0"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--merged-coco-root", type=Path, default=DEFAULT_MERGED_COCO_ROOT)
    parser.add_argument("--coo-annotations", type=Path, default=DEFAULT_COO_ANNOTATIONS)
    parser.add_argument("--images-root", type=Path, default=DEFAULT_IMAGES_ROOT)
    parser.add_argument("--teacher-root", type=Path, default=DEFAULT_TEACHER_ROOT)
    parser.add_argument("--mts2025-root", type=Path, default=DEFAULT_MTS2025_ROOT)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument(
        "--workers",
        type=int,
        default=min(32, os.cpu_count() or 1),
        help="CPU processes for mask decode, CTD review grouping, and RLE encode.",
    )
    parser.add_argument(
        "--review-min-area",
        type=int,
        default=24,
        help="Minimum unexplained CTD ink pixels retained as a review candidate.",
    )
    parser.add_argument(
        "--supplement-min-area",
        type=int,
        default=24,
        help="Minimum teacher-mask pixels in an automatically added CTD text block.",
    )
    parser.add_argument(
        "--supplement-unmatched-text",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Add CTD block-level text instances outside human text and COO annotations.",
    )
    parser.add_argument(
        "--books",
        nargs="*",
        help="Optional book subset for smoke testing; omit for the release build.",
    )
    parser.add_argument("--overwrite", action="store_true")
    return parser.parse_args()


def atomic_write_json(path: Path, payload: Any, *, compact: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    with temporary.open("w", encoding="utf-8", newline="\n") as file:
        if compact:
            json.dump(payload, file, ensure_ascii=False, separators=(",", ":"))
        else:
            json.dump(payload, file, ensure_ascii=False, indent=2)
        file.write("\n")
    os.replace(temporary, path)


def atomic_write_jsonl(path: Path, records: Iterable[dict[str, Any]]) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    count = 0
    with temporary.open("w", encoding="utf-8", newline="\n") as file:
        for record in records:
            file.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")))
            file.write("\n")
            count += 1
    os.replace(temporary, path)
    return count


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_merged_splits(merged_coco_root: Path) -> dict[str, str]:
    output_names = {"train": "train", "valid": "validation", "test": "test"}
    result: dict[str, str] = {}
    for source_split, output_split in output_names.items():
        root = merged_coco_root / source_split
        for path in root.iterdir():
            if not path.is_dir():
                continue
            if path.name in result:
                raise ValueError(f"book appears in multiple merged splits: {path.name}")
            result[path.name] = output_split
    return result


def parse_coo_polygon(element: ET.Element) -> list[list[float]]:
    indices = sorted(
        int(key[1:])
        for key in element.attrib
        if key.startswith("x") and key[1:].isdigit()
    )
    return [
        [float(element.attrib[f"x{index}"]), float(element.attrib[f"y{index}"])]
        for index in indices
    ]


def load_coo_records(coo_annotations: Path) -> dict[str, list[dict[str, Any]]]:
    records: dict[str, list[dict[str, Any]]] = defaultdict(list)
    paths = sorted(coo_annotations.glob("*.xml"))
    if len(paths) != 109:
        raise ValueError(f"expected 109 local COO XML files, found {len(paths)} in {coo_annotations}")
    for path in paths:
        book = path.stem
        root = ET.parse(path).getroot()
        pages = root.find("pages")
        if pages is None:
            continue
        for page in pages:
            page_index = int(page.attrib["index"])
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
            image_name = f"{book}/{page_index:03d}.jpg"
            for child in page.findall("onomatopoeia"):
                identifier = child.attrib["id"]
                records[image_name].append(
                    {
                        "sample_id": f"coo:{book}:{page_index:03d}:{identifier}",
                        "image": image_name,
                        "target_text": child.text or "",
                        "coo": {
                            "annotation_id": identifier,
                            "polygon": parse_coo_polygon(child),
                            "links": links_by_object.get(identifier, []),
                        },
                    }
                )
    for values in records.values():
        values.sort(key=lambda record: record["sample_id"])
    return records


def load_missing_v2026_text(merged_coco_root: Path) -> dict[str, list[dict[str, Any]]]:
    path = merged_coco_root / "text_reconciliation.jsonl"
    if not path.is_file():
        raise FileNotFoundError(path)
    records: dict[str, list[dict[str, Any]]] = defaultdict(list)
    with path.open(encoding="utf-8") as file:
        for line in file:
            record = json.loads(line)
            if record.get("status") == "missing_mask":
                records[record["image"]].append(record)
    for values in records.values():
        values.sort(key=lambda record: record["source_text_id"])
    return records


def decode_rle(rle: dict[str, Any]) -> np.ndarray:
    normalized = dict(rle)
    if isinstance(normalized["counts"], str):
        normalized["counts"] = normalized["counts"].encode("ascii")
    decoded = mask_utils.decode(normalized)
    if decoded.ndim == 3:
        decoded = decoded[..., 0]
    return decoded.astype(bool)


def encode_rle(mask: np.ndarray) -> dict[str, Any]:
    encoded = mask_utils.encode(np.asfortranarray(mask.astype(np.uint8)))
    encoded["counts"] = encoded["counts"].decode("ascii")
    return {"size": [int(value) for value in encoded["size"]], "counts": encoded["counts"]}


def mask_geometry(mask: np.ndarray) -> tuple[list[int], int]:
    rows, columns = np.where(mask)
    if not len(columns):
        raise ValueError("cannot encode an empty instance mask")
    x0, x1 = int(columns.min()), int(columns.max()) + 1
    y0, y1 = int(rows.min()), int(rows.max()) + 1
    return [x0, y0, x1 - x0, y1 - y0], int(mask.sum())


def connected_component_boxes(mask: np.ndarray) -> list[list[int]]:
    count, _, stats, _ = cv2.connectedComponentsWithStats(
        mask.astype(np.uint8), connectivity=8
    )
    return [
        [
            int(stats[label, cv2.CC_STAT_LEFT]),
            int(stats[label, cv2.CC_STAT_TOP]),
            int(stats[label, cv2.CC_STAT_WIDTH]),
            int(stats[label, cv2.CC_STAT_HEIGHT]),
        ]
        for label in range(1, count)
        if int(stats[label, cv2.CC_STAT_AREA]) > 0
    ]


def remove_rule_like_components(mask: np.ndarray) -> np.ndarray:
    """Remove barcode bars and page rules without erasing ordinary glyph strokes."""
    count, labels, stats, _ = cv2.connectedComponentsWithStats(
        mask.astype(np.uint8), connectivity=8
    )
    result = mask.copy()
    for label in range(1, count):
        width = int(stats[label, cv2.CC_STAT_WIDTH])
        height = int(stats[label, cv2.CC_STAT_HEIGHT])
        long_side, short_side = max(width, height), max(1, min(width, height))
        if long_side >= 32 and long_side / short_side >= 12.0:
            result[labels == label] = False
    return result


def polygon_mask(points: Iterable[Iterable[float]], width: int, height: int) -> np.ndarray:
    values = np.asarray(list(points), dtype=np.float64)
    if values.ndim != 2 or values.shape[0] < 3 or values.shape[1] != 2:
        return np.zeros((height, width), dtype=bool)
    values[:, 0] = np.clip(np.round(values[:, 0]), 0, width - 1)
    values[:, 1] = np.clip(np.round(values[:, 1]), 0, height - 1)
    result = np.zeros((height, width), dtype=np.uint8)
    cv2.fillPoly(result, [values.astype(np.int32)], 1)
    return result.astype(bool)


def rectangular_envelope(
    box: Iterable[float], width: int, height: int, padding: int = 1
) -> tuple[np.ndarray, tuple[int, int, int, int]]:
    x, y, box_width, box_height = (int(round(value)) for value in box)
    x0, y0 = max(0, x - padding), max(0, y - padding)
    x1, y1 = min(width, x + box_width + padding), min(height, y + box_height + padding)
    envelope = np.zeros((height, width), dtype=bool)
    if x1 > x0 and y1 > y0:
        envelope[y0:y1, x0:x1] = True
    return envelope, (x0, y0, x1, y1)


def refine_ink_mask(
    mangaseg_mask: np.ndarray,
    ctd_mask: np.ndarray,
    mts2025_mask: np.ndarray,
    envelope: np.ndarray,
    roi: tuple[int, int, int, int],
) -> tuple[np.ndarray, dict[str, Any]]:
    """Union pixel masks inside authoritative human geometry, then fill as fallback."""
    x0, y0, x1, y1 = roi
    manga = mangaseg_mask[y0:y1, x0:x1] & envelope[y0:y1, x0:x1]
    ctd = ctd_mask[y0:y1, x0:x1] & envelope[y0:y1, x0:x1]
    mts2025 = mts2025_mask[y0:y1, x0:x1] & envelope[y0:y1, x0:x1]
    manga_area, ctd_area, mts2025_area = (
        int(manga.sum()),
        int(ctd.sum()),
        int(mts2025.sum()),
    )
    usable_ctd = ctd if ctd_area >= 4 else np.zeros_like(ctd)
    usable_mts2025 = mts2025 if mts2025_area >= 4 else np.zeros_like(mts2025)
    result_roi = manga | usable_ctd | usable_mts2025
    pixel_sources = [
        name
        for name, mask in (
            ("mangaseg", manga),
            ("ctd", usable_ctd),
            ("mts2025", usable_mts2025),
        )
        if mask.any()
    ]
    human_envelope_area = int(envelope[y0:y1, x0:x1].sum())
    pixel_union_area = int(result_roi.sum())
    minimum_usable_area = max(4, int(math.ceil(human_envelope_area * 0.01)))
    pairwise_iou: dict[str, float] = {}
    masks = {"mangaseg": manga, "ctd": ctd, "mts2025": mts2025}
    for first, second in (("mangaseg", "ctd"), ("mangaseg", "mts2025"), ("ctd", "mts2025")):
        first_area, second_area = int(masks[first].sum()), int(masks[second].sum())
        if first_area < 16 or second_area < 16:
            continue
        intersection = int((masks[first] & masks[second]).sum())
        union = first_area + second_area - intersection
        pairwise_iou[f"{first}_{second}"] = round(intersection / union, 6)
    source_disagreement = bool(pairwise_iou) and max(pairwise_iou.values()) < 0.05
    sparse_disagreement = (
        source_disagreement
        and pixel_union_area / max(1, human_envelope_area) < 0.10
    )
    if pixel_union_area < minimum_usable_area or sparse_disagreement:
        result_roi = envelope[y0:y1, x0:x1].copy()
        strategy = "human_envelope_fallback"
    else:
        strategy = "_".join(pixel_sources) + ("_union" if len(pixel_sources) > 1 else "_only")
    result = np.zeros_like(mangaseg_mask, dtype=bool)
    result[y0:y1, x0:x1] = result_roi
    return result, {
        "strategy": strategy,
        "pixel_sources": pixel_sources,
        "mangaseg_area_clipped": manga_area,
        "ctd_area_clipped": ctd_area,
        "mts2025_area_clipped": mts2025_area,
        "pairwise_iou": pairwise_iou,
        "ctd_used_area": int(usable_ctd.sum()),
        "mts2025_used_area": int(usable_mts2025.sum()),
        "pixel_union_area": pixel_union_area,
        "pixel_coverage": round(pixel_union_area / max(1, human_envelope_area), 6),
        "minimum_usable_area": minimum_usable_area,
        "human_envelope_area": human_envelope_area,
        "sparse_disagreement_fallback": sparse_disagreement,
        "needs_review": source_disagreement,
        "final_area": int(result.sum()),
    }


def refinement_quality_tier(
    refinement: dict[str, Any], *, text: bool
) -> str:
    if refinement["strategy"] == "human_envelope_fallback":
        return "gold_v2026_bbox_fallback" if text else "gold_polygon"
    sources = refinement["pixel_sources"]
    prefix = "gold_v2026_" if text else "gold_"
    if len(sources) > 1:
        return prefix + "_".join(sources) + "_union"
    source = sources[0]
    if source == "mangaseg":
        return prefix + "mangaseg"
    return prefix + source + "_recovered"


def bbox_intersection(first: Iterable[float], second: Iterable[float]) -> float:
    ax, ay, aw, ah = (float(value) for value in first)
    bx, by, bw, bh = (float(value) for value in second)
    return max(0.0, min(ax + aw, bx + bw) - max(ax, bx)) * max(
        0.0, min(ay + ah, by + bh) - max(ay, by)
    )


def bbox_area(box: Iterable[float]) -> float:
    _, _, width, height = (float(value) for value in box)
    return max(0.0, width) * max(0.0, height)


def polygon_bbox_xyxy(points: Iterable[Iterable[float]]) -> tuple[float, float, float, float]:
    values = np.asarray(list(points), dtype=np.float64)
    return (
        float(values[:, 0].min()),
        float(values[:, 1].min()),
        float(values[:, 0].max()),
        float(values[:, 1].max()),
    )


def xywh_to_xyxy(box: Iterable[float]) -> tuple[float, float, float, float]:
    x, y, width, height = (float(value) for value in box)
    return x, y, x + width, y + height


def xyxy_area(box: tuple[float, float, float, float]) -> float:
    return max(0.0, box[2] - box[0]) * max(0.0, box[3] - box[1])


def xyxy_intersection(
    first: tuple[float, float, float, float],
    second: tuple[float, float, float, float],
) -> float:
    return max(0.0, min(first[2], second[2]) - max(first[0], second[0])) * max(
        0.0, min(first[3], second[3]) - max(first[1], second[1])
    )


def point_in_polygon(point: tuple[float, float], points: Iterable[Iterable[float]]) -> bool:
    x, y = point
    polygon = [tuple(float(value) for value in item) for item in points]
    inside = False
    previous = polygon[-1]
    for current in polygon:
        x0, y0 = previous
        x1, y1 = current
        if (y0 > y) != (y1 > y) and x < (x1 - x0) * (y - y0) / (y1 - y0) + x0:
            inside = not inside
        previous = current
    return inside


def match_local_coo_to_masks(
    source_records: list[dict[str, Any]], annotations: list[dict[str, Any]]
) -> tuple[dict[int, dict[str, Any]], set[int]]:
    candidates: list[tuple[float, int, int]] = []
    for source_index, record in enumerate(source_records):
        polygon = record["coo"]["polygon"]
        source_box = polygon_bbox_xyxy(polygon)
        source_center = (
            (source_box[0] + source_box[2]) / 2,
            (source_box[1] + source_box[3]) / 2,
        )
        diagonal = math.hypot(source_box[2] - source_box[0], source_box[3] - source_box[1]) or 1.0
        for mask_index, annotation in enumerate(annotations):
            mask_box = xywh_to_xyxy(annotation["bbox"])
            intersection = xyxy_intersection(source_box, mask_box)
            mask_area = xyxy_area(mask_box)
            containment = intersection / mask_area if mask_area else 0.0
            union = xyxy_area(source_box) + mask_area - intersection
            iou = intersection / union if union else 0.0
            center = ((mask_box[0] + mask_box[2]) / 2, (mask_box[1] + mask_box[3]) / 2)
            center_inside = point_in_polygon(center, polygon)
            if containment < 0.25 and not center_inside:
                continue
            distance = math.dist(source_center, center) / diagonal
            score = 4.0 * containment + iou + (1.0 if center_inside else 0.0) - distance
            candidates.append((score, source_index, mask_index))
    matches: dict[int, dict[str, Any]] = {}
    used_masks: set[int] = set()
    for _, source_index, mask_index in sorted(candidates, reverse=True):
        if source_index in matches or mask_index in used_masks:
            continue
        matches[source_index] = annotations[mask_index]
        used_masks.add(mask_index)
    return matches, used_masks


def best_container(
    box: list[float], candidates: list[dict[str, Any]], minimum: float = 0.4
) -> tuple[int | None, float]:
    area = bbox_area(box) or 1.0
    scored = [
        (bbox_intersection(box, candidate["bbox"]) / area, -bbox_area(candidate["bbox"]), candidate["id"])
        for candidate in candidates
    ]
    if not scored:
        return None, 0.0
    score, _, identifier = max(scored)
    return (identifier, score) if score >= minimum else (None, score)


def reading_comparator(first: dict[str, Any], second: dict[str, Any]) -> int:
    ax, ay, aw, ah = first["bbox"]
    bx, by, bw, bh = second["bbox"]
    vertical_overlap = max(0.0, min(ay + ah, by + bh) - max(ay, by))
    same_row = vertical_overlap / max(1.0, min(ah, bh)) >= 0.35
    if same_row:
        ac, bc = ax + aw / 2, bx + bw / 2
        if abs(ac - bc) > 1:
            return -1 if ac > bc else 1
    if abs(ay - by) > 1:
        return -1 if ay < by else 1
    return -1 if first["id"] < second["id"] else 1


class UnionFind:
    def __init__(self, keys: Iterable[str]) -> None:
        self.parent = {key: key for key in keys}

    def find(self, key: str) -> str:
        parent = self.parent[key]
        if parent != key:
            self.parent[key] = self.find(parent)
        return self.parent[key]

    def union(self, first: str, second: str) -> None:
        root_first, root_second = self.find(first), self.find(second)
        if root_first != root_second:
            self.parent[max(root_first, root_second)] = min(root_first, root_second)


@dataclass
class CooNode:
    key: str
    mask: np.ndarray
    quality: str
    coo_id: str | None
    mangaseg_id: int | None
    text: str | None
    links: tuple[dict[str, Any], ...]
    ctd_coverage: float
    mts2025_coverage: float
    refinement: dict[str, Any]


def load_teacher(
    teacher_root: Path, image_name: str, expected_size: tuple[int, int]
) -> tuple[np.ndarray, list[dict[str, Any]]]:
    relative = Path(image_name)
    record_path = teacher_root / "records" / relative.with_suffix(".json")
    if not record_path.is_file():
        raise FileNotFoundError(f"missing Comic Text Detector record: {record_path}")
    record = json.loads(record_path.read_text(encoding="utf-8"))
    if record.get("status") == "error" or "comic_text" not in record.get("teachers", {}):
        raise ValueError(f"invalid Comic Text Detector record: {record_path}")
    comic_text = record["teachers"]["comic_text"]
    mask_path = teacher_root / comic_text["refined_mask"]
    mask = cv2.imread(str(mask_path), cv2.IMREAD_GRAYSCALE)
    if mask is None:
        raise OSError(f"failed to read {mask_path}")
    width, height = expected_size
    if mask.shape != (height, width):
        raise ValueError(f"teacher mask size {mask.shape} != {(height, width)} for {image_name}")
    return mask >= 128, comic_text["blocks"]


def load_mts2025_teacher(
    teacher_root: Path, image_name: str, expected_size: tuple[int, int]
) -> np.ndarray:
    mask_path = teacher_root / "masks" / Path(image_name).with_suffix(".png")
    mask = cv2.imread(str(mask_path), cv2.IMREAD_GRAYSCALE)
    if mask is None:
        raise FileNotFoundError(f"missing MTS-2025 teacher mask: {mask_path}")
    width, height = expected_size
    if mask.shape != (height, width):
        raise ValueError(
            f"MTS-2025 mask size {mask.shape} != {(height, width)} for {image_name}"
        )
    return mask >= 128


def make_coo_nodes(
    image: dict[str, Any],
    source_annotations: list[dict[str, Any]],
    source_records: list[dict[str, Any]],
    ctd_mask: np.ndarray,
    mts2025_mask: np.ndarray,
) -> list[CooNode]:
    height, width = int(image["height"]), int(image["width"])
    existing = [annotation for annotation in source_annotations if annotation["category_id"] == 2]
    matches, used_indices = match_local_coo_to_masks(source_records, existing)
    nodes: list[CooNode] = []
    for source_index, record in enumerate(source_records):
        coo = record.get("coo")
        if coo is None:
            continue
        annotation = matches.get(source_index)
        mangaseg_id = (
            int(annotation["source_annotation_id"])
            if annotation is not None and "source_annotation_id" in annotation
            else None
        )
        if annotation is not None:
            manga = decode_rle(annotation["segmentation"])
            envelope = polygon_mask(coo["polygon"], width, height)
            source_box = polygon_bbox_xyxy(coo["polygon"])
            roi = (
                max(0, int(math.floor(source_box[0])) - 1),
                max(0, int(math.floor(source_box[1])) - 1),
                min(width, int(math.ceil(source_box[2])) + 2),
                min(height, int(math.ceil(source_box[3])) + 2),
            )
            mask, refinement = refine_ink_mask(
                manga, ctd_mask, mts2025_mask, envelope, roi
            )
            quality = refinement_quality_tier(refinement, text=False)
        else:
            envelope = polygon_mask(coo["polygon"], width, height)
            source_box = polygon_bbox_xyxy(coo["polygon"])
            roi = (
                max(0, int(math.floor(source_box[0]))),
                max(0, int(math.floor(source_box[1]))),
                min(width, int(math.ceil(source_box[2])) + 1),
                min(height, int(math.ceil(source_box[3])) + 1),
            )
            mask, refinement = refine_ink_mask(
                np.zeros_like(ctd_mask), ctd_mask, mts2025_mask, envelope, roi
            )
            quality = refinement_quality_tier(refinement, text=False)
        if not mask.any():
            continue
        coverage = float((mask & ctd_mask).sum() / mask.sum())
        nodes.append(
            CooNode(
                key=str(coo["annotation_id"]),
                mask=mask,
                quality=quality,
                coo_id=str(coo["annotation_id"]),
                mangaseg_id=mangaseg_id,
                text=record.get("target_text"),
                links=tuple(coo.get("links", ())),
                ctd_coverage=coverage,
                mts2025_coverage=float((mask & mts2025_mask).sum() / mask.sum()),
                refinement=refinement,
            )
        )

    for mask_index, annotation in enumerate(existing):
        if mask_index in used_indices:
            continue
        mangaseg_id = int(annotation.get("source_annotation_id", annotation["id"]))
        mask = decode_rle(annotation["segmentation"])
        if not mask.any():
            continue
        nodes.append(
            CooNode(
                key=f"mangaseg:{mangaseg_id}",
                mask=mask,
                quality="gold_legacy",
                coo_id=None,
                mangaseg_id=mangaseg_id,
                text=None,
                links=(),
                ctd_coverage=float((mask & ctd_mask).sum() / mask.sum()),
                mts2025_coverage=float((mask & mts2025_mask).sum() / mask.sum()),
                refinement={
                    "strategy": "mangaseg_legacy_no_human_coo_envelope",
                    "pixel_sources": ["mangaseg"],
                    "mangaseg_area_clipped": int(mask.sum()),
                    "ctd_area_clipped": int((mask & ctd_mask).sum()),
                    "mts2025_area_clipped": int((mask & mts2025_mask).sum()),
                    "final_area": int(mask.sum()),
                },
            )
        )
    return nodes


def merge_coo_nodes(nodes: list[CooNode]) -> list[dict[str, Any]]:
    by_key = {node.key: node for node in nodes}
    union = UnionFind(by_key)
    for node in nodes:
        for link in node.links:
            other = str(link["other_id"])
            if other in by_key:
                union.union(node.key, other)
    groups: dict[str, list[CooNode]] = defaultdict(list)
    for node in nodes:
        groups[union.find(node.key)].append(node)

    result: list[dict[str, Any]] = []
    for values in groups.values():
        mask = np.logical_or.reduce([value.mask for value in values])
        bbox, area = mask_geometry(mask)
        ordered = sorted(
            values,
            key=lambda value: (
                min((int(link["position"]) for link in value.links), default=99),
                value.key,
            ),
        )
        link_ids = sorted({str(link["link_id"]) for value in values for link in value.links})
        qualities = sorted({value.quality for value in values})
        result.append(
            {
                "category_id": 2,
                "segmentation": encode_rle(mask),
                "area": area,
                "bbox": bbox,
                "iscrowd": 0,
                "source_dataset": "COO+MangaSegmentation+ComicTextDetector+MTS-2025",
                "transcription": "".join(value.text or "" for value in ordered) or None,
                "attributes": {
                    "quality_tier": qualities[-1] if len(qualities) == 1 else "gold_mixed",
                    "component_quality_tiers": qualities,
                    "semantic_grouping": "coo_link_graph" if link_ids else "single_source_instance",
                    "coo_annotation_ids": [value.coo_id for value in ordered if value.coo_id],
                    "mangaseg_annotation_ids": [
                        value.mangaseg_id for value in ordered if value.mangaseg_id is not None
                    ],
                    "component_source_ids": [
                        {
                            "coo_annotation_id": value.coo_id,
                            "mangaseg_annotation_id": value.mangaseg_id,
                        }
                        for value in ordered
                    ],
                    "coo_link_ids": link_ids,
                    "component_mask_refinements": [value.refinement for value in ordered],
                    "ctd_mask_coverage": round(
                        sum(value.ctd_coverage * int(value.mask.sum()) for value in values)
                        / max(1, sum(int(value.mask.sum()) for value in values)),
                        6,
                    ),
                    "mts2025_mask_coverage": round(
                        sum(
                            value.mts2025_coverage * int(value.mask.sum())
                            for value in values
                        )
                        / max(1, sum(int(value.mask.sum()) for value in values)),
                        6,
                    ),
                    "mask_role": "MangaSeg, CTD, and MTS-2025 union inside human COO polygons",
                },
                "_mask": mask,
            }
        )
    result.sort(key=lambda annotation: (annotation["bbox"][1], annotation["bbox"][0]))
    return result


def refine_existing_text_masks(
    source_annotations: list[dict[str, Any]],
    ctd_mask: np.ndarray,
    mts2025_mask: np.ndarray,
) -> list[dict[str, Any]]:
    height, width = ctd_mask.shape
    result: list[dict[str, Any]] = []
    for source in source_annotations:
        if source["category_id"] != 1:
            continue
        manga = decode_rle(source["segmentation"])
        human_box = source.get("v2026_bbox", source["bbox"])
        envelope, roi = rectangular_envelope(human_box, width, height, padding=0)
        mask, refinement = refine_ink_mask(
            manga, ctd_mask, mts2025_mask, envelope, roi
        )
        if not mask.any():
            continue
        bbox, area = mask_geometry(mask)
        annotation = {
            key: value
            for key, value in source.items()
            if key not in {"id", "image_id", "segmentation", "area", "bbox", "attributes"}
        }
        annotation.update(
            {
                "category_id": 1,
                "segmentation": encode_rle(mask),
                "area": area,
                "bbox": bbox,
                "iscrowd": 0,
                "source_dataset": "Manga109-v2026+MangaSegmentation+ComicTextDetector+MTS-2025",
                "attributes": {
                    "quality_tier": refinement_quality_tier(refinement, text=True),
                    "class_authority": "Manga109-v2026",
                    "mask_refinement": refinement,
                },
            }
        )
        result.append(annotation)
    return result


def recover_missing_text_masks(
    image_name: str,
    records: list[dict[str, Any]],
    ctd_mask: np.ndarray,
    mts2025_mask: np.ndarray,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    height, width = ctd_mask.shape
    annotations: list[dict[str, Any]] = []
    review: list[dict[str, Any]] = []
    for record in records:
        envelope, roi = rectangular_envelope(
            record["v2026_bbox"], width, height, padding=0
        )
        mask, refinement = refine_ink_mask(
            np.zeros_like(ctd_mask), ctd_mask, mts2025_mask, envelope, roi
        )
        bbox, area = mask_geometry(mask)
        quality = refinement_quality_tier(refinement, text=True)
        annotations.append(
            {
                "category_id": 1,
                "segmentation": encode_rle(mask),
                "area": area,
                "bbox": bbox,
                "iscrowd": 0,
                "source_dataset": "Manga109-v2026+ComicTextDetector+MTS-2025",
                "source_text_id": record["source_text_id"],
                "v2026_bbox": record["v2026_bbox"],
                "transcription": record["transcription"],
                "attributes": {
                    "quality_tier": quality,
                    "class_authority": "Manga109-v2026",
                    "mask_refinement": refinement,
                },
            }
        )
        if refinement["strategy"] == "human_envelope_fallback":
            review.append(
                {
                    "schema_version": SCHEMA_VERSION,
                    "image": image_name,
                    "candidate_id": f"v2026-missing-mask:{record['source_text_id']}",
                    "bbox": record["v2026_bbox"],
                    "transcription": record["transcription"],
                    "status": "review",
                    "candidate_class": "text",
                    "reason": "filled human v2026 text box fallback: neither MangaSeg nor CTD had usable ink",
                    "auto_promoted": False,
                }
            )
    return annotations, review


def mask_refinement_reviews(
    image_name: str,
    refined_text: list[dict[str, Any]],
    coo_annotations: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    reviews: list[dict[str, Any]] = []
    for annotation in refined_text:
        refinement = annotation["attributes"]["mask_refinement"]
        fallback = refinement["strategy"] == "human_envelope_fallback"
        if not fallback and not refinement.get("needs_review"):
            continue
        source_id = annotation.get(
            "source_text_id", annotation.get("source_annotation_id", "unknown")
        )
        reviews.append(
            {
                "schema_version": SCHEMA_VERSION,
                "image": image_name,
                "candidate_id": f"mask-disagreement:text:{source_id}",
                "bbox": annotation.get("v2026_bbox", annotation["bbox"]),
                "status": "review",
                "candidate_class": "text",
                "reason": (
                    "filled human text box fallback: pixel union was too sparse or strongly disagreed below 10% coverage"
                    if fallback
                    else "pixel-mask teachers have no pair with IoU >=0.05 inside the human envelope"
                ),
                "auto_promoted": False,
                "mask_refinement": refinement,
            }
        )
    for annotation in coo_annotations:
        attributes = annotation["attributes"]
        source_ids = attributes.get("component_source_ids", [])
        for index, refinement in enumerate(
            attributes.get("component_mask_refinements", [])
        ):
            fallback = refinement["strategy"] == "human_envelope_fallback"
            if not fallback and not refinement.get("needs_review"):
                continue
            source = source_ids[index] if index < len(source_ids) else {}
            source_id = source.get("coo_annotation_id") or source.get(
                "mangaseg_annotation_id", f"component-{index}"
            )
            reviews.append(
                {
                    "schema_version": SCHEMA_VERSION,
                    "image": image_name,
                    "candidate_id": f"mask-disagreement:coo:{source_id}",
                    "bbox": annotation["bbox"],
                    "status": "review",
                    "candidate_class": "COO",
                    "reason": (
                        "filled human COO polygon fallback: pixel union was too sparse or strongly disagreed below 10% coverage"
                        if fallback
                        else "pixel-mask teachers have no pair with IoU >=0.05 inside the human envelope"
                    ),
                    "auto_promoted": False,
                    "mask_refinement": refinement,
                }
            )
    return reviews


def supplemental_text_from_ctd_blocks(
    image_name: str,
    ctd_mask: np.ndarray,
    mts2025_mask: np.ndarray,
    ctd_blocks: list[dict[str, Any]],
    source_annotations: list[dict[str, Any]],
    missing_text_records: list[dict[str, Any]],
    coo_annotations: list[dict[str, Any]],
    minimum_area: int,
) -> list[dict[str, Any]]:
    """Promote unmatched CTD blocks without splitting prose into glyph components."""
    height, width = ctd_mask.shape
    human_text_boxes = [
        annotation.get("v2026_bbox", annotation["bbox"])
        for annotation in source_annotations
        if annotation["category_id"] == 1
    ]
    human_text_boxes.extend(record["v2026_bbox"] for record in missing_text_records)

    human_text_envelope = np.zeros_like(ctd_mask, dtype=np.uint8)
    for box in human_text_boxes:
        _, (x0, y0, x1, y1) = rectangular_envelope(
            box, width, height, padding=2
        )
        human_text_envelope[y0:y1, x0:x1] = 1

    coo_mask = np.zeros_like(ctd_mask, dtype=np.uint8)
    coo_envelope = np.zeros_like(ctd_mask, dtype=np.uint8)
    coo_boxes: list[list[int]] = []
    for annotation in coo_annotations:
        instance_mask = np.asarray(annotation["_mask"], dtype=bool)
        coo_mask[instance_mask] = 1
        annotation_box = annotation["bbox"]
        _, (x0, y0, x1, y1) = rectangular_envelope(
            annotation_box, width, height, padding=2
        )
        coo_envelope[y0:y1, x0:x1] = 1
        coo_boxes.append(annotation_box)
        for box in connected_component_boxes(instance_mask):
            _, (x0, y0, x1, y1) = rectangular_envelope(
                box, width, height, padding=2
            )
            coo_envelope[y0:y1, x0:x1] = 1
            coo_boxes.append(box)
    exclusion = cv2.dilate(
        (human_text_envelope | coo_envelope).astype(np.uint8),
        np.ones((5, 5), np.uint8),
    ).astype(bool)

    prepared: list[dict[str, Any]] = []
    for block_index, block in enumerate(ctd_blocks):
        values = block.get("xyxy")
        if not isinstance(values, list) or len(values) != 4:
            continue
        x0 = max(0, min(width, int(math.floor(float(values[0])))))
        y0 = max(0, min(height, int(math.floor(float(values[1])))))
        x1 = max(0, min(width, int(math.ceil(float(values[2])))))
        y1 = max(0, min(height, int(math.ceil(float(values[3])))))
        if x1 - x0 < 3 or y1 - y0 < 3:
            continue
        block_box = [x0, y0, x1 - x0, y1 - y0]
        block_area = bbox_area(block_box)
        if block_area <= 0:
            continue

        # A CTD block that substantially matches an existing human instance is
        # a duplicate. A much larger block may legitimately contain nearby
        # supplemental prose, so it is retained and the gold region is removed
        # from its pixel mask below.
        duplicate_human = False
        for human_box in human_text_boxes:
            intersection = bbox_intersection(block_box, human_box)
            smaller = min(block_area, bbox_area(human_box))
            area_ratio = max(block_area, bbox_area(human_box)) / max(1.0, smaller)
            if intersection / block_area >= 0.50 or (
                intersection / max(1.0, smaller) >= 0.85 and area_ratio <= 4.0
            ):
                duplicate_human = True
                break
        if duplicate_human:
            continue

        duplicate_coo = False
        for human_box in coo_boxes:
            intersection = bbox_intersection(block_box, human_box)
            smaller = min(block_area, bbox_area(human_box))
            area_ratio = max(block_area, bbox_area(human_box)) / max(1.0, smaller)
            if intersection / block_area >= 0.25 or (
                intersection / max(1.0, smaller) >= 0.85 and area_ratio <= 4.0
            ):
                duplicate_coo = True
                break
        if duplicate_coo:
            continue

        font_size = float(block.get("font_size") or 0.0)
        padding = max(2, min(8, int(round(font_size * 0.08))))
        envelope, roi = rectangular_envelope(
            block_box, width, height, padding=padding
        )
        rx0, ry0, rx1, ry1 = roi
        ctd = ctd_mask[ry0:ry1, rx0:rx1] & envelope[ry0:ry1, rx0:rx1]
        mts2025 = mts2025_mask[ry0:ry1, rx0:rx1] & envelope[ry0:ry1, rx0:rx1]
        union = ctd | mts2025
        if not union.any():
            continue
        global_union = np.zeros_like(ctd_mask, dtype=bool)
        global_union[ry0:ry1, rx0:rx1] = union
        raw_area = int(global_union.sum())
        if int((global_union & coo_envelope.astype(bool)).sum()) / max(1, raw_area) >= 0.20:
            continue
        global_union &= ~exclusion
        global_union = remove_rule_like_components(global_union)
        final_area = int(global_union.sum())
        if final_area < minimum_area:
            continue

        ctd_global = np.zeros_like(ctd_mask, dtype=bool)
        mts_global = np.zeros_like(ctd_mask, dtype=bool)
        ctd_global[ry0:ry1, rx0:rx1] = ctd
        mts_global[ry0:ry1, rx0:rx1] = mts2025
        ctd_global &= ~exclusion
        mts_global &= ~exclusion
        ctd_area = int(ctd_global.sum())
        mts_area = int(mts_global.sum())
        intersection = int((ctd_global & mts_global).sum())
        source_union = ctd_area + mts_area - intersection
        teacher_iou = intersection / source_union if source_union else 0.0
        if (
            ctd_area < minimum_area
            or mts_area < minimum_area
            or teacher_iou < 0.05
        ):
            continue
        bbox, area = mask_geometry(global_union)
        prepared.append(
            {
                "block_index": block_index,
                "block_box": block_box,
                "mask": global_union,
                "bbox": bbox,
                "area": area,
                "ctd_area": ctd_area,
                "mts2025_area": mts_area,
                "teacher_iou": teacher_iou,
                "block": block,
            }
        )

    # CTD can emit both a paragraph and one of its constituent lines. Prefer
    # the larger semantic block so a paragraph is not duplicated as instances.
    prepared.sort(
        key=lambda item: (-bbox_area(item["block_box"]), item["block_index"])
    )
    accepted: list[dict[str, Any]] = []
    for candidate in prepared:
        duplicate = False
        for existing in accepted:
            intersection = int((candidate["mask"] & existing["mask"]).sum())
            if not intersection:
                continue
            mask_containment = intersection / max(
                1, min(candidate["area"], existing["area"])
            )
            box_intersection = bbox_intersection(
                candidate["block_box"], existing["block_box"]
            )
            box_containment = box_intersection / max(
                1.0,
                min(
                    bbox_area(candidate["block_box"]),
                    bbox_area(existing["block_box"]),
                ),
            )
            if mask_containment >= 0.70 or box_containment >= 0.90:
                duplicate = True
                break
        if not duplicate:
            accepted.append(candidate)

    result: list[dict[str, Any]] = []
    for candidate in sorted(
        accepted, key=lambda item: (item["bbox"][1], item["bbox"][0])
    ):
        sources = []
        if candidate["ctd_area"]:
            sources.append("ctd")
        if candidate["mts2025_area"]:
            sources.append("mts2025")
        strategy = "_".join(sources) + ("_union" if len(sources) > 1 else "_only")
        quality = "silver_ctd_mts2025_union"
        block = candidate["block"]
        result.append(
            {
                "category_id": 1,
                "segmentation": encode_rle(candidate["mask"]),
                "area": candidate["area"],
                "bbox": candidate["bbox"],
                "iscrowd": 0,
                "source_dataset": "ComicTextDetector+MTS-2025",
                "transcription": None,
                "attributes": {
                    "quality_tier": quality,
                    "class_authority": "ComicTextDetector block proposal",
                    "annotation_scope": "all_visible_non_onomatopoeia_text",
                    "requires_human_review": True,
                    "source_block_index": candidate["block_index"],
                    "source_block_bbox": candidate["block_box"],
                    "source_block_language": block.get("language", "unknown"),
                    "source_block_vertical": bool(block.get("vertical", False)),
                    "mask_refinement": {
                        "strategy": strategy,
                        "pixel_sources": sources,
                        "ctd_area_clipped": candidate["ctd_area"],
                        "mts2025_area_clipped": candidate["mts2025_area"],
                        "ctd_mts2025_iou": round(candidate["teacher_iou"], 6),
                        "final_area": candidate["area"],
                    },
                },
                "_mask": candidate["mask"],
                "_source_envelope_bbox": candidate["block_box"],
            }
        )
    return result


def supplemental_text_from_residual_consensus(
    ctd_mask: np.ndarray,
    mts2025_mask: np.ndarray,
    source_annotations: list[dict[str, Any]],
    missing_text_records: list[dict[str, Any]],
    coo_annotations: list[dict[str, Any]],
    block_supplemental_text: list[dict[str, Any]],
    minimum_area: int,
) -> list[dict[str, Any]]:
    """Group high-precision teacher consensus missed by CTD's block grouper."""
    height, width = ctd_mask.shape
    known = np.zeros_like(ctd_mask, dtype=np.uint8)
    known_instance_boxes: list[list[float]] = []
    for annotation in source_annotations:
        if annotation["category_id"] != 1:
            continue
        box = annotation.get("v2026_bbox", annotation["bbox"])
        known_instance_boxes.append(box)
        _, (x0, y0, x1, y1) = rectangular_envelope(box, width, height, padding=2)
        known[y0:y1, x0:x1] = 1
    for record in missing_text_records:
        known_instance_boxes.append(record["v2026_bbox"])
        _, (x0, y0, x1, y1) = rectangular_envelope(
            record["v2026_bbox"], width, height, padding=2
        )
        known[y0:y1, x0:x1] = 1
    for annotation in coo_annotations:
        known_instance_boxes.append(annotation["bbox"])
        instance_mask = np.asarray(annotation["_mask"], dtype=bool)
        _, (x0, y0, x1, y1) = rectangular_envelope(
            annotation["bbox"], width, height, padding=2
        )
        known[y0:y1, x0:x1] = 1
        for box in connected_component_boxes(instance_mask):
            _, (x0, y0, x1, y1) = rectangular_envelope(
                box, width, height, padding=2
            )
            known[y0:y1, x0:x1] = 1
    for annotation in block_supplemental_text:
        box = annotation["_source_envelope_bbox"]
        _, (x0, y0, x1, y1) = rectangular_envelope(box, width, height, padding=3)
        known[y0:y1, x0:x1] = 1

    exclusion = cv2.dilate(known, np.ones((5, 5), np.uint8)).astype(bool)
    consensus = ctd_mask & mts2025_mask & ~exclusion
    if not consensus.any():
        return []
    horizontal = cv2.dilate(
        consensus.astype(np.uint8), np.ones((25, 61), np.uint8)
    )
    vertical = cv2.dilate(
        consensus.astype(np.uint8), np.ones((61, 25), np.uint8)
    )
    grouped = horizontal | vertical
    # Manga109 files are two-page spreads. Do not merge text instances across
    # the center gutter even when large title/scoreboard glyphs align.
    midpoint = width // 2
    grouped[:, max(0, midpoint - 2) : min(width, midpoint + 2)] = 0
    count, labels, _, _ = cv2.connectedComponentsWithStats(grouped, connectivity=8)
    teacher_union = (ctd_mask | mts2025_mask) & ~exclusion
    result: list[dict[str, Any]] = []
    for label in range(1, count):
        mask = teacher_union & (labels == label)
        mask = remove_rule_like_components(mask)
        area = int(mask.sum())
        if area < minimum_area:
            continue
        bbox, area = mask_geometry(mask)
        if bbox[2] < 3 or bbox[3] < 3:
            continue
        candidate_box_area = bbox_area(bbox)
        if any(
            bbox_intersection(bbox, known_box) / max(1.0, candidate_box_area) >= 0.10
            for known_box in known_instance_boxes
        ):
            continue
        ctd_area = int((ctd_mask & mask).sum())
        mts_area = int((mts2025_mask & mask).sum())
        intersection = int((ctd_mask & mts2025_mask & mask).sum())
        source_union = ctd_area + mts_area - intersection
        result.append(
            {
                "category_id": 1,
                "segmentation": encode_rle(mask),
                "area": area,
                "bbox": bbox,
                "iscrowd": 0,
                "source_dataset": "ComicTextDetector+MTS-2025",
                "transcription": None,
                "attributes": {
                    "quality_tier": "silver_ctd_mts2025_consensus",
                    "class_authority": "ComicTextDetector and MTS-2025 pixel consensus",
                    "annotation_scope": "all_visible_non_onomatopoeia_text",
                    "requires_human_review": True,
                    "source_component_index": label,
                    "mask_refinement": {
                        "strategy": "ctd_mts2025_union_around_consensus",
                        "pixel_sources": ["ctd", "mts2025"],
                        "ctd_area_clipped": ctd_area,
                        "mts2025_area_clipped": mts_area,
                        "ctd_mts2025_iou": round(
                            intersection / source_union if source_union else 0.0, 6
                        ),
                        "final_area": area,
                    },
                },
                "_mask": mask,
                "_source_envelope_bbox": bbox,
            }
        )
    return sorted(result, key=lambda annotation: (annotation["bbox"][1], annotation["bbox"][0]))


def unexplained_ctd_candidates(
    image_name: str,
    ctd_mask: np.ndarray,
    ctd_blocks: list[dict[str, Any]],
    source_annotations: list[dict[str, Any]],
    missing_text_records: list[dict[str, Any]],
    coo_annotations: list[dict[str, Any]],
    supplemental_text: list[dict[str, Any]],
    minimum_area: int,
) -> list[dict[str, Any]]:
    height, width = ctd_mask.shape
    known = np.zeros_like(ctd_mask, dtype=np.uint8)
    # Revised dialogue boxes are deliberately used as exclusion envelopes. This
    # is conservative around overlapping bubbles and prevents generic text from
    # being mislabeled as COO.
    for annotation in source_annotations:
        if annotation["category_id"] != 1:
            continue
        human_box = annotation.get("v2026_bbox", annotation["bbox"])
        x, y, box_width, box_height = (int(round(value)) for value in human_box)
        cv2.rectangle(
            known,
            (max(0, x - 2), max(0, y - 2)),
            (min(width - 1, x + box_width + 2), min(height - 1, y + box_height + 2)),
            1,
            thickness=-1,
        )
    for record in missing_text_records:
        x, y, box_width, box_height = (int(round(value)) for value in record["v2026_bbox"])
        cv2.rectangle(
            known,
            (max(0, x - 2), max(0, y - 2)),
            (min(width - 1, x + box_width + 2), min(height - 1, y + box_height + 2)),
            1,
            thickness=-1,
        )
    for annotation in coo_annotations:
        known[np.asarray(annotation["_mask"], dtype=bool)] = 1
    for annotation in supplemental_text:
        box = annotation["_source_envelope_bbox"]
        x, y, box_width, box_height = (int(round(value)) for value in box)
        cv2.rectangle(
            known,
            (max(0, x - 2), max(0, y - 2)),
            (min(width - 1, x + box_width + 2), min(height - 1, y + box_height + 2)),
            1,
            thickness=-1,
        )
    unexplained = ctd_mask & ~cv2.dilate(known, np.ones((5, 5), np.uint8)).astype(bool)
    grouped = cv2.dilate(unexplained.astype(np.uint8), np.ones((11, 11), np.uint8))
    count, labels, _, _ = cv2.connectedComponentsWithStats(grouped, connectivity=8)
    candidates: list[dict[str, Any]] = []
    for label in range(1, count):
        mask = unexplained & (labels == label)
        area = int(mask.sum())
        if area < minimum_area:
            continue
        bbox, _ = mask_geometry(mask)
        if bbox[2] < 3 or bbox[3] < 3:
            continue
        overlap = max(
            (
                bbox_intersection(bbox, [
                    block["xyxy"][0],
                    block["xyxy"][1],
                    block["xyxy"][2] - block["xyxy"][0],
                    block["xyxy"][3] - block["xyxy"][1],
                ])
                / max(1.0, bbox_area(bbox))
                for block in ctd_blocks
            ),
            default=0.0,
        )
        candidates.append(
            {
                "schema_version": SCHEMA_VERSION,
                "image": image_name,
                "candidate_id": f"ctd-unmatched:{image_name}:{len(candidates):04d}",
                "bbox": bbox,
                "area": area,
                "segmentation": encode_rle(mask),
                "ctd_block_overlap": round(overlap, 6),
                "status": "review",
                "candidate_class": "generic_text_unknown",
                "reason": "CTD ink outside revised dialogue boxes and human COO masks",
                "auto_promoted_to_onomatopoeia": False,
            }
        )
    return candidates


def append_relations(
    image: dict[str, Any], annotations: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    by_category: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for annotation in annotations:
        by_category[annotation["category_id"]].append(annotation)
        annotation.setdefault("attributes", {})
    relations: list[dict[str, Any]] = []
    bubble_text: dict[int, list[dict[str, Any]]] = defaultdict(list)
    panel_objects: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for text in by_category[1]:
        bubble_id, bubble_score = best_container(text["bbox"], by_category[3])
        panel_id, panel_score = best_container(text["bbox"], by_category[4])
        if bubble_id is not None:
            text["attributes"]["bubble_id"] = bubble_id
            bubble_text[bubble_id].append(text)
            relations.append(
                {"type": "contained_by_bubble", "from": text["id"], "to": bubble_id, "score": round(bubble_score, 6)}
            )
        if panel_id is not None:
            text["attributes"]["panel_id"] = panel_id
            panel_objects[panel_id].append(text)
            relations.append(
                {"type": "contained_by_panel", "from": text["id"], "to": panel_id, "score": round(panel_score, 6)}
            )
    for coo in by_category[2]:
        panel_id, panel_score = best_container(coo["bbox"], by_category[4])
        if panel_id is not None:
            coo["attributes"]["panel_id"] = panel_id
            panel_objects[panel_id].append(coo)
            relations.append(
                {"type": "contained_by_panel", "from": coo["id"], "to": panel_id, "score": round(panel_score, 6)}
            )
    bubble_by_id = {annotation["id"]: annotation for annotation in by_category[3]}
    for bubble_id, texts in bubble_text.items():
        ordered = sorted(texts, key=cmp_to_key(reading_comparator))
        bubble_by_id[bubble_id]["attributes"]["contained_text_ids"] = [text["id"] for text in texts]
        bubble_by_id[bubble_id]["attributes"]["text_reading_order_hint"] = [text["id"] for text in ordered]
    panel_by_id = {annotation["id"]: annotation for annotation in by_category[4]}
    for panel_id, objects in panel_objects.items():
        panel_by_id[panel_id]["attributes"]["contained_object_ids"] = [item["id"] for item in objects]
    ordered_text = sorted(by_category[1], key=cmp_to_key(reading_comparator))
    image["text_reading_order_hint"] = [annotation["id"] for annotation in ordered_text]
    image["reading_order_quality"] = "geometry_heuristic_not_ground_truth"
    return relations


def public_annotation(annotation: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in annotation.items() if not key.startswith("_")}


def process_page_geometry(
    task: tuple[
        dict[str, Any],
        list[dict[str, Any]],
        list[dict[str, Any]],
        list[dict[str, Any]],
    ],
    *,
    teacher_root: Path,
    mts2025_root: Path,
    review_min_area: int,
    supplement_min_area: int,
    supplement_unmatched_text: bool,
) -> tuple[
    list[dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
]:
    source_image, source_annotations, source_records, missing_text_records = task
    image_name = source_image["file_name"]
    ctd_mask, ctd_blocks = load_teacher(
        teacher_root,
        image_name,
        (int(source_image["width"]), int(source_image["height"])),
    )
    mts2025_mask = load_mts2025_teacher(
        mts2025_root,
        image_name,
        (int(source_image["width"]), int(source_image["height"])),
    )
    nodes = make_coo_nodes(
        source_image,
        source_annotations,
        source_records,
        ctd_mask,
        mts2025_mask,
    )
    coo_annotations = merge_coo_nodes(nodes)
    refined_text = refine_existing_text_masks(
        source_annotations, ctd_mask, mts2025_mask
    )
    recovered_text, missing_text_review = recover_missing_text_masks(
        image_name,
        missing_text_records,
        ctd_mask,
        mts2025_mask,
    )
    block_supplemental_text = (
        supplemental_text_from_ctd_blocks(
            image_name,
            ctd_mask,
            mts2025_mask,
            ctd_blocks,
            source_annotations,
            missing_text_records,
            coo_annotations,
            supplement_min_area,
        )
        if supplement_unmatched_text
        else []
    )
    supplemental_text = block_supplemental_text
    if supplement_unmatched_text:
        supplemental_text = supplemental_text + supplemental_text_from_residual_consensus(
            ctd_mask,
            mts2025_mask,
            source_annotations,
            missing_text_records,
            coo_annotations,
            block_supplemental_text,
            supplement_min_area,
        )
    review = unexplained_ctd_candidates(
        image_name,
        ctd_mask,
        ctd_blocks,
        source_annotations,
        missing_text_records,
        coo_annotations,
        supplemental_text,
        review_min_area,
    )
    review.extend(missing_text_review)
    review.extend(mask_refinement_reviews(image_name, refined_text, coo_annotations))
    for annotation in coo_annotations:
        annotation.pop("_mask", None)
    for annotation in supplemental_text:
        annotation.pop("_mask", None)
        annotation.pop("_source_envelope_bbox", None)
    return coo_annotations, refined_text, recovered_text, supplemental_text, review


def initialize_worker() -> None:
    cv2.setNumThreads(1)


def validate_output(dataset: dict[str, Any], expected_books: int) -> dict[str, Any]:
    image_ids = [image["id"] for image in dataset["images"]]
    annotation_ids = [annotation["id"] for annotation in dataset["annotations"]]
    if len(image_ids) != len(set(image_ids)) or len(annotation_ids) != len(set(annotation_ids)):
        raise ValueError("duplicate output IDs")
    image_by_id = {image["id"]: image for image in dataset["images"]}
    category_counts: Counter[str] = Counter()
    quality_counts: Counter[str] = Counter()
    human_coo_components = 0
    for annotation in dataset["annotations"]:
        image = image_by_id[annotation["image_id"]]
        if annotation["segmentation"]["size"] != [image["height"], image["width"]]:
            raise ValueError(f"RLE size mismatch in annotation {annotation['id']}")
        x, y, width, height = annotation["bbox"]
        if x < 0 or y < 0 or x + width > image["width"] + 1 or y + height > image["height"] + 1:
            raise ValueError(f"out-of-bounds annotation {annotation['id']}")
        category_counts[str(annotation["category_id"])] += 1
        quality_counts[annotation.get("attributes", {}).get("quality_tier", "unspecified")] += 1
        if annotation["category_id"] == 2:
            human_coo_components += len(
                annotation.get("attributes", {}).get("coo_annotation_ids", [])
            )
    books = {image["book"] for image in dataset["images"]}
    if len(books) != expected_books:
        raise ValueError(f"expected {expected_books} books, found {len(books)}")
    return {
        "books": len(books),
        "images": len(dataset["images"]),
        "annotations": len(dataset["annotations"]),
        "relations": len(dataset["relations"]),
        "category_counts": dict(category_counts),
        "quality_counts": dict(quality_counts),
        "human_coo_components": human_coo_components,
    }


def main() -> None:
    args = parse_args()
    if args.workers <= 0:
        raise ValueError("--workers must be positive")
    mts2025_summary_path = args.mts2025_root / "summary.json"
    if not args.books:
        if not mts2025_summary_path.is_file():
            raise FileNotFoundError(mts2025_summary_path)
        mts2025_summary = json.loads(mts2025_summary_path.read_text(encoding="utf-8"))
        if not mts2025_summary.get("complete"):
            raise ValueError(f"incomplete MTS-2025 teacher build: {mts2025_summary_path}")
        if mts2025_summary.get("runtime") != "PyTorch":
            raise ValueError("MTS-2025 teacher masks must be generated with PyTorch")
    cv2.setNumThreads(1)
    output = args.output.resolve()
    marker = output / ".manga109-segmentation-dataset"
    if output.exists() and any(output.iterdir()):
        if not args.overwrite:
            raise FileExistsError(f"output exists; pass --overwrite: {output}")
        if not marker.is_file():
            raise ValueError(f"refusing to replace unmarked directory: {output}")
        for path in sorted(output.rglob("*"), reverse=True):
            if path.is_file():
                path.unlink()
            elif path.is_dir():
                path.rmdir()
    output.mkdir(parents=True, exist_ok=True)
    marker.write_text("generated Manga109 Segmentation dataset\n", encoding="utf-8")

    split_by_book = load_merged_splits(args.merged_coco_root)
    selected_books = set(args.books) if args.books else set(split_by_book)
    unknown = selected_books - set(split_by_book)
    if unknown:
        raise ValueError(f"unknown books: {sorted(unknown)}")
    coo_records = load_coo_records(args.coo_annotations)
    missing_text_records = load_missing_v2026_text(args.merged_coco_root)

    outputs: dict[str, dict[str, Any]] = {}
    reviews: dict[str, list[dict[str, Any]]] = {split: [] for split in OFFICIAL_SPLITS}
    expected_human_text = Counter({split: 0 for split in OFFICIAL_SPLITS})
    expected_human_coo = Counter({split: 0 for split in OFFICIAL_SPLITS})
    supplemental_text_counts = Counter({split: 0 for split in OFFICIAL_SPLITS})
    next_image_id = {split: 1 for split in OFFICIAL_SPLITS}
    next_annotation_id = {split: 1 for split in OFFICIAL_SPLITS}
    for split in OFFICIAL_SPLITS:
        outputs[split] = {
            "info": {
                "description": "Manga109 Segmentation: text, onomatopoeia, bubble, and panel instance masks",
                "version": DATASET_VERSION,
                "year": 2026,
                "date_created": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "annotation_policy": "Human Manga109-v2026 text and release COO polygons are gold; unmatched CTD blocks supplement other visible non-onomatopoeia text with silver masks.",
                "reading_order_policy": "Hints are geometric and are not human ground truth.",
            },
            "licenses": [
                {"id": 1, "name": "Manga109 terms (academic-only; no third-party image transfer)", "url": "http://www.manga109.org/en/download.html"},
                {"id": 2, "name": "COO annotations CC BY 4.0", "url": "https://creativecommons.org/licenses/by/4.0/"},
            ],
            "categories": list(CATEGORIES),
            "images": [],
            "annotations": [],
            "relations": [],
        }

    for source_split in SOURCE_SPLITS:
        source_path = args.merged_coco_root / source_split / "_annotations.coco.json"
        source = json.loads(source_path.read_text(encoding="utf-8"))
        annotations_by_image: dict[int, list[dict[str, Any]]] = defaultdict(list)
        for annotation in source["annotations"]:
            annotations_by_image[int(annotation["image_id"])].append(annotation)
        selected_source_images = [
            source_image
            for source_image in source["images"]
            if Path(source_image["file_name"]).parts[0] in selected_books
        ]
        tasks = [
            (
                source_image,
                annotations_by_image[int(source_image["id"])],
                coo_records.get(source_image["file_name"], []),
                missing_text_records.get(source_image["file_name"], []),
            )
            for source_image in selected_source_images
        ]
        for source_image, source_annotations, source_coo, source_missing_text in tasks:
            split = split_by_book[Path(source_image["file_name"]).parts[0]]
            expected_human_text[split] += sum(
                annotation["category_id"] == 1 for annotation in source_annotations
            ) + len(source_missing_text)
            expected_human_coo[split] += sum(
                record.get("coo") is not None for record in source_coo
            )
        worker = partial(
            process_page_geometry,
            teacher_root=args.teacher_root,
            mts2025_root=args.mts2025_root,
            review_min_area=args.review_min_area,
            supplement_min_area=args.supplement_min_area,
            supplement_unmatched_text=args.supplement_unmatched_text,
        )
        with concurrent.futures.ProcessPoolExecutor(
            max_workers=args.workers,
            initializer=initialize_worker,
        ) as executor:
            geometries = executor.map(worker, tasks, chunksize=2)
            iterator = tqdm(
                zip(selected_source_images, geometries),
                total=len(tasks),
                desc=f"Build {source_split}",
                unit="page",
            )
            for source_image, (
                coo_annotations,
                refined_text,
                recovered_text,
                supplemental_text,
                page_reviews,
            ) in iterator:
                image_name = source_image["file_name"]
                book = Path(image_name).parts[0]
                split = split_by_book[book]
                dataset = outputs[split]
                image_id = next_image_id[split]
                next_image_id[split] += 1
                image = {
                    **source_image,
                    "id": image_id,
                    "book": book,
                    "page_index": int(Path(image_name).stem),
                    "source_split": source_split,
                    "dataset_split": split,
                    "license": 1,
                    "text_annotation_scope": "manga109_gold_plus_ctd_silver",
                    "text_exhaustive": False,
                    "supplemental_text_instances": len(supplemental_text),
                }
                dataset["images"].append(image)
                source_annotations = annotations_by_image[int(source_image["id"])]
                page_annotations: list[dict[str, Any]] = []
                for source_annotation in source_annotations:
                    if source_annotation["category_id"] in {1, 2}:
                        continue
                    annotation = {
                        key: value
                        for key, value in source_annotation.items()
                        if key not in {"id", "image_id"}
                    }
                    annotation["id"] = next_annotation_id[split]
                    next_annotation_id[split] += 1
                    annotation["image_id"] = image_id
                    annotation["attributes"] = {
                        "quality_tier": (
                            "gold_v2026_mangaseg"
                            if annotation["category_id"] == 1
                            else "gold_mangaseg"
                        )
                    }
                    page_annotations.append(annotation)
                for annotation in refined_text:
                    annotation["id"] = next_annotation_id[split]
                    next_annotation_id[split] += 1
                    annotation["image_id"] = image_id
                    page_annotations.append(annotation)
                for annotation in recovered_text:
                    annotation["id"] = next_annotation_id[split]
                    next_annotation_id[split] += 1
                    annotation["image_id"] = image_id
                    page_annotations.append(annotation)
                for annotation in supplemental_text:
                    annotation["id"] = next_annotation_id[split]
                    next_annotation_id[split] += 1
                    annotation["image_id"] = image_id
                    page_annotations.append(annotation)
                supplemental_text_counts[split] += len(supplemental_text)
                for annotation in coo_annotations:
                    annotation["id"] = next_annotation_id[split]
                    next_annotation_id[split] += 1
                    annotation["image_id"] = image_id
                    page_annotations.append(annotation)
                dataset["relations"].extend(append_relations(image, page_annotations))
                dataset["annotations"].extend(
                    public_annotation(annotation) for annotation in page_annotations
                )
                reviews[split].extend(page_reviews)

    split_stats: dict[str, Any] = {}
    expected_book_counts = {"train": 87, "validation": 11, "test": 11}
    if args.books:
        expected_book_counts = {
            split: sum(split_by_book[book] == split for book in selected_books)
            for split in OFFICIAL_SPLITS
        }
    for split in OFFICIAL_SPLITS:
        dataset = outputs[split]
        dataset["images"].sort(key=lambda image: image["file_name"])
        stats = validate_output(dataset, expected_book_counts[split])
        actual_text = stats["category_counts"].get("1", 0)
        expected_text = expected_human_text[split] + supplemental_text_counts[split]
        if actual_text != expected_text:
            raise ValueError(
                f"{split}: represented {actual_text} of "
                f"{expected_text} expected text instances"
            )
        if stats["human_coo_components"] != expected_human_coo[split]:
            raise ValueError(
                f"{split}: represented {stats['human_coo_components']} of "
                f"{expected_human_coo[split]} human COO components"
            )
        stats["human_text_instances_expected"] = expected_human_text[split]
        stats["supplemental_text_instances"] = supplemental_text_counts[split]
        stats["human_coo_components_expected"] = expected_human_coo[split]
        stats["review_candidates"] = len(reviews[split])
        split_stats[split] = stats
        atomic_write_json(output / "annotations" / f"{split}.coco.json", dataset, compact=True)
        atomic_write_jsonl(output / "review" / f"{split}.jsonl", reviews[split])

    build = {
        "schema_version": SCHEMA_VERSION,
        "name": "manga109-segmentation",
        "version": DATASET_VERSION,
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "sources": {
            "manga109_v2026_mangaseg_join": "manga109-v2026-mangaseg-coco",
            "coo_annotations": "Manga109_released_2026_05_21/annotations_COO",
            "coo_annotations_xml_count": len(list(args.coo_annotations.glob("*.xml"))),
            "v2026_missing_mangaseg_text_masks": sum(
                len(values) for values in missing_text_records.values()
            ),
            "images": "Manga109_released_2026_05_21/images (not included)",
            "comic_text_teacher_outputs": "local PyTorch build artifacts (not included)",
            "teacher_models_sha256": sha256(args.teacher_root / "teacher_models.json"),
            "mts2025_teacher_outputs": "local PyTorch build artifacts (not included)",
            "mts2025_models_sha256": sha256(args.mts2025_root / "teacher_models.json"),
        },
        "policy": {
            "class_authority": "human annotations are gold; unmatched Comic Text Detector blocks are silver text proposals",
            "instance_geometry": "Manga109-v2026 text boxes and local release COO polygons are gold; CTD blocks define supplemental text instances",
            "mask_join": "union MangaSeg, CTD, and MTS-2025 pixels clipped to each authoritative human envelope",
            "mask_fallback": "filled human geometry when the pixel union covers <1%, or when every available teacher-pair IoU is <0.05 and union coverage is <10%",
            "mask_disagreement": "queue for review when at least two teachers have >=16 pixels and every available pair has IoU <0.05",
            "mtsv3": "excluded",
            "mts2025": "included as a PyTorch pixel-mask teacher; never a class authority",
            "unmatched_ctd": "block-level proposals outside human text and COO coverage are promoted as silver text; residual pixels remain review candidates",
            "text_scope": "all visible linguistic text except onomatopoeia, including titles, credits, prose, and back matter",
            "evaluation_warning": "supplemental validation/test text is teacher-generated and not exhaustive human ground truth",
            "default_split": "current merged Manga109-v2026/MangaSeg book-disjoint 87/11/11",
        },
        "splits": split_stats,
    }
    atomic_write_json(output / "build.json", build)
    print(json.dumps(build, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
