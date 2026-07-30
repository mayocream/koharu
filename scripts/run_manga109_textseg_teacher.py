#!/usr/bin/env python3
"""Run Koharu TextSeg on Manga109 pages not covered by the Zenodo masks.

The output is resumable.  Records are appended after each completed batch and
the corresponding mask must exist for a record to be considered complete.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from collections import defaultdict
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import ijson
import numpy as np
import torch
from PIL import Image
from safetensors.torch import load_file

from run_koharu_text_sam_ts_l_batch import (
    install_checkpoint_compatibility,
    mask_iou,
    natural_key,
    prepare,
    sha256,
)


ROOT = Path(__file__).resolve().parents[1]
MARKER = ".manga109-textseg-teacher"
SPLITS = ("train", "valid", "test")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--rfdetr-view",
        type=Path,
        default=ROOT / "data" / "manga109-segmentation-rfdetr",
    )
    parser.add_argument(
        "--images-root",
        type=Path,
        default=ROOT / "data" / "Manga109_released_2026_05_21" / "images",
    )
    parser.add_argument(
        "--zenodo-root",
        type=Path,
        default=ROOT / "data" / "manga109-zenodo-sam-ts",
    )
    parser.add_argument(
        "--weights",
        type=Path,
        default=ROOT / "runs" / "huggingface" / "koharu-text-sam-ts-l" / "model.safetensors",
    )
    parser.add_argument("--hi-sam-root", type=Path, default=ROOT / "temp" / "Hi-SAM")
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "data" / "manga109-textseg-teacher",
    )
    parser.add_argument("--batch-size", type=int, default=4)
    parser.add_argument(
        "--pilot-pages-per-book",
        type=int,
        default=0,
        help="Select evenly spaced non-Zenodo pages from every book; zero runs all pages.",
    )
    return parser.parse_args()


def zenodo_pages(root: Path) -> set[str]:
    pages: set[str] = set()
    for split in ("train", "val", "test"):
        path = root / "manifests" / f"{split}.jsonl"
        with path.open(encoding="utf-8") as stream:
            for line in stream:
                pages.add(str(json.loads(line)["source_image"]))
    return pages


def inventory_pages(rfdetr_view: Path) -> list[dict[str, Any]]:
    pages: list[dict[str, Any]] = []
    for split in SPLITS:
        annotation = rfdetr_view / split / "_annotations.coco.json"
        with annotation.open("rb") as stream:
            for image in ijson.items(stream, "images.item", use_float=True):
                relative = str(image["file_name"])
                pages.append(
                    {
                        "image": relative,
                        "split": split,
                        "book": relative.split("/", 1)[0],
                        "width": int(image["width"]),
                        "height": int(image["height"]),
                    }
                )
    pages.sort(key=lambda item: natural_key(Path(item["image"])))
    return pages


def select_pilot(pages: list[dict[str, Any]], per_book: int) -> list[dict[str, Any]]:
    if per_book <= 0:
        return pages
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for page in pages:
        grouped[page["book"]].append(page)
    selected: list[dict[str, Any]] = []
    for book in sorted(grouped):
        values = grouped[book]
        count = min(per_book, len(values))
        # Interior quantiles avoid selecting only covers and final colophons.
        indices = np.linspace(0.20, 0.80, count)
        chosen = sorted({min(len(values) - 1, int(round(value * (len(values) - 1)))) for value in indices})
        selected.extend(values[index] for index in chosen)
    selected.sort(key=lambda item: natural_key(Path(item["image"])))
    return selected


def load_completed(output: Path) -> dict[str, dict[str, Any]]:
    records_path = output / "records.jsonl"
    if not records_path.is_file():
        return {}
    records: dict[str, dict[str, Any]] = {}
    with records_path.open(encoding="utf-8") as stream:
        for line in stream:
            record = json.loads(line)
            if (output / record["mask"]).is_file():
                records[record["image"]] = record
    return records


def prepare_output(output: Path) -> None:
    if output.exists() and any(output.iterdir()):
        if not (output / MARKER).is_file():
            raise FileExistsError(f"unmarked output already exists: {output}")
    else:
        output.mkdir(parents=True, exist_ok=True)
        (output / MARKER).write_text("resumable Manga109 TextSeg teacher cache\n", encoding="utf-8")
    (output / "masks").mkdir(parents=True, exist_ok=True)


def load_model(weights: Path, hi_sam_root: Path) -> torch.nn.Module:
    install_checkpoint_compatibility()
    sys.path.insert(0, str(hi_sam_root))
    from hi_sam.modeling.build import model_registry

    model_args = SimpleNamespace(
        checkpoint=None,
        model_type="vit_l",
        attn_layers=1,
        prompt_len=12,
        hier_det=False,
    )
    model = model_registry["vit_l"](args=model_args)
    state = load_file(weights, device="cpu")
    model.load_state_dict(state, strict=True)
    return model.eval().cuda()


def write_summary(
    output: Path,
    pages: list[dict[str, Any]],
    completed: dict[str, dict[str, Any]],
    excluded_count: int,
    weights: Path,
    started: float,
    batch_size: int,
) -> None:
    records = [completed[page["image"]] for page in pages if page["image"] in completed]
    ratios = np.asarray([record["predicted_ratio"] for record in records], dtype=np.float64)
    agreements = np.asarray([record["low_hr_iou_1024"] for record in records], dtype=np.float64)
    summary = {
        "status": "complete" if len(records) == len(pages) else "partial",
        "model": "mayocream/koharu-text-sam-ts-l",
        "weights": str(weights),
        "weight_sha256": sha256(weights),
        "pages_selected": len(pages),
        "pages_complete": len(records),
        "zenodo_pages_excluded": excluded_count,
        "seconds_this_run": time.perf_counter() - started,
        "runtime": {
            "device": torch.cuda.get_device_name(0),
            "torch": torch.__version__,
            "cuda": torch.version.cuda,
            "batch_size": batch_size,
        },
        "predicted_ratio": {
            "minimum": float(ratios.min()),
            "median": float(np.median(ratios)),
            "mean": float(ratios.mean()),
            "maximum": float(ratios.max()),
        } if len(ratios) else None,
        "low_hr_iou_1024": {
            "minimum": float(agreements.min()),
            "median": float(np.median(agreements)),
            "mean": float(agreements.mean()),
            "maximum": float(agreements.max()),
        } if len(agreements) else None,
    }
    temporary = output / f".summary.{os.getpid()}.json"
    temporary.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, output / "summary.json")


def main() -> None:
    args = parse_args()
    rfdetr_view = args.rfdetr_view.resolve()
    images_root = args.images_root.resolve()
    zenodo_root = args.zenodo_root.resolve()
    weights = args.weights.resolve()
    hi_sam_root = args.hi_sam_root.resolve()
    output = args.output.resolve()
    if args.batch_size < 1 or args.pilot_pages_per_book < 0:
        raise ValueError("batch size must be positive and pilot pages non-negative")
    if not torch.cuda.is_available():
        raise RuntimeError("CUDA is required")
    excluded = zenodo_pages(zenodo_root)
    all_pages = inventory_pages(rfdetr_view)
    remaining = [page for page in all_pages if page["image"] not in excluded]
    pages = select_pilot(remaining, args.pilot_pages_per_book)
    if len(all_pages) != 10602 or len(excluded) != 449 or len(remaining) != 10153:
        raise RuntimeError(
            f"unexpected inventory: all={len(all_pages)} zenodo={len(excluded)} remaining={len(remaining)}"
        )
    prepare_output(output)
    completed = load_completed(output)
    pending = [page for page in pages if page["image"] not in completed]
    started = time.perf_counter()
    print(
        json.dumps(
            {
                "event": "inventory",
                "selected": len(pages),
                "already_complete": len(pages) - len(pending),
                "pending": len(pending),
                "zenodo_excluded": len(excluded),
            }
        ),
        flush=True,
    )
    if pending:
        model = load_model(weights, hi_sam_root)
        print(json.dumps({"event": "model_ready", "device": torch.cuda.get_device_name(0)}), flush=True)
        records_path = output / "records.jsonl"
        with records_path.open("a", encoding="utf-8", newline="\n") as record_file:
            for offset in range(0, len(pending), args.batch_size):
                batch = pending[offset : offset + args.batch_size]
                prepared = [prepare(images_root / page["image"]) for page in batch]
                images = torch.stack([item[0] for item in prepared]).cuda()
                inputs = [
                    {"image": image.contiguous(), "original_size": (1024, 1024)}
                    for image in images
                ]
                with torch.inference_mode(), torch.autocast(
                    device_type="cuda", dtype=torch.bfloat16
                ):
                    _, low, _, _, high, _ = model(inputs, multimask_output=False)
                low = low.bool().cpu().numpy()
                high = high.bool().cpu().numpy()
                for index, page in enumerate(batch):
                    resized_width, resized_height = prepared[index][1]
                    original_size = prepared[index][2]
                    low_mask = low[index, 0, :resized_height, :resized_width]
                    high_mask = high[index, 0, :resized_height, :resized_width]
                    mask_image = Image.fromarray(high_mask.astype(np.uint8) * 255, mode="L")
                    mask_image = mask_image.resize(original_size, Image.Resampling.NEAREST)
                    relative_mask = Path("masks") / Path(page["image"]).with_suffix(".png")
                    destination = output / relative_mask
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    mask_image.save(destination)
                    original_mask = np.asarray(mask_image) > 127
                    record = {
                        **page,
                        "mask": relative_mask.as_posix(),
                        "predicted_pixels": int(original_mask.sum()),
                        "predicted_ratio": float(original_mask.mean()),
                        "low_hr_iou_1024": mask_iou(low_mask, high_mask),
                    }
                    record_file.write(json.dumps(record, separators=(",", ":")) + "\n")
                    completed[page["image"]] = record
                record_file.flush()
                os.fsync(record_file.fileno())
                done = min(offset + args.batch_size, len(pending))
                if done == len(pending) or done % 100 < args.batch_size:
                    elapsed = time.perf_counter() - started
                    print(
                        json.dumps(
                            {
                                "event": "progress",
                                "completed_this_run": done,
                                "pending_this_run": len(pending),
                                "pages_per_second": done / elapsed,
                            }
                        ),
                        flush=True,
                    )
    write_summary(
        output,
        pages,
        completed,
        len(excluded),
        weights,
        started,
        args.batch_size,
    )
    print(json.dumps({"event": "complete", "output": str(output), "pages": len(pages)}), flush=True)


if __name__ == "__main__":
    main()
