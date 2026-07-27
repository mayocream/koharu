# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "ijson>=3.4",
#   "tqdm>=4.67",
# ]
# ///
"""Create an RF-DETR view containing PP-DocLayoutV3 distillation regions.

The original four-category annotations and images are preserved.  PP teacher
polygons are appended only to the training COCO file and use ``iscrowd`` as an
internal marker:

* 1: confident auxiliary-only normal-text region
* 2: ambiguous normal-text region that must be ignored by the dense loss
* 3: confident normal-text pseudo-instance missing from the gold annotations

The companion training data module preserves these records through geometric
augmentation and removes them before RF-DETR's instance matching loss.  COCO
validation and test annotations remain byte-for-byte semantic equivalents of
the strict base view.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import math
import os
import shutil
from collections import Counter, defaultdict
from collections.abc import Iterable
from pathlib import Path
from typing import Any

import ijson
from tqdm import tqdm

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASE = ROOT / "data" / "manga109-segmentation-rfdetr"
DEFAULT_TEACHERS = ROOT / "data" / "manga109-segmentation-pp-doclayoutv3"
DEFAULT_OUTPUT = ROOT / "data" / "manga109-segmentation-rfdetr-ppteacher"
MARKER = ".manga109-pp-doclayout-distillation-view"
SPLITS = ("train", "valid", "test")
EXPECTED_CATEGORIES = {
    1: "text",
    2: "onomatopoeia",
    3: "bubble",
    4: "panel",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, default=DEFAULT_BASE)
    parser.add_argument("--teachers", type=Path, default=DEFAULT_TEACHERS)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument(
        "--positive-score",
        type=float,
        default=0.70,
        help="Teacher predictions at or above this score become weak positives.",
    )
    parser.add_argument(
        "--ignore-score",
        type=float,
        default=0.45,
        help="Predictions between this and --positive-score become ignore regions.",
    )
    parser.add_argument("--minimum-area", type=float, default=16.0)
    parser.add_argument(
        "--pseudo-max-gold-coverage",
        type=float,
        default=0.05,
        help="Send a confident teacher region to RF-DETR only when gold text/COO covers less than this fraction of its box.",
    )
    parser.add_argument("--workers", type=int, default=min(32, os.cpu_count() or 1))
    parser.add_argument(
        "--allow-missing-teachers",
        action="store_true",
        help="Build a partial smoke-test view instead of requiring one teacher record per training image.",
    )
    parser.add_argument("--overwrite", action="store_true")
    return parser.parse_args()


def read_scalar(path: Path, prefix: str) -> Any:
    with path.open("rb") as file:
        return next(ijson.items(file, prefix, use_float=True))


def iter_items(path: Path, prefix: str) -> Iterable[dict[str, Any]]:
    with path.open("rb") as file:
        yield from ijson.items(file, prefix, use_float=True)


def safe_relative_path(value: str) -> Path:
    relative = Path(value.replace("\\", "/"))
    if (
        relative.is_absolute()
        or not relative.parts
        or any(part in {"", ".", ".."} for part in relative.parts)
    ):
        raise ValueError(f"unsafe relative path: {value!r}")
    return relative


def prepare_output(output: Path, overwrite: bool) -> None:
    if output.exists() and any(output.iterdir()):
        if not overwrite:
            raise FileExistsError(f"output exists; pass --overwrite: {output}")
        if not (output / MARKER).is_file():
            raise ValueError(f"refusing to replace unmarked directory: {output}")
        if ROOT.resolve() not in output.resolve().parents:
            raise ValueError(f"refusing to replace output outside repository: {output}")
        shutil.rmtree(output)
    output.mkdir(parents=True, exist_ok=True)
    (output / MARKER).write_text(
        "generated PP-DocLayoutV3 distillation view\n", encoding="utf-8"
    )


def polygon_area(points: list[list[float]]) -> float:
    return abs(
        sum(
            points[index][0] * points[(index + 1) % len(points)][1]
            - points[index][1] * points[(index + 1) % len(points)][0]
            for index in range(len(points))
        )
        * 0.5
    )


def box_gold_coverage(
    box: tuple[float, float, float, float],
    gold_boxes: list[tuple[float, float, float, float]],
) -> float:
    x1, y1, x2, y2 = box
    area = max(1e-9, (x2 - x1) * (y2 - y1))
    intersection = 0.0
    for gold_x1, gold_y1, gold_x2, gold_y2 in gold_boxes:
        intersection += max(0.0, min(x2, gold_x2) - max(x1, gold_x1)) * max(
            0.0, min(y2, gold_y2) - max(y1, gold_y1)
        )
    return min(1.0, intersection / area)


def teacher_annotation(
    prediction: dict[str, Any],
    annotation_id: int,
    image_id: int,
    width: int,
    height: int,
    positive_score: float,
    ignore_score: float,
    minimum_area: float,
    gold_boxes: list[tuple[float, float, float, float]],
    pseudo_max_gold_coverage: float,
) -> dict[str, Any] | None:
    score = float(prediction.get("score", 0.0))
    if score < ignore_score:
        return None
    raw_polygon = prediction.get("polygon")
    if not isinstance(raw_polygon, list) or len(raw_polygon) < 3:
        return None
    points: list[list[float]] = []
    for point in raw_polygon:
        if not isinstance(point, list) or len(point) != 2:
            return None
        x, y = float(point[0]), float(point[1])
        if not math.isfinite(x) or not math.isfinite(y):
            return None
        points.append([max(0.0, min(float(width), x)), max(0.0, min(float(height), y))])
    area = polygon_area(points)
    if area < minimum_area:
        return None
    xs = [point[0] for point in points]
    ys = [point[1] for point in points]
    x1, x2 = min(xs), max(xs)
    y1, y2 = min(ys), max(ys)
    if x2 <= x1 or y2 <= y1:
        return None
    if score < positive_score:
        marker = 2
    elif box_gold_coverage((x1, y1, x2, y2), gold_boxes) < pseudo_max_gold_coverage:
        marker = 3
    else:
        marker = 1
    flat_polygon = [coordinate for point in points for coordinate in point]
    return {
        "id": annotation_id,
        "image_id": image_id,
        "category_id": 1,
        "bbox": [x1, y1, x2 - x1, y2 - y1],
        "segmentation": [flat_polygon],
        "area": area,
        # Internal marker preserved by the custom distillation data module.
        "iscrowd": marker,
    }


def write_item(file: Any, item: dict[str, Any], first: bool) -> bool:
    if not first:
        file.write(",")
    json.dump(item, file, ensure_ascii=False, separators=(",", ":"))
    return False


def transform_annotation(
    source: Path,
    destination: Path,
    split: str,
    teachers: Path,
    positive_score: float,
    ignore_score: float,
    minimum_area: float,
    pseudo_max_gold_coverage: float,
    allow_missing_teachers: bool,
) -> tuple[list[str], Counter[str]]:
    info = read_scalar(source, "info")
    licenses = read_scalar(source, "licenses")
    categories = read_scalar(source, "categories")
    category_map = {
        int(category["id"]): str(category["name"]) for category in categories
    }
    if category_map != EXPECTED_CATEGORIES:
        raise ValueError(f"category mismatch in {source}: {category_map}")

    images: list[dict[str, Any]] = []
    names: list[str] = []
    for image in iter_items(source, "images.item"):
        relative = safe_relative_path(str(image["file_name"]))
        image["file_name"] = relative.as_posix()
        images.append(image)
        names.append(relative.as_posix())

    temporary = destination.with_name(f".{destination.name}.{os.getpid()}.tmp")
    counts: Counter[str] = Counter()
    gold_typography_boxes: dict[int, list[tuple[float, float, float, float]]] = (
        defaultdict(list)
    )
    maximum_annotation_id = 0
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
            for image in images:
                first = write_item(file, image, first)
            file.write('],"annotations":[')
            first = True
            for annotation in iter_items(source, "annotations.item"):
                maximum_annotation_id = max(
                    maximum_annotation_id, int(annotation["id"])
                )
                if split == "train" and int(annotation["category_id"]) in (1, 2):
                    x, y, width, height = map(float, annotation["bbox"])
                    gold_typography_boxes[int(annotation["image_id"])].append(
                        (x, y, x + width, y + height)
                    )
                first = write_item(file, annotation, first)
                counts["gold"] += 1

            if split == "train":
                next_id = maximum_annotation_id + 1
                for image in tqdm(
                    images, desc="append PP teacher regions", unit="page"
                ):
                    relative = safe_relative_path(str(image["file_name"]))
                    record_path = teachers / "records" / relative.with_suffix(".json")
                    if not record_path.is_file():
                        if allow_missing_teachers:
                            counts["teacher_record_missing"] += 1
                            continue
                        raise FileNotFoundError(
                            f"missing PP-DocLayoutV3 teacher record for {relative}: {record_path}"
                        )
                    record = json.loads(record_path.read_text(encoding="utf-8"))
                    if record.get("image") != relative.as_posix():
                        raise ValueError(f"teacher/image mismatch in {record_path}")
                    if (
                        int(record.get("width", -1)),
                        int(record.get("height", -1)),
                    ) != (
                        int(image["width"]),
                        int(image["height"]),
                    ):
                        raise ValueError(f"teacher size mismatch in {record_path}")
                    predictions = record.get("predictions")
                    if not isinstance(predictions, list):
                        raise TypeError(f"invalid predictions in {record_path}")
                    for prediction in predictions:
                        annotation = teacher_annotation(
                            prediction,
                            next_id,
                            int(image["id"]),
                            int(image["width"]),
                            int(image["height"]),
                            positive_score,
                            ignore_score,
                            minimum_area,
                            gold_typography_boxes[int(image["id"])],
                            pseudo_max_gold_coverage,
                        )
                        if annotation is None:
                            counts["teacher_rejected"] += 1
                            continue
                        first = write_item(file, annotation, first)
                        marker = int(annotation["iscrowd"])
                        count_name = {
                            1: "teacher_auxiliary",
                            2: "teacher_ignore",
                            3: "teacher_pseudo_instance",
                        }[marker]
                        counts[count_name] += 1
                        next_id += 1
            file.write("]}\n")
        os.replace(temporary, destination)
    finally:
        if temporary.exists():
            temporary.unlink()
    return names, counts


def place_image(source: Path, destination: Path) -> None:
    if not source.is_file():
        raise FileNotFoundError(source)
    destination.parent.mkdir(parents=True, exist_ok=True)
    os.link(source, destination)


def link_images(
    base_split: Path, output_split: Path, names: list[str], workers: int
) -> None:
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
        futures = [
            executor.submit(
                place_image,
                base_split / safe_relative_path(name),
                output_split / safe_relative_path(name),
            )
            for name in names
        ]
        for future in tqdm(
            concurrent.futures.as_completed(futures),
            total=len(futures),
            desc=f"{output_split.name} image links",
            unit="image",
        ):
            future.result()


def main() -> None:
    args = parse_args()
    base = args.base.resolve()
    teachers = args.teachers.resolve()
    output = args.output.resolve()
    if not base.is_dir():
        raise FileNotFoundError(base)
    if not teachers.is_dir():
        raise FileNotFoundError(teachers)
    if args.workers <= 0:
        raise ValueError("--workers must be positive")
    if not 0.0 < args.ignore_score < args.positive_score < 1.0:
        raise ValueError("require 0 < --ignore-score < --positive-score < 1")
    if args.minimum_area <= 0:
        raise ValueError("--minimum-area must be positive")
    if not 0.0 < args.pseudo_max_gold_coverage < 1.0:
        raise ValueError("--pseudo-max-gold-coverage must be between zero and one")
    prepare_output(output, args.overwrite)

    total_counts: Counter[str] = Counter()
    split_summary: dict[str, Any] = {}
    for split in SPLITS:
        base_split = base / split
        source = base_split / "_annotations.coco.json"
        if not source.is_file():
            raise FileNotFoundError(source)
        output_split = output / split
        output_split.mkdir(parents=True, exist_ok=True)
        names, counts = transform_annotation(
            source,
            output_split / "_annotations.coco.json",
            split,
            teachers,
            args.positive_score,
            args.ignore_score,
            args.minimum_area,
            args.pseudo_max_gold_coverage,
            args.allow_missing_teachers,
        )
        link_images(base_split, output_split, names, args.workers)
        total_counts.update(counts)
        split_summary[split] = {"images": len(names), **dict(counts)}

    teacher_metadata_path = teachers / "teacher_model.json"
    teacher_metadata = (
        json.loads(teacher_metadata_path.read_text(encoding="utf-8"))
        if teacher_metadata_path.is_file()
        else None
    )
    manifest = {
        "schema_version": 1,
        "name": "manga109-segmentation-rfdetr-ppteacher",
        "base": str(base),
        "teachers": str(teachers),
        "teacher_model": teacher_metadata,
        "teacher_markers": {
            "iscrowd=1": "weak positive for dense typography only",
            "iscrowd=2": "ignore region for dense typography only",
            "iscrowd=3": "novel weak-positive pseudo-instance for dense typography and RF-DETR",
            "RF-DETR instance loss": "iscrowd=0 and iscrowd=3 records enter matching",
        },
        "thresholds": {
            "positive_score": args.positive_score,
            "ignore_score": args.ignore_score,
            "minimum_area": args.minimum_area,
            "pseudo_max_gold_coverage": args.pseudo_max_gold_coverage,
        },
        "partial_teacher_cache_allowed": args.allow_missing_teachers,
        "splits": split_summary,
        "totals": dict(total_counts),
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    (output / "README.md").write_text(
        """# Manga109 PP-DocLayoutV3 distillation view

Local generated training view. Images are hardlinks to the licensed Manga109
release and must not be redistributed. The four-class gold annotations are
unchanged. Training-only PP-DocLayoutV3 regions are weak/ignore supervision for
the auxiliary dense typography head. Only confident regions with negligible
gold text/COO overlap are also passed to RF-DETR as text pseudo-instances.
""",
        encoding="utf-8",
    )
    print(json.dumps(manifest, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
