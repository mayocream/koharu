# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "numpy>=2.0",
#   "opencv-python-headless>=4.10",
#   "pillow>=11",
#   "safetensors>=0.5",
#   "segmentation-models-pytorch>=0.5",
#   "torch>=2.6",
#   "tqdm>=4.67",
# ]
# ///
"""Generate MTS-2025 binary teacher masks for every local Manga109 page."""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import math
import os
import time
from collections import defaultdict, deque
from pathlib import Path
from typing import Iterable

import cv2
import numpy as np
from PIL import Image
import torch
from tqdm import tqdm

from run_manga_text_segmentation_2025 import load_model


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_IMAGES = REPOSITORY_ROOT / "data" / "Manga109_released_2026_05_21" / "images"
DEFAULT_MODEL = (
    REPOSITORY_ROOT
    / "data"
    / "koharu-manga-layoutseg"
    / "models"
    / "manga-text-segmentation-2025"
    / "model.safetensors"
)
DEFAULT_OUTPUT = REPOSITORY_ROOT / "data" / "koharu-manga-layoutseg" / "teachers-mts2025"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--images", type=Path, default=DEFAULT_IMAGES)
    parser.add_argument("--model", type=Path, default=DEFAULT_MODEL)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--batch-size", type=int, default=8)
    parser.add_argument("--workers", type=int, default=min(24, os.cpu_count() or 1))
    parser.add_argument("--threshold", type=float, default=0.5)
    parser.add_argument("--tta", action="store_true")
    parser.add_argument("--limit", type=int)
    parser.add_argument("--overwrite", action="store_true")
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def output_path(output: Path, images: Path, image_path: Path) -> Path:
    return output / "masks" / image_path.relative_to(images).with_suffix(".png")


def read_size(path: Path) -> tuple[Path, tuple[int, int]]:
    with Image.open(path) as image:
        return path, (image.height, image.width)


def load_rgb(path: Path) -> tuple[Path, np.ndarray]:
    image = cv2.imread(str(path), cv2.IMREAD_COLOR)
    if image is None:
        raise OSError(f"failed to read {path}")
    return path, cv2.cvtColor(image, cv2.COLOR_BGR2RGB)


def write_mask(path: Path, mask: np.ndarray) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.stem}.{os.getpid()}.tmp.png")
    if not cv2.imwrite(str(temporary), mask.astype(np.uint8) * 255):
        raise OSError(f"failed to write {temporary}")
    os.replace(temporary, path)


def bounded_load(
    executor: concurrent.futures.ThreadPoolExecutor,
    paths: Iterable[Path],
    prefetch: int,
) -> Iterable[tuple[Path, np.ndarray]]:
    iterator = iter(paths)
    pending: deque[concurrent.futures.Future[tuple[Path, np.ndarray]]] = deque()
    for path in iterator:
        pending.append(executor.submit(load_rgb, path))
        if len(pending) >= prefetch:
            yield pending.popleft().result()
    while pending:
        yield pending.popleft().result()


def infer(
    model: torch.nn.Module,
    images: list[np.ndarray],
    device: torch.device,
    threshold: float,
    tta: bool,
) -> list[np.ndarray]:
    batch = torch.from_numpy(np.stack(images)).to(device).permute(0, 3, 1, 2).float().div_(255.0)
    mean = torch.tensor((0.485, 0.456, 0.406), device=device)[None, :, None, None]
    std = torch.tensor((0.229, 0.224, 0.225), device=device)[None, :, None, None]
    batch = ((batch - mean) / std).contiguous(memory_format=torch.channels_last)
    height, width = images[0].shape[:2]
    batch = torch.nn.functional.pad(
        batch,
        (0, (32 - width % 32) % 32, 0, (32 - height % 32) % 32),
    )
    variants = [(batch, ())]
    if tta:
        variants.extend(
            ((torch.flip(batch, (3,)), (3,)), (torch.flip(batch, (2,)), (2,)))
        )
    probabilities = None
    with torch.inference_mode():
        for value, flip_dims in variants:
            with torch.autocast(device_type=device.type, enabled=device.type == "cuda"):
                prediction = model(value).sigmoid()
            if flip_dims:
                prediction = torch.flip(prediction, flip_dims)
            probabilities = prediction if probabilities is None else probabilities + prediction
    probabilities = probabilities[:, 0, :height, :width] / len(variants)
    masks = probabilities >= threshold
    return [mask.cpu().numpy() for mask in masks]


def main() -> None:
    args = parse_args()
    if args.batch_size <= 0 or args.workers <= 0:
        raise ValueError("--batch-size and --workers must be positive")
    if not 0.0 < args.threshold < 1.0:
        raise ValueError("--threshold must be between zero and one")
    if not args.model.is_file():
        raise FileNotFoundError(args.model)
    image_paths = sorted(args.images.glob("*/*.jpg"))
    if len(image_paths) != 10_602:
        raise ValueError(f"expected 10,602 Manga109 pages, found {len(image_paths)}")
    args.output.mkdir(parents=True, exist_ok=True)
    if args.overwrite:
        selected = image_paths
    else:
        selected = [
            path
            for path in image_paths
            if not output_path(args.output, args.images, path).is_file()
        ]
    if args.limit is not None:
        selected = selected[: args.limit]

    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as executor:
        sized = list(executor.map(read_size, selected))
    by_shape: dict[tuple[int, int], list[Path]] = defaultdict(list)
    for path, shape in sized:
        by_shape[shape].append(path)

    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    if device.type != "cuda":
        raise RuntimeError("CUDA is required for the all-pages teacher build")
    torch.backends.cudnn.benchmark = True
    model = load_model(args.model, device).to(memory_format=torch.channels_last)
    torch.cuda.reset_peak_memory_stats()
    started = time.perf_counter()
    written = 0
    pending_writes: set[concurrent.futures.Future[None]] = set()
    progress = tqdm(total=len(selected), desc="MTS-2025", unit="page")
    with (
        concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as loader,
        concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as writer,
    ):
        for shape in sorted(by_shape, key=lambda value: (-value[0] * value[1], value)):
            loaded = bounded_load(
                loader,
                by_shape[shape],
                prefetch=max(args.batch_size * 4, args.workers * 2),
            )
            batch_items: list[tuple[Path, np.ndarray]] = []
            for item in loaded:
                batch_items.append(item)
                if len(batch_items) < args.batch_size:
                    continue
                masks = infer(
                    model,
                    [image for _, image in batch_items],
                    device,
                    args.threshold,
                    args.tta,
                )
                for (path, _), mask in zip(batch_items, masks, strict=True):
                    pending_writes.add(
                        writer.submit(
                            write_mask,
                            output_path(args.output, args.images, path),
                            mask,
                        )
                    )
                written += len(batch_items)
                progress.update(len(batch_items))
                batch_items = []
                if len(pending_writes) >= args.workers * 4:
                    done, pending_writes = concurrent.futures.wait(
                        pending_writes,
                        return_when=concurrent.futures.FIRST_COMPLETED,
                    )
                    for future in done:
                        future.result()
            if batch_items:
                masks = infer(
                    model,
                    [image for _, image in batch_items],
                    device,
                    args.threshold,
                    args.tta,
                )
                for (path, _), mask in zip(batch_items, masks, strict=True):
                    pending_writes.add(
                        writer.submit(
                            write_mask,
                            output_path(args.output, args.images, path),
                            mask,
                        )
                    )
                written += len(batch_items)
                progress.update(len(batch_items))
        for future in concurrent.futures.as_completed(pending_writes):
            future.result()
    progress.close()
    torch.cuda.synchronize()
    elapsed = time.perf_counter() - started
    total_masks = sum(1 for path in (args.output / "masks").glob("*/*.png"))
    model_metadata = {
        "model": "mayocream/manga-text-segmentation-2025",
        "architecture": "UnetPlusPlus/tu-efficientnetv2_rw_m/scse",
        "runtime": "PyTorch",
        "model_sha256": sha256(args.model),
        "threshold": args.threshold,
        "tta": args.tta,
    }
    (args.output / "teacher_models.json").write_text(
        json.dumps(model_metadata, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    summary = {
        **model_metadata,
        "device": torch.cuda.get_device_name(),
        "requested_this_run": len(selected),
        "written_this_run": written,
        "total_masks": total_masks,
        "expected_masks": len(image_paths),
        "complete": total_masks == len(image_paths),
        "batch_size": args.batch_size,
        "workers": args.workers,
        "seconds": round(elapsed, 3),
        "pages_per_second": round(written / max(elapsed, 1e-9), 3),
        "peak_cuda_memory_gib": round(torch.cuda.max_memory_allocated() / 1024**3, 3),
        "shape_groups": {f"{height}x{width}": len(paths) for (height, width), paths in by_shape.items()},
    }
    (args.output / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
