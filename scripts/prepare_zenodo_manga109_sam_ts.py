# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "numpy>=2.0",
#   "Pillow>=11",
#   "tqdm>=4.67",
# ]
# ///
"""Prepare the Zenodo Manga109 text masks for SAM-TS fine-tuning.

This script uses only the manually painted masks from Zenodo record 4511796
and the locally licensed Manga109 page images. It does not read or depend on
the manga109-segmentation dataset.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import re
import shutil
import tempfile
import time
import urllib.request
import zipfile
from io import BytesIO
from pathlib import Path, PurePosixPath
from typing import Any

import numpy as np
from PIL import Image
from tqdm import tqdm

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_IMAGES = ROOT / "data" / "Manga109_released_2026_05_21" / "images"
DEFAULT_ARCHIVE = ROOT / "data" / "zenodo-4511796" / "pre-processed.zip"
DEFAULT_OUTPUT = ROOT / "data" / "manga109-zenodo-sam-ts"
ZENODO_RECORD = "https://zenodo.org/records/4511796"
ARCHIVE_URL = "https://zenodo.org/api/records/4511796/files/pre-processed.zip/content"
ARCHIVE_MD5 = "8d878735631bfd5fa98398ae5203a53d"
MARKER = ".manga109-zenodo-sam-ts-dataset"
EXPECTED_BOOKS = 45
EXPECTED_PAGES_PER_BOOK = 10
EXPECTED_MASKS = EXPECTED_BOOKS * EXPECTED_PAGES_PER_BOOK
SPLIT_BOOK_COUNTS = {"train": 36, "val": 5, "test": 4}
BACKGROUND = (255, 255, 255)
EASY_TEXT = (1, 1, 1)
HARD_TEXT = (255, 1, 255)
ALLOWED_COLORS = {BACKGROUND, EASY_TEXT, HARD_TEXT}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--images", type=Path, default=DEFAULT_IMAGES)
    parser.add_argument("--archive", type=Path, default=DEFAULT_ARCHIVE)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument(
        "--crop-size",
        type=int,
        default=1024,
        help="Native square crop size for the training crop pool.",
    )
    parser.add_argument(
        "--crops-per-train-page",
        type=int,
        default=4,
        help=(
            "Mask-aware native crops per training page. Four crops plus one "
            "full page give an exact 80/20 crop/full-page mixed manifest."
        ),
    )
    parser.add_argument(
        "--link-mode",
        choices=("hardlink", "copy"),
        default="hardlink",
        help="Hardlinks avoid duplicating the locally licensed Manga109 images.",
    )
    parser.add_argument("--overwrite", action="store_true")
    return parser.parse_args()


def file_md5(path: Path) -> str:
    digest = hashlib.md5(usedforsecurity=False)
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download_archive(destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(destination.suffix + ".download")
    request = urllib.request.Request(
        ARCHIVE_URL,
        headers={"User-Agent": "koharu-manga109-dataset-preparer/1.0"},
    )
    try:
        with (
            urllib.request.urlopen(request) as response,
            temporary.open("wb") as output,
        ):
            total = int(response.headers.get("Content-Length", 0)) or None
            with tqdm(
                total=total, desc="Zenodo archive", unit="B", unit_scale=True
            ) as bar:
                while chunk := response.read(1024 * 1024):
                    output.write(chunk)
                    bar.update(len(chunk))
        temporary.replace(destination)
    finally:
        if temporary.exists():
            temporary.unlink()


def ensure_archive(path: Path) -> str:
    if not path.is_file():
        download_archive(path)
    checksum = file_md5(path)
    if checksum != ARCHIVE_MD5:
        raise ValueError(
            f"Zenodo archive checksum mismatch: {checksum} != {ARCHIVE_MD5}"
        )
    return checksum


def inventory_masks(archive: zipfile.ZipFile) -> dict[str, str]:
    masks: dict[str, str] = {}
    pages_by_book: dict[str, set[int]] = {}
    for info in archive.infolist():
        path = PurePosixPath(info.filename)
        if info.is_dir():
            continue
        if (
            path.is_absolute()
            or ".." in path.parts
            or len(path.parts) != 3
            or path.parts[0] != "pre-processed"
            or path.suffix.lower() != ".png"
        ):
            raise ValueError(f"unexpected archive member: {info.filename!r}")
        book = path.parts[1]
        if not re.fullmatch(r"\d{3}", path.stem):
            raise ValueError(f"unexpected page name: {info.filename!r}")
        page_index = int(path.stem)
        source_name = f"{book}/{path.stem}.jpg"
        if source_name in masks:
            raise ValueError(f"duplicate source page in archive: {source_name}")
        masks[source_name] = info.filename
        pages_by_book.setdefault(book, set()).add(page_index)

    if len(masks) != EXPECTED_MASKS or len(pages_by_book) != EXPECTED_BOOKS:
        raise ValueError(
            f"expected {EXPECTED_MASKS} masks from {EXPECTED_BOOKS} books, got "
            f"{len(masks)} masks from {len(pages_by_book)} books"
        )
    expected_pages = set(range(EXPECTED_PAGES_PER_BOOK))
    invalid = {
        book: sorted(pages)
        for book, pages in pages_by_book.items()
        if pages != expected_pages
    }
    if invalid:
        raise ValueError(f"books do not contain pages 000-009: {invalid}")
    return masks


def split_books(books: set[str], seed: int) -> dict[str, set[str]]:
    shuffled = sorted(books)
    random.Random(seed).shuffle(shuffled)
    train_end = SPLIT_BOOK_COUNTS["train"]
    val_end = train_end + SPLIT_BOOK_COUNTS["val"]
    splits = {
        "train": set(shuffled[:train_end]),
        "val": set(shuffled[train_end:val_end]),
        "test": set(shuffled[val_end:]),
    }
    if {name: len(items) for name, items in splits.items()} != SPLIT_BOOK_COUNTS:
        raise ValueError("book split allocation failed")
    if set().union(*splits.values()) != books:
        raise ValueError("book split does not cover the archive inventory")
    return splits


def safe_output_stem(book: str, page_index: int) -> str:
    safe_book = re.sub(r"[^A-Za-z0-9._-]+", "_", book).strip("._")
    if not safe_book:
        raise ValueError(f"book name has no safe output characters: {book!r}")
    return f"m109z_{safe_book}__{page_index:03d}"


def read_mask(
    archive: zipfile.ZipFile,
    member: str,
    expected_size: tuple[int, int],
) -> tuple[np.ndarray, int, int]:
    with archive.open(member) as file:
        data = file.read()
    with Image.open(BytesIO(data)) as source_mask:
        mask = source_mask.convert("RGB")
        if mask.size != expected_size:
            raise ValueError(
                f"mask size mismatch for {member}: {mask.size} != {expected_size}"
            )
        colors = mask.getcolors(mask.width * mask.height)
        if colors is None:
            raise ValueError(f"too many colors in mask: {member}")
        unexpected = {color for _, color in colors} - ALLOWED_COLORS
        if unexpected:
            raise ValueError(f"unexpected colors in {member}: {sorted(unexpected)}")
        array = np.asarray(mask)

    easy = np.all(array == EASY_TEXT, axis=2)
    hard = np.all(array == HARD_TEXT, axis=2)
    binary = np.logical_or(easy, hard).astype(np.uint8) * 255
    return binary, int(easy.sum()), int(hard.sum())


def place_image(source: Path, destination: Path, mode: str) -> None:
    if mode == "hardlink":
        os.link(source, destination)
    else:
        shutil.copy2(source, destination)


def integral_image(mask: np.ndarray) -> np.ndarray:
    integral = mask.astype(np.uint64).cumsum(axis=0).cumsum(axis=1)
    return np.pad(integral, ((1, 0), (1, 0)))


def box_sum(integral: np.ndarray, box: tuple[int, int, int, int]) -> int:
    x0, y0, x1, y1 = box
    return (
        int(integral[y1, x1])
        - int(integral[y0, x1])
        - int(integral[y1, x0])
        + int(integral[y0, x0])
    )


def crop_axis_positions(
    foreground_coordinates: np.ndarray, length: int, crop_size: int
) -> list[int]:
    maximum = length - crop_size
    if maximum < 0:
        raise ValueError(
            f"page axis {length} is smaller than native crop size {crop_size}"
        )
    if maximum == 0:
        return [0]

    positions = {0, maximum, maximum // 2}
    stride = max(32, crop_size // 8)
    positions.update(range(0, maximum + 1, stride))
    positions.add(maximum)
    if foreground_coordinates.size:
        for quantile in np.linspace(0.05, 0.95, 10):
            center = int(
                np.quantile(foreground_coordinates, quantile, method="nearest")
            )
            positions.add(min(max(center - crop_size // 2, 0), maximum))
    return sorted(positions)


def select_native_crop_boxes(
    binary: np.ndarray, crop_size: int, count: int
) -> tuple[list[tuple[int, int, int, int]], int]:
    if crop_size <= 0:
        raise ValueError(f"crop size must be positive, got {crop_size}")
    if count < 0:
        raise ValueError(f"crop count cannot be negative, got {count}")
    if count == 0:
        return [], 0

    foreground = binary > 0
    ys, xs = np.nonzero(foreground)
    if not len(xs):
        raise ValueError("cannot select foreground-aware crops from an empty mask")
    height, width = foreground.shape
    x_positions = crop_axis_positions(xs, width, crop_size)
    y_positions = crop_axis_positions(ys, height, crop_size)
    candidates = [
        (x, y, x + crop_size, y + crop_size) for y in y_positions for x in x_positions
    ]
    if len(candidates) < count:
        raise ValueError(
            f"only {len(candidates)} unique crop boxes available; requested {count}"
        )

    total_integral = integral_image(foreground)
    total_by_box = {box: box_sum(total_integral, box) for box in candidates}
    positive_candidates = [box for box in candidates if total_by_box[box] > 0]
    if len(positive_candidates) < count:
        raise ValueError(
            f"only {len(positive_candidates)} foreground-positive crops available; "
            f"requested {count}"
        )

    selected: list[tuple[int, int, int, int]] = []
    uncovered = foreground.copy()
    for _ in range(count):
        uncovered_integral = integral_image(uncovered)

        def rank(
            box: tuple[int, int, int, int],
            current_integral: np.ndarray = uncovered_integral,
        ) -> tuple[int, int, int, int, int]:
            new_pixels = box_sum(current_integral, box)
            if selected:
                x0, y0, _, _ = box
                diversity = min(
                    abs(x0 - old_x0) + abs(y0 - old_y0)
                    for old_x0, old_y0, _, _ in selected
                )
            else:
                diversity = 0
            return (
                new_pixels,
                total_by_box[box],
                diversity,
                -box[1],
                -box[0],
            )

        remaining = [box for box in positive_candidates if box not in selected]
        chosen = max(remaining, key=rank)
        selected.append(chosen)
        x0, y0, x1, y1 = chosen
        uncovered[y0:y1, x0:x1] = False

    covered_pixels = int(np.logical_and(foreground, ~uncovered).sum())
    return selected, covered_pixels


def prepare_destination(output: Path, overwrite: bool) -> None:
    if not output.exists():
        return
    if not any(output.iterdir()):
        output.rmdir()
        return
    if not overwrite:
        raise FileExistsError(f"output exists; pass --overwrite: {output}")
    if not (output / MARKER).is_file():
        raise ValueError(f"refusing to replace unmarked directory: {output}")
    shutil.rmtree(output)


def prepare_split(
    archive: zipfile.ZipFile,
    inventory: dict[str, str],
    books: set[str],
    images_root: Path,
    staging: Path,
    split: str,
    link_mode: str,
    crop_size: int,
    crops_per_train_page: int,
) -> tuple[dict[str, Any], set[str]]:
    image_dir = staging / f"{split}_images"
    mask_dir = staging / f"{split}_gt"
    manifest_dir = staging / "manifests"
    image_dir.mkdir(parents=True)
    mask_dir.mkdir(parents=True)
    manifest_dir.mkdir(parents=True, exist_ok=True)
    make_crops = split == "train" and crops_per_train_page > 0
    crop_image_dir = staging / "train_crop_images"
    crop_mask_dir = staging / "train_crop_gt"
    if make_crops:
        crop_image_dir.mkdir(parents=True)
        crop_mask_dir.mkdir(parents=True)

    records = [
        (source_name, member)
        for source_name, member in inventory.items()
        if source_name.split("/", 1)[0] in books
    ]
    records.sort()
    output_names: set[str] = set()
    included_sources: set[str] = set()
    skipped_empty: list[str] = []
    easy_total = 0
    hard_total = 0
    full_records: list[dict[str, Any]] = []
    crop_records: list[dict[str, Any]] = []
    crop_foreground_total = 0
    crop_source_covered_total = 0
    for source_name, member in tqdm(records, desc=f"{split} masks", unit="page"):
        source_image = images_root / Path(source_name)
        if not source_image.is_file():
            raise FileNotFoundError(source_image)
        book, filename = source_name.split("/", 1)
        page_index = int(Path(filename).stem)
        with Image.open(source_image) as image:
            if image.mode != "RGB":
                raise ValueError(
                    f"expected RGB Manga109 image, got {image.mode}: {source_image}"
                )
            width, height = image.size

        binary, easy_pixels, hard_pixels = read_mask(archive, member, (width, height))
        foreground_pixels = easy_pixels + hard_pixels
        if foreground_pixels == 0:
            skipped_empty.append(source_name)
            continue

        stem = safe_output_stem(book, page_index)
        image_name = f"{stem}.jpg"
        mask_name = f"{stem}.png"
        if image_name in output_names:
            raise ValueError(f"duplicate output name: {image_name}")
        output_names.add(image_name)
        output_image = image_dir / image_name
        output_mask = mask_dir / mask_name
        place_image(source_image, output_image, link_mode)
        Image.fromarray(binary, mode="L").save(
            output_mask, format="PNG", compress_level=6
        )

        if link_mode == "hardlink" and not os.path.samefile(source_image, output_image):
            raise ValueError(f"not a hardlink to source: {output_image}")
        easy_total += easy_pixels
        hard_total += hard_pixels
        included_sources.add(source_name)
        full_record = {
            "sample_type": "full_page",
            "image": f"{split}_images/{image_name}",
            "mask": f"{split}_gt/{mask_name}",
            "source_image": source_name,
            "source_mask": member,
            "book": book,
            "page_index": page_index,
            "is_cover": page_index == 0,
            "width": width,
            "height": height,
            "easy_text_pixels": easy_pixels,
            "hard_text_pixels": hard_pixels,
            "foreground_pixels": foreground_pixels,
            "foreground_ratio": foreground_pixels / (width * height),
        }
        full_records.append(full_record)

        if make_crops:
            crop_boxes, covered_pixels = select_native_crop_boxes(
                binary, crop_size, crops_per_train_page
            )
            crop_source_covered_total += covered_pixels
            with Image.open(source_image) as image:
                for crop_index, box in enumerate(crop_boxes):
                    x0, y0, x1, y1 = box
                    crop_stem = f"{stem}__crop{crop_index:02d}"
                    crop_image_name = f"{crop_stem}.jpg"
                    crop_mask_name = f"{crop_stem}.png"
                    crop_binary = binary[y0:y1, x0:x1]
                    crop_foreground = int(np.count_nonzero(crop_binary))
                    if crop_binary.shape != (crop_size, crop_size):
                        raise ValueError(
                            f"unexpected crop shape {crop_binary.shape}: {source_name}"
                        )
                    if crop_foreground == 0:
                        raise ValueError(f"empty selected crop: {source_name} {box}")
                    image.crop(box).save(
                        crop_image_dir / crop_image_name,
                        format="JPEG",
                        quality=95,
                        subsampling=0,
                        optimize=True,
                    )
                    Image.fromarray(crop_binary, mode="L").save(
                        crop_mask_dir / crop_mask_name,
                        format="PNG",
                        compress_level=6,
                    )
                    crop_foreground_total += crop_foreground
                    crop_records.append(
                        {
                            "sample_type": "native_crop",
                            "image": f"train_crop_images/{crop_image_name}",
                            "mask": f"train_crop_gt/{crop_mask_name}",
                            "source_image": source_name,
                            "source_mask": member,
                            "full_page_image": f"train_images/{image_name}",
                            "full_page_mask": f"train_gt/{mask_name}",
                            "book": book,
                            "page_index": page_index,
                            "is_cover": page_index == 0,
                            "crop_index": crop_index,
                            "crop_box_xyxy": list(box),
                            "width": crop_size,
                            "height": crop_size,
                            "foreground_pixels": crop_foreground,
                            "foreground_ratio": crop_foreground
                            / (crop_size * crop_size),
                            "source_foreground_pixels": foreground_pixels,
                            "source_foreground_fraction": crop_foreground
                            / foreground_pixels,
                        }
                    )

    def write_jsonl(path: Path, items: list[dict[str, Any]]) -> None:
        with path.open("w", encoding="utf-8", newline="\n") as file:
            for item in items:
                json.dump(item, file, ensure_ascii=False, separators=(",", ":"))
                file.write("\n")

    write_jsonl(manifest_dir / f"{split}.jsonl", full_records)
    if make_crops:
        write_jsonl(manifest_dir / "train_crops.jsonl", crop_records)
        write_jsonl(
            manifest_dir / "train_mixed_80crop_20full.jsonl",
            full_records + crop_records,
        )

    crop_summary = {
        "enabled": make_crops,
        "crop_size": crop_size if make_crops else None,
        "crops_per_page": crops_per_train_page if make_crops else 0,
        "samples": len(crop_records),
        "foreground_pixel_appearances": crop_foreground_total,
        "source_foreground_pixels_covered": crop_source_covered_total,
        "source_foreground_coverage": (
            crop_source_covered_total / (easy_total + hard_total)
            if make_crops and easy_total + hard_total
            else None
        ),
    }

    return (
        {
            "books": len(books),
            "book_names": sorted(books),
            "pages_available": len(records),
            "pages": len(included_sources),
            "covers": sum(name.endswith("/000.jpg") for name in included_sources),
            "skipped_empty_masks": skipped_empty,
            "easy_text_pixels": easy_total,
            "hard_text_pixels": hard_total,
            "foreground_pixels": easy_total + hard_total,
            "crop_pool": crop_summary,
        },
        included_sources,
    )


def write_readme(output: Path) -> None:
    (output / "README.md").write_text(
        """# Zenodo Manga109 SAM-TS training view

Generated by `scripts/prepare_zenodo_manga109_sam_ts.py` exclusively from the
manually painted `pre-processed.zip` masks in Zenodo record 4511796 and the
locally licensed Manga109 images. The `manga109-segmentation` dataset is not
used for labels, filtering, or split assignment.

The split is deterministic and book-disjoint: 36 train books, 5 validation
books, and 4 test books. Every valid mask from pages 000-009 is included. The
known empty mask `EvaLady/006.png` is excluded automatically.

Training has two explicitly separated pools:

- `train_crop_images` / `train_crop_gt`: four deterministic, mask-aware
  1024x1024 crops per page, cut directly from native pixels without resizing
- `train_images` / `train_gt`: one native full-page pair per page

`manifests/train_mixed_80crop_20full.jsonl` contains four crop samples and one
full-page sample for every training page. Uniform shuffled sampling of that
manifest therefore implements the requested 80% native-crop / 20% full-page
policy. Validation and test remain full-page-only.

Zenodo mask colors are converted as follows:

- white `(255, 255, 255)` -> background `0`
- black `(1, 1, 1)` easy text -> foreground `255`
- pink `(255, 1, 255)` hard text -> foreground `255`

Full-page images and masks retain the native 1654x1170 resolution; crop pairs
are 1024x1024 native-pixel windows. Full-page images are hardlinks to the
Manga109 release by default. Manga109 images remain academic-use-only and must
not be uploaded or redistributed. The Zenodo annotations are CC BY 4.0 and
require attribution to their authors.
""",
        encoding="utf-8",
        newline="\n",
    )


def main() -> None:
    args = parse_args()
    images = args.images.resolve()
    archive_path = args.archive.resolve()
    output = args.output.resolve()
    if not images.is_dir():
        raise FileNotFoundError(images)
    if args.crop_size <= 0:
        raise ValueError("--crop-size must be positive")
    if args.crops_per_train_page not in {0, 4}:
        raise ValueError(
            "--crops-per-train-page must be 4 for the 80/20 mixed policy, or "
            "0 to disable crop generation"
        )
    output.parent.mkdir(parents=True, exist_ok=True)
    prepare_destination(output, args.overwrite)
    archive_md5 = ensure_archive(archive_path)

    started = time.perf_counter()
    staging = Path(
        tempfile.mkdtemp(prefix=f".{output.name}.", dir=str(output.parent))
    ).resolve()
    try:
        (staging / MARKER).write_text(
            "generated Zenodo Manga109 SAM-TS training view\n", encoding="utf-8"
        )
        with zipfile.ZipFile(archive_path) as archive:
            bad_member = archive.testzip()
            if bad_member is not None:
                raise ValueError(f"corrupt ZIP member: {bad_member}")
            inventory = inventory_masks(archive)
            books = {name.split("/", 1)[0] for name in inventory}
            book_splits = split_books(books, args.seed)

            summaries: dict[str, Any] = {}
            split_sources: dict[str, set[str]] = {}
            for split in ("train", "val", "test"):
                summaries[split], split_sources[split] = prepare_split(
                    archive=archive,
                    inventory=inventory,
                    books=book_splits[split],
                    images_root=images,
                    staging=staging,
                    split=split,
                    link_mode=args.link_mode,
                    crop_size=args.crop_size,
                    crops_per_train_page=args.crops_per_train_page,
                )

        for left, right in (("train", "val"), ("train", "test"), ("val", "test")):
            if book_splits[left] & book_splits[right]:
                raise ValueError(f"book leakage between {left} and {right}")
            if split_sources[left] & split_sources[right]:
                raise ValueError(f"source leakage between {left} and {right}")

        manifest = {
            "schema_version": 2,
            "name": "manga109-zenodo-sam-ts",
            "source_record": ZENODO_RECORD,
            "source_archive": str(archive_path),
            "source_archive_md5": archive_md5,
            "source_images": str(images),
            "mask_variant": "pre-processed raw GIMP masks",
            "seed": args.seed,
            "link_mode": args.link_mode,
            "split_policy": "deterministic shuffled book split (36/5/4)",
            "uses_manga109_segmentation": False,
            "training_sampling_policy": {
                "crop_probability": 0.8,
                "full_page_probability": 0.2,
                "crop_size": args.crop_size,
                "crops_per_train_page": args.crops_per_train_page,
                "crop_resizing": "none; native source pixels",
                "mixed_manifest": (
                    "manifests/train_mixed_80crop_20full.jsonl"
                    if args.crops_per_train_page
                    else None
                ),
                "implementation": (
                    "uniform shuffled sampling of four crop entries plus one "
                    "full-page entry per source page"
                ),
                "validation_and_test": "full pages only",
            },
            "label_format": {
                "type": "binary semantic PNG",
                "background": 0,
                "foreground": 255,
                "foreground_sources": ["easy text", "hard text"],
            },
            "filters": {
                "exclude_empty_masks": True,
                "expected_exclusion": "EvaLady/006.jpg",
            },
            "splits": summaries,
            "totals": {
                "books": sum(item["books"] for item in summaries.values()),
                "pages_available": sum(
                    item["pages_available"] for item in summaries.values()
                ),
                "pages": sum(item["pages"] for item in summaries.values()),
                "covers": sum(item["covers"] for item in summaries.values()),
                "easy_text_pixels": sum(
                    item["easy_text_pixels"] for item in summaries.values()
                ),
                "hard_text_pixels": sum(
                    item["hard_text_pixels"] for item in summaries.values()
                ),
                "foreground_pixels": sum(
                    item["foreground_pixels"] for item in summaries.values()
                ),
            },
            "elapsed_seconds": round(time.perf_counter() - started, 3),
        }
        (staging / "manifest.json").write_text(
            json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        write_readme(staging)
        staging.replace(output)
        print(json.dumps(manifest, ensure_ascii=False, indent=2))
    finally:
        if staging.exists():
            if staging.parent != output.parent or not staging.name.startswith(
                f".{output.name}."
            ):
                raise ValueError(
                    f"refusing to remove unexpected staging path: {staging}"
                )
            shutil.rmtree(staging)


if __name__ == "__main__":
    main()
