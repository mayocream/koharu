# /// script
# requires-python = ">=3.12,<3.15"
# ///
"""Build the annotation-only Manga109 Segmentation v2.0.0 HF package."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SOURCE = ROOT / "data" / "manga109-segmentation-textseg-filtered"
DEFAULT_BASE_PACKAGE = ROOT / "data" / "manga109-segmentation"
DEFAULT_OUTPUT = ROOT / "data" / "manga109-segmentation-v2.0.0"
MARKER = ".manga109-segmentation-dataset"
SPLITS = ("train", "validation", "test")
WINDOWS_ABSOLUTE_PATH = re.compile(r"^[A-Za-z]:[\\/]")


README = """---
pretty_name: Manga109 Segmentation
license: other
language:
  - ja
task_categories:
  - image-segmentation
  - object-detection
size_categories:
  - 10K<n<100K
tags:
  - manga
  - comics
  - layout-analysis
  - instance-segmentation
  - text-detection
  - reading-order
---

# Manga109 Segmentation

Manga109 Segmentation is an **annotation-only** dataset for manga layout and
instance segmentation. The current release is **v2.0.0**. It contains COCO RLE
masks for `text`, `onomatopoeia`, `bubble`, and `panel`, plus containment
relations and Japanese transcriptions where available.

> **Manga109 images are not included.** Obtain Manga109 separately and follow
> its terms. Every `images[].file_name` is relative to the Manga109 `images/`
> directory.

## What changed in v2.0.0

This is a breaking supervision update intended for standard RF-DETR-style
instance-segmentation training:

- Text masks on 449 pages use the manually painted Zenodo Manga109 text-mask
  dataset as the highest-priority pixel source.
- The remaining pages use
  [`mayocream/koharu-text-sam-ts-l`](https://huggingface.co/mayocream/koharu-text-sam-ts-l)
  to refine text/COO pixels inside authoritative human geometry.
- Good existing text masks are unioned with clipped teacher ink. Filled
  box/polygon fallbacks are replaced when the teacher has sufficient support.
- PP-DocLayoutV3 is used **only for bounding-box proposals**. For the 3,372
  accepted train-only pseudo instances, the mask is always TextSeg ink clipped
  to the proposal; the stored box is tightened to the resulting mask.
- 504 pages with materially incomplete positive labels were removed so their
  unlabeled text cannot become false-negative COCO background. This includes
  102 `000.jpg` cover pages.
- All 454,606 published annotations have `iscrowd: 0`. No custom dense head,
  ignore-region encoding, or synthetic negative-mask class is required.

The previous release remains available at the immutable `v1.1.0` tag.

## Dataset summary

The split remains book-disjoint. Filtering removes pages, not books.

| Split | Books | Pages | Text | COO | Bubbles | Panels | All annotations |
|---|---:|---:|---:|---:|---:|---:|---:|
| Train | 87 | 8,128 | 129,608 | 45,165 | 102,088 | 81,638 | 358,499 |
| Validation | 11 | 1,001 | 15,877 | 7,395 | 13,784 | 11,157 | 48,213 |
| Test | 11 | 969 | 16,826 | 6,388 | 13,835 | 10,845 | 47,894 |
| **Total** | **109** | **10,098** | **162,311** | **58,948** | **129,707** | **103,640** | **454,606** |

The annotations contain 355,817 geometric containment relations. The three
`review/*.jsonl` files contain sanitized per-page diagnostics for all 10,602
candidate pages, including the 504 excluded pages; they are not training
annotations.

## Package layout

```text
manga109-segmentation/
├── annotations/
│   ├── train.coco.json
│   ├── validation.coco.json
│   └── test.coco.json
├── review/
│   ├── train.jsonl
│   ├── validation.jsonl
│   └── test.jsonl
├── build.json
├── checksums.sha256
└── package_manifest.json
```

Use the relative image paths with a separately obtained Manga109 release:

```python
import json
from pathlib import Path

dataset_root = Path("manga109-segmentation")
image_root = Path("Manga109_released_2026_05_21/images")

with (dataset_root / "annotations/train.coco.json").open(encoding="utf-8") as f:
    coco = json.load(f)

image_path = image_root / coco["images"][0]["file_name"]
```

Masks use compressed COCO RLE. `bbox` is COCO `[x, y, width, height]`, `area`
is the mask-pixel count, and every annotation uses `iscrowd: 0`.

## Categories and relations

| ID | Category |
|---:|---|
| 1 | `text` |
| 2 | `onomatopoeia` |
| 3 | `bubble` |
| 4 | `panel` |

The top-level `relations` array records `contained_by_bubble` and
`contained_by_panel` geometry. Image and bubble reading-order fields are
heuristic hints, not human reading-order ground truth.

## Mask provenance

`attributes.quality_tier` gives the direct training provenance:

- `gold_mangaseg`: retained MangaSegmentation bubble/panel mask.
- `gold_zenodo_refined`: human-geometry instance refined with manually painted
  Zenodo text-mask pixels.
- `silver_textseg_refined`: human-geometry instance refined with TextSeg.
- `silver_pp_bbox_textseg_mask`: PP-DocLayoutV3 proposal whose pixels come from
  TextSeg; used only in train.

Counts by split are recorded in `build.json`. Detailed page-level agreement,
teacher coverage, proposal boxes, exclusion reasons, and visual-review flags
are in `review/*.jsonl` without local filesystem paths.

## Page filtering and negative supervision

A page is excluded when it has no target typography but at least 512 teacher
foreground pixels, or when it has at least 10,000 teacher pixels and final
recall below 0.20. The two tests may overlap.

Seven retained pages have no typography instances and a near-empty teacher
mask (at most 382 pixels). They provide safe implicit background supervision.
There is deliberately no `negative` segmentation category: ordinary COCO
background is the negative signal, while incomplete pages are omitted.

## Limitations

- Most text/COO instance identities and envelopes are human-authored, but many
  final pixel masks are model-assisted.
- PP-DocLayoutV3 can introduce class/proposal errors in the 3,372 train-only
  pseudo instances, even though TextSeg supplies their pixels.
- Validation and test include teacher-refined pixels, so use independent human
  or Zenodo evaluation when measuring absolute mask quality.
- Bubble/panel masks inherit MangaSegmentation ambiguity and source errors.
- Relations and reading order are geometric heuristics.
- Manga109 images and their usage rights are not distributed here.

## License and sources

`license: other` is intentional because the package combines derived
annotations from multiple sources. Users must follow every upstream license
and attribution requirement. In particular:

- [Manga109 and Manga109-v2026](https://manga109.github.io/manga109-project-website/en/)
- [MangaSegmentation](https://huggingface.co/datasets/MS92/MangaSegmentation)
- [COO: Comic Onomatopoeia Dataset](https://github.com/ku21fan/COO-Comic-Onomatopoeia)
- [Zenodo Manga text-mask dataset](https://doi.org/10.5281/zenodo.4511796)
  (CC BY 4.0)
- [`mayocream/koharu-text-sam-ts-l`](https://huggingface.co/mayocream/koharu-text-sam-ts-l)
- [`PaddlePaddle/PP-DocLayoutV3_safetensors`](https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors)

The package does not grant access to or a license for Manga109 images. Never
upload the Manga109 image files with this repository. Exact source revisions,
policy thresholds, split counts, and quality-tier counts are in `build.json`;
file integrity is recorded in `checksums.sha256`.
"""


SPLIT_STATS = {
    "train": {
        "books": 87,
        "candidate_images": 8475,
        "images": 8128,
        "excluded_images": 347,
        "annotations": 358499,
        "relations": 280456,
        "category_counts": {"1": 129608, "2": 45165, "3": 102088, "4": 81638},
        "quality_counts": {
            "gold_mangaseg": 183726,
            "gold_zenodo_refined": 6662,
            "silver_pp_bbox_textseg_mask": 3372,
            "silver_textseg_refined": 164739,
        },
    },
    "validation": {
        "books": 11,
        "candidate_images": 1057,
        "images": 1001,
        "excluded_images": 56,
        "annotations": 48213,
        "relations": 37502,
        "category_counts": {"1": 15877, "2": 7395, "3": 13784, "4": 11157},
        "quality_counts": {
            "gold_mangaseg": 24941,
            "gold_zenodo_refined": 884,
            "silver_textseg_refined": 22388,
        },
    },
    "test": {
        "books": 11,
        "candidate_images": 1070,
        "images": 969,
        "excluded_images": 101,
        "annotations": 47894,
        "relations": 37859,
        "category_counts": {"1": 16826, "2": 6388, "3": 13835, "4": 10845},
        "quality_counts": {
            "gold_mangaseg": 24680,
            "gold_zenodo_refined": 789,
            "silver_textseg_refined": 22425,
        },
    },
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--base-package", type=Path, default=DEFAULT_BASE_PACKAGE)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--overwrite", action="store_true")
    return parser.parse_args()


def atomic_text(path: Path, value: str) -> None:
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(value, encoding="utf-8", newline="\n")
    os.replace(temporary, path)


def prepare_output(output: Path, overwrite: bool) -> None:
    if output.exists():
        if not overwrite:
            raise FileExistsError(f"output already exists: {output}")
        if not (output / MARKER).is_file():
            raise ValueError(f"refusing to replace unmarked directory: {output}")
        shutil.rmtree(output)
    (output / "annotations").mkdir(parents=True)
    (output / "review").mkdir()
    atomic_text(output / MARKER, "generated Manga109 Segmentation package\n")


def link_or_copy(source: Path, target: Path) -> None:
    try:
        os.link(source, target)
    except OSError:
        shutil.copy2(source, target)


def sanitize(value: Any) -> Any:
    if isinstance(value, dict):
        return {
            key: sanitize(item)
            for key, item in value.items()
            if key not in {"source_path", "teacher_path", "audit_masks"}
        }
    if isinstance(value, list):
        return [sanitize(item) for item in value]
    if isinstance(value, str) and WINDOWS_ABSOLUTE_PATH.match(value):
        raise ValueError(f"unexpected absolute path in audit metadata: {value}")
    return value


def sanitize_audit(source: Path, target: Path) -> dict[str, int]:
    records = 0
    excluded = 0
    covers_excluded = 0
    safe_empty_pages = 0
    review_flags = 0
    with source.open(encoding="utf-8") as input_file, target.open(
        "w", encoding="utf-8", newline="\n"
    ) as output_file:
        for line_number, line in enumerate(input_file, start=1):
            record = sanitize(json.loads(line))
            records += 1
            is_excluded = bool(record.get("excluded_from_dataset"))
            excluded += int(is_excluded)
            covers_excluded += int(is_excluded and record.get("image", "").endswith("/000.jpg"))
            safe_empty_pages += int(
                not is_excluded
                and record.get("existing_typography_instances", 0) == 0
                and record.get("pp_pseudo_instances", 0) == 0
            )
            review_flags += len(record.get("review_flags", []))
            output_file.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
    return {
        "records": records,
        "excluded": excluded,
        "excluded_000_covers": covers_excluded,
        "retained_empty_typography_pages": safe_empty_pages,
        "review_flags": review_flags,
    }


def build_metadata(audit_stats: dict[str, dict[str, int]]) -> dict[str, Any]:
    return {
        "schema_version": 2,
        "name": "manga109-segmentation",
        "version": "2.0.0",
        "created_at": datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "annotation_only": True,
        "contains_manga109_images": False,
        "sources": {
            "previous_release": {
                "repo": "mayocream/manga109-segmentation",
                "version": "1.1.0",
                "revision": "23f4a56758fd5f8a48a3b1ff84338e290e77174e",
            },
            "zenodo_text_masks": {
                "title": "Mask Dataset for: Unconstrained Text Detection in Manga: a New Dataset and Baseline",
                "doi": "10.5281/zenodo.4511796",
                "version": "1.0",
                "license": "CC-BY-4.0",
                "pages_used": 449,
            },
            "textseg": {
                "repo": "mayocream/koharu-text-sam-ts-l",
                "revision": "5dd97423e0fbf2404264979136d47e8101144046",
                "weights_sha256": "bcd9525291677f467f0603509a0ca3df35711b4e3417cefce8da6bfc97164f45",
                "pages_used": 10153,
            },
            "pp_doclayoutv3": {
                "repo": "PaddlePaddle/PP-DocLayoutV3_safetensors",
                "revision": "f6f3f2b438702c53e94cfd535c2ea05aafb7985f",
                "role": "train-only bbox proposals; never a mask source",
            },
            "images": "Manga109 images are referenced by relative path and are not included",
        },
        "policy": {
            "human_geometry": "authoritative text boxes and COO polygons",
            "good_existing_masks": "union with clipped TextSeg ink",
            "geometry_fallbacks": "replace with clipped TextSeg ink when supported",
            "zenodo": "replace with clipped manually painted ink when supported",
            "pp_doclayoutv3": "bbox proposal only; mask is TextSeg ink clipped to the proposal; output bbox is tight mask geometry",
            "pp_positive_score": 0.7,
            "pp_max_gold_box_coverage": 0.05,
            "page_filter": {
                "empty_target_teacher_pixels": 512,
                "low_coverage_teacher_pixels": 10000,
                "low_coverage_recall": 0.2,
                "purpose": "remove incomplete pages that would become false-negative COCO background",
            },
            "negative_supervision": "standard COCO implicit background only; no synthetic negative mask class",
            "rfdetr_compatibility": "all annotations have iscrowd=0; no custom dense head or ignore-region encoding",
        },
        "totals": {
            "candidate_images": 10602,
            "images": 10098,
            "excluded_images": 504,
            "excluded_000_covers": 102,
            "annotations": 454606,
            "relations": 355817,
            "category_counts": {"1": 162311, "2": 58948, "3": 129707, "4": 103640},
            "quality_counts": {
                "gold_mangaseg": 233347,
                "gold_zenodo_refined": 8335,
                "silver_pp_bbox_textseg_mask": 3372,
                "silver_textseg_refined": 209552,
            },
            "retained_empty_typography_pages": 7,
        },
        "splits": {
            split: {**SPLIT_STATS[split], "review": audit_stats[split]}
            for split in SPLITS
        },
    }


def main() -> None:
    args = parse_args()
    source = args.source.resolve()
    base_package = args.base_package.resolve()
    output = args.output.resolve()
    if not (source / ".manga109-textseg-refinement").is_file():
        raise ValueError(f"not a generated TextSeg refinement dataset: {source}")
    if not (base_package / MARKER).is_file():
        raise ValueError(f"not the annotation-only base package: {base_package}")
    prepare_output(output, args.overwrite)

    for filename in (".gitattributes", ".hfignore"):
        shutil.copy2(base_package / filename, output / filename)
    for split in SPLITS:
        link_or_copy(
            source / "annotations" / f"{split}.coco.json",
            output / "annotations" / f"{split}.coco.json",
        )

    audit_stats = {
        split: sanitize_audit(
            source / "audit" / f"{split}.jsonl",
            output / "review" / f"{split}.jsonl",
        )
        for split in SPLITS
    }
    expected_audit = {
        "train": {"records": 8475, "excluded": 347, "excluded_000_covers": 81, "retained_empty_typography_pages": 5},
        "validation": {"records": 1057, "excluded": 56, "excluded_000_covers": 10, "retained_empty_typography_pages": 2},
        "test": {"records": 1070, "excluded": 101, "excluded_000_covers": 11, "retained_empty_typography_pages": 0},
    }
    for split, expected in expected_audit.items():
        for key, expected_value in expected.items():
            actual = audit_stats[split][key]
            if actual != expected_value:
                raise ValueError(f"unexpected {split} {key}: expected {expected_value}, got {actual}")

    atomic_text(output / "README.md", README)
    atomic_text(
        output / "build.json",
        json.dumps(build_metadata(audit_stats), ensure_ascii=False, indent=2) + "\n",
    )
    print(json.dumps({"output": str(output), "audit": audit_stats}, indent=2))


if __name__ == "__main__":
    main()
