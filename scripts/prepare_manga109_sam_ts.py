# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "ijson>=3.4",
#   "numpy>=2.0",
#   "Pillow>=11",
#   "pycocotools>=2.0.10",
#   "tqdm>=4.67",
# ]
# ///
"""Prepare a compact, book-balanced Manga109 dataset for SAM-TS fine-tuning.

The source package contains COCO instance masks for text, onomatopoeia,
bubbles, and panels. SAM-TS needs one binary semantic stroke mask per image.
This script selects a deterministic subset of clean pages, unions text and
onomatopoeia masks, hardlinks uniquely named source images, and writes 0/255
PNG labels in the flat directory structure expected by Hi-SAM's loader.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import shutil
import statistics
import tempfile
import time
from collections import Counter, defaultdict
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import ijson
import numpy as np
from PIL import Image
from pycocotools import mask as mask_utils
from tqdm import tqdm

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DATASET = ROOT / "data" / "manga109-segmentation"
DEFAULT_IMAGES = ROOT / "data" / "Manga109_released_2026_05_21" / "images"
DEFAULT_OUTPUT = ROOT / "data" / "manga109-sam-ts"
MARKER = ".manga109-sam-ts-dataset"
SPLITS = (
    ("train", "train", 10),
    ("validation", "val", 15),
    ("test", "test", 15),
)
TEXT_CATEGORY = 1
COO_CATEGORY = 2
EXPECTED_CATEGORIES = {
    1: "text",
    2: "onomatopoeia",
    3: "bubble",
    4: "panel",
}
REJECTED_QUALITY_TIERS = {
    "gold_v2026_bbox_fallback",
    "gold_polygon",
    "gold_legacy",
}


@dataclass
class Page:
    image_id: int
    file_name: str
    book: str
    page_index: int
    width: int
    height: int
    text_instances: int = 0
    coo_instances: int = 0
    instance_area_sum: int = 0
    has_rejected_mask: bool = False
    rejected_quality_tiers: Counter[str] = field(default_factory=Counter)

    @property
    def instances(self) -> int:
        return self.text_instances + self.coo_instances

    @property
    def foreground_ratio_estimate(self) -> float:
        return self.instance_area_sum / (self.width * self.height)


@dataclass(frozen=True)
class Selection:
    page: Page
    reason: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=DEFAULT_DATASET)
    parser.add_argument("--images", type=Path, default=DEFAULT_IMAGES)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--train-pages-per-book", type=int, default=10)
    parser.add_argument("--val-pages-per-book", type=int, default=15)
    parser.add_argument("--test-pages-per-book", type=int, default=15)
    parser.add_argument(
        "--link-mode",
        choices=("hardlink", "copy"),
        default="hardlink",
        help="Hardlinks avoid duplicating the locally licensed Manga109 images.",
    )
    parser.add_argument(
        "--include-fallback-pages",
        action="store_true",
        help=(
            "Include pages containing filled box/polygon or legacy masks. "
            "This is not recommended for pixel-level stroke training."
        ),
    )
    parser.add_argument("--overwrite", action="store_true")
    return parser.parse_args()


def iter_items(path: Path, prefix: str) -> Iterable[dict[str, Any]]:
    with path.open("rb") as file:
        yield from ijson.items(file, prefix, use_float=True)


def read_scalar(path: Path, prefix: str) -> Any:
    with path.open("rb") as file:
        return next(ijson.items(file, prefix, use_float=True))


def safe_relative_path(value: str) -> Path:
    path = Path(value.replace("\\", "/"))
    if path.is_absolute() or not path.parts:
        raise ValueError(f"unsafe image path: {value!r}")
    if any(part in {"", ".", ".."} for part in path.parts):
        raise ValueError(f"unsafe image path: {value!r}")
    if path.suffix.lower() not in {".jpg", ".jpeg"}:
        raise ValueError(f"SAM-TS preparation expects JPEG source images: {value!r}")
    return path


def annotation_quality(annotation: dict[str, Any]) -> str:
    return str(annotation.get("attributes", {}).get("quality_tier", "unspecified"))


def has_nonstroke_fallback(annotation: dict[str, Any]) -> bool:
    attributes = annotation.get("attributes", {})
    quality = str(attributes.get("quality_tier", ""))
    if quality in REJECTED_QUALITY_TIERS:
        return True
    return quality == "gold_mixed" and "gold_polygon" in attributes.get(
        "component_quality_tiers", []
    )


def load_pages(annotation_path: Path) -> tuple[dict[int, Page], dict[int, str]]:
    categories = read_scalar(annotation_path, "categories")
    category_map = {int(item["id"]): str(item["name"]) for item in categories}
    if category_map != EXPECTED_CATEGORIES:
        raise ValueError(
            f"category mapping mismatch in {annotation_path}: {category_map}"
        )

    pages: dict[int, Page] = {}
    for image in iter_items(annotation_path, "images.item"):
        image_id = int(image["id"])
        if image_id in pages:
            raise ValueError(f"duplicate image ID {image_id} in {annotation_path}")
        file_name = safe_relative_path(str(image["file_name"])).as_posix()
        pages[image_id] = Page(
            image_id=image_id,
            file_name=file_name,
            book=str(image["book"]),
            page_index=int(image["page_index"]),
            width=int(image["width"]),
            height=int(image["height"]),
        )

    quality_counts: Counter[str] = Counter()
    for annotation in iter_items(annotation_path, "annotations.item"):
        category = int(annotation["category_id"])
        if category not in {TEXT_CATEGORY, COO_CATEGORY}:
            continue
        image_id = int(annotation["image_id"])
        page = pages.get(image_id)
        if page is None:
            raise ValueError(
                f"annotation {annotation.get('id')} refers to missing image {image_id}"
            )
        quality = annotation_quality(annotation)
        quality_counts[quality] += 1
        if has_nonstroke_fallback(annotation):
            page.has_rejected_mask = True
            page.rejected_quality_tiers[quality] += 1
        if category == TEXT_CATEGORY:
            page.text_instances += 1
        else:
            page.coo_instances += 1
        page.instance_area_sum += int(annotation["area"])
    return pages, dict(sorted(quality_counts.items()))


def book_rng(seed: int, split: str, book: str) -> random.Random:
    digest = hashlib.sha256(f"{seed}:{split}:{book}".encode()).digest()
    return random.Random(int.from_bytes(digest[:8], "big"))


def select_book_pages(
    pages: list[Page], target: int, seed: int, split: str, book: str
) -> list[Selection]:
    if target == 0 or target >= len(pages):
        return [Selection(page, "all_clean_pages") for page in pages]
    if target < 4:
        rng = book_rng(seed, split, book)
        chosen = rng.sample(pages, target)
        return [Selection(page, "deterministic_random") for page in chosen]

    rng = book_rng(seed, split, book)
    tie_break = {page.image_id: rng.random() for page in pages}
    selected: dict[int, Selection] = {}

    def take(candidates: Iterable[Page], count: int, reason: str) -> None:
        for page in candidates:
            if page.image_id in selected:
                continue
            selected[page.image_id] = Selection(page, reason)
            if sum(item.reason == reason for item in selected.values()) >= count:
                return

    coo_quota = max(1, target // 5)
    dense_quota = max(1, target // 10)
    lower_density_quota = max(1, target // 10)
    take(
        sorted(
            (page for page in pages if page.coo_instances > 0),
            key=lambda page: (
                -page.coo_instances,
                -page.foreground_ratio_estimate,
                tie_break[page.image_id],
            ),
        ),
        coo_quota,
        "coo_heavy",
    )
    take(
        sorted(
            pages,
            key=lambda page: (
                -page.foreground_ratio_estimate,
                -page.instances,
                tie_break[page.image_id],
            ),
        ),
        dense_quota,
        "dense",
    )
    positive_ratios = sorted(
        page.foreground_ratio_estimate
        for page in pages
        if page.foreground_ratio_estimate > 0
    )
    lower_quartile_ratio = positive_ratios[(len(positive_ratios) - 1) // 4]
    take(
        sorted(
            pages,
            key=lambda page: (
                abs(page.foreground_ratio_estimate - lower_quartile_ratio),
                tie_break[page.image_id],
            ),
        ),
        lower_density_quota,
        "lower_density",
    )

    remaining_count = target - len(selected)
    median_ratio = statistics.median(positive_ratios) if positive_ratios else 0.0
    typical = sorted(
        pages,
        key=lambda page: (
            abs(page.foreground_ratio_estimate - median_ratio),
            tie_break[page.image_id],
        ),
    )
    take(typical, remaining_count, "typical")

    if len(selected) < target:
        remaining = [page for page in pages if page.image_id not in selected]
        rng.shuffle(remaining)
        for page in remaining[: target - len(selected)]:
            selected[page.image_id] = Selection(page, "fill")
    if len(selected) != target:
        raise ValueError(
            f"failed to select {target} pages from {book}; selected {len(selected)}"
        )
    return sorted(selected.values(), key=lambda item: item.page.page_index)


def select_pages(
    pages: dict[int, Page],
    pages_per_book: int,
    seed: int,
    split: str,
    include_fallback_pages: bool,
) -> tuple[list[Selection], dict[str, Any]]:
    by_book: dict[str, list[Page]] = defaultdict(list)
    excluded_cover_pages: Counter[str] = Counter()
    excluded_unannotated_pages: Counter[str] = Counter()
    rejected_pages: Counter[str] = Counter()
    for page in pages.values():
        if page.page_index == 0:
            excluded_cover_pages[page.book] += 1
            continue
        if page.instances == 0 or page.instance_area_sum == 0:
            excluded_unannotated_pages[page.book] += 1
            continue
        if page.has_rejected_mask and not include_fallback_pages:
            rejected_pages[page.book] += 1
            continue
        by_book[page.book].append(page)

    selections: list[Selection] = []
    shortages: dict[str, int] = {}
    for book in sorted(by_book):
        candidates = sorted(by_book[book], key=lambda page: page.page_index)
        target = len(candidates) if pages_per_book == 0 else pages_per_book
        if len(candidates) < target:
            shortages[book] = target - len(candidates)
            target = len(candidates)
        selections.extend(select_book_pages(candidates, target, seed, split, book))

    source_books = {page.book for page in pages.values()}
    if set(by_book) != source_books:
        missing_books = sorted(source_books - set(by_book))
        raise ValueError(f"all pages were rejected for books: {missing_books}")
    return selections, {
        "available_candidate_pages": sum(len(items) for items in by_book.values()),
        "excluded_cover_pages": sum(excluded_cover_pages.values()),
        "excluded_cover_pages_by_book": dict(sorted(excluded_cover_pages.items())),
        "excluded_unannotated_pages": sum(excluded_unannotated_pages.values()),
        "excluded_unannotated_pages_by_book": dict(
            sorted(excluded_unannotated_pages.items())
        ),
        "rejected_pages": sum(rejected_pages.values()),
        "rejected_pages_by_book": dict(sorted(rejected_pages.items())),
        "shortages": shortages,
    }


def normalize_rle(segmentation: dict[str, Any]) -> dict[str, Any]:
    rle = dict(segmentation)
    if set(rle) != {"size", "counts"}:
        raise ValueError("expected compressed COCO RLE with size and counts")
    if isinstance(rle["counts"], str):
        rle["counts"] = rle["counts"].encode("ascii")
    return rle


def collect_selected_rles(
    annotation_path: Path, selected_ids: set[int], include_fallback_pages: bool
) -> tuple[dict[int, list[dict[str, Any]]], Counter[int]]:
    rles: dict[int, list[dict[str, Any]]] = defaultdict(list)
    category_counts: Counter[int] = Counter()
    for annotation in iter_items(annotation_path, "annotations.item"):
        image_id = int(annotation["image_id"])
        category = int(annotation["category_id"])
        if image_id not in selected_ids or category not in {
            TEXT_CATEGORY,
            COO_CATEGORY,
        }:
            continue
        if has_nonstroke_fallback(annotation) and not include_fallback_pages:
            raise ValueError(
                f"selected page {image_id} unexpectedly contains a rejected mask"
            )
        rles[image_id].append(normalize_rle(annotation["segmentation"]))
        category_counts[category] += 1
    return rles, category_counts


def render_mask(page: Page, rles: list[dict[str, Any]]) -> np.ndarray:
    if not rles:
        return np.zeros((page.height, page.width), dtype=np.uint8)
    mask = mask_utils.decode(mask_utils.merge(rles))
    if mask.ndim == 3:
        mask = mask[..., 0]
    if mask.shape != (page.height, page.width):
        raise ValueError(
            f"mask shape mismatch for {page.file_name}: {mask.shape} != "
            f"{(page.height, page.width)}"
        )
    return mask.astype(np.uint8) * 255


def place_image(source: Path, destination: Path, mode: str) -> None:
    if mode == "hardlink":
        os.link(source, destination)
    else:
        shutil.copy2(source, destination)


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
    annotation_path: Path,
    images_root: Path,
    staging: Path,
    source_split: str,
    output_split: str,
    pages_per_book: int,
    seed: int,
    link_mode: str,
    include_fallback_pages: bool,
) -> tuple[dict[str, Any], set[str], set[str]]:
    pages, source_quality_counts = load_pages(annotation_path)
    selections, selection_audit = select_pages(
        pages,
        pages_per_book,
        seed,
        source_split,
        include_fallback_pages,
    )
    selected_ids = {selection.page.image_id for selection in selections}
    rles, selected_category_counts = collect_selected_rles(
        annotation_path, selected_ids, include_fallback_pages
    )

    image_dir = staging / f"{output_split}_images"
    mask_dir = staging / f"{output_split}_gt"
    manifest_dir = staging / "manifests"
    image_dir.mkdir(parents=True)
    mask_dir.mkdir(parents=True)
    manifest_dir.mkdir(parents=True, exist_ok=True)

    reason_counts: Counter[str] = Counter()
    book_counts: Counter[str] = Counter()
    total_foreground = 0
    empty_masks = 0
    output_names: set[str] = set()
    manifest_path = manifest_dir / f"{output_split}.jsonl"
    with manifest_path.open("w", encoding="utf-8", newline="\n") as manifest_file:
        for selection in tqdm(selections, desc=f"{output_split} masks", unit="page"):
            page = selection.page
            source_relative = safe_relative_path(page.file_name)
            source_image = images_root / source_relative
            if not source_image.is_file():
                raise FileNotFoundError(source_image)
            with Image.open(source_image) as image:
                if image.size != (page.width, page.height):
                    raise ValueError(
                        f"image size mismatch for {source_image}: {image.size} != "
                        f"{(page.width, page.height)}"
                    )
                if image.mode != "RGB":
                    raise ValueError(
                        f"expected RGB image for {source_image}, got {image.mode}"
                    )

            stem = f"m109_{page.image_id:09d}"
            image_name = f"{stem}.jpg"
            mask_name = f"{stem}.png"
            if image_name in output_names:
                raise ValueError(f"duplicate output image name: {image_name}")
            output_names.add(image_name)
            output_image = image_dir / image_name
            output_mask = mask_dir / mask_name
            place_image(source_image, output_image, link_mode)

            mask = render_mask(page, rles.get(page.image_id, []))
            foreground_pixels = int(np.count_nonzero(mask))
            if foreground_pixels == 0:
                raise ValueError(
                    f"selected page rendered an empty text mask: {page.file_name}"
                )
            total_foreground += foreground_pixels
            empty_masks += foreground_pixels == 0
            Image.fromarray(mask).save(output_mask, format="PNG", compress_level=6)

            if link_mode == "hardlink" and not os.path.samefile(
                source_image, output_image
            ):
                raise ValueError(f"not a hardlink to source image: {output_image}")
            with Image.open(output_mask) as saved_mask:
                if saved_mask.mode != "L" or saved_mask.size != (
                    page.width,
                    page.height,
                ):
                    raise ValueError(f"invalid saved mask: {output_mask}")

            reason_counts[selection.reason] += 1
            book_counts[page.book] += 1
            record = {
                "image": f"{output_split}_images/{image_name}",
                "mask": f"{output_split}_gt/{mask_name}",
                "source_image": page.file_name,
                "image_id": page.image_id,
                "book": page.book,
                "page_index": page.page_index,
                "width": page.width,
                "height": page.height,
                "selection_reason": selection.reason,
                "text_instances": page.text_instances,
                "onomatopoeia_instances": page.coo_instances,
                "foreground_pixels": foreground_pixels,
                "foreground_ratio": foreground_pixels / (page.width * page.height),
            }
            json.dump(record, manifest_file, ensure_ascii=False, separators=(",", ":"))
            manifest_file.write("\n")

    total_pixels = sum(
        selection.page.width * selection.page.height for selection in selections
    )
    summary = {
        "source_split": source_split,
        "pages_per_book_requested": pages_per_book,
        "books": len(book_counts),
        "pages": len(selections),
        "book_counts": dict(sorted(book_counts.items())),
        "selection_reasons": dict(sorted(reason_counts.items())),
        "text_instances": selected_category_counts[TEXT_CATEGORY],
        "onomatopoeia_instances": selected_category_counts[COO_CATEGORY],
        "empty_masks": empty_masks,
        "foreground_pixels": total_foreground,
        "total_pixels": total_pixels,
        "foreground_ratio": total_foreground / total_pixels,
        "source_quality_counts": source_quality_counts,
        **selection_audit,
    }
    return (
        summary,
        {selection.page.book for selection in selections},
        {selection.page.file_name for selection in selections},
    )


def write_readme(output: Path) -> None:
    (output / "README.md").write_text(
        """# Manga109 SAM-TS training view

Generated by `scripts/prepare_manga109_sam_ts.py`.

Each split has a flat image directory and a matching ground-truth directory.
Image and mask basenames are identical. Masks are single-channel PNG files
with `0` for background and `255` for the union of text and onomatopoeia
stroke pixels. Bubble and panel annotations are not training targets.

Images are hardlinks to the locally licensed Manga109 release by default.
They are academic-use-only and must not be uploaded or redistributed.

The default subset is selected independently inside every source book to
cover typical, COO-heavy, dense, and lower-quartile-density pages. Source
book-disjoint train, validation, and test splits are preserved. Page-zero
covers and pages without authoritative text or onomatopoeia pixels are
excluded. Pages containing filled box or polygon fallbacks, mixed masks with
polygon components, or legacy COO masks are also excluded because those
regions are not pixel-level stroke labels.

The files remain full Manga109 pages. Draw native-resolution 1024x1024 crops
on the fly during training, with a small probability of using a resized full
page. Do not split generated crops independently across train/val/test.

When registering these paths in Hi-SAM, use dataset names such as
`Manga109-train` and `Manga109-val`. Do not include `TextSeg` in the name:
Hi-SAM's special TextSeg loader interprets value 100 as foreground and value
255 as ignore, whereas these masks deliberately use the normal binary 0/255
convention. Add Manga109 as a supported binary dataset in the evaluator.
""",
        encoding="utf-8",
        newline="\n",
    )


def main() -> None:
    args = parse_args()
    page_counts = {
        "train": args.train_pages_per_book,
        "validation": args.val_pages_per_book,
        "test": args.test_pages_per_book,
    }
    if any(value < 0 for value in page_counts.values()):
        raise ValueError("pages per book must be non-negative; use 0 for all pages")

    dataset = args.dataset.resolve()
    images = args.images.resolve()
    output = args.output.resolve()
    if not dataset.is_dir():
        raise FileNotFoundError(dataset)
    if not images.is_dir():
        raise FileNotFoundError(images)
    output.parent.mkdir(parents=True, exist_ok=True)
    prepare_destination(output, args.overwrite)

    source_build_path = dataset / "build.json"
    source_build = (
        json.loads(source_build_path.read_text(encoding="utf-8"))
        if source_build_path.is_file()
        else None
    )
    started = time.perf_counter()
    staging = Path(
        tempfile.mkdtemp(prefix=f".{output.name}.", dir=str(output.parent))
    ).resolve()
    try:
        (staging / MARKER).write_text(
            "generated Manga109 SAM-TS training view\n", encoding="utf-8"
        )
        split_summaries: dict[str, Any] = {}
        split_books: dict[str, set[str]] = {}
        split_image_names: dict[str, set[str]] = {}
        for source_split, output_split, default_pages in SPLITS:
            annotation_path = dataset / "annotations" / f"{source_split}.coco.json"
            if not annotation_path.is_file():
                raise FileNotFoundError(annotation_path)
            requested_pages = page_counts.get(source_split, default_pages)
            summary, books, image_names = prepare_split(
                annotation_path=annotation_path,
                images_root=images,
                staging=staging,
                source_split=source_split,
                output_split=output_split,
                pages_per_book=requested_pages,
                seed=args.seed,
                link_mode=args.link_mode,
                include_fallback_pages=args.include_fallback_pages,
            )
            split_summaries[output_split] = summary
            split_books[output_split] = books
            split_image_names[output_split] = image_names

        output_splits = [item[1] for item in SPLITS]
        for index, left in enumerate(output_splits):
            for right in output_splits[index + 1 :]:
                shared_books = split_books[left] & split_books[right]
                shared_images = split_image_names[left] & split_image_names[right]
                if shared_books or shared_images:
                    raise ValueError(
                        f"split leakage between {left} and {right}: "
                        f"books={sorted(shared_books)}, images={sorted(shared_images)}"
                    )

        manifest = {
            "schema_version": 1,
            "name": "manga109-sam-ts",
            "source_dataset": str(dataset),
            "source_images": str(images),
            "source_version": source_build.get("version") if source_build else None,
            "seed": args.seed,
            "link_mode": args.link_mode,
            "label_format": {
                "type": "binary semantic PNG",
                "background": 0,
                "foreground": 255,
                "foreground_categories": ["text", "onomatopoeia"],
                "excluded_categories": ["bubble", "panel"],
            },
            "sampling": {
                "train_pages_per_book": args.train_pages_per_book,
                "val_pages_per_book": args.val_pages_per_book,
                "test_pages_per_book": args.test_pages_per_book,
                "strategy": "book-balanced typical/coo-heavy/dense/lower-density",
            },
            "filters": {
                "include_fallback_pages": args.include_fallback_pages,
                "exclude_page_index_zero": True,
                "exclude_unannotated_pages": True,
                "rejected_quality_tiers": sorted(REJECTED_QUALITY_TIERS),
                "reject_mixed_with_polygon_component": True,
                "review_jsonl_is_training_data": False,
            },
            "splits": split_summaries,
            "totals": {
                "pages": sum(item["pages"] for item in split_summaries.values()),
                "text_instances": sum(
                    item["text_instances"] for item in split_summaries.values()
                ),
                "onomatopoeia_instances": sum(
                    item["onomatopoeia_instances"] for item in split_summaries.values()
                ),
                "foreground_pixels": sum(
                    item["foreground_pixels"] for item in split_summaries.values()
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
