#!/usr/bin/env python3
"""Render RF-DETR segmentation predictions beside held-out COCO ground truth."""

from __future__ import annotations

import argparse
import json
import random
from collections import Counter
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageOps
from pycocotools import mask as mask_utils
from rfdetr import RFDETRSeg2XLarge


COLORS = {
    0: (255, 55, 75),
    1: (255, 50, 210),
    2: (40, 145, 255),
    3: (55, 205, 100),
}
ALPHAS = {0: 0.28, 1: 0.30, 2: 0.16, 3: 0.08}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("dataset", type=Path)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--count", type=int, default=8)
    parser.add_argument("--seed", type=int, default=20260724)
    parser.add_argument("--split", choices=("train", "valid", "test"), default="valid")
    parser.add_argument("--selection-manifest", type=Path)
    parser.add_argument("--threshold", type=float, default=0.25)
    parser.add_argument("--resolution", type=int, default=1152)
    parser.add_argument("--batch-size", type=int, default=2)
    parser.add_argument("--max-detections", type=int, default=160)
    return parser.parse_args()


def load_font(size: int) -> ImageFont.ImageFont | ImageFont.FreeTypeFont:
    for path in (
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "C:/Windows/Fonts/arialbd.ttf",
    ):
        try:
            return ImageFont.truetype(path, size=size)
        except OSError:
            pass
    return ImageFont.load_default()


def decode_segmentation(segmentation: Any, height: int, width: int) -> np.ndarray | None:
    if isinstance(segmentation, dict):
        decoded = mask_utils.decode(segmentation)
        if decoded.ndim == 3:
            decoded = decoded.any(axis=2)
        return decoded.astype(bool)
    if isinstance(segmentation, list):
        rles = mask_utils.frPyObjects(segmentation, height, width)
        decoded = mask_utils.decode(mask_utils.merge(rles))
        return decoded.astype(bool)
    return None


def render_overlay(
    image: Image.Image,
    instances: list[dict[str, Any]],
    names: dict[int, str],
    title: str,
) -> Image.Image:
    pixels = np.asarray(image.convert("RGB")).copy()
    height, width = pixels.shape[:2]
    for instance in sorted(instances, key=lambda item: item.get("area", 0), reverse=True):
        class_id = int(instance["class_id"])
        mask = instance.get("mask")
        if mask is not None and mask.shape == (height, width):
            color = np.asarray(COLORS[class_id], dtype=np.float32)
            alpha = ALPHAS[class_id]
            pixels[mask] = pixels[mask] * (1.0 - alpha) + color * alpha

    rendered = Image.fromarray(np.clip(pixels, 0, 255).astype(np.uint8))
    draw = ImageDraw.Draw(rendered)
    line_width = max(2, round(min(width, height) / 700))
    label_font = load_font(max(12, round(min(width, height) / 85)))
    for instance in instances:
        class_id = int(instance["class_id"])
        x1, y1, x2, y2 = instance["xyxy"]
        color = COLORS[class_id]
        draw.rectangle((x1, y1, x2, y2), outline=color, width=line_width)
        label = names[class_id]
        score = instance.get("score")
        if score is not None:
            label = f"{label} {score:.2f}"
        box = draw.textbbox((x1, y1), label, font=label_font, stroke_width=1)
        label_height = box[3] - box[1] + 4
        label_y = max(0, y1 - label_height)
        label_width = box[2] - box[0] + 6
        draw.rectangle((x1, label_y, x1 + label_width, label_y + label_height), fill=color)
        draw.text((x1 + 3, label_y + 2), label, fill="white", font=label_font, stroke_width=1, stroke_fill="black")

    title_font = load_font(max(18, round(min(width, height) / 55)))
    title_box = draw.textbbox((0, 0), title, font=title_font, stroke_width=1)
    draw.rectangle((0, 0, title_box[2] + 12, title_box[3] + 10), fill=(0, 0, 0))
    draw.text((6, 4), title, fill="white", font=title_font, stroke_width=1, stroke_fill="black")
    return rendered


def make_card(gt: Image.Image, prediction: Image.Image, caption: str, width: int = 900, height: int = 700) -> Image.Image:
    header = 36
    half_width = width // 2
    panel_height = height - header
    card = Image.new("RGB", (width, height), "white")
    for index, source in enumerate((gt, prediction)):
        fitted = ImageOps.contain(source, (half_width, panel_height), Image.Resampling.LANCZOS)
        x = index * half_width + (half_width - fitted.width) // 2
        y = header + (panel_height - fitted.height) // 2
        card.paste(fitted, (x, y))
    draw = ImageDraw.Draw(card)
    font = load_font(18)
    draw.text((8, 7), caption, fill="black", font=font)
    draw.line((half_width, header, half_width, height), fill=(80, 80, 80), width=2)
    return card


def main() -> None:
    args = parse_args()
    split_dir = args.dataset.resolve() / args.split
    annotation_path = split_dir / "_annotations.coco.json"
    document = json.loads(annotation_path.read_text(encoding="utf-8"))
    images = sorted(document["images"], key=lambda item: item["file_name"])
    if args.selection_manifest is not None:
        manifest = json.loads(args.selection_manifest.resolve().read_text(encoding="utf-8"))
        allowed = {str(item["file_name"]) for item in manifest["images"]}
        images = [image for image in images if str(image["file_name"]) in allowed]
        if len(images) != len(allowed):
            raise ValueError(
                f"selection manifest contains {len(allowed)} images but {len(images)} were found in {args.split}"
            )
    selected = sorted(
        random.Random(args.seed).sample(images, min(args.count, len(images))),
        key=lambda item: item["file_name"],
    )
    selected_ids = {int(item["id"]) for item in selected}
    annotations: dict[int, list[dict[str, Any]]] = {image_id: [] for image_id in selected_ids}
    for annotation in document["annotations"]:
        image_id = int(annotation["image_id"])
        if image_id in annotations:
            annotations[image_id].append(annotation)

    categories = sorted(document["categories"], key=lambda item: int(item["id"]))
    category_to_class = {int(item["id"]): index for index, item in enumerate(categories)}
    names = {index: str(item["name"]) for index, item in enumerate(categories)}
    names = {class_id: ("COO" if name == "onomatopoeia" else name) for class_id, name in names.items()}
    args.output.mkdir(parents=True, exist_ok=True)
    samples_dir = args.output / "samples"
    samples_dir.mkdir(exist_ok=True)

    model = RFDETRSeg2XLarge(
        pretrain_weights=str(args.checkpoint.resolve()),
        resolution=args.resolution,
        num_select=args.max_detections,
    )

    cards: list[Image.Image] = []
    prediction_counts: Counter[int] = Counter()
    records: list[dict[str, Any]] = []
    for start in range(0, len(selected), args.batch_size):
        batch = selected[start : start + args.batch_size]
        source_images = [Image.open(split_dir / item["file_name"]).convert("RGB") for item in batch]
        detections_batch = model.predict(
            source_images,
            threshold=args.threshold,
            shape=(args.resolution, args.resolution),
            include_source_image=False,
        )
        if not isinstance(detections_batch, list):
            detections_batch = [detections_batch]

        for image_info, source_image, detections in zip(batch, source_images, detections_batch, strict=True):
            order = np.argsort(-detections.confidence)[: args.max_detections]
            detections = detections[order]
            gt_instances: list[dict[str, Any]] = []
            for annotation in annotations[int(image_info["id"])]:
                x, y, width, height = annotation["bbox"]
                gt_instances.append(
                    {
                        "class_id": category_to_class[int(annotation["category_id"])],
                        "xyxy": (x, y, x + width, y + height),
                        "mask": decode_segmentation(
                            annotation.get("segmentation"),
                            int(image_info["height"]),
                            int(image_info["width"]),
                        ),
                        "area": float(annotation.get("area", width * height)),
                    }
                )

            prediction_instances: list[dict[str, Any]] = []
            for index in range(len(detections)):
                class_id = int(detections.class_id[index])
                mask = detections.mask[index].astype(bool) if detections.mask is not None else None
                box = detections.xyxy[index].tolist()
                prediction_counts[class_id] += 1
                prediction_instances.append(
                    {
                        "class_id": class_id,
                        "xyxy": box,
                        "mask": mask,
                        "score": float(detections.confidence[index]),
                        "area": float(max(0, box[2] - box[0]) * max(0, box[3] - box[1])),
                    }
                )

            file_name = str(image_info["file_name"])
            stem = file_name.replace("/", "__").replace("\\", "__")
            gt_render = render_overlay(source_image, gt_instances, names, "Ground truth")
            prediction_render = render_overlay(source_image, prediction_instances, names, "Prediction")
            card = make_card(gt_render, prediction_render, file_name)
            card.save(samples_dir / f"{stem}.jpg", quality=90, subsampling=0)
            cards.append(card)
            records.append(
                {
                    "image_id": int(image_info["id"]),
                    "file_name": file_name,
                    "ground_truth": len(gt_instances),
                    "predictions": len(prediction_instances),
                }
            )

    columns = 2
    rows = (len(cards) + columns - 1) // columns
    sheet = Image.new("RGB", (columns * 900, rows * 700), (225, 225, 225))
    for index, card in enumerate(cards):
        sheet.paste(card, ((index % columns) * 900, (index // columns) * 700))
    sheet.save(args.output / "contact_sheet.jpg", quality=90, subsampling=0)

    payload = {
        "checkpoint": str(args.checkpoint.resolve()),
        "dataset": str(args.dataset.resolve()),
        "split": args.split,
        "selection_manifest": str(args.selection_manifest.resolve()) if args.selection_manifest else None,
        "seed": args.seed,
        "threshold": args.threshold,
        "resolution": args.resolution,
        "pages": records,
        "prediction_counts": {names[key]: value for key, value in sorted(prediction_counts.items())},
    }
    (args.output / "render_summary.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(payload, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
