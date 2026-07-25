"""Evaluate baseline and fine-tuned SAM-TS-L on Zenodo Manga109 masks."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import time
from collections import defaultdict
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import numpy as np
import torch
from PIL import Image, ImageDraw
from safetensors.torch import load_file as load_safetensors

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from train_zenodo_sam_ts import (  # noqa: E402
    ManifestDataset,
    amp_settings,
    build_model,
    make_loader,
    read_jsonl,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dataset", default=ROOT / "data" / "manga109-zenodo-sam-ts", type=Path
    )
    parser.add_argument("--hi-sam-root", default=ROOT / "temp" / "Hi-SAM", type=Path)
    parser.add_argument(
        "--checkpoint",
        default=ROOT / "models" / "hi-sam" / "sam_tss_l_textseg.pth",
        type=Path,
    )
    parser.add_argument(
        "--sam-checkpoint",
        default=ROOT / "models" / "hi-sam" / "sam_vit_l_0b3195.pth",
        type=Path,
    )
    parser.add_argument(
        "--fine-tuned",
        default=(
            ROOT
            / "runs"
            / "sam-ts-l-textseg-zenodo-b300-full"
            / "sam_tss_l_zenodo_best.pth"
        ),
        type=Path,
    )
    parser.add_argument(
        "--output",
        default=ROOT / "runs" / "sam-ts-l-textseg-zenodo-b300-local-validation",
        type=Path,
    )
    parser.add_argument("--workers", default=2, type=int)
    parser.add_argument("--seed", default=42, type=int)
    parser.add_argument(
        "--amp-dtype", choices=("bfloat16", "float16", "none"), default="bfloat16"
    )
    parser.add_argument("--visual-count", default=8, type=int)
    return parser.parse_args()


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def counts_template() -> dict[str, int]:
    return {
        f"{branch}_{kind}": 0
        for branch in ("low", "hr")
        for kind in ("tp", "fp", "fn", "tn")
    }


def metrics_from_counts(counts: dict[str, int]) -> dict[str, float | int]:
    result: dict[str, float | int] = {}
    for branch in ("low", "hr"):
        tp = counts[f"{branch}_tp"]
        fp = counts[f"{branch}_fp"]
        fn = counts[f"{branch}_fn"]
        tn = counts[f"{branch}_tn"]
        result[f"{branch}_iou"] = tp / max(tp + fp + fn, 1)
        result[f"{branch}_fscore"] = 2 * tp / max(2 * tp + fp + fn, 1)
        result[f"{branch}_precision"] = tp / max(tp + fp, 1)
        result[f"{branch}_recall"] = tp / max(tp + fn, 1)
        result[f"{branch}_accuracy"] = (tp + tn) / max(tp + fp + fn + tn, 1)
    return result


def prediction_counts(
    prediction: torch.Tensor, target: torch.Tensor
) -> dict[str, int]:
    prediction = prediction.bool()
    target = target.bool()
    return {
        "tp": int((prediction & target).sum()),
        "fp": int((prediction & ~target).sum()),
        "fn": int((~prediction & target).sum()),
        "tn": int((~prediction & ~target).sum()),
    }


def evaluate_split(
    model: torch.nn.Module,
    loader: torch.utils.data.DataLoader[dict[str, Any]],
    device: torch.device,
    amp_enabled: bool,
    amp_dtype: torch.dtype | None,
    capture: bool,
) -> tuple[dict[str, Any], dict[str, dict[str, np.ndarray]]]:
    model.eval()
    aggregate = counts_template()
    book_counts: defaultdict[str, dict[str, int]] = defaultdict(counts_template)
    pages: list[dict[str, Any]] = []
    captures: dict[str, dict[str, np.ndarray]] = {}
    torch.cuda.synchronize(device)
    started = time.perf_counter()
    with torch.inference_mode():
        for batch in loader:
            if len(batch["source_image"]) != 1:
                raise ValueError("evaluation expects batch size one")
            source_image = batch["source_image"][0]
            book = source_image.split("/", 1)[0]
            image = batch["image"].to(device, non_blocking=True)
            target = batch["label"].to(device, non_blocking=True) > 127
            inputs = [
                {
                    "image": item.contiguous(),
                    "original_size": item.shape[-2:],
                }
                for item in image
            ]
            with torch.autocast(
                device_type="cuda", dtype=amp_dtype, enabled=amp_enabled
            ):
                _, low, _, _, high, _ = model(inputs, multimask_output=False)

            page_counts: dict[str, int] = {}
            for branch, prediction in (("low", low), ("hr", high)):
                branch_counts = prediction_counts(prediction, target)
                for kind, value in branch_counts.items():
                    key = f"{branch}_{kind}"
                    page_counts[key] = value
                    aggregate[key] += value
                    book_counts[book][key] += value
            pages.append(
                {
                    "source_image": source_image,
                    "book": book,
                    **metrics_from_counts(page_counts),
                    "foreground_pixels_1024": int(target.sum()),
                }
            )
            if capture:
                captures[source_image] = {
                    "image": image[0].byte().permute(1, 2, 0).cpu().numpy(),
                    "target": target[0, 0].cpu().numpy(),
                    "low": low[0, 0].bool().cpu().numpy(),
                    "hr": high[0, 0].bool().cpu().numpy(),
                }
    torch.cuda.synchronize(device)
    elapsed = time.perf_counter() - started
    page_hr_ious = [float(page["hr_iou"]) for page in pages]
    page_hr_fscores = [float(page["hr_fscore"]) for page in pages]
    summary = {
        **metrics_from_counts(aggregate),
        "macro_page_hr_iou": float(np.mean(page_hr_ious)),
        "macro_page_hr_fscore": float(np.mean(page_hr_fscores)),
        "seconds": elapsed,
        "pages": len(pages),
        "per_book": {
            book: metrics_from_counts(counts)
            for book, counts in sorted(book_counts.items())
        },
        "per_page": pages,
    }
    return summary, captures


def load_fine_tuned(model: torch.nn.Module, path: Path) -> dict[str, int]:
    state = (
        load_safetensors(path, device="cpu")
        if path.suffix == ".safetensors"
        else torch.load(path, map_location="cpu", weights_only=True)
    )
    if not isinstance(state, dict) or not state:
        raise ValueError(f"invalid fine-tuned state dict: {path}")
    model_state = model.state_dict()
    if set(state) == set(model_state):
        mismatched = sorted(
            key for key, value in state.items() if value.shape != model_state[key].shape
        )
        if mismatched:
            raise ValueError(f"full-state tensor shape mismatch: {mismatched[:5]}")
        incompatible = model.load_state_dict(state, strict=True)
        if incompatible.missing_keys or incompatible.unexpected_keys:
            raise ValueError(f"strict full-state load failed: {incompatible}")
        return {
            "loaded_tensors": len(state),
            "trainable_tensors": len(
                [parameter for parameter in model.parameters() if parameter.requires_grad]
            ),
            "frozen_state_entries": 0,
        }
    unexpected = sorted(set(state) - set(model_state))
    mismatched = sorted(
        key
        for key, value in state.items()
        if key in model_state and value.shape != model_state[key].shape
    )
    trainable = {
        name for name, parameter in model.named_parameters() if parameter.requires_grad
    }
    missing_trainable = sorted(trainable - set(state))
    if unexpected or mismatched or missing_trainable:
        raise ValueError(
            "fine-tuned checkpoint mismatch: "
            f"unexpected={unexpected[:5]}, mismatched={mismatched[:5]}, "
            f"missing_trainable={missing_trainable[:5]}"
        )
    incompatible = model.load_state_dict(state, strict=False)
    if incompatible.unexpected_keys:
        raise ValueError(f"unexpected loaded keys: {incompatible.unexpected_keys[:5]}")
    return {
        "loaded_tensors": len(state),
        "trainable_tensors": len(trainable),
        "frozen_state_entries": len(incompatible.missing_keys),
    }


def delta_metrics(
    baseline: dict[str, Any], fine_tuned: dict[str, Any]
) -> dict[str, float]:
    names = (
        "low_iou",
        "low_fscore",
        "low_precision",
        "low_recall",
        "hr_iou",
        "hr_fscore",
        "hr_precision",
        "hr_recall",
        "macro_page_hr_iou",
        "macro_page_hr_fscore",
    )
    return {name: float(fine_tuned[name] - baseline[name]) for name in names}


def blend_mask(
    image: np.ndarray, mask: np.ndarray, color: tuple[int, int, int]
) -> Image.Image:
    canvas = image.astype(np.float32).copy()
    canvas[mask] = canvas[mask] * 0.35 + np.asarray(color) * 0.65
    return Image.fromarray(np.clip(canvas, 0, 255).astype(np.uint8))


def error_overlay(
    image: np.ndarray, target: np.ndarray, prediction: np.ndarray
) -> Image.Image:
    canvas = image.astype(np.float32).copy()
    true_positive = target & prediction
    false_positive = ~target & prediction
    false_negative = target & ~prediction
    for mask, color in (
        (true_positive, (0, 255, 80)),
        (false_positive, (255, 40, 40)),
        (false_negative, (255, 210, 0)),
    ):
        canvas[mask] = canvas[mask] * 0.25 + np.asarray(color) * 0.75
    return Image.fromarray(np.clip(canvas, 0, 255).astype(np.uint8))


def panel(image: Image.Image, title: str, size: int = 384) -> Image.Image:
    resized = image.resize((size, size), Image.Resampling.LANCZOS)
    result = Image.new("RGB", (size, size + 28), "white")
    result.paste(resized, (0, 28))
    ImageDraw.Draw(result).text((8, 8), title, fill="black")
    return result


def select_visual_pages(
    baseline_pages: list[dict[str, Any]],
    fine_pages: list[dict[str, Any]],
    count: int,
) -> list[dict[str, Any]]:
    baseline_by_name = {page["source_image"]: page for page in baseline_pages}
    joined = []
    for page in fine_pages:
        baseline = baseline_by_name[page["source_image"]]
        joined.append(
            {
                "source_image": page["source_image"],
                "fine_hr_iou": page["hr_iou"],
                "baseline_hr_iou": baseline["hr_iou"],
                "gain_hr_iou": page["hr_iou"] - baseline["hr_iou"],
            }
        )
    selected: list[dict[str, Any]] = []
    groups = (
        sorted(joined, key=lambda row: row["fine_hr_iou"]),
        sorted(joined, key=lambda row: row["gain_hr_iou"], reverse=True),
        sorted(joined, key=lambda row: row["fine_hr_iou"], reverse=True),
    )
    index = 0
    while len(selected) < min(count, len(joined)):
        added = False
        for group in groups:
            if index >= len(group):
                continue
            candidate = group[index]
            if candidate["source_image"] not in {
                row["source_image"] for row in selected
            }:
                selected.append(candidate)
                added = True
                if len(selected) == min(count, len(joined)):
                    break
        index += 1
        if not added and index >= len(joined):
            break
    return selected


def render_visuals(
    output: Path,
    selected: list[dict[str, Any]],
    baseline: dict[str, dict[str, np.ndarray]],
    fine_tuned: dict[str, dict[str, np.ndarray]],
) -> list[str]:
    visuals = output / "visuals"
    visuals.mkdir()
    rows: list[Image.Image] = []
    paths: list[str] = []
    for rank, selection in enumerate(selected, start=1):
        name = selection["source_image"]
        base = baseline[name]
        fine = fine_tuned[name]
        page = Image.new("RGB", (384 * 4, 412), "white")
        page_panels = (
            panel(Image.fromarray(fine["image"]), f"Source: {name}"),
            panel(blend_mask(fine["image"], fine["target"], (0, 150, 255)), "Ground truth"),
            panel(
                error_overlay(base["image"], base["target"], base["hr"]),
                f"Baseline HR IoU {selection['baseline_hr_iou']:.3f}",
            ),
            panel(
                error_overlay(fine["image"], fine["target"], fine["hr"]),
                f"Fine-tuned HR IoU {selection['fine_hr_iou']:.3f}",
            ),
        )
        for column, item in enumerate(page_panels):
            page.paste(item, (column * 384, 0))
        safe_name = name.replace("/", "__").replace(".jpg", "")
        path = visuals / f"{rank:02d}_{safe_name}.png"
        page.save(path, optimize=True)
        paths.append(str(path.relative_to(output)).replace("\\", "/"))
        rows.append(page.resize((768, 206), Image.Resampling.LANCZOS))

    contact = Image.new("RGB", (768, 206 * len(rows) + 30), "white")
    draw = ImageDraw.Draw(contact)
    draw.text(
        (8, 8),
        "Error colors: green=true positive, red=false positive, yellow=missed text",
        fill="black",
    )
    for row, rendered in enumerate(rows):
        contact.paste(rendered, (0, 30 + row * 206))
    contact.save(output / "test_contact_sheet.png", optimize=True)
    return paths


def strip_pages(summary: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in summary.items() if key != "per_page"}


def main() -> None:
    args = parse_args()
    for name in (
        "dataset",
        "hi_sam_root",
        "checkpoint",
        "sam_checkpoint",
        "fine_tuned",
        "output",
    ):
        setattr(args, name, getattr(args, name).resolve())
    required = (
        args.dataset / "manifests" / "val.jsonl",
        args.dataset / "manifests" / "test.jsonl",
        args.hi_sam_root / "hi_sam" / "modeling" / "build.py",
        args.checkpoint,
        args.sam_checkpoint,
        args.fine_tuned,
    )
    missing = [str(path) for path in required if not path.is_file()]
    if missing:
        raise FileNotFoundError(f"missing evaluation inputs: {missing}")
    if args.output.exists():
        raise FileExistsError(f"output already exists: {args.output}")
    if not torch.cuda.is_available():
        raise RuntimeError("CUDA is required for SAM-TS-L evaluation")
    if args.workers < 0 or args.visual_count < 1:
        raise ValueError("workers must be non-negative and visual count positive")

    torch.manual_seed(args.seed)
    torch.backends.cuda.matmul.allow_tf32 = True
    torch.backends.cudnn.allow_tf32 = True
    torch.set_float32_matmul_precision("high")
    device = torch.device("cuda:0")
    amp_enabled, amp_dtype = amp_settings(args.amp_dtype)
    records = {
        split: read_jsonl(args.dataset / "manifests" / f"{split}.jsonl")
        for split in ("val", "test")
    }
    if len(records["val"]) != 50 or len(records["test"]) != 40:
        raise ValueError(
            f"unexpected split sizes: val={len(records['val'])}, test={len(records['test'])}"
        )
    loaders = {
        split: make_loader(
            ManifestDataset(args.dataset, split_records, augment=False),
            batch_size=1,
            workers=args.workers,
            shuffle=False,
            seed=args.seed,
        )
        for split, split_records in records.items()
    }

    args.output.mkdir(parents=True)
    model_args = SimpleNamespace(
        hi_sam_root=args.hi_sam_root,
        checkpoint=args.checkpoint,
        sam_checkpoint=args.sam_checkpoint,
    )
    model = build_model(model_args, args.output).to(device)
    print(json.dumps({"event": "model_ready", "device": torch.cuda.get_device_name(0)}))

    baseline: dict[str, dict[str, Any]] = {}
    baseline_captures: dict[str, dict[str, np.ndarray]] = {}
    for split in ("val", "test"):
        summary, captures = evaluate_split(
            model,
            loaders[split],
            device,
            amp_enabled,
            amp_dtype,
            capture=split == "test",
        )
        baseline[split] = summary
        baseline_captures.update(captures)
        print(json.dumps({"event": "baseline", "split": split, **strip_pages(summary)}))

    load_report = load_fine_tuned(model, args.fine_tuned)
    model.to(device)
    print(json.dumps({"event": "fine_tuned_loaded", **load_report}))
    fine_tuned: dict[str, dict[str, Any]] = {}
    fine_captures: dict[str, dict[str, np.ndarray]] = {}
    for split in ("val", "test"):
        summary, captures = evaluate_split(
            model,
            loaders[split],
            device,
            amp_enabled,
            amp_dtype,
            capture=split == "test",
        )
        fine_tuned[split] = summary
        fine_captures.update(captures)
        print(json.dumps({"event": "fine_tuned", "split": split, **strip_pages(summary)}))

    selected = select_visual_pages(
        baseline["test"]["per_page"],
        fine_tuned["test"]["per_page"],
        args.visual_count,
    )
    visual_paths = render_visuals(
        args.output, selected, baseline_captures, fine_captures
    )
    report = {
        "status": "complete",
        "dataset": str(args.dataset),
        "fine_tuned_checkpoint": str(args.fine_tuned),
        "checkpoint_sha256": file_sha256(args.fine_tuned),
        "base_textseg_sha256": file_sha256(args.checkpoint),
        "runtime": {
            "device": torch.cuda.get_device_name(0),
            "torch": torch.__version__,
            "cuda": torch.version.cuda,
            "amp_dtype": args.amp_dtype,
        },
        "load_report": load_report,
        "splits": {
            split: {
                "baseline": strip_pages(baseline[split]),
                "fine_tuned": strip_pages(fine_tuned[split]),
                "delta": delta_metrics(baseline[split], fine_tuned[split]),
            }
            for split in ("val", "test")
        },
        "test_per_page": {
            "baseline": baseline["test"]["per_page"],
            "fine_tuned": fine_tuned["test"]["per_page"],
        },
        "selected_visuals": [
            {**selection, "path": path}
            for selection, path in zip(selected, visual_paths, strict=True)
        ],
        "legend": {
            "green": "true positive",
            "red": "false positive",
            "yellow": "false negative",
        },
    }
    (args.output / "metrics.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    (args.output / "VALIDATION_COMPLETE.json").write_text(
        json.dumps(
            {
                "status": "complete",
                "checkpoint_sha256": report["checkpoint_sha256"],
                "validation": report["splits"]["val"],
                "test": report["splits"]["test"],
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {
                "event": "evaluation_complete",
                "output": str(args.output),
                "test": report["splits"]["test"],
            }
        )
    )


if __name__ == "__main__":
    main()
