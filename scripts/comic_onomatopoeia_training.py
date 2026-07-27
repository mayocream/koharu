"""Shared data and evaluation helpers for comic onomatopoeia training."""

from __future__ import annotations

import hashlib
import json
import math
import os
import unicodedata
from collections import defaultdict
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from pathlib import Path
from typing import Any, Iterable

import numpy as np
from PIL import Image
from tqdm import tqdm


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DATASET = REPOSITORY_ROOT / "data" / "datasets" / "comic-onomatopoeia-verifier"
DEFAULT_IMAGE_ROOT = (
    REPOSITORY_ROOT / "data" / "Manga109_released_2021_12_30" / "images"
)
CHARACTER_SET_PATH = (
    REPOSITORY_ROOT
    / "crates"
    / "koharu-ml"
    / "src"
    / "comic_onomatopoeia"
    / "recognizer"
    / "character_set.txt"
)
SPECIAL_TOKENS = ("[PAD]", "[UNK]", "[SOS]", "[EOS]", " ")
PAD_INDEX = 0
UNK_INDEX = 1
SOS_INDEX = 2
EOS_INDEX = 3
MAX_TEXT_LENGTH = 25
IMAGE_SIZE = 100


def load_records(dataset: Path, split: str) -> list[dict[str, Any]]:
    manifest = dataset / "balanced" / f"{split}.jsonl"
    with manifest.open("r", encoding="utf-8") as handle:
        return [json.loads(line) for line in handle if line.strip()]


def image_root_from_audit(dataset: Path) -> Path:
    audit_path = dataset / "audit.json"
    if audit_path.exists():
        audit = json.loads(audit_path.read_text(encoding="utf-8"))
        image_root = Path(audit["sources"]["image_root"])
        if image_root.exists():
            return image_root
    return DEFAULT_IMAGE_ROOT


def manifest_sha256(dataset: Path, split: str) -> str:
    manifest = dataset / "balanced" / f"{split}.jsonl"
    digest = hashlib.sha256()
    with manifest.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def ensure_crop_cache(
    dataset: Path,
    split: str,
    records: list[dict[str, Any]],
    workers: int,
) -> np.ndarray:
    """Build a deterministic uint8 NHWC cache while opening each page only once."""

    cache_dir = dataset / "cache" / f"{IMAGE_SIZE}x{IMAGE_SIZE}"
    cache_dir.mkdir(parents=True, exist_ok=True)
    cache_path = cache_dir / f"{split}.npy"
    metadata_path = cache_dir / f"{split}.json"
    expected = {
        "schema_version": 1,
        "manifest_sha256": manifest_sha256(dataset, split),
        "records": len(records),
        "shape": [len(records), IMAGE_SIZE, IMAGE_SIZE, 3],
        "resize": "Pillow.BICUBIC",
    }
    if cache_path.exists() and metadata_path.exists():
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        if metadata == expected:
            return np.load(cache_path, mmap_mode="r")

    image_root = image_root_from_audit(dataset)
    by_image: dict[str, list[tuple[int, list[int]]]] = defaultdict(list)
    for index, record in enumerate(records):
        by_image[record["image"]].append((index, record["crop_box_xyxy"]))

    cache = np.lib.format.open_memmap(
        cache_path,
        mode="w+",
        dtype=np.uint8,
        shape=(len(records), IMAGE_SIZE, IMAGE_SIZE, 3),
    )

    def crop_page(
        item: tuple[str, list[tuple[int, list[int]]]],
    ) -> list[tuple[int, np.ndarray]]:
        relative_path, crops = item
        path = image_root / Path(relative_path)
        with Image.open(path) as page:
            page = page.convert("RGB")
            return [
                (
                    index,
                    np.asarray(
                        page.crop(tuple(box)).resize(
                            (IMAGE_SIZE, IMAGE_SIZE), Image.Resampling.BICUBIC
                        ),
                        dtype=np.uint8,
                    ),
                )
                for index, box in crops
            ]

    items = iter(sorted(by_image.items()))
    with ThreadPoolExecutor(max_workers=max(1, workers)) as executor:
        pending = set()
        for _ in range(max(1, workers) * 2):
            try:
                pending.add(executor.submit(crop_page, next(items)))
            except StopIteration:
                break
        with tqdm(
            total=len(by_image), desc=f"cache {split} crops", unit="page"
        ) as progress:
            while pending:
                completed, pending = wait(pending, return_when=FIRST_COMPLETED)
                for future in completed:
                    for index, crop in future.result():
                        cache[index] = crop
                    progress.update()
                    try:
                        pending.add(executor.submit(crop_page, next(items)))
                    except StopIteration:
                        pass
    cache.flush()
    metadata_path.write_text(
        json.dumps(expected, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return np.load(cache_path, mmap_mode="r")


def load_tokens() -> tuple[list[str], dict[str, int]]:
    characters = CHARACTER_SET_PATH.read_text(encoding="utf-8-sig").strip()
    tokens = [*SPECIAL_TOKENS, *characters]
    if len(tokens) != 187:
        raise RuntimeError(f"expected 187 TRBA tokens, found {len(tokens)}")
    return tokens, {token: index for index, token in enumerate(tokens)}


def encode_texts(
    texts: Iterable[str], token_to_index: dict[str, int], length: int = MAX_TEXT_LENGTH
) -> np.ndarray:
    texts = list(texts)
    encoded = np.full((len(texts), length), PAD_INDEX, dtype=np.int64)
    for row, text in enumerate(texts):
        for column, character in enumerate(text[:length]):
            encoded[row, column] = token_to_index.get(character, UNK_INDEX)
    return encoded


def encode_recognizer_targets(
    texts: Iterable[str], token_to_index: dict[str, int]
) -> np.ndarray:
    texts = list(texts)
    encoded = np.full((len(texts), MAX_TEXT_LENGTH + 2), PAD_INDEX, dtype=np.int64)
    encoded[:, 0] = SOS_INDEX
    for row, text in enumerate(texts):
        values = [token_to_index.get(character, UNK_INDEX) for character in text]
        values = values[:MAX_TEXT_LENGTH]
        encoded[row, 1 : 1 + len(values)] = values
        encoded[row, 1 + len(values)] = EOS_INDEX
    return encoded


def normalize_text(text: str) -> str:
    return "".join(
        character
        for character in unicodedata.normalize("NFKC", text)
        if not character.isspace()
    )


def edit_distance(left: str, right: str) -> int:
    if len(left) < len(right):
        left, right = right, left
    previous = list(range(len(right) + 1))
    for row, left_character in enumerate(left, start=1):
        current = [row]
        for column, right_character in enumerate(right, start=1):
            current.append(
                min(
                    current[-1] + 1,
                    previous[column] + 1,
                    previous[column - 1] + (left_character != right_character),
                )
            )
        previous = current
    return previous[-1]


def character_error_rate(prediction: str, target: str) -> float:
    prediction = normalize_text(prediction)
    target = normalize_text(target)
    if not target:
        return 0.0 if not prediction else 1.0
    return edit_distance(prediction, target) / len(target)


def binary_metrics(
    labels: np.ndarray, probabilities: np.ndarray, threshold: float
) -> dict[str, float | int]:
    labels = labels.astype(np.int64)
    predictions = probabilities >= threshold
    true_positive = int(np.sum((predictions == 1) & (labels == 1)))
    true_negative = int(np.sum((predictions == 0) & (labels == 0)))
    false_positive = int(np.sum((predictions == 1) & (labels == 0)))
    false_negative = int(np.sum((predictions == 0) & (labels == 1)))
    precision = true_positive / max(1, true_positive + false_positive)
    recall = true_positive / max(1, true_positive + false_negative)
    f1 = 2 * precision * recall / max(1e-12, precision + recall)
    return {
        "samples": int(len(labels)),
        "threshold": float(threshold),
        "accuracy": float((true_positive + true_negative) / max(1, len(labels))),
        "precision": float(precision),
        "recall": float(recall),
        "f1": float(f1),
        "auroc": float(binary_auroc(labels, probabilities)),
        "true_positive": true_positive,
        "true_negative": true_negative,
        "false_positive": false_positive,
        "false_negative": false_negative,
    }


def binary_auroc(labels: np.ndarray, probabilities: np.ndarray) -> float:
    positive = int(np.sum(labels == 1))
    negative = int(np.sum(labels == 0))
    if positive == 0 or negative == 0:
        return float("nan")
    order = np.argsort(probabilities, kind="stable")
    ranks = np.empty(len(order), dtype=np.float64)
    ranks[order] = np.arange(1, len(order) + 1, dtype=np.float64)
    sorted_probabilities = probabilities[order]
    start = 0
    while start < len(order):
        end = start + 1
        while (
            end < len(order)
            and sorted_probabilities[end] == sorted_probabilities[start]
        ):
            end += 1
        ranks[order[start:end]] = (start + 1 + end) / 2
        start = end
    rank_sum = float(ranks[labels == 1].sum())
    return (rank_sum - positive * (positive + 1) / 2) / (positive * negative)


def best_f1_threshold(labels: np.ndarray, probabilities: np.ndarray) -> float:
    best_threshold = 0.5
    best_f1 = -math.inf
    for threshold in np.linspace(0.05, 0.95, 181):
        f1 = float(binary_metrics(labels, probabilities, float(threshold))["f1"])
        if f1 > best_f1:
            best_f1 = f1
            best_threshold = float(threshold)
    return best_threshold


def batched_indices(
    indices: np.ndarray, batch_size: int, rng: np.random.Generator | None = None
) -> Iterable[np.ndarray]:
    indices = indices.copy()
    if rng is not None:
        rng.shuffle(indices)
    for start in range(0, len(indices), batch_size):
        yield indices[start : start + batch_size]


def torch_images(crops: np.ndarray, indices: np.ndarray, device: Any) -> Any:
    import torch

    # Advanced indexing returns a writable contiguous copy instead of a read-only memmap view.
    values = np.asarray(crops[indices]).copy()
    return (
        torch.from_numpy(values)
        .to(device=device, non_blocking=True)
        .permute(0, 3, 1, 2)
        .float()
        .div_(127.5)
        .sub_(1.0)
    )


def atomic_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    os.replace(temporary, path)
