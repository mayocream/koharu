#!/usr/bin/env python3
"""Evaluate the trained RF-DETR typography head on the two domain folders.

The checkpoint exposes a high-recall region branch and a tighter glyph-ink
branch.  This script records both, then uses a deliberately conservative
ink>=0.99 union with the ordinary text/onomatopoeia instance masks for the
pre-inpainting output.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import time
from pathlib import Path
from typing import Any

import numpy as np
import torch
import torch.nn.functional as F
from PIL import Image, ImageDraw, ImageOps

from compare_textseg_rfdetr_masks import load_model, save_binary
from evaluate_preinpainting_masks import postprocess, save_white_painted
from rfdetr_typography_distillation import DenseTypographyHead


ROOT = Path(__file__).resolve().parents[1]
DATASETS = ("bluearchive_comics", "marriagetoxin-chapter-1")
TEXT_CLASS = 0
ONOMATOPOEIA_CLASS = 1
TEXT_THRESHOLD = 0.25
ONOMATOPOEIA_THRESHOLD = 0.40
REGION_THRESHOLD = 0.50
INK_THRESHOLD = 0.99
DILATION_RADIUS = 4


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--textseg-root",
        type=Path,
        default=ROOT / "runs" / "koharu-text-sam-ts-l-domain-evaluation",
    )
    parser.add_argument(
        "--model-root",
        type=Path,
        default=ROOT / "runs" / "hf-koharu-layout-rfdetr-seg-2xl-1152",
    )
    parser.add_argument(
        "--dense-checkpoint",
        type=Path,
        default=(
            ROOT
            / "runs"
            / "rfdetr-seg-2xl-1152-direct-pseudo-h100x8-b2-stage3-final"
            / "checkpoint_best_total.pth"
        ),
    )
    parser.add_argument(
        "--baseline-root",
        type=Path,
        default=(
            ROOT
            / "runs"
            / "koharu-text-sam-ts-l-vs-layout-rfdetr-preinpainting"
        ),
    )
    parser.add_argument(
        "--output-root",
        type=Path,
        default=(
            ROOT
            / "runs"
            / "koharu-text-sam-ts-l-vs-layout-rfdetr-dense-head"
        ),
    )
    parser.add_argument("--batch-size", type=int, default=2)
    parser.add_argument("--review-pages", type=int, default=24)
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_dense_model(model_root: Path, checkpoint: Path) -> tuple[Any, Any, Any]:
    model = load_model(model_root)
    detector = model.model.model
    head = DenseTypographyHead(256, 1, 96, 4)
    payload = torch.load(checkpoint, map_location="cpu", weights_only=False)
    state = payload["model"]
    head_state = {
        key.removeprefix("typography_head."): value
        for key, value in state.items()
        if key.startswith("typography_head.")
    }
    if not head_state:
        raise RuntimeError(f"typography head not found in {checkpoint}")
    head.load_state_dict(head_state, strict=True)
    detector.typography_head = head
    captured: list[list[torch.Tensor]] = []

    def capture_features(_module: Any, _inputs: Any, output: Any) -> None:
        features = output[0] if isinstance(output, tuple) else output
        captured.append([feature.tensors for feature in features])

    handle = detector.backbone.register_forward_hook(capture_features)
    return model, captured, handle


def pair_counts(reference: np.ndarray, prediction: np.ndarray) -> dict[str, Any]:
    if reference.shape != prediction.shape:
        raise ValueError(f"shape mismatch: {reference.shape} != {prediction.shape}")
    intersection = int((reference & prediction).sum())
    reference_pixels = int(reference.sum())
    prediction_pixels = int(prediction.sum())
    union = reference_pixels + prediction_pixels - intersection
    return {
        "pixels": int(reference.size),
        "reference_pixels": reference_pixels,
        "prediction_pixels": prediction_pixels,
        "intersection_pixels": intersection,
        "union_pixels": union,
        "iou": float(intersection / union) if union else 1.0,
        "dice": float(2 * intersection / (reference_pixels + prediction_pixels))
        if reference_pixels + prediction_pixels
        else 1.0,
        "precision": float(intersection / prediction_pixels)
        if prediction_pixels
        else float(reference_pixels == 0),
        "recall": float(intersection / reference_pixels)
        if reference_pixels
        else float(prediction_pixels == 0),
    }


def aggregate_pairs(records: list[dict[str, Any]], path: tuple[str, ...]) -> dict[str, Any]:
    values = records
    pairs: list[dict[str, Any]] = []
    for record in values:
        value: Any = record
        for key in path:
            value = value[key]
        pairs.append(value)
    pixels = sum(pair["pixels"] for pair in pairs)
    reference = sum(pair["reference_pixels"] for pair in pairs)
    prediction = sum(pair["prediction_pixels"] for pair in pairs)
    intersection = sum(pair["intersection_pixels"] for pair in pairs)
    union = sum(pair["union_pixels"] for pair in pairs)
    return {
        "pages": len(pairs),
        "pixels": pixels,
        "reference_pixels": reference,
        "prediction_pixels": prediction,
        "reference_ratio": float(reference / pixels),
        "prediction_ratio": float(prediction / pixels),
        "intersection_pixels": intersection,
        "union_pixels": union,
        "micro_iou": float(intersection / union) if union else 1.0,
        "micro_dice": float(2 * intersection / (reference + prediction))
        if reference + prediction
        else 1.0,
        "precision": float(intersection / prediction) if prediction else float(reference == 0),
        "recall": float(intersection / reference) if reference else float(prediction == 0),
        "mean_page_iou": float(np.mean([pair["iou"] for pair in pairs])),
    }


def save_mask(mask: np.ndarray, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(mask.astype(np.uint8) * 255, mode="L").save(destination)


def fit(image: Image.Image, width: int, height: int) -> Image.Image:
    canvas = Image.new("RGB", (width, height), "white")
    contained = ImageOps.contain(
        image.convert("RGB"), (width, height), Image.Resampling.LANCZOS
    )
    canvas.paste(
        contained,
        ((width - contained.width) // 2, (height - contained.height) // 2),
    )
    return canvas


def region_overlay(source: Image.Image, region: Image.Image) -> Image.Image:
    base = np.asarray(source.convert("RGB")).copy()
    mask = np.asarray(region.convert("L")) > 127
    base[mask] = (0.35 * base[mask] + 0.65 * np.array([255, 35, 35])).astype(
        np.uint8
    )
    return Image.fromarray(base, mode="RGB")


def select_reviews(records: list[dict[str, Any]], count: int) -> list[dict[str, Any]]:
    chosen: dict[str, tuple[dict[str, Any], set[str]]] = {}

    def add(record: dict[str, Any], reason: str) -> None:
        entry = chosen.setdefault(record["image"], (record, set()))
        entry[1].add(reason)

    for index in np.linspace(0, len(records) - 1, max(1, count // 2), dtype=int):
        add(records[int(index)], "evenly_spaced")
    rankings = (
        (lambda item: item["raw"]["selected_vs_textseg"]["iou"], "lowest_raw_iou"),
        (
            lambda item: item["dense_addition"]["agreement_with_textseg"],
            "least_useful_dense_addition",
        ),
        (
            lambda item: -item["dense_addition"]["added_ratio"],
            "largest_dense_addition",
        ),
    )
    for ranking, reason in rankings:
        for record in sorted(records, key=ranking)[: max(2, count // 6)]:
            add(record, reason)
    return [
        {**record, "review_reason": sorted(reasons)}
        for record, reasons in list(chosen.values())[:count]
    ]


def render_reviews(
    output: Path, baseline: Path, selected: list[dict[str, Any]]
) -> None:
    review_root = output / "review_panels"
    review_root.mkdir(parents=True, exist_ok=True)
    columns = 3
    cell_width, cell_height, label_height = 600, 460, 74
    rows = math.ceil(len(selected) / columns)
    sheet = Image.new(
        "RGB",
        (columns * cell_width, rows * (cell_height + label_height)),
        (225, 225, 225),
    )
    draw = ImageDraw.Draw(sheet)
    labels = ("source", "TextSeg", "instances", "dense ink", "region@.5")
    for index, record in enumerate(selected):
        with (
            Image.open(record["source_path"]) as source_image,
            Image.open(baseline / record["dataset"] / record["textseg_painted"]) as sam_image,
            Image.open(baseline / record["dataset"] / record["baseline_painted"]) as baseline_image,
            Image.open(output / record["selected_painted"]) as dense_image,
            Image.open(output / record["dense_region_mask"]) as region_image,
        ):
            source = source_image.convert("RGB")
            panels = [
                source,
                sam_image.convert("RGB"),
                baseline_image.convert("RGB"),
                dense_image.convert("RGB"),
                region_overlay(source, region_image),
            ]
            preview = Image.new("RGB", (2000, 1240), "white")
            preview_draw = ImageDraw.Draw(preview)
            for panel_index, (label, panel) in enumerate(zip(labels, panels, strict=True)):
                preview.paste(fit(panel, 400, 1200), (panel_index * 400, 40))
                preview_draw.text((panel_index * 400 + 5, 8), label, fill="black")
        preview_path = review_root / f"{Path(record['image']).stem}.jpg"
        preview.save(preview_path, quality=94, subsampling=0)
        record["review_panel"] = preview_path.relative_to(output).as_posix()

        x = (index % columns) * cell_width
        y = (index // columns) * (cell_height + label_height)
        panel_width = cell_width // len(panels)
        for panel_index, panel in enumerate(panels):
            sheet.paste(
                fit(panel, panel_width, cell_height),
                (x + panel_index * panel_width, y + label_height),
            )
        raw = record["raw"]
        draw.text(
            (x + 5, y + 4),
            f"{record['image']}  instance IoU={raw['instance_vs_textseg']['iou']:.3f}  dense={raw['selected_vs_textseg']['iou']:.3f}",
            fill="black",
        )
        draw.text(
            (x + 5, y + 25),
            f"added={record['dense_addition']['added_ratio']:.2%}  useful={record['dense_addition']['agreement_with_textseg']:.1%}",
            fill=(35, 35, 35),
        )
        draw.text(
            (x + 5, y + 47),
            " | ".join(labels),
            fill=(35, 35, 35),
        )
    sheet.save(output / "review_contact_sheet.jpg", quality=94, subsampling=0)
    (output / "review_selection.json").write_text(
        json.dumps(selected, indent=2) + "\n", encoding="utf-8"
    )


def evaluate_dataset(
    model: Any,
    captured: list[list[torch.Tensor]],
    dataset: str,
    textseg_root: Path,
    baseline_root: Path,
    output_root: Path,
    batch_size: int,
    review_pages: int,
) -> dict[str, Any]:
    metadata = json.loads(
        (textseg_root / dataset / "metrics.json").read_text(encoding="utf-8")
    )
    source_root = Path(metadata["source"])
    manifest = [item for item in metadata["records"] if "/" not in item["image"]]
    output = output_root / dataset
    output.mkdir(parents=True, exist_ok=True)
    records: list[dict[str, Any]] = []
    started = time.perf_counter()

    for offset in range(0, len(manifest), batch_size):
        batch = manifest[offset : offset + batch_size]
        images: list[Image.Image] = []
        textseg_masks: list[np.ndarray] = []
        for item in batch:
            with Image.open(source_root / item["image"]) as image:
                images.append(image.convert("RGB"))
            with Image.open(textseg_root / dataset / item["mask"]) as image:
                textseg_masks.append(np.asarray(image.convert("L")) > 127)

        with torch.inference_mode():
            detections = model.predict(
                images,
                threshold=TEXT_THRESHOLD,
                shape=(1152, 1152),
                include_source_image=False,
            )
            if not isinstance(detections, list):
                detections = [detections]
            if len(captured) != 1:
                raise RuntimeError(f"expected one captured feature batch, got {len(captured)}")
            dense = model.model.model.typography_head(captured.pop(0))

        for index, (item, source, textseg, detection) in enumerate(
            zip(batch, images, textseg_masks, detections, strict=True)
        ):
            width, height = source.size
            instance = np.zeros((height, width), dtype=bool)
            for class_id, score, mask in zip(
                detection.class_id,
                detection.confidence,
                detection.mask,
                strict=True,
            ):
                if (int(class_id) == TEXT_CLASS and float(score) >= TEXT_THRESHOLD) or (
                    int(class_id) == ONOMATOPOEIA_CLASS
                    and float(score) >= ONOMATOPOEIA_THRESHOLD
                ):
                    instance |= mask

            with torch.inference_mode():
                region = (
                    F.interpolate(
                        dense["region_logits"][index : index + 1],
                        size=(height, width),
                        mode="bilinear",
                        align_corners=False,
                    )[0, 0]
                    .sigmoid()
                    .ge(REGION_THRESHOLD)
                    .cpu()
                    .numpy()
                )
                ink = (
                    F.interpolate(
                        dense["ink_logits"][index : index + 1],
                        size=(height, width),
                        mode="bilinear",
                        align_corners=False,
                    )[0, 0]
                    .sigmoid()
                    .ge(INK_THRESHOLD)
                    .cpu()
                    .numpy()
                )
            region_fused = instance | region
            selected = instance | ink
            selected_processed, selected_postprocess = postprocess(
                selected, width, height, DILATION_RADIUS
            )

            relative = Path(item["image"]).with_suffix(".png")
            region_destination = Path("dense_region_masks") / relative
            ink_destination = Path("dense_ink_099_masks") / relative
            selected_destination = Path("dense_fused_masks") / relative
            processed_destination = Path("dense_fused_processed_masks") / relative
            painted_destination = Path("dense_fused_white_painted") / relative
            save_binary(region, output / region_destination)
            save_binary(ink, output / ink_destination)
            save_binary(selected, output / selected_destination)
            save_mask(selected_processed, output / processed_destination)
            source_array = np.asarray(source).copy()
            save_white_painted(source_array, selected_processed, output / painted_destination)

            baseline_mask_path = baseline_root / dataset / "rfdetr_processed_masks" / relative
            textseg_processed_path = baseline_root / dataset / "textseg_processed_masks" / relative
            with (
                Image.open(baseline_mask_path) as image,
                Image.open(textseg_processed_path) as sam_processed_image,
            ):
                baseline_processed = np.asarray(image.convert("L")) > 127
                textseg_processed = np.asarray(sam_processed_image.convert("L")) > 127

            added = ink & ~instance
            added_pixels = int(added.sum())
            added_agreement = int((added & textseg).sum())
            records.append(
                {
                    "dataset": dataset,
                    "image": item["image"],
                    "source_path": str(source_root / item["image"]),
                    "width": width,
                    "height": height,
                    "dense_region_mask": region_destination.as_posix(),
                    "dense_ink_mask": ink_destination.as_posix(),
                    "selected_raw_mask": selected_destination.as_posix(),
                    "selected_processed_mask": processed_destination.as_posix(),
                    "selected_painted": painted_destination.as_posix(),
                    "textseg_painted": f"textseg_white_painted/{relative.as_posix()}",
                    "baseline_painted": f"rfdetr_white_painted/{relative.as_posix()}",
                    "raw": {
                        "instance_vs_textseg": pair_counts(textseg, instance),
                        "region_fused_vs_textseg": pair_counts(textseg, region_fused),
                        "selected_vs_textseg": pair_counts(textseg, selected),
                    },
                    "processed": {
                        "baseline_vs_textseg": pair_counts(
                            textseg_processed, baseline_processed
                        ),
                        "selected_vs_textseg": pair_counts(
                            textseg_processed, selected_processed
                        ),
                        "selected_vs_baseline": pair_counts(
                            baseline_processed, selected_processed
                        ),
                        "selected_covers_textseg_raw": pair_counts(
                            textseg, selected_processed
                        ),
                    },
                    "dense_addition": {
                        "region_pixels": int(region.sum()),
                        "ink_pixels": int(ink.sum()),
                        "added_pixels": added_pixels,
                        "added_ratio": float(added_pixels / added.size),
                        "agreement_with_textseg_pixels": added_agreement,
                        "agreement_with_textseg": float(added_agreement / added_pixels)
                        if added_pixels
                        else 1.0,
                    },
                    "postprocess": selected_postprocess,
                }
            )
        completed = min(offset + batch_size, len(manifest))
        if completed == len(manifest) or completed % 20 < batch_size:
            elapsed = time.perf_counter() - started
            print(
                json.dumps(
                    {
                        "event": "progress",
                        "dataset": dataset,
                        "completed": completed,
                        "total": len(manifest),
                        "pages_per_second": completed / elapsed,
                    }
                ),
                flush=True,
            )

    aggregate = {
        "raw": {
            key: aggregate_pairs(records, ("raw", key))
            for key in (
                "instance_vs_textseg",
                "region_fused_vs_textseg",
                "selected_vs_textseg",
            )
        },
        "processed": {
            key: aggregate_pairs(records, ("processed", key))
            for key in (
                "baseline_vs_textseg",
                "selected_vs_textseg",
                "selected_vs_baseline",
                "selected_covers_textseg_raw",
            )
        },
        "dense_addition": {
            "pixels": sum(item["dense_addition"]["added_pixels"] for item in records),
            "agreement_with_textseg_pixels": sum(
                item["dense_addition"]["agreement_with_textseg_pixels"]
                for item in records
            ),
        },
    }
    addition = aggregate["dense_addition"]
    addition["agreement_with_textseg"] = float(
        addition["agreement_with_textseg_pixels"] / addition["pixels"]
    ) if addition["pixels"] else 1.0
    selected_reviews = select_reviews(records, review_pages)
    render_reviews(output, baseline_root, selected_reviews)
    summary = {
        "name": dataset,
        "source": str(source_root),
        "pages": len(records),
        "seconds": time.perf_counter() - started,
        "aggregate": aggregate,
        "review_pages": len(selected_reviews),
        "records": records,
    }
    (output / "metrics.json").write_text(
        json.dumps(summary, indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )
    return summary


def main() -> None:
    args = parse_args()
    textseg_root = args.textseg_root.resolve()
    model_root = args.model_root.resolve()
    checkpoint = args.dense_checkpoint.resolve()
    baseline_root = args.baseline_root.resolve()
    output_root = args.output_root.resolve()
    if output_root.exists():
        raise FileExistsError(output_root)
    if args.batch_size < 1 or args.review_pages < 1:
        raise ValueError("batch size and review pages must be positive")
    output_root.mkdir(parents=True)
    model, captured, handle = load_dense_model(model_root, checkpoint)
    try:
        summaries = {}
        for dataset in DATASETS:
            result = evaluate_dataset(
                model,
                captured,
                dataset,
                textseg_root,
                baseline_root,
                output_root,
                args.batch_size,
                args.review_pages,
            )
            summaries[dataset] = {
                key: value for key, value in result.items() if key != "records"
            }
    finally:
        handle.remove()

    report = {
        "status": "complete",
        "models": {
            "textseg": "mayocream/koharu-text-sam-ts-l",
            "rfdetr": "mayocream/koharu-layout-rfdetr-seg-2xl-1152",
            "rfdetr_weights": str(model_root / "model.safetensors"),
            "rfdetr_weights_sha256": sha256(model_root / "model.safetensors"),
            "dense_checkpoint": str(checkpoint),
            "dense_checkpoint_sha256": sha256(checkpoint),
            "dense_head_parameters": sum(
                parameter.numel()
                for parameter in model.model.model.typography_head.parameters()
            ),
        },
        "policy": {
            "instance_mask": "text@0.25 union onomatopoeia@0.40",
            "trained_region_audit": "instance mask union region sigmoid>=0.50",
            "selected_dense_mask": "instance mask union ink sigmoid>=0.99",
            "selection_reason": "most conservative tested dense branch; unrestricted region output was unsafe",
            "postprocessing": "4px elliptical dilation at 1024px long side, fill holes, paint exact white",
            "ground_truth": False,
            "comparison_reference": "TextSeg output, not human labels",
        },
        "datasets": summaries,
    }
    (output_root / "summary.json").write_text(
        json.dumps(report, indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )
    print(json.dumps({"event": "complete", "output": str(output_root)}))


if __name__ == "__main__":
    main()
