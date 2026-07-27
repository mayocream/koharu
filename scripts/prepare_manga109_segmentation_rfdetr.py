# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "ijson>=3.4",
#   "tqdm>=4.67",
# ]
# ///
"""Prepare a slim, hardlinked RF-DETR view of Manga109 Segmentation.

The public dataset is annotation-only and retains provenance, relations, and
review metadata. RF-DETR's Roboflow-style COCO loader needs split directories
containing images and an ``_annotations.coco.json`` file. This script creates
that layout without duplicating the Manga109 image bytes, strips fields unused
by training, and excludes explicitly non-authoritative ``gold_legacy`` COO
records.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import shutil
import time
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

import ijson
from tqdm import tqdm


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DATASET = ROOT / "data" / "manga109-segmentation"
DEFAULT_IMAGES = ROOT / "data" / "Manga109_released_2026_05_21" / "images"
DEFAULT_OUTPUT = ROOT / "data" / "manga109-segmentation-rfdetr"
MARKER = ".manga109-segmentation-rfdetr-view"
SPLITS = (("train", "train"), ("validation", "valid"), ("test", "test"))
EXPECTED_CATEGORIES = {
    1: "text",
    2: "onomatopoeia",
    3: "bubble",
    4: "panel",
}
IMAGE_KEYS = ("id", "width", "height", "file_name", "license")
ANNOTATION_KEYS = (
    "id",
    "image_id",
    "category_id",
    "bbox",
    "segmentation",
    "area",
    "iscrowd",
)


@dataclass(frozen=True)
class SplitResult:
    source_split: str
    output_split: str
    images: int
    annotations: int
    excluded_annotations: int
    category_counts: dict[int, int]
    max_annotations_per_image: int
    output_bytes: int
    output_sha256: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=DEFAULT_DATASET)
    parser.add_argument("--images", type=Path, default=DEFAULT_IMAGES)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument(
        "--workers",
        type=int,
        default=min(32, os.cpu_count() or 1),
        help="Threads used to create image links.",
    )
    parser.add_argument(
        "--link-mode",
        choices=("hardlink", "copy"),
        default="hardlink",
        help="Hardlinks use no additional image storage and are the default.",
    )
    parser.add_argument(
        "--include-gold-legacy",
        action="store_true",
        help="Keep the 10 legacy MangaSeg COO records without current human COO polygons.",
    )
    parser.add_argument("--overwrite", action="store_true")
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_scalar(path: Path, prefix: str) -> Any:
    with path.open("rb") as file:
        return next(ijson.items(file, prefix, use_float=True))


def iter_items(path: Path, prefix: str) -> Iterable[dict[str, Any]]:
    with path.open("rb") as file:
        yield from ijson.items(file, prefix, use_float=True)


def safe_relative_image_path(value: str) -> Path:
    path = Path(value.replace("\\", "/"))
    if path.is_absolute() or not path.parts or any(part in {"", ".", ".."} for part in path.parts):
        raise ValueError(f"unsafe image path in COCO record: {value!r}")
    if path.suffix.lower() not in {".jpg", ".jpeg", ".png", ".webp"}:
        raise ValueError(f"unsupported image extension: {value!r}")
    return path


def prepare_output(output: Path, overwrite: bool) -> None:
    output = output.resolve()
    if output.exists() and any(output.iterdir()):
        if not overwrite:
            raise FileExistsError(f"output exists; pass --overwrite: {output}")
        if not (output / MARKER).is_file():
            raise ValueError(f"refusing to replace unmarked directory: {output}")
        resolved_root = ROOT.resolve()
        if resolved_root not in output.parents:
            raise ValueError(f"refusing to replace output outside repository: {output}")
        shutil.rmtree(output)
    output.mkdir(parents=True, exist_ok=True)
    (output / MARKER).write_text(
        "generated RF-DETR view of Manga109 Segmentation\n", encoding="utf-8"
    )


def slim_image(image: dict[str, Any]) -> dict[str, Any]:
    result = {key: image[key] for key in IMAGE_KEYS if key in image}
    required = {"id", "width", "height", "file_name"}
    missing = required - result.keys()
    if missing:
        raise ValueError(f"image record is missing keys {sorted(missing)}: {image}")
    result["file_name"] = safe_relative_image_path(str(result["file_name"])).as_posix()
    return result


def slim_annotation(annotation: dict[str, Any]) -> dict[str, Any]:
    result = {key: annotation[key] for key in ANNOTATION_KEYS if key in annotation}
    missing = set(ANNOTATION_KEYS) - result.keys()
    if missing:
        raise ValueError(
            f"annotation {annotation.get('id')} is missing keys {sorted(missing)}"
        )
    category_id = int(result["category_id"])
    if category_id not in EXPECTED_CATEGORIES:
        raise ValueError(f"unknown category ID {category_id}")
    bbox = result["bbox"]
    if len(bbox) != 4 or bbox[2] <= 0 or bbox[3] <= 0:
        raise ValueError(f"invalid bbox for annotation {result['id']}: {bbox}")
    segmentation = result["segmentation"]
    if not isinstance(segmentation, dict) or set(segmentation) != {"size", "counts"}:
        raise ValueError(f"annotation {result['id']} does not contain COCO RLE")
    if not isinstance(segmentation["counts"], str):
        raise ValueError(f"annotation {result['id']} RLE counts must be a string")
    return result


def is_excluded(annotation: dict[str, Any], include_gold_legacy: bool) -> bool:
    if include_gold_legacy:
        return False
    return annotation.get("attributes", {}).get("quality_tier") == "gold_legacy"


def write_json_item(file: Any, item: dict[str, Any], first: bool) -> bool:
    if not first:
        file.write(",")
    json.dump(item, file, ensure_ascii=False, separators=(",", ":"))
    return False


def transform_split(
    source_path: Path,
    output_path: Path,
    source_split: str,
    output_split: str,
    include_gold_legacy: bool,
) -> tuple[SplitResult, list[str]]:
    info = read_scalar(source_path, "info")
    licenses = read_scalar(source_path, "licenses")
    categories = read_scalar(source_path, "categories")
    category_map = {int(item["id"]): str(item["name"]) for item in categories}
    if category_map != EXPECTED_CATEGORIES:
        raise ValueError(
            f"category mapping mismatch in {source_path}: {category_map}"
        )

    image_names: list[str] = []
    image_ids: set[int] = set()
    annotation_ids: set[int] = set()
    annotations_per_image: Counter[int] = Counter()
    category_counts: Counter[int] = Counter()
    excluded = 0
    temporary = output_path.with_name(f".{output_path.name}.{os.getpid()}.tmp")
    temporary.parent.mkdir(parents=True, exist_ok=True)
    try:
        with temporary.open("w", encoding="utf-8", newline="\n") as file:
            file.write('{"info":')
            json.dump(info, file, ensure_ascii=False, separators=(",", ":"))
            file.write(',"licenses":')
            json.dump(licenses, file, ensure_ascii=False, separators=(",", ":"))
            file.write(',"categories":')
            json.dump(categories, file, ensure_ascii=False, separators=(",", ":"))
            file.write(',"images":[')
            first = True
            for raw_image in iter_items(source_path, "images.item"):
                image = slim_image(raw_image)
                image_id = int(image["id"])
                if image_id in image_ids:
                    raise ValueError(f"duplicate image ID {image_id} in {source_path}")
                image_ids.add(image_id)
                image_names.append(str(image["file_name"]))
                first = write_json_item(file, image, first)
            file.write('],"annotations":[')
            first = True
            for raw_annotation in iter_items(source_path, "annotations.item"):
                if is_excluded(raw_annotation, include_gold_legacy):
                    excluded += 1
                    continue
                annotation = slim_annotation(raw_annotation)
                annotation_id = int(annotation["id"])
                image_id = int(annotation["image_id"])
                if annotation_id in annotation_ids:
                    raise ValueError(
                        f"duplicate annotation ID {annotation_id} in {source_path}"
                    )
                if image_id not in image_ids:
                    raise ValueError(
                        f"annotation {annotation_id} refers to missing image {image_id}"
                    )
                annotation_ids.add(annotation_id)
                annotations_per_image[image_id] += 1
                category_counts[int(annotation["category_id"])] += 1
                first = write_json_item(file, annotation, first)
            file.write("]}\n")
        os.replace(temporary, output_path)
    finally:
        if temporary.exists():
            temporary.unlink()

    if len(image_names) != len(set(image_names)):
        raise ValueError(f"duplicate image file names in {source_path}")
    missing_categories = set(EXPECTED_CATEGORIES) - category_counts.keys()
    if missing_categories:
        raise ValueError(
            f"split {source_split} has no annotations for {sorted(missing_categories)}"
        )
    result = SplitResult(
        source_split=source_split,
        output_split=output_split,
        images=len(image_ids),
        annotations=len(annotation_ids),
        excluded_annotations=excluded,
        category_counts=dict(sorted(category_counts.items())),
        max_annotations_per_image=max(annotations_per_image.values(), default=0),
        output_bytes=output_path.stat().st_size,
        output_sha256=sha256(output_path),
    )
    return result, image_names


def place_image(source: Path, destination: Path, mode: str) -> None:
    if not source.is_file():
        raise FileNotFoundError(source)
    destination.parent.mkdir(parents=True, exist_ok=True)
    if mode == "hardlink":
        os.link(source, destination)
    else:
        shutil.copy2(source, destination)


def link_images(
    images_root: Path,
    split_root: Path,
    image_names: list[str],
    mode: str,
    workers: int,
) -> None:
    def task(name: str) -> None:
        relative = safe_relative_image_path(name)
        place_image(images_root / relative, split_root / relative, mode)

    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
        futures = [executor.submit(task, name) for name in image_names]
        for future in tqdm(
            concurrent.futures.as_completed(futures),
            total=len(futures),
            desc=f"{split_root.name} images",
            unit="image",
        ):
            future.result()


def verify_hardlinks(
    images_root: Path, split_root: Path, image_names: list[str]
) -> None:
    if not image_names:
        return
    indexes = sorted({0, len(image_names) // 2, len(image_names) - 1})
    for index in indexes:
        relative = safe_relative_image_path(image_names[index])
        source = images_root / relative
        destination = split_root / relative
        if not os.path.samefile(source, destination):
            raise ValueError(f"not a hardlink to the source image: {destination}")


def main() -> None:
    args = parse_args()
    if args.workers <= 0:
        raise ValueError("--workers must be positive")
    dataset = args.dataset.resolve()
    images = args.images.resolve()
    output = args.output.resolve()
    for path in (dataset, images):
        if not path.is_dir():
            raise FileNotFoundError(path)
    prepare_output(output, args.overwrite)

    started = time.perf_counter()
    results: list[SplitResult] = []
    all_names: dict[str, list[str]] = {}
    for source_split, output_split in SPLITS:
        source_path = dataset / "annotations" / f"{source_split}.coco.json"
        if not source_path.is_file():
            raise FileNotFoundError(source_path)
        split_root = output / output_split
        split_root.mkdir(parents=True, exist_ok=True)
        result, image_names = transform_split(
            source_path,
            split_root / "_annotations.coco.json",
            source_split,
            output_split,
            args.include_gold_legacy,
        )
        link_images(images, split_root, image_names, args.link_mode, args.workers)
        if args.link_mode == "hardlink":
            verify_hardlinks(images, split_root, image_names)
        results.append(result)
        all_names[output_split] = image_names

    overlap: dict[str, list[str]] = {}
    for index, (_, left) in enumerate(SPLITS):
        for _, right in SPLITS[index + 1 :]:
            shared = sorted(set(all_names[left]) & set(all_names[right]))
            if shared:
                overlap[f"{left}:{right}"] = shared[:20]
    if overlap:
        raise ValueError(f"image leakage across splits: {overlap}")

    manifest = {
        "schema_version": 1,
        "name": "manga109-segmentation-rfdetr",
        "source": str(dataset),
        "images": str(images),
        "link_mode": args.link_mode,
        "annotation_format": "slim COCO RLE for RF-DETR Roboflow loader",
        "filters": {
            "excluded_quality_tiers": (
                [] if args.include_gold_legacy else ["gold_legacy"]
            ),
            "review_jsonl_is_training_data": False,
        },
        "splits": {
            result.output_split: {
                "source_split": result.source_split,
                "images": result.images,
                "annotations": result.annotations,
                "excluded_annotations": result.excluded_annotations,
                "category_counts": {
                    EXPECTED_CATEGORIES[key]: value
                    for key, value in result.category_counts.items()
                },
                "max_annotations_per_image": result.max_annotations_per_image,
                "annotation_bytes": result.output_bytes,
                "annotation_sha256": result.output_sha256,
            }
            for result in results
        },
        "totals": {
            "images": sum(result.images for result in results),
            "annotations": sum(result.annotations for result in results),
            "excluded_annotations": sum(
                result.excluded_annotations for result in results
            ),
            "annotation_bytes": sum(result.output_bytes for result in results),
        },
        "elapsed_seconds": round(time.perf_counter() - started, 3),
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    (output / "README.md").write_text(
        """# Manga109 Segmentation RF-DETR training view

Generated by `scripts/prepare_manga109_segmentation_rfdetr.py`.

Images are hardlinks to the locally licensed Manga109 release by default; they
must not be uploaded or redistributed. The COCO files contain only fields used
by RF-DETR. Review candidates and annotation provenance remain in the source
`manga109-segmentation` package and are not training samples.
""",
        encoding="utf-8",
        newline="\n",
    )
    print(json.dumps(manifest, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
