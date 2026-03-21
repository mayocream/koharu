#!/usr/bin/env python3
"""Refine Manga109 annotations into PaddleOCR-ready detection/recognition data."""

from __future__ import annotations

import argparse
import json
import math
import os
import random
import shutil
import sys
import types
import xml.etree.ElementTree as ET
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Sequence

import cv2
import numpy as np

try:
    from tqdm import tqdm
except Exception:  # pragma: no cover
    tqdm = None


IMAGE_EXT = ".jpg"
DEFAULT_SPLIT_SPEC = "87,11,11"


@dataclass(frozen=True)
class Box:
    x1: int
    y1: int
    x2: int
    y2: int

    @property
    def width(self) -> int:
        return max(0, self.x2 - self.x1)

    @property
    def height(self) -> int:
        return max(0, self.y2 - self.y1)

    @property
    def area(self) -> int:
        return self.width * self.height

    @property
    def center_x(self) -> float:
        return self.x1 + self.width / 2.0

    @property
    def center_y(self) -> float:
        return self.y1 + self.height / 2.0

    def expand(self, image_shape: tuple[int, int, int], ratio: float = 0.06, min_pad: int = 4) -> "Box":
        pad_x = max(min_pad, int(round(self.width * ratio)))
        pad_y = max(min_pad, int(round(self.height * ratio)))
        h, w = image_shape[:2]
        return Box(
            max(0, self.x1 - pad_x),
            max(0, self.y1 - pad_y),
            min(w, self.x2 + pad_x),
            min(h, self.y2 + pad_y),
        )

    def intersection_area(self, other: "Box") -> int:
        x1 = max(self.x1, other.x1)
        y1 = max(self.y1, other.y1)
        x2 = min(self.x2, other.x2)
        y2 = min(self.y2, other.y2)
        if x2 <= x1 or y2 <= y1:
            return 0
        return (x2 - x1) * (y2 - y1)

    def iou(self, other: "Box") -> float:
        inter = self.intersection_area(other)
        if inter <= 0:
            return 0.0
        union = self.area + other.area - inter
        return inter / max(union, 1)

    def overlap_ratio(self, other: "Box") -> float:
        inter = self.intersection_area(other)
        return inter / max(self.area, 1)

    def contains_center(self, other: "Box") -> bool:
        return self.x1 <= other.center_x <= self.x2 and self.y1 <= other.center_y <= self.y2

    def to_quad(self) -> list[list[int]]:
        return [
            [self.x1, self.y1],
            [self.x2, self.y1],
            [self.x2, self.y2],
            [self.x1, self.y2],
        ]

    def to_list(self) -> list[int]:
        return [self.x1, self.y1, self.x2, self.y2]


@dataclass
class OriginalText:
    text_id: str
    bbox: Box
    transcript: str
    orientation: str


@dataclass
class CTDBlock:
    bbox: Box
    quad: list[list[int]]
    line_polygons: list[list[list[int]]]
    vertical: bool
    score: float
    support: float


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Refine Manga109 annotations using OpenCV candidates and comic-text-detector."
    )
    parser.add_argument("--dataset-root", default="data/Manga109_released_2021_12_30")
    parser.add_argument("--output-root", default="data/manga109_refined_paddleocr")
    parser.add_argument("--ctd-root", default="temp/comic-text-detector")
    parser.add_argument("--model-path", default="temp/comic-text-detector/data/comictextdetector.pt")
    parser.add_argument("--device", default="cuda", choices=["cuda", "cpu"])
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--split-spec", default=DEFAULT_SPLIT_SPEC)
    parser.add_argument("--overwrite", action="store_true")
    parser.add_argument("--book-limit", type=int, default=0)
    parser.add_argument("--page-limit", type=int, default=0)
    parser.add_argument("--ctd-input-size", type=int, default=1024)
    parser.add_argument("--ctd-conf-thresh", type=float, default=0.4)
    parser.add_argument("--ctd-nms-thresh", type=float, default=0.35)
    parser.add_argument("--cv2-min-area-ratio", type=float, default=0.015)
    parser.add_argument("--cv2-max-area-ratio", type=float, default=0.95)
    parser.add_argument("--cv2-max-candidates", type=int, default=8)
    return parser.parse_args()


def iter_with_progress(iterable: Sequence, desc: str) -> Iterable:
    if tqdm is None:
        return iterable
    return tqdm(iterable, desc=desc)


def install_ctd_compat_shims() -> None:
    aliases = {
        "bool8": np.bool_,
        "float_": np.float64,
        "int_": np.int64,
        "uint": np.uint64,
    }
    for name, value in aliases.items():
        if not hasattr(np, name):
            setattr(np, name, value)

    if "wandb" not in sys.modules:
        sys.modules["wandb"] = types.SimpleNamespace(init=lambda *args, **kwargs: None)

    if "torchsummary" not in sys.modules:
        torchsummary = types.ModuleType("torchsummary")
        torchsummary.summary = lambda *args, **kwargs: None
        sys.modules["torchsummary"] = torchsummary


def load_ctd_detector(
    ctd_root: Path,
    model_path: Path,
    device: str,
    input_size: int,
    conf_thresh: float,
    nms_thresh: float,
):
    install_ctd_compat_shims()
    sys.path.insert(0, str(ctd_root.resolve()))
    from inference import TextDetector  # type: ignore
    import torch

    if device == "cuda" and not torch.cuda.is_available():
        raise RuntimeError("CUDA was requested but torch.cuda.is_available() is false.")

    return TextDetector(
        model_path=str(model_path.resolve()),
        input_size=input_size,
        device=device,
        conf_thresh=conf_thresh,
        nms_thresh=nms_thresh,
        act="leaky",
    )


def load_books(dataset_root: Path) -> list[str]:
    books_file = dataset_root / "books.txt"
    return [line.strip() for line in books_file.read_text(encoding="utf-8").splitlines() if line.strip()]


def compute_split_counts(total_books: int, spec: str) -> tuple[int, int, int]:
    weights = [int(part.strip()) for part in spec.split(",")]
    if len(weights) != 3 or any(weight < 0 for weight in weights):
        raise ValueError(f"Invalid split spec: {spec}")
    if total_books <= 0:
        return 0, 0, 0

    weight_sum = sum(weights)
    raw = [total_books * weight / weight_sum for weight in weights]
    counts = [math.floor(value) for value in raw]
    remainder = total_books - sum(counts)
    order = sorted(
        range(3),
        key=lambda idx: (raw[idx] - counts[idx], weights[idx]),
        reverse=True,
    )
    for idx in order[:remainder]:
        counts[idx] += 1

    if total_books >= 3:
        for idx in range(3):
            if counts[idx] == 0:
                donor = max(range(3), key=lambda j: counts[j])
                if counts[donor] > 1:
                    counts[donor] -= 1
                    counts[idx] += 1

    return counts[0], counts[1], counts[2]


def split_books(books: list[str], seed: int, spec: str) -> dict[str, list[str]]:
    rng = random.Random(seed)
    shuffled = list(books)
    rng.shuffle(shuffled)
    train_count, val_count, test_count = compute_split_counts(len(shuffled), spec)
    train_books = shuffled[:train_count]
    val_books = shuffled[train_count : train_count + val_count]
    test_books = shuffled[train_count + val_count : train_count + val_count + test_count]
    return {"train": train_books, "val": val_books, "test": test_books}


def ensure_clean_dir(path: Path, overwrite: bool) -> None:
    if path.exists() and overwrite:
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def hardlink_or_copy(src: Path, dst: Path) -> None:
    if dst.exists():
        return
    dst.parent.mkdir(parents=True, exist_ok=True)
    try:
        os.link(src, dst)
    except Exception:
        shutil.copy2(src, dst)


def parse_original_texts(page: ET.Element) -> list[OriginalText]:
    texts: list[OriginalText] = []
    for text in page.findall("./text"):
        transcript = (text.text or "").strip()
        if not transcript:
            continue
        bbox = Box(
            int(text.attrib["xmin"]),
            int(text.attrib["ymin"]),
            int(text.attrib["xmax"]),
            int(text.attrib["ymax"]),
        )
        orientation = "vertical" if bbox.height >= bbox.width else "horizontal"
        texts.append(
            OriginalText(
                text_id=text.attrib["id"],
                bbox=bbox,
                transcript=transcript,
                orientation=orientation,
            )
        )
    return texts


def order_points_clockwise(points: np.ndarray) -> list[list[int]]:
    points = np.asarray(points, dtype=np.float32)
    center = points.mean(axis=0)
    angles = np.arctan2(points[:, 1] - center[1], points[:, 0] - center[0])
    ordered = points[np.argsort(angles)]
    start_idx = int(np.argmin(ordered.sum(axis=1)))
    ordered = np.roll(ordered, -start_idx, axis=0)
    return [[int(round(point[0])), int(round(point[1]))] for point in ordered]


def quad_from_line_polygons(line_polygons: Sequence[Sequence[Sequence[int]]], fallback_box: Box) -> list[list[int]]:
    if not line_polygons:
        return fallback_box.to_quad()
    points = np.array(line_polygons, dtype=np.float32).reshape(-1, 2)
    rect = cv2.minAreaRect(points)
    quad = cv2.boxPoints(rect)
    return order_points_clockwise(quad)


def merge_overlapping_boxes(boxes: Sequence[Box], iou_thresh: float, expand_px: int = 0) -> list[Box]:
    merged: list[Box] = []
    for box in sorted(boxes, key=lambda item: item.area, reverse=True):
        matched = False
        for idx, existing in enumerate(merged):
            compare_existing = Box(
                existing.x1 - expand_px,
                existing.y1 - expand_px,
                existing.x2 + expand_px,
                existing.y2 + expand_px,
            )
            compare_box = Box(
                box.x1 - expand_px,
                box.y1 - expand_px,
                box.x2 + expand_px,
                box.y2 + expand_px,
            )
            if compare_existing.iou(compare_box) >= iou_thresh or compare_existing.overlap_ratio(compare_box) >= 0.6:
                merged[idx] = Box(
                    min(existing.x1, box.x1),
                    min(existing.y1, box.y1),
                    max(existing.x2, box.x2),
                    max(existing.y2, box.y2),
                )
                matched = True
                break
        if not matched:
            merged.append(box)
    return merged


def connected_text_candidates(
    image: np.ndarray,
    parent_box: Box,
    min_area_ratio: float,
    max_area_ratio: float,
    max_candidates: int,
) -> list[Box]:
    crop_box = parent_box.expand(image.shape, ratio=0.08, min_pad=6)
    crop = image[crop_box.y1 : crop_box.y2, crop_box.x1 : crop_box.x2]
    if crop.size == 0:
        return []

    gray = cv2.cvtColor(crop, cv2.COLOR_BGR2GRAY)
    orientation = "vertical" if parent_box.height >= parent_box.width else "horizontal"
    parent_area = max(parent_box.area, 1)

    if orientation == "vertical":
        primary_kernel = cv2.getStructuringElement(
            cv2.MORPH_RECT,
            (3, max(9, int(round(parent_box.height * 0.12)))),
        )
    else:
        primary_kernel = cv2.getStructuringElement(
            cv2.MORPH_RECT,
            (max(9, int(round(parent_box.width * 0.12))), 3),
        )
    cleanup_kernel = cv2.getStructuringElement(cv2.MORPH_RECT, (3, 3))

    candidates: list[Box] = []
    for source in (gray, 255 - gray):
        binary = cv2.adaptiveThreshold(
            source,
            255,
            cv2.ADAPTIVE_THRESH_GAUSSIAN_C,
            cv2.THRESH_BINARY_INV,
            31,
            11,
        )
        binary = cv2.morphologyEx(binary, cv2.MORPH_OPEN, cleanup_kernel)
        merged = cv2.morphologyEx(binary, cv2.MORPH_CLOSE, cleanup_kernel)
        merged = cv2.dilate(merged, primary_kernel, iterations=1)

        num_labels, _, stats, _ = cv2.connectedComponentsWithStats(merged, connectivity=8)
        for label in range(1, num_labels):
            x, y, w, h, area = stats[label].tolist()
            bbox_area = max(1, w * h)
            if bbox_area < parent_area * min_area_ratio or bbox_area > parent_area * max_area_ratio:
                continue
            if w < 6 or h < 6:
                continue
            density = area / bbox_area
            if density < 0.10:
                continue
            box = Box(crop_box.x1 + x, crop_box.y1 + y, crop_box.x1 + x + w, crop_box.y1 + y + h)
            candidates.append(box)

    merged_candidates = merge_overlapping_boxes(candidates, iou_thresh=0.20, expand_px=6)
    merged_candidates.sort(key=lambda box: box.area, reverse=True)
    return merged_candidates[:max_candidates]


def reading_order(boxes: Sequence[Box], orientation: str) -> list[int]:
    indexed = list(enumerate(boxes))
    if orientation == "vertical":
        indexed.sort(key=lambda item: (-item[1].center_x, item[1].y1))
    else:
        indexed.sort(key=lambda item: (item[1].y1, item[1].x1))
    return [idx for idx, _ in indexed]


def split_transcript(transcript: str) -> list[str]:
    return [part.strip() for part in transcript.splitlines() if part.strip()]


def sanitize_filename(value: str) -> str:
    safe = []
    for char in value:
        if char.isalnum() or char in "-_.":
            safe.append(char)
        else:
            safe.append("_")
    return "".join(safe)


def ctd_blocks_for_page(blk_list: Sequence) -> list[dict]:
    blocks: list[dict] = []
    for block in blk_list:
        bbox = Box(int(block.xyxy[0]), int(block.xyxy[1]), int(block.xyxy[2]), int(block.xyxy[3]))
        line_polygons = []
        for line in block.lines:
            polygon = [[int(point[0]), int(point[1])] for point in line]
            line_polygons.append(polygon)
        blocks.append(
            {
                "bbox": bbox,
                "line_polygons": line_polygons,
                "vertical": bool(block.vertical),
            }
        )
    return blocks


def select_ctd_blocks(
    parent: OriginalText,
    ctd_blocks: Sequence[dict],
    cv2_candidates: Sequence[Box],
) -> list[CTDBlock]:
    expanded_parent = Box(
        parent.bbox.x1 - 8,
        parent.bbox.y1 - 8,
        parent.bbox.x2 + 8,
        parent.bbox.y2 + 8,
    )
    chosen: list[CTDBlock] = []
    for block in ctd_blocks:
        bbox: Box = block["bbox"]
        inter_parent = bbox.intersection_area(expanded_parent)
        if inter_parent <= 0:
            continue

        in_parent_ratio = inter_parent / max(bbox.area, 1)
        parent_cover_ratio = inter_parent / max(parent.bbox.area, 1)
        center_inside = expanded_parent.contains_center(bbox)
        if not center_inside and in_parent_ratio < 0.30 and parent_cover_ratio < 0.08:
            continue

        best_candidate_cover = 0.0
        best_candidate_iou = 0.0
        for candidate in cv2_candidates:
            inter_candidate = bbox.intersection_area(candidate)
            if inter_candidate <= 0:
                continue
            best_candidate_cover = max(best_candidate_cover, inter_candidate / max(bbox.area, 1))
            best_candidate_iou = max(best_candidate_iou, bbox.iou(candidate))

        line_count = len(block["line_polygons"])
        candidate_support = max(best_candidate_cover, best_candidate_iou)
        score = in_parent_ratio * 0.55 + parent_cover_ratio * 0.15 + candidate_support * 0.20 + min(line_count, 4) * 0.05
        if center_inside:
            score += 0.10

        is_tiny = bbox.area < max(100, int(parent.bbox.area * 0.03))
        if is_tiny and candidate_support < 0.22 and line_count <= 1:
            continue
        if candidate_support < 0.12 and in_parent_ratio < 0.55 and line_count <= 1:
            continue

        chosen.append(
            CTDBlock(
                bbox=bbox,
                quad=quad_from_line_polygons(block["line_polygons"], bbox),
                line_polygons=block["line_polygons"],
                vertical=bool(block["vertical"]),
                score=score,
                support=candidate_support,
            )
        )

    chosen.sort(key=lambda item: (item.score, item.bbox.area), reverse=True)
    deduped: list[CTDBlock] = []
    for block in chosen:
        duplicate = False
        for existing in deduped:
            if block.bbox.iou(existing.bbox) >= 0.65:
                duplicate = True
                break
            inter = block.bbox.intersection_area(existing.bbox)
            smaller = min(block.bbox.area, existing.bbox.area)
            if smaller > 0 and inter / smaller >= 0.80:
                duplicate = True
                break
        if not duplicate:
            deduped.append(block)
    return deduped


def final_blocks_for_text(
    parent: OriginalText,
    ctd_matches: Sequence[CTDBlock],
) -> tuple[str, list[dict], list[str]]:
    if not ctd_matches:
        return (
            "keep_original",
            [
                {
                    "bbox": parent.bbox,
                    "quad": parent.bbox.to_quad(),
                    "transcription": parent.transcript,
                    "source": "original",
                    "orientation": parent.orientation,
                }
            ],
            [],
        )

    orientation = "vertical" if sum(1 for item in ctd_matches if item.vertical) >= len(ctd_matches) / 2 else "horizontal"
    order = reading_order([item.bbox for item in ctd_matches], orientation)
    ordered_matches = [ctd_matches[idx] for idx in order]
    transcript_segments = split_transcript(parent.transcript)

    if len(ordered_matches) == 1:
        block = ordered_matches[0]
        return (
            "refined_single",
            [
                {
                    "bbox": block.bbox,
                    "quad": block.quad,
                    "transcription": parent.transcript,
                    "source": "ctd",
                    "orientation": orientation,
                    "score": round(block.score, 4),
                    "support": round(block.support, 4),
                }
            ],
            transcript_segments,
        )

    if transcript_segments and len(transcript_segments) == len(ordered_matches):
        final = []
        for segment, block in zip(transcript_segments, ordered_matches):
            final.append(
                {
                    "bbox": block.bbox,
                    "quad": block.quad,
                    "transcription": segment,
                    "source": "ctd_split",
                    "orientation": orientation,
                    "score": round(block.score, 4),
                    "support": round(block.support, 4),
                }
            )
        return "refined_split", final, transcript_segments

    line_counts = [max(1, len(block.line_polygons)) for block in ordered_matches]
    if transcript_segments and sum(line_counts) == len(transcript_segments):
        final = []
        cursor = 0
        grouped_segments: list[str] = []
        for block, line_count in zip(ordered_matches, line_counts):
            segment = "\n".join(transcript_segments[cursor : cursor + line_count])
            cursor += line_count
            grouped_segments.append(segment)
            final.append(
                {
                    "bbox": block.bbox,
                    "quad": block.quad,
                    "transcription": segment,
                    "source": "ctd_split_grouped",
                    "orientation": orientation,
                    "score": round(block.score, 4),
                    "support": round(block.support, 4),
                }
            )
        return "refined_split_grouped", final, grouped_segments

    return (
        "keep_original_split_mismatch",
        [
            {
                "bbox": parent.bbox,
                "quad": parent.bbox.to_quad(),
                "transcription": parent.transcript,
                "source": "original",
                "orientation": parent.orientation,
            }
        ],
        transcript_segments,
    )


def write_crop(image: np.ndarray, bbox: Box, output_path: Path) -> None:
    crop = image[bbox.y1 : bbox.y2, bbox.x1 : bbox.x2]
    if crop.size == 0:
        return
    output_path.parent.mkdir(parents=True, exist_ok=True)
    cv2.imwrite(str(output_path), crop)


def page_label_line(image_rel_path: str, entries: list[dict]) -> str:
    payload = [
        {"transcription": entry["transcription"], "points": entry["points"]}
        for entry in entries
    ]
    return f"{image_rel_path}\t{json.dumps(payload, ensure_ascii=False)}"


def main() -> None:
    args = parse_args()

    dataset_root = Path(args.dataset_root)
    output_root = Path(args.output_root)
    ctd_root = Path(args.ctd_root)
    model_path = Path(args.model_path)

    if not dataset_root.exists():
        raise FileNotFoundError(f"Manga109 root not found: {dataset_root}")
    if not model_path.exists():
        raise FileNotFoundError(f"comictextdetector.pt not found: {model_path}")

    ensure_clean_dir(output_root, overwrite=args.overwrite)
    (output_root / "det").mkdir(parents=True, exist_ok=True)
    (output_root / "rec").mkdir(parents=True, exist_ok=True)
    (output_root / "images").mkdir(parents=True, exist_ok=True)
    (output_root / "manifests").mkdir(parents=True, exist_ok=True)
    (output_root / "stats").mkdir(parents=True, exist_ok=True)

    books = load_books(dataset_root)
    if args.book_limit > 0:
        books = books[: args.book_limit]
    split_map = split_books(books, seed=args.seed, spec=args.split_spec)

    detector = load_ctd_detector(
        ctd_root=ctd_root,
        model_path=model_path,
        device=args.device,
        input_size=args.ctd_input_size,
        conf_thresh=args.ctd_conf_thresh,
        nms_thresh=args.ctd_nms_thresh,
    )

    summary = {
        "dataset_root": str(dataset_root.resolve()),
        "output_root": str(output_root.resolve()),
        "device": args.device,
        "model_path": str(model_path.resolve()),
        "split_spec": args.split_spec,
        "seed": args.seed,
        "books": {},
        "global": Counter(),
    }

    annotations_root = dataset_root / "annotations"
    images_root = dataset_root / "images"

    for split, split_books_list in split_map.items():
        det_lines: list[str] = []
        rec_lines: list[str] = []
        page_manifest_path = output_root / "manifests" / f"pages.{split}.jsonl"
        text_manifest_path = output_root / "manifests" / f"texts.{split}.jsonl"
        split_counter = Counter()

        with (
            page_manifest_path.open("w", encoding="utf-8") as page_manifest,
            text_manifest_path.open("w", encoding="utf-8") as text_manifest,
        ):
            for book in iter_with_progress(split_books_list, f"{split} books"):
                split_counter["books"] += 1
                xml_path = annotations_root / f"{book}.xml"
                image_dir = images_root / book

                tree = ET.parse(xml_path)
                pages = tree.getroot().findall("./pages/page")
                if args.page_limit > 0:
                    pages = pages[: args.page_limit]

                for page in pages:
                    page_index = int(page.attrib["index"])
                    image_path = image_dir / f"{page_index:03d}{IMAGE_EXT}"
                    image_rel_path = Path("images") / split / book / image_path.name
                    output_image_path = output_root / image_rel_path
                    hardlink_or_copy(image_path, output_image_path)

                    image = cv2.imread(str(image_path), cv2.IMREAD_COLOR)
                    if image is None:
                        continue

                    original_texts = parse_original_texts(page)
                    split_counter["pages"] += 1
                    split_counter["original_texts"] += len(original_texts)

                    _, _, blk_list = detector(image)
                    page_ctd_blocks = ctd_blocks_for_page(blk_list)
                    split_counter["ctd_blocks"] += len(page_ctd_blocks)

                    page_det_entries: list[dict] = []
                    page_manifest_record = {
                        "book_title": book,
                        "page_index": page_index,
                        "image_path": image_rel_path.as_posix(),
                        "original_text_count": len(original_texts),
                        "ctd_block_count": len(page_ctd_blocks),
                        "texts": [],
                    }

                    for original in original_texts:
                        cv2_candidates = connected_text_candidates(
                            image=image,
                            parent_box=original.bbox,
                            min_area_ratio=args.cv2_min_area_ratio,
                            max_area_ratio=args.cv2_max_area_ratio,
                            max_candidates=args.cv2_max_candidates,
                        )
                        split_counter["cv2_candidates"] += len(cv2_candidates)

                        ctd_matches = select_ctd_blocks(
                            parent=original,
                            ctd_blocks=page_ctd_blocks,
                            cv2_candidates=cv2_candidates,
                        )
                        action, final_blocks, transcript_segments = final_blocks_for_text(
                            parent=original,
                            ctd_matches=ctd_matches,
                        )
                        split_counter[action] += 1
                        split_counter["final_blocks"] += len(final_blocks)

                        manifest_blocks = []
                        for block_idx, block in enumerate(final_blocks):
                            bbox: Box = block["bbox"]
                            quad = block["quad"]
                            transcription = block["transcription"]
                            manifest_blocks.append(
                                {
                                    "bbox_xyxy": bbox.to_list(),
                                    "quad_clockwise": quad,
                                    "transcription": transcription,
                                    "source": block["source"],
                                    "orientation": block["orientation"],
                                    "score": block.get("score"),
                                    "support": block.get("support"),
                                }
                            )
                            page_det_entries.append(
                                {
                                    "points": quad,
                                    "transcription": transcription,
                                }
                            )

                            crop_name = (
                                f"{sanitize_filename(book)}_{page_index:03d}_"
                                f"{sanitize_filename(original.text_id)}_{block_idx:02d}.png"
                            )
                            crop_rel_path = Path("rec") / split / crop_name
                            crop_output_path = output_root / crop_rel_path
                            write_crop(image, bbox, crop_output_path)
                            if crop_output_path.exists():
                                rec_lines.append(f"{crop_rel_path.as_posix()}\t{transcription}")
                                split_counter["rec_crops"] += 1

                        text_record = {
                            "book_title": book,
                            "page_index": page_index,
                            "image_path": image_rel_path.as_posix(),
                            "text_id": original.text_id,
                            "original_bbox_xyxy": original.bbox.to_list(),
                            "original_quad_clockwise": original.bbox.to_quad(),
                            "original_transcript": original.transcript,
                            "original_orientation": original.orientation,
                            "cv2_candidates": [candidate.to_list() for candidate in cv2_candidates],
                            "ctd_matches": [
                                {
                                    "bbox_xyxy": match.bbox.to_list(),
                                    "quad_clockwise": match.quad,
                                    "vertical": match.vertical,
                                    "score": round(match.score, 4),
                                    "support": round(match.support, 4),
                                    "line_polygons": match.line_polygons,
                                }
                                for match in ctd_matches
                            ],
                            "transcript_segments": transcript_segments,
                            "action": action,
                            "final_blocks": manifest_blocks,
                        }
                        text_manifest.write(json.dumps(text_record, ensure_ascii=False) + "\n")
                        page_manifest_record["texts"].append(
                            {
                                "text_id": original.text_id,
                                "action": action,
                                "original_bbox_xyxy": original.bbox.to_list(),
                                "final_blocks": manifest_blocks,
                            }
                        )

                    det_lines.append(page_label_line(image_rel_path.as_posix(), page_det_entries))
                    page_manifest.write(json.dumps(page_manifest_record, ensure_ascii=False) + "\n")

        (output_root / "det" / f"{split}.txt").write_text("\n".join(det_lines), encoding="utf-8")
        (output_root / "rec" / f"rec_gt_{split}.txt").write_text("\n".join(rec_lines), encoding="utf-8")
        summary["books"][split] = dict(split_counter)
        summary["global"].update(split_counter)

    summary["global"] = dict(summary["global"])
    (output_root / "stats" / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
