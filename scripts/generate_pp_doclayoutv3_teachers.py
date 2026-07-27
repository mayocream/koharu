# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "ijson>=3.4",
#   "opencv-python-headless>=4.13",
#   "pillow>=11",
#   "torch>=2.7",
#   "torchvision>=0.22",
#   "transformers>=5.13,<5.14",
#   "tqdm>=4.67",
# ]
# ///
"""Cache PP-DocLayoutV3 normal-text predictions for Manga109 pages.

The official SafeTensors checkpoint is executed with PyTorch through
Transformers.  One atomic JSON record is written per page, which makes the
expensive pass resumable and shardable across GPUs.  This cache is not a new
source of gold annotations: the downstream training-view builder assigns
reduced weight to confident predictions and turns ambiguous predictions into
ignore regions.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import time
from collections import deque
from collections.abc import Iterable
from pathlib import Path
from typing import Any

import ijson
import numpy as np
import torch
from PIL import Image
from tqdm import tqdm
from transformers import AutoImageProcessor, AutoModelForObjectDetection

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_ANNOTATION = (
    ROOT / "data" / "manga109-segmentation-rfdetr" / "train" / "_annotations.coco.json"
)
DEFAULT_IMAGES = ROOT / "data" / "manga109-segmentation-rfdetr" / "train"
DEFAULT_OUTPUT = ROOT / "data" / "manga109-segmentation-pp-doclayoutv3"

MODEL_ID = "PaddlePaddle/PP-DocLayoutV3_safetensors"
# Pin the official Transformers conversion used when this runner was written.
MODEL_REVISION = "f6f3f2b438702c53e94cfd535c2ea05aafb7985f"
TEXT_LABELS = frozenset(
    {
        "abstract",
        "algorithm",
        "aside_text",
        "content",
        "doc_title",
        "figure_title",
        "footer",
        "footnote",
        "header",
        "number",
        "paragraph_title",
        "reference",
        "reference_content",
        "text",
        "vision_footnote",
    }
)
IMAGE_EXTENSIONS = frozenset({".jpg", ".jpeg", ".png", ".webp"})


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--annotation", type=Path, default=DEFAULT_ANNOTATION)
    parser.add_argument("--images", type=Path, default=DEFAULT_IMAGES)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--model", default=MODEL_ID)
    parser.add_argument("--revision", default=MODEL_REVISION)
    parser.add_argument("--device", default="cuda:0")
    parser.add_argument(
        "--dtype", choices=("float32", "float16", "bfloat16"), default="bfloat16"
    )
    parser.add_argument("--batch-size", type=int, default=8)
    parser.add_argument("--workers", type=int, default=min(24, os.cpu_count() or 1))
    parser.add_argument(
        "--minimum-score",
        type=float,
        default=0.45,
        help="Retain predictions down to this score; the builder treats lower-confidence records as ignore regions.",
    )
    parser.add_argument("--limit", type=int)
    parser.add_argument("--num-shards", type=int, default=1)
    parser.add_argument("--shard-index", type=int, default=0)
    parser.add_argument("--overwrite", action="store_true")
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(
        json.dumps(payload, ensure_ascii=False, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, path)


def safe_relative_path(value: str) -> Path:
    relative = Path(value.replace("\\", "/"))
    if (
        relative.is_absolute()
        or not relative.parts
        or any(part in {"", ".", ".."} for part in relative.parts)
    ):
        raise ValueError(f"unsafe image path in COCO annotation: {value!r}")
    if relative.suffix.lower() not in IMAGE_EXTENSIONS:
        raise ValueError(f"unsupported image type: {value!r}")
    return relative


def read_image_records(annotation: Path) -> list[tuple[Path, int, int]]:
    records: list[tuple[Path, int, int]] = []
    with annotation.open("rb") as file:
        for image in ijson.items(file, "images.item", use_float=True):
            relative = safe_relative_path(str(image["file_name"]))
            records.append((relative, int(image["width"]), int(image["height"])))
    records.sort(key=lambda record: record[0].as_posix())
    if len(records) != len({record[0] for record in records}):
        raise ValueError(f"duplicate image file names in {annotation}")
    return records


def record_path(output: Path, relative: Path) -> Path:
    return output / "records" / relative.with_suffix(".json")


def load_rgb(
    item: tuple[Path, int, int], images: Path
) -> tuple[Path, Image.Image, int, int]:
    relative, expected_width, expected_height = item
    path = images / relative
    with Image.open(path) as source:
        source.load()
        image = source.convert("RGB")
    if image.size != (expected_width, expected_height):
        raise ValueError(
            f"COCO/image size mismatch for {path}: expected "
            f"{expected_width}x{expected_height}, got {image.width}x{image.height}"
        )
    return relative, image, expected_width, expected_height


def bounded_load(
    executor: concurrent.futures.ThreadPoolExecutor,
    items: Iterable[tuple[Path, int, int]],
    images: Path,
    prefetch: int,
) -> Iterable[tuple[Path, Image.Image, int, int]]:
    iterator = iter(items)
    pending: deque[concurrent.futures.Future[tuple[Path, Image.Image, int, int]]] = (
        deque()
    )
    for item in iterator:
        pending.append(executor.submit(load_rgb, item, images))
        if len(pending) >= prefetch:
            yield pending.popleft().result()
    while pending:
        yield pending.popleft().result()


def clean_polygon(value: Any, width: int, height: int) -> list[list[float]] | None:
    if value is None:
        return None
    points = np.asarray(value, dtype=np.float32).reshape(-1, 2)
    if len(points) < 3 or not np.isfinite(points).all():
        return None
    points[:, 0] = np.clip(points[:, 0], 0, width)
    points[:, 1] = np.clip(points[:, 1], 0, height)
    shifted = np.roll(points, -1, axis=0)
    area = abs(
        float(np.sum(points[:, 0] * shifted[:, 1] - points[:, 1] * shifted[:, 0]) * 0.5)
    )
    if area < 4.0:
        return None
    return [[round(float(x), 3), round(float(y), 3)] for x, y in points]


def tensor_list(value: torch.Tensor) -> list[Any]:
    return value.detach().float().cpu().tolist()


def serialize_result(
    result: dict[str, Any],
    id2label: dict[int, str],
    width: int,
    height: int,
) -> list[dict[str, Any]]:
    scores = tensor_list(result["scores"])
    labels = result["labels"].detach().cpu().tolist()
    boxes = tensor_list(result["boxes"])
    orders = result["order_seq"].detach().cpu().tolist()
    polygons = result["polygon_points"]
    predictions: list[dict[str, Any]] = []
    for score, label_id, box, order, raw_polygon in zip(
        scores, labels, boxes, orders, polygons, strict=True
    ):
        label = str(id2label[int(label_id)])
        if label not in TEXT_LABELS:
            continue
        polygon = clean_polygon(raw_polygon, width, height)
        if polygon is None:
            continue
        x1, y1, x2, y2 = box
        clipped_box = [
            round(max(0.0, min(float(width), x1)), 3),
            round(max(0.0, min(float(height), y1)), 3),
            round(max(0.0, min(float(width), x2)), 3),
            round(max(0.0, min(float(height), y2)), 3),
        ]
        if clipped_box[2] <= clipped_box[0] or clipped_box[3] <= clipped_box[1]:
            continue
        predictions.append(
            {
                "label": label,
                "label_id": int(label_id),
                "score": round(float(score), 6),
                "order": int(order),
                "bbox_xyxy": clipped_box,
                "polygon": polygon,
            }
        )
    predictions.sort(key=lambda item: (item["order"], -item["score"]))
    return predictions


def select_predictions_for_cpu(
    outputs: Any,
    target_sizes: list[tuple[int, int]],
    threshold: float,
) -> list[dict[str, torch.Tensor]]:
    """Perform query selection on GPU and transfer only retained masks."""

    boxes = outputs.pred_boxes
    logits = outputs.logits
    order_scores = outputs.order_logits.sigmoid()
    masks = outputs.out_masks

    batch_size, sequence_length, _ = order_scores.shape
    order_votes = order_scores.triu(diagonal=1).sum(dim=1) + (
        1.0 - order_scores.transpose(1, 2)
    ).tril(diagonal=-1).sum(dim=1)
    order_pointers = torch.argsort(order_votes, dim=1)
    order_sequences = torch.empty_like(order_pointers)
    ranks = torch.arange(
        sequence_length,
        device=order_pointers.device,
        dtype=order_pointers.dtype,
    ).expand(batch_size, -1)
    order_sequences.scatter_(1, order_pointers, ranks)

    centers, dimensions = boxes.split(2, dim=-1)
    boxes = torch.cat((centers - 0.5 * dimensions, centers + 0.5 * dimensions), dim=-1)
    sizes = torch.as_tensor(target_sizes, device=boxes.device)
    image_height, image_width = sizes.unbind(1)
    scale = torch.stack((image_width, image_height, image_width, image_height), dim=1)
    boxes = boxes * scale[:, None, :]

    num_queries = logits.shape[1]
    num_classes = logits.shape[2]
    scores, indexes = torch.topk(logits.sigmoid().flatten(1), num_queries, dim=-1)
    labels = indexes % num_classes
    query_indexes = indexes // num_classes
    boxes = boxes.gather(1, query_indexes.unsqueeze(-1).expand(-1, -1, 4))
    masks = masks.gather(
        1,
        query_indexes[:, :, None, None].expand(
            -1, -1, masks.shape[-2], masks.shape[-1]
        ),
    )
    order_sequences = order_sequences.gather(1, query_indexes)

    keep_matrix = scores >= threshold
    counts = keep_matrix.sum(dim=1).detach().cpu().tolist()
    selected_gpu: list[dict[str, torch.Tensor]] = []
    for image_index in range(batch_size):
        keep = keep_matrix[image_index]
        selected_order, order_index = torch.sort(order_sequences[image_index, keep])
        selected_gpu.append(
            {
                "scores": scores[image_index, keep][order_index],
                "labels": labels[image_index, keep][order_index],
                "boxes": boxes[image_index, keep][order_index],
                "order_seq": selected_order,
                "masks": (
                    masks[image_index, keep][order_index].sigmoid() > threshold
                ).to(torch.uint8),
            }
        )

    total = sum(counts)
    if total == 0:
        return [
            {
                "scores": torch.empty(0),
                "labels": torch.empty(0, dtype=torch.long),
                "boxes": torch.empty((0, 4)),
                "order_seq": torch.empty(0, dtype=torch.long),
                "masks": torch.empty(
                    (0, masks.shape[-2], masks.shape[-1]), dtype=torch.uint8
                ),
            }
            for _ in target_sizes
        ]

    combined = {
        key: torch.cat([selected[key] for selected in selected_gpu]).detach().cpu()
        for key in ("scores", "labels", "boxes", "order_seq", "masks")
    }
    selected_cpu: list[dict[str, torch.Tensor]] = []
    offset = 0
    for count in counts:
        selected_cpu.append(
            {key: value[offset : offset + count] for key, value in combined.items()}
        )
        offset += count
    return selected_cpu


def write_prediction_record(
    processor: Any,
    id2label: dict[int, str],
    output: Path,
    relative: Path,
    width: int,
    height: int,
    selected: dict[str, torch.Tensor],
) -> int:
    """Extract polygons and write one record on a CPU worker."""

    boxes = selected["boxes"].float().numpy()
    masks = selected["masks"].numpy()
    polygons = processor._extract_polygon_points_by_masks(
        boxes,
        masks,
        [processor.size["width"] / width, processor.size["height"] / height],
    )
    predictions = serialize_result(
        {**selected, "polygon_points": polygons},
        id2label,
        width,
        height,
    )
    atomic_write_json(
        record_path(output, relative),
        {
            "schema_version": 1,
            "image": relative.as_posix(),
            "width": width,
            "height": height,
            "predictions": predictions,
        },
    )
    return len(predictions)


def infer_batch(
    model: torch.nn.Module,
    processor: Any,
    batch: list[tuple[Path, Image.Image, int, int]],
    device: torch.device,
    dtype: torch.dtype,
    threshold: float,
) -> list[dict[str, torch.Tensor]]:
    inputs = processor(images=[item[1] for item in batch], return_tensors="pt")
    inputs = {
        key: value.to(device, non_blocking=True)
        for key, value in inputs.items()
        if isinstance(value, torch.Tensor)
    }
    with (
        torch.inference_mode(),
        torch.autocast(
            device_type=device.type,
            dtype=dtype,
            enabled=device.type == "cuda" and dtype != torch.float32,
        ),
    ):
        outputs = model(**inputs)
        return select_predictions_for_cpu(
            outputs,
            [(item[3], item[2]) for item in batch],
            threshold,
        )


def main() -> None:
    args = parse_args()
    annotation = args.annotation.resolve()
    images = args.images.resolve()
    output = args.output.resolve()
    if not annotation.is_file():
        raise FileNotFoundError(annotation)
    if not images.is_dir():
        raise FileNotFoundError(images)
    if args.batch_size <= 0 or args.workers <= 0:
        raise ValueError("--batch-size and --workers must be positive")
    if not 0.0 < args.minimum_score < 1.0:
        raise ValueError("--minimum-score must be between zero and one")
    if args.num_shards <= 0 or not 0 <= args.shard_index < args.num_shards:
        raise ValueError(
            "require --num-shards > 0 and 0 <= --shard-index < --num-shards"
        )

    device = torch.device(args.device)
    if device.type == "cuda" and not torch.cuda.is_available():
        raise RuntimeError("CUDA was requested but PyTorch cannot access it")
    dtype = {
        "float32": torch.float32,
        "float16": torch.float16,
        "bfloat16": torch.bfloat16,
    }[args.dtype]
    if device.type == "cpu" and dtype != torch.float32:
        raise ValueError("CPU inference requires --dtype float32")

    records = read_image_records(annotation)
    if args.limit is not None:
        records = records[: args.limit]
    records = records[args.shard_index :: args.num_shards]
    for relative, _, _ in records:
        if not (images / relative).is_file():
            raise FileNotFoundError(images / relative)
    if not args.overwrite:
        records = [
            item for item in records if not record_path(output, item[0]).is_file()
        ]

    torch.set_float32_matmul_precision("high")
    processor = AutoImageProcessor.from_pretrained(args.model, revision=args.revision)
    model = AutoModelForObjectDetection.from_pretrained(
        args.model,
        revision=args.revision,
        dtype=dtype,
    )
    model.eval().to(device)
    if device.type == "cuda":
        model.to(memory_format=torch.channels_last)
        torch.backends.cudnn.benchmark = True
        torch.cuda.reset_peak_memory_stats(device)

    model_metadata = {
        "schema_version": 1,
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "runtime": "PyTorch/Transformers",
        "model": args.model,
        "revision": args.revision,
        "torch": torch.__version__,
        "device": str(device),
        "dtype": args.dtype,
        "minimum_score": args.minimum_score,
        "text_labels": sorted(TEXT_LABELS),
    }
    output.mkdir(parents=True, exist_ok=True)
    if args.num_shards == 1 or args.shard_index == 0:
        atomic_write_json(output / "teacher_model.json", model_metadata)

    started = time.perf_counter()
    written = 0
    prediction_count = 0
    loader = concurrent.futures.ThreadPoolExecutor(
        max_workers=args.workers,
        thread_name_prefix="pp-image-loader",
    )
    writer = concurrent.futures.ThreadPoolExecutor(
        max_workers=args.workers,
        thread_name_prefix="pp-polygon-writer",
    )
    loaded = bounded_load(
        loader,
        records,
        images,
        prefetch=max(args.batch_size * 4, args.workers * 2),
    )
    batch: list[tuple[Path, Image.Image, int, int]] = []
    pending_writes: set[concurrent.futures.Future[int]] = set()
    maximum_pending_writes = max(args.batch_size * 4, args.workers * 8)
    progress = tqdm(total=len(records), desc="PP-DocLayoutV3", unit="page")

    def collect_writes(*, wait_for_one: bool) -> None:
        nonlocal written, prediction_count
        if not pending_writes:
            return
        if wait_for_one:
            done, _ = concurrent.futures.wait(
                pending_writes,
                return_when=concurrent.futures.FIRST_COMPLETED,
            )
        else:
            done = {future for future in pending_writes if future.done()}
        for future in done:
            pending_writes.remove(future)
            prediction_count += future.result()
            written += 1
            progress.update(1)

    def submit_batch(
        current_batch: list[tuple[Path, Image.Image, int, int]],
    ) -> None:
        selected_batch = infer_batch(
            model,
            processor,
            current_batch,
            device,
            dtype,
            args.minimum_score,
        )
        for (relative, _, width, height), selected in zip(
            current_batch, selected_batch, strict=True
        ):
            pending_writes.add(
                writer.submit(
                    write_prediction_record,
                    processor,
                    dict(model.config.id2label),
                    output,
                    relative,
                    width,
                    height,
                    selected,
                )
            )
        collect_writes(wait_for_one=False)
        while len(pending_writes) >= maximum_pending_writes:
            collect_writes(wait_for_one=True)

    try:
        for item in loaded:
            batch.append(item)
            if len(batch) < args.batch_size:
                continue
            submit_batch(batch)
            batch = []

        if batch:
            submit_batch(batch)

        while pending_writes:
            collect_writes(wait_for_one=True)
    finally:
        loader.shutdown(wait=True, cancel_futures=True)
        writer.shutdown(wait=True, cancel_futures=True)
        progress.close()

    if device.type == "cuda":
        torch.cuda.synchronize(device)
    elapsed = time.perf_counter() - started
    summary = {
        **model_metadata,
        "annotation": str(annotation),
        "images": str(images),
        "num_shards": args.num_shards,
        "shard_index": args.shard_index,
        "written_this_run": written,
        "normal_text_predictions": prediction_count,
        "seconds": round(elapsed, 3),
        "pages_per_second": round(written / max(elapsed, 1e-9), 3),
        "peak_cuda_memory_gib": (
            round(torch.cuda.max_memory_allocated(device) / 1024**3, 3)
            if device.type == "cuda"
            else None
        ),
    }
    summary_name = (
        "summary.json"
        if args.num_shards == 1
        else f"summary-shard-{args.shard_index:03d}-of-{args.num_shards:03d}.json"
    )
    atomic_write_json(output / summary_name, summary)
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
