#!/usr/bin/env python3
"""Render a stratified visual audit of the Manga109 TextSeg refinement build."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path
from typing import Any

import ijson
import numpy as np
from PIL import Image, ImageDraw, ImageFont

from build_manga109_segmentation import decode_rle


ROOT = Path(__file__).resolve().parents[1]
SPLITS = ("train", "validation", "test")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dataset",
        type=Path,
        default=ROOT / "data" / "manga109-segmentation-textseg",
    )
    parser.add_argument(
        "--base-dataset",
        type=Path,
        default=ROOT / "data" / "manga109-segmentation",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "runs" / "manga109-textseg-refinement-audit",
    )
    parser.add_argument("--per-stratum", type=int, default=6)
    parser.add_argument("--panel-width", type=int, default=360)
    parser.add_argument("--panel-height", type=int, default=270)
    return parser.parse_args()


def load_records(root: Path) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for split in SPLITS:
        with (root / "audit" / f"{split}.jsonl").open(encoding="utf-8") as stream:
            for line in stream:
                record = json.loads(line)
                record["split"] = split
                record["book"] = str(record["image"]).split("/", 1)[0]
                records.append(record)
    return records


def unique_books(
    candidates: list[dict[str, Any]], count: int, used_images: set[str] | None = None
) -> list[dict[str, Any]]:
    selected: list[dict[str, Any]] = []
    books: set[str] = set()
    used = used_images or set()
    for record in candidates:
        if record["image"] in used or record["book"] in books:
            continue
        selected.append(record)
        books.add(record["book"])
        if len(selected) == count:
            break
    if len(selected) != count:
        raise RuntimeError(f"could select only {len(selected)} of {count} unique books")
    return selected


def select_strata(records: list[dict[str, Any]], count: int) -> dict[str, list[dict[str, Any]]]:
    eligible = [
        record
        for record in records
        if not record.get("excluded_from_dataset", False)
        and record["existing_typography_instances"] > 0
        and record["old_vs_new"]["prediction_pixels"] > 0
    ]
    typical = sorted(
        eligible,
        key=lambda record: abs(record["old_vs_new"]["iou"] - 0.84)
        + abs(record["added_pixels"] - 6700) / 100_000,
    )
    large_add = sorted(eligible, key=lambda record: record["added_pixels"], reverse=True)
    large_remove = sorted(
        (record for record in eligible if record["removed_pixels"] > 0),
        key=lambda record: record["removed_pixels"],
        reverse=True,
    )
    pp_recovery = sorted(
        (
            record
            for record in records
            if not record.get("excluded_from_dataset", False)
            and record["existing_typography_instances"] == 0
            and record["pp_pseudo_instances"] > 0
        ),
        key=lambda record: (record["pp_pseudo_instances"], record["added_pixels"]),
        reverse=True,
    )
    covers = sorted(
        (
            record
            for record in records
            if record["image"].endswith("/000.jpg")
            and record.get("excluded_from_dataset", False)
        ),
        key=lambda record: record["teacher_vs_new"]["reference_pixels"],
        reverse=True,
    )
    zenodo = sorted(
        (record for record in eligible if record["teacher_source"] == "zenodo"),
        key=lambda record: record["removed_pixels"],
        reverse=True,
    )
    excluded_low_coverage = sorted(
        (
            record
            for record in records
            if record.get("excluded_from_dataset", False)
            and "low_teacher_coverage" in record.get("exclusion_reasons", [])
        ),
        key=lambda record: record["teacher_vs_new"]["iou"],
    )
    return {
        "typical": unique_books(typical, count),
        "largest-additions": unique_books(large_add, count),
        "largest-removals": unique_books(large_remove, count),
        "pp-recovery-no-human-text": unique_books(pp_recovery, count),
        "covers-excluded": unique_books(covers, count),
        "zenodo-corrections": unique_books(zenodo, count),
        "excluded-low-coverage": unique_books(excluded_low_coverage, count),
    }


def selected_unions(
    annotation_path: Path,
    selected_names: set[str],
) -> dict[str, np.ndarray]:
    image_by_id: dict[int, tuple[str, int, int]] = {}
    with annotation_path.open("rb") as stream:
        for image in ijson.items(stream, "images.item", use_float=True):
            name = str(image["file_name"]).replace("\\", "/")
            if name in selected_names:
                image_by_id[int(image["id"])] = (
                    name,
                    int(image["height"]),
                    int(image["width"]),
                )
    unions = {
        name: np.zeros((height, width), dtype=bool)
        for name, height, width in image_by_id.values()
    }
    with annotation_path.open("rb") as stream:
        for annotation in ijson.items(stream, "annotations.item", use_float=True):
            image = image_by_id.get(int(annotation["image_id"]))
            if image is None or int(annotation["category_id"]) not in (1, 2):
                continue
            unions[image[0]] |= decode_rle(annotation["segmentation"])
    return unions


def load_all_unions(
    root: Path, selected_by_split: dict[str, set[str]]
) -> dict[str, np.ndarray]:
    result: dict[str, np.ndarray] = {}
    for split, names in selected_by_split.items():
        if names:
            result.update(selected_unions(root / "annotations" / f"{split}.coco.json", names))
    return result


def overlay_mask(image: Image.Image, mask: np.ndarray, color: tuple[int, int, int]) -> Image.Image:
    base = image.convert("RGBA")
    layer = np.zeros((mask.shape[0], mask.shape[1], 4), dtype=np.uint8)
    layer[mask, :3] = color
    layer[mask, 3] = 150
    return Image.alpha_composite(base, Image.fromarray(layer, mode="RGBA")).convert("RGB")


def panel(
    image: Image.Image,
    label: str,
    width: int,
    height: int,
    pp_boxes: list[dict[str, Any]] | None = None,
) -> Image.Image:
    rendered = image.copy()
    if pp_boxes:
        draw = ImageDraw.Draw(rendered)
        stroke = max(3, round(max(rendered.size) / 450))
        for record in pp_boxes:
            x, y, box_width, box_height = map(int, record["proposal_bbox"])
            draw.rectangle((x, y, x + box_width, y + box_height), outline=(255, 0, 220), width=stroke)
    rendered.thumbnail((width, height), Image.Resampling.LANCZOS)
    output = Image.new("RGB", (width, height + 28), "white")
    output.paste(rendered, ((width - rendered.width) // 2, 28 + (height - rendered.height) // 2))
    ImageDraw.Draw(output).text((8, 7), label, fill="black", font=ImageFont.load_default())
    return output


def render_sheet(
    stratum: str,
    records: list[dict[str, Any]],
    old_unions: dict[str, np.ndarray],
    new_unions: dict[str, np.ndarray],
    output: Path,
    panel_width: int,
    panel_height: int,
) -> None:
    header_height = 48
    metadata_height = 42
    row_height = panel_height + 28 + metadata_height
    sheet = Image.new(
        "RGB", (panel_width * 4, header_height + row_height * len(records)), "#eeeeee"
    )
    draw = ImageDraw.Draw(sheet)
    draw.text((12, 14), stratum, fill="black", font=ImageFont.load_default())
    for row, record in enumerate(records):
        name = record["image"]
        with Image.open(record["source_path"]) as stream:
            source = stream.convert("RGB")
        with Image.open(record["teacher_path"]) as stream:
            teacher = np.asarray(stream.convert("L")) > 127
        new_mask = new_unions.get(name)
        if new_mask is None:
            new_mask = np.zeros_like(teacher)
        final_label = (
            "EXCLUDED FROM DATASET"
            if record.get("excluded_from_dataset", False)
            else "NEW MASK (GREEN) / PP BOX (MAGENTA)"
        )
        variants = (
            panel(source, "SOURCE", panel_width, panel_height),
            panel(overlay_mask(source, old_unions[name], (255, 40, 20)), "OLD MASK (RED)", panel_width, panel_height),
            panel(overlay_mask(source, teacher, (0, 210, 255)), "TEACHER (CYAN)", panel_width, panel_height),
            panel(
                overlay_mask(source, new_mask, (20, 255, 70)),
                final_label,
                panel_width,
                panel_height,
                record["pp_boxes"],
            ),
        )
        y = header_height + row * row_height
        for column, variant in enumerate(variants):
            sheet.paste(variant, (column * panel_width, y))
        metrics = (
            f"{name} [{record['split']}/{record['teacher_source']}]  "
            f"old-new IoU={record['old_vs_new']['iou']:.3f}  "
            f"teacher-new IoU={record['teacher_vs_new']['iou']:.3f}  "
            f"+{record['added_pixels']:,} -{record['removed_pixels']:,} px  "
            f"human={record['existing_typography_instances']} PP={record['pp_pseudo_instances']}"
            f"  excluded={record.get('excluded_from_dataset', False)}"
        )
        draw.text((10, y + panel_height + 34), metrics, fill="black", font=ImageFont.load_default())
    destination = output / f"{stratum}.jpg"
    destination.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(destination, quality=94, subsampling=0)


def main() -> None:
    args = parse_args()
    records = load_records(args.dataset)
    strata = select_strata(records, args.per_stratum)
    selected = [record for values in strata.values() for record in values]
    selected_by_split: dict[str, set[str]] = defaultdict(set)
    for record in selected:
        selected_by_split[record["split"]].add(record["image"])
    old_unions = load_all_unions(args.base_dataset, selected_by_split)
    new_unions = load_all_unions(args.dataset, selected_by_split)
    args.output.mkdir(parents=True, exist_ok=True)
    for stratum, values in strata.items():
        render_sheet(
            stratum,
            values,
            old_unions,
            new_unions,
            args.output,
            args.panel_width,
            args.panel_height,
        )
    selection = {
        stratum: [
            {
                "image": record["image"],
                "split": record["split"],
                "teacher_source": record["teacher_source"],
                "old_vs_new_iou": record["old_vs_new"]["iou"],
                "teacher_vs_new_iou": record["teacher_vs_new"]["iou"],
                "added_pixels": record["added_pixels"],
                "removed_pixels": record["removed_pixels"],
                "human_instances": record["existing_typography_instances"],
                "pp_instances": record["pp_pseudo_instances"],
                "excluded_from_dataset": record.get("excluded_from_dataset", False),
                "exclusion_reasons": record.get("exclusion_reasons", []),
            }
            for record in values
        ]
        for stratum, values in strata.items()
    }
    (args.output / "selection.json").write_text(
        json.dumps(selection, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps({"output": str(args.output), "strata": len(strata), "pages": len(selected)}))


if __name__ == "__main__":
    main()
