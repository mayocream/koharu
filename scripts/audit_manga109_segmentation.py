# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "numpy>=2.0",
#   "opencv-python-headless>=4.10",
#   "pycocotools>=2.0.10",
# ]
# ///
"""Render deterministic geometry audits for Manga109 Segmentation."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import cv2
import numpy as np
from pycocotools import mask as mask_utils


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DATASET = ROOT / "data" / "manga109-segmentation"
DEFAULT_IMAGES = ROOT / "data" / "Manga109_released_2026_05_21" / "images"
SAMPLES = (
    "ARMS/009.jpg",
    "UltraEleven/109.jpg",
    "MoeruOnisan_vol19/087.jpg",
    "HighschoolKimengumi_vol20/003.jpg",
)
COLORS = {
    1: (255, 120, 40),
    2: (30, 30, 235),
    3: (50, 210, 60),
    4: (30, 190, 240),
}
SILVER_TEXT_COLOR = (220, 30, 220)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=DEFAULT_DATASET)
    parser.add_argument("--images-root", type=Path, default=DEFAULT_IMAGES)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--samples", nargs="*", default=list(SAMPLES))
    parser.add_argument("--hide-review", action="store_true")
    return parser.parse_args()


def decode(rle: dict[str, Any]) -> np.ndarray:
    normalized = dict(rle)
    if isinstance(normalized["counts"], str):
        normalized["counts"] = normalized["counts"].encode("ascii")
    mask = mask_utils.decode(normalized)
    return mask[..., 0].astype(bool) if mask.ndim == 3 else mask.astype(bool)


def load_selected(dataset: Path, sample_names: list[str]) -> tuple[dict[str, tuple[dict[str, Any], list[dict[str, Any]]]], dict[str, list[dict[str, Any]]]]:
    wanted = set(sample_names)
    selected: dict[str, tuple[dict[str, Any], list[dict[str, Any]]]] = {}
    reviews: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for split in ("train", "validation", "test"):
        document = json.loads((dataset / "annotations" / f"{split}.coco.json").read_text(encoding="utf-8"))
        images = {image["id"]: image for image in document["images"] if image["file_name"] in wanted}
        annotations: dict[int, list[dict[str, Any]]] = defaultdict(list)
        for annotation in document["annotations"]:
            if annotation["image_id"] in images:
                annotations[annotation["image_id"]].append(annotation)
        for identifier, image in images.items():
            selected[image["file_name"]] = image, annotations[identifier]
        with (dataset / "review" / f"{split}.jsonl").open(encoding="utf-8") as file:
            for line in file:
                record = json.loads(line)
                if record["image"] in wanted:
                    reviews[record["image"]].append(record)
    missing = wanted - set(selected)
    if missing:
        raise ValueError(f"audit samples not found: {sorted(missing)}")
    return selected, reviews


def render(
    source: np.ndarray,
    annotations: list[dict[str, Any]],
    reviews: list[dict[str, Any]],
) -> np.ndarray:
    result = source.copy()
    for annotation in annotations:
        category = int(annotation["category_id"])
        quality = annotation.get("attributes", {}).get("quality_tier", "")
        color = (
            SILVER_TEXT_COLOR
            if category == 1 and quality.startswith("silver_")
            else COLORS[category]
        )
        mask = decode(annotation["segmentation"])
        if category in {1, 2}:
            colored = np.zeros_like(result)
            colored[:] = color
            result[mask] = cv2.addWeighted(result, 0.35, colored, 0.65, 0)[mask]
        x, y, width, height = (int(round(value)) for value in annotation["bbox"])
        thickness = 2 if category in {1, 2} else 1
        cv2.rectangle(result, (x, y), (x + width, y + height), color, thickness)
        if quality.startswith("silver_") or "recovered" in quality or quality in {"gold_polygon", "gold_mixed"}:
            cv2.putText(
                result,
                quality.replace("gold_", "").replace("silver_", "silver:"),
                (x, max(12, y - 3)),
                cv2.FONT_HERSHEY_SIMPLEX,
                0.35,
                color,
                1,
                cv2.LINE_AA,
            )
    for review in sorted(reviews, key=lambda item: item.get("area", 0), reverse=True)[:80]:
        x, y, width, height = (int(round(value)) for value in review["bbox"])
        cv2.rectangle(result, (x, y), (x + width, y + height), (220, 40, 220), 1)
    return result


def fit(image: np.ndarray, width: int = 800, height: int = 580) -> np.ndarray:
    scale = min(width / image.shape[1], height / image.shape[0])
    resized = cv2.resize(image, None, fx=scale, fy=scale, interpolation=cv2.INTER_AREA)
    canvas = np.full((height, width, 3), 245, dtype=np.uint8)
    top = (height - resized.shape[0]) // 2
    left = (width - resized.shape[1]) // 2
    canvas[top : top + resized.shape[0], left : left + resized.shape[1]] = resized
    return canvas


def main() -> None:
    args = parse_args()
    output = args.output or args.dataset / "audit"
    output.mkdir(parents=True, exist_ok=True)
    selected, reviews = load_selected(args.dataset, args.samples)
    summaries: list[dict[str, Any]] = []
    tiles: list[np.ndarray] = []
    for image_name in args.samples:
        image_info, annotations = selected[image_name]
        source = cv2.imread(str(args.images_root / image_name), cv2.IMREAD_COLOR)
        if source is None:
            raise OSError(f"failed to read {args.images_root / image_name}")
        rendered = render(
            source,
            annotations,
            [] if args.hide_review else reviews[image_name],
        )
        path = output / f"{Path(image_name).parts[0]}-{Path(image_name).stem}.jpg"
        cv2.imwrite(str(path), rendered, [cv2.IMWRITE_JPEG_QUALITY, 92])
        tile = fit(rendered)
        cv2.putText(tile, image_name, (12, 24), cv2.FONT_HERSHEY_SIMPLEX, 0.6, (20, 20, 20), 2, cv2.LINE_AA)
        tiles.append(tile)
        summaries.append(
            {
                "image": image_name,
                "categories": dict(Counter(str(annotation["category_id"]) for annotation in annotations)),
                "quality_tiers": dict(
                    Counter(annotation.get("attributes", {}).get("quality_tier", "unspecified") for annotation in annotations)
                ),
                "review_candidates": len(reviews[image_name]),
                "multi_text_bubbles": sum(
                    annotation["category_id"] == 3
                    and len(annotation.get("attributes", {}).get("contained_text_ids", [])) > 1
                    for annotation in annotations
                ),
                "reading_order_quality": image_info["reading_order_quality"],
            }
        )
    rows = [np.hstack(tiles[index : index + 2]) for index in range(0, len(tiles), 2)]
    if rows[-1].shape[1] < rows[0].shape[1]:
        rows[-1] = np.hstack((rows[-1], np.full_like(tiles[0], 245)))
    contact = np.vstack(rows)
    cv2.imwrite(str(output / "contact-sheet.jpg"), contact, [cv2.IMWRITE_JPEG_QUALITY, 92])
    (output / "audit.json").write_text(
        json.dumps({"samples": summaries}, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({"output": str(output.resolve()), "samples": summaries}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
