# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "ijson>=3.4",
#   "pillow>=11",
#   "tqdm>=4.67",
# ]
# ///
"""Render a stratified visual audit of PP-DocLayoutV3 teacher regions."""

from __future__ import annotations

import argparse
import json
import random
from collections import defaultdict
from pathlib import Path
from typing import Any

import ijson
from PIL import Image, ImageDraw, ImageFont
from tqdm import tqdm

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASE = ROOT / "data" / "manga109-segmentation-rfdetr" / "train"
DEFAULT_TEACHERS = ROOT / "data" / "manga109-segmentation-pp-doclayoutv3"
DEFAULT_OUTPUT = ROOT / "runs" / "manga109-ppteacher-audit-100"
POSITIVE_SCORE = 0.70
IGNORE_SCORE = 0.45
SECONDARY_LABELS = {
    "figure_title",
    "footer",
    "header",
    "paragraph_title",
    "vision_footnote",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, default=DEFAULT_BASE)
    parser.add_argument("--teachers", type=Path, default=DEFAULT_TEACHERS)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--per-stratum", type=int, default=20)
    parser.add_argument("--seed", type=int, default=20260724)
    parser.add_argument("--page-max-size", type=int, default=1400)
    parser.add_argument("--sheet-columns", type=int, default=4)
    parser.add_argument("--sheet-tile-width", type=int, default=380)
    parser.add_argument("--sheet-tile-height", type=int, default=500)
    return parser.parse_args()


def polygon_area(points: list[list[float]]) -> float:
    return abs(
        sum(
            points[index][0] * points[(index + 1) % len(points)][1]
            - points[index][1] * points[(index + 1) % len(points)][0]
            for index in range(len(points))
        )
        * 0.5
    )


def teacher_statistics(path: Path) -> dict[str, Any]:
    record = json.loads(path.read_text(encoding="utf-8"))
    positives = [
        prediction
        for prediction in record["predictions"]
        if float(prediction["score"]) >= POSITIVE_SCORE
    ]
    ignores = [
        prediction
        for prediction in record["predictions"]
        if IGNORE_SCORE <= float(prediction["score"]) < POSITIVE_SCORE
    ]
    page_area = max(1, int(record["width"]) * int(record["height"]))
    largest = max(
        (polygon_area(prediction["polygon"]) / page_area for prediction in positives),
        default=0.0,
    )
    return {
        "path": path,
        "image": str(record["image"]),
        "positive": len(positives),
        "ignore": len(ignores),
        "doc_title": max(
            (
                float(prediction["score"])
                for prediction in positives
                if prediction["label"] == "doc_title"
            ),
            default=0.0,
        ),
        "secondary": sum(
            prediction["label"] in SECONDARY_LABELS for prediction in positives
        ),
        "largest": largest,
    }


def select_strata(
    statistics: list[dict[str, Any]], per_stratum: int, seed: int
) -> list[dict[str, Any]]:
    rng = random.Random(seed)
    shuffled = statistics.copy()
    rng.shuffle(shuffled)
    rankings = {
        "document-title": sorted(
            (item for item in shuffled if item["doc_title"] > 0),
            key=lambda item: item["doc_title"],
            reverse=True,
        ),
        "header-footer": sorted(
            (item for item in shuffled if item["secondary"] > 0),
            key=lambda item: item["secondary"],
            reverse=True,
        ),
        "large-region": sorted(
            shuffled, key=lambda item: item["largest"], reverse=True
        ),
        "dense-page": sorted(shuffled, key=lambda item: item["positive"], reverse=True),
        "ambiguous": sorted(shuffled, key=lambda item: item["ignore"], reverse=True),
    }
    selected: list[dict[str, Any]] = []
    used: set[str] = set()
    for stratum, candidates in rankings.items():
        count = 0
        for candidate in candidates:
            if candidate["image"] in used:
                continue
            selected.append({**candidate, "stratum": stratum})
            used.add(candidate["image"])
            count += 1
            if count == per_stratum:
                break
        if count != per_stratum:
            raise RuntimeError(f"not enough unique candidates for {stratum}: {count}")
    return selected


def read_gold_annotations(
    annotation: Path, selected_names: set[str]
) -> dict[str, list[dict[str, Any]]]:
    image_by_id: dict[int, str] = {}
    with annotation.open("rb") as file:
        for image in ijson.items(file, "images.item", use_float=True):
            name = str(image["file_name"]).replace("\\", "/")
            if name in selected_names:
                image_by_id[int(image["id"])] = name
    annotations: dict[str, list[dict[str, Any]]] = defaultdict(list)
    with annotation.open("rb") as file:
        for annotation_item in ijson.items(file, "annotations.item", use_float=True):
            name = image_by_id.get(int(annotation_item["image_id"]))
            if name is not None and int(annotation_item["category_id"]) in (1, 2):
                annotations[name].append(annotation_item)
    return annotations


def annotation_polygons(annotation: dict[str, Any]) -> list[list[tuple[float, float]]]:
    segmentation = annotation.get("segmentation")
    if not isinstance(segmentation, list):
        return []
    polygons: list[list[tuple[float, float]]] = []
    for raw in segmentation:
        if not isinstance(raw, list) or len(raw) < 6 or len(raw) % 2:
            continue
        polygons.append(
            [
                (float(raw[index]), float(raw[index + 1]))
                for index in range(0, len(raw), 2)
            ]
        )
    return polygons


def render_page(
    image_path: Path,
    teacher_record: dict[str, Any],
    gold: list[dict[str, Any]],
    maximum_size: int,
) -> Image.Image:
    with Image.open(image_path) as source:
        source.load()
        image = source.convert("RGBA")
    overlay = Image.new("RGBA", image.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(overlay)
    width = max(3, round(max(image.size) / 550))

    for prediction in teacher_record["predictions"]:
        score = float(prediction["score"])
        if score < IGNORE_SCORE:
            continue
        points = [(float(x), float(y)) for x, y in prediction["polygon"]]
        if score >= POSITIVE_SCORE:
            fill, outline = (45, 255, 70, 48), (40, 255, 70, 255)
        else:
            fill, outline = (255, 150, 25, 42), (255, 145, 20, 255)
        draw.polygon(points, fill=fill)
        draw.line(points + [points[0]], fill=outline, width=width, joint="curve")

    for annotation in gold:
        color = (
            (20, 210, 255, 255)
            if int(annotation["category_id"]) == 1
            else (255, 35, 210, 255)
        )
        polygons = annotation_polygons(annotation)
        if polygons:
            for points in polygons:
                draw.line(points + [points[0]], fill=color, width=width, joint="curve")
        else:
            x, y, box_width, box_height = map(float, annotation["bbox"])
            draw.rectangle(
                (x, y, x + box_width, y + box_height), outline=color, width=width
            )

    rendered = Image.alpha_composite(image, overlay).convert("RGB")
    rendered.thumbnail((maximum_size, maximum_size), Image.Resampling.LANCZOS)
    return rendered


def make_contact_sheets(
    rendered: list[tuple[dict[str, Any], Path]],
    output: Path,
    columns: int,
    tile_width: int,
    tile_height: int,
) -> None:
    font = ImageFont.load_default(size=16)
    rows = 4
    per_sheet = columns * rows
    for sheet_index in range(0, len(rendered), per_sheet):
        entries = rendered[sheet_index : sheet_index + per_sheet]
        sheet = Image.new(
            "RGB", (columns * tile_width, rows * tile_height), (245, 245, 245)
        )
        draw = ImageDraw.Draw(sheet)
        for index, (selection, path) in enumerate(entries):
            column, row = index % columns, index // columns
            x, y = column * tile_width, row * tile_height
            with Image.open(path) as source:
                page = source.convert("RGB")
            page.thumbnail(
                (tile_width - 16, tile_height - 58), Image.Resampling.LANCZOS
            )
            page_x = x + (tile_width - page.width) // 2
            page_y = y + 46 + (tile_height - 54 - page.height) // 2
            sheet.paste(page, (page_x, page_y))
            caption = (
                f"{selection['stratum']} | {selection['image']}\n"
                f"positive={selection['positive']} ignore={selection['ignore']}"
            )
            draw.multiline_text(
                (x + 7, y + 6), caption, fill=(10, 10, 10), font=font, spacing=2
            )
            draw.rectangle(
                (x, y, x + tile_width - 1, y + tile_height - 1),
                outline=(170, 170, 170),
            )
        sheet.save(
            output / f"contact-sheet-{sheet_index // per_sheet + 1:02d}.jpg", quality=92
        )


def main() -> None:
    args = parse_args()
    if args.per_stratum <= 0 or args.page_max_size <= 0:
        raise ValueError("sample counts and page size must be positive")
    base = args.base.resolve()
    teachers = args.teachers.resolve()
    output = args.output.resolve()
    records_root = teachers / "records"
    records = sorted(records_root.rglob("*.json"))
    statistics = [
        teacher_statistics(path)
        for path in tqdm(records, desc="rank teacher pages", unit="page")
    ]
    selected = select_strata(statistics, args.per_stratum, args.seed)
    gold = read_gold_annotations(
        base / "_annotations.coco.json", {item["image"] for item in selected}
    )

    samples = output / "samples"
    samples.mkdir(parents=True, exist_ok=True)
    rendered: list[tuple[dict[str, Any], Path]] = []
    for index, selection in enumerate(tqdm(selected, desc="render audit", unit="page")):
        record = json.loads(selection["path"].read_text(encoding="utf-8"))
        image_name = Path(selection["image"])
        result = render_page(
            base / image_name,
            record,
            gold[selection["image"]],
            args.page_max_size,
        )
        destination = samples / (
            f"{index + 1:03d}-{selection['stratum']}-{image_name.parent.name}-{image_name.stem}.jpg"
        )
        result.save(destination, quality=94)
        rendered.append((selection, destination))

    make_contact_sheets(
        rendered,
        output,
        args.sheet_columns,
        args.sheet_tile_width,
        args.sheet_tile_height,
    )
    manifest = {
        "schema_version": 1,
        "legend": {
            "green": "PP teacher weak positive (score >= 0.70)",
            "orange": "PP teacher ignore region (0.45 <= score < 0.70)",
            "cyan": "gold text",
            "magenta": "gold onomatopoeia",
        },
        "seed": args.seed,
        "per_stratum": args.per_stratum,
        "samples": [
            {key: value for key, value in selection.items() if key != "path"}
            for selection in selected
        ],
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps({"output": str(output), "samples": len(selected)}, indent=2))


if __name__ == "__main__":
    main()
