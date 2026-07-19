# /// script
# requires-python = ">=3.12,<3.13"
# dependencies = [
#   "numpy==2.4.1",
#   "packaging==26.0",
#   "pillow==12.1.0",
#   "safetensors==0.7.0",
#   "torch==2.9.1",
#   "tqdm==4.67.3",
# ]
#
# [tool.uv.sources]
# torch = { index = "pytorch-cu130" }
#
# [[tool.uv.index]]
# name = "pytorch-cu130"
# url = "https://download.pytorch.org/whl/cu130"
# explicit = true
# ///
"""Train a small OCR-agnostic crop/candidate verifier.

The two heads answer independent questions: whether the crop visually resembles
comic onomatopoeia, and whether a supplied OCR string meaningfully agrees with
the crop. No OCR-engine-specific hidden states or vocabulary IDs are consumed.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import random
import time
from collections import OrderedDict
from pathlib import Path
from typing import Any

import numpy as np
import torch
from safetensors.torch import load_file, save_file
from torch import nn
from torch.nn import functional as F
from tqdm import tqdm

from comic_onomatopoeia_training import (
    DEFAULT_DATASET,
    MAX_TEXT_LENGTH,
    REPOSITORY_ROOT,
    atomic_json,
    batched_indices,
    best_f1_threshold,
    binary_metrics,
    character_error_rate,
    encode_texts,
    ensure_crop_cache,
    load_records,
    load_tokens,
    normalize_text,
    torch_images,
)


DEFAULT_RECOGNIZER_OUTPUT = (
    REPOSITORY_ROOT / "data" / "models" / "comic-onomatopoeia" / "trba-finetuned"
)
DEFAULT_OUTPUT = REPOSITORY_ROOT / "data" / "models" / "comic-onomatopoeia" / "verifier"
MEANINGFUL_CER = 0.25
UNUSABLE_CER = 0.60


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=DEFAULT_DATASET)
    parser.add_argument(
        "--recognizer-output", type=Path, default=DEFAULT_RECOGNIZER_OUTPUT
    )
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--epochs", type=int, default=10)
    parser.add_argument("--batch-size", type=int, default=512)
    parser.add_argument("--learning-rate", type=float, default=3e-4)
    parser.add_argument("--weight-decay", type=float, default=1e-4)
    parser.add_argument("--workers", type=int, default=16)
    parser.add_argument("--seed", type=int, default=20260720)
    parser.add_argument("--device", default="cuda")
    parser.add_argument("--no-amp", action="store_true")
    parser.add_argument(
        "--limit",
        type=int,
        help="Limit each split for smoke testing; never use for reported metrics.",
    )
    return parser.parse_args()


class ImageEncoder(nn.Module):
    def __init__(self) -> None:
        super().__init__()
        self.conv1 = nn.Conv2d(3, 24, 3, stride=2, padding=1, bias=False)
        self.norm1 = nn.GroupNorm(4, 24)
        self.conv2 = nn.Conv2d(24, 32, 3, stride=2, padding=1, bias=False)
        self.norm2 = nn.GroupNorm(4, 32)
        self.conv3 = nn.Conv2d(32, 64, 3, stride=2, padding=1, bias=False)
        self.norm3 = nn.GroupNorm(8, 64)
        self.conv4 = nn.Conv2d(64, 96, 3, stride=2, padding=1, bias=False)
        self.norm4 = nn.GroupNorm(8, 96)
        self.conv5 = nn.Conv2d(96, 128, 3, stride=2, padding=1, bias=False)
        self.norm5 = nn.GroupNorm(8, 128)
        self.projection = nn.Linear(128, 96)

    def forward(self, images: torch.Tensor) -> torch.Tensor:
        hidden = F.silu(self.norm1(self.conv1(images)))
        hidden = F.silu(self.norm2(self.conv2(hidden)))
        hidden = F.silu(self.norm3(self.conv3(hidden)))
        hidden = F.silu(self.norm4(self.conv4(hidden)))
        hidden = F.silu(self.norm5(self.conv5(hidden)))
        hidden = F.adaptive_avg_pool2d(hidden, (1, 1)).flatten(1)
        return self.projection(hidden)


class TextEncoder(nn.Module):
    def __init__(self) -> None:
        super().__init__()
        self.embedding = nn.Embedding(187, 32, padding_idx=0)
        self.conv1 = nn.Conv1d(32, 64, 3, padding=1, bias=False)
        self.norm1 = nn.GroupNorm(8, 64)
        self.conv2 = nn.Conv1d(64, 64, 3, padding=1, bias=False)
        self.norm2 = nn.GroupNorm(8, 64)
        self.projection = nn.Linear(128, 96)

    def forward(self, token_ids: torch.Tensor) -> torch.Tensor:
        mask = token_ids.ne(0).unsqueeze(1)
        hidden = self.embedding(token_ids).transpose(1, 2)
        hidden = F.silu(self.norm1(self.conv1(hidden)))
        hidden = F.silu(self.norm2(self.conv2(hidden)))
        mask_float = mask.to(hidden.dtype)
        mean = (hidden * mask_float).sum(dim=2) / mask_float.sum(dim=2).clamp_min(1)
        maximum = hidden.masked_fill(~mask, -10_000.0).amax(dim=2)
        maximum = torch.where(mask.any(dim=2), maximum, torch.zeros_like(maximum))
        return self.projection(torch.cat((mean, maximum), dim=1))


class Verifier(nn.Module):
    def __init__(self) -> None:
        super().__init__()
        self.image_encoder = ImageEncoder()
        self.text_encoder = TextEncoder()
        self.is_onomatopoeia_head = nn.Linear(96, 1)
        self.match_head = nn.Sequential(
            OrderedDict(
                (
                    ("linear1", nn.Linear(96 * 4, 128)),
                    ("activation", nn.SiLU()),
                    ("dropout", nn.Dropout(0.1)),
                    ("linear2", nn.Linear(128, 1)),
                )
            )
        )

    def forward(
        self, images: torch.Tensor, token_ids: torch.Tensor
    ) -> tuple[torch.Tensor, torch.Tensor]:
        image_features = self.image_encoder(images)
        text_features = self.text_encoder(token_ids)
        fusion = torch.cat(
            (
                image_features,
                text_features,
                torch.abs(image_features - text_features),
                image_features * text_features,
            ),
            dim=1,
        )
        return (
            self.is_onomatopoeia_head(image_features).squeeze(1),
            self.match_head(fusion).squeeze(1),
        )


def load_predictions(path: Path) -> dict[str, dict[str, Any]]:
    if not path.exists():
        return {}
    with path.open("r", encoding="utf-8") as handle:
        return {
            value["sample_id"]: value
            for line in handle
            if line.strip()
            for value in (json.loads(line),)
        }


def corrupt_text(
    target: str,
    characters: list[str],
    rng: np.random.Generator,
    hard: bool,
) -> str:
    values = list(target)
    if not values:
        return characters[int(rng.integers(0, len(characters)))]
    edits = max(1, math.ceil(len(values) * (0.7 if hard else 0.2)))
    if not hard:
        edits = 1
    positions = rng.choice(len(values), size=min(edits, len(values)), replace=False)
    for position in np.atleast_1d(positions).tolist():
        replacement = characters[int(rng.integers(0, len(characters)))]
        while replacement == values[position] and len(characters) > 1:
            replacement = characters[int(rng.integers(0, len(characters)))]
        values[position] = replacement
    if hard and len(values) > 2 and rng.random() < 0.5:
        keep = max(1, len(values) // 3)
        values = values[:keep]
    return "".join(values)


def make_candidates(
    records: list[dict[str, Any]],
    indices: np.ndarray,
    predictions: dict[str, dict[str, Any]],
    characters: list[str],
    rng: np.random.Generator,
) -> tuple[list[str], np.ndarray, np.ndarray, np.ndarray]:
    candidates: list[str] = []
    visual_labels = np.zeros(len(indices), dtype=np.float32)
    match_labels = np.zeros(len(indices), dtype=np.float32)
    match_mask = np.zeros(len(indices), dtype=np.float32)
    for position, index in enumerate(indices.tolist()):
        record = records[index]
        target = record["target_text"]
        visual_labels[position] = float(record["label_is_onomatopoeia"])
        eligible = (
            target is not None
            and not record["is_truncated"]
            and 0 < len(target) <= MAX_TEXT_LENGTH
        )
        if not eligible:
            candidates.append(
                predictions.get(record["sample_id"], {}).get("prediction", "")
            )
            continue

        choice = rng.random()
        if choice < 0.35:
            candidate = target
            label = 1.0
        elif choice < 0.50 and len(normalize_text(target)) >= 4:
            candidate = corrupt_text(target, characters, rng, hard=False)
            if character_error_rate(candidate, target) <= MEANINGFUL_CER:
                label = 1.0
            else:
                candidate = target
                label = 1.0
        elif choice < 0.80:
            candidate = corrupt_text(target, characters, rng, hard=True)
            if character_error_rate(candidate, target) < UNUSABLE_CER:
                candidate = characters[int(rng.integers(0, len(characters)))] * max(
                    1, len(target)
                )
            label = 0.0
        else:
            result = predictions.get(record["sample_id"])
            if result is None:
                candidate = corrupt_text(target, characters, rng, hard=True)
                label = 0.0
            else:
                candidate = result["prediction"]
                cer = character_error_rate(candidate, target)
                if cer <= MEANINGFUL_CER:
                    label = 1.0
                elif cer >= UNUSABLE_CER:
                    label = 0.0
                else:
                    candidates.append(candidate)
                    continue
        candidates.append(candidate)
        match_labels[position] = label
        match_mask[position] = 1.0
    return candidates, visual_labels, match_labels, match_mask


def augment_images(images: torch.Tensor, rng: np.random.Generator) -> torch.Tensor:
    batch = len(images)
    contrast = torch.from_numpy(
        rng.uniform(0.9, 1.1, size=batch).astype(np.float32)
    ).to(images.device)
    brightness = torch.from_numpy(
        rng.uniform(-0.08, 0.08, size=batch).astype(np.float32)
    ).to(images.device)
    images = (
        images * contrast[:, None, None, None] + brightness[:, None, None, None]
    ).clamp(-1, 1)
    rotations = rng.integers(0, 4, size=batch)
    for rotation in (1, 2, 3):
        positions = np.flatnonzero(rotations == rotation)
        if len(positions):
            tensor_positions = torch.from_numpy(positions).to(images.device)
            images[tensor_positions] = torch.rot90(
                images[tensor_positions], rotation, dims=(2, 3)
            )
    return images


def train_epoch(
    model: Verifier,
    records: list[dict[str, Any]],
    crops: np.ndarray,
    predictions: dict[str, dict[str, Any]],
    token_to_index: dict[str, int],
    characters: list[str],
    optimizer: torch.optim.Optimizer,
    scaler: torch.amp.GradScaler,
    batch_size: int,
    device: torch.device,
    amp: bool,
    rng: np.random.Generator,
    epoch: int,
) -> dict[str, float]:
    model.train()
    indices = np.arange(len(records), dtype=np.int64)
    visual_total = 0.0
    match_total = 0.0
    batches = 0
    for batch in tqdm(
        batched_indices(indices, batch_size, rng),
        total=(len(indices) + batch_size - 1) // batch_size,
        desc=f"verifier epoch {epoch}",
        unit="batch",
    ):
        candidates, visual_labels, match_labels, match_mask = make_candidates(
            records, batch, predictions, characters, rng
        )
        images = augment_images(torch_images(crops, batch, device), rng)
        token_ids = torch.from_numpy(encode_texts(candidates, token_to_index)).to(
            device
        )
        visual_labels_tensor = torch.from_numpy(visual_labels).to(device)
        match_labels_tensor = torch.from_numpy(match_labels).to(device)
        match_mask_tensor = torch.from_numpy(match_mask).to(device)
        optimizer.zero_grad(set_to_none=True)
        with torch.autocast(device_type=device.type, dtype=torch.float16, enabled=amp):
            visual_logits, match_logits = model(images, token_ids)
            visual_loss = F.binary_cross_entropy_with_logits(
                visual_logits, visual_labels_tensor
            )
            raw_match_loss = F.binary_cross_entropy_with_logits(
                match_logits, match_labels_tensor, reduction="none"
            )
            match_loss = (
                raw_match_loss * match_mask_tensor
            ).sum() / match_mask_tensor.sum().clamp_min(1)
            loss = visual_loss + match_loss
        scaler.scale(loss).backward()
        scaler.unscale_(optimizer)
        nn.utils.clip_grad_norm_(model.parameters(), 5.0)
        scaler.step(optimizer)
        scaler.update()
        visual_total += float(visual_loss.detach())
        match_total += float(match_loss.detach())
        batches += 1
    return {
        "visual_loss": visual_total / max(1, batches),
        "match_loss": match_total / max(1, batches),
    }


@torch.inference_mode()
def evaluate(
    model: Verifier,
    records: list[dict[str, Any]],
    crops: np.ndarray,
    predictions: dict[str, dict[str, Any]],
    token_to_index: dict[str, int],
    characters: list[str],
    batch_size: int,
    device: torch.device,
    amp: bool,
    seed: int,
    description: str,
) -> dict[str, np.ndarray]:
    model.eval()
    visual_labels = []
    visual_probabilities = []
    real_labels = []
    real_probabilities = []
    synthetic_labels = []
    synthetic_probabilities = []
    rng = np.random.default_rng(seed)
    indices = np.arange(len(records), dtype=np.int64)
    for batch in tqdm(
        batched_indices(indices, batch_size),
        total=(len(indices) + batch_size - 1) // batch_size,
        desc=description,
        unit="batch",
    ):
        batch_records = [records[index] for index in batch.tolist()]
        target_candidates = [record["target_text"] or "" for record in batch_records]
        images = torch_images(crops, batch, device)
        target_ids = torch.from_numpy(
            encode_texts(target_candidates, token_to_index)
        ).to(device)
        with torch.autocast(device_type=device.type, dtype=torch.float16, enabled=amp):
            visual_logits, _ = model(images, target_ids)
        visual_probabilities.extend(visual_logits.sigmoid().float().cpu().tolist())
        visual_labels.extend(
            int(record["label_is_onomatopoeia"]) for record in batch_records
        )

        pair_positions = []
        pair_candidates = []
        pair_labels = []
        for position, record in enumerate(batch_records):
            target = record["target_text"]
            if (
                target is None
                or record["is_truncated"]
                or not 0 < len(target) <= MAX_TEXT_LENGTH
            ):
                continue
            result = predictions.get(record["sample_id"])
            if result is None:
                continue
            cer = character_error_rate(result["prediction"], target)
            if cer <= MEANINGFUL_CER:
                label = 1
            elif cer >= UNUSABLE_CER:
                label = 0
            else:
                continue
            pair_positions.append(position)
            pair_candidates.append(result["prediction"])
            pair_labels.append(label)
        if pair_positions:
            positions = torch.tensor(pair_positions, device=device)
            pair_ids = torch.from_numpy(
                encode_texts(pair_candidates, token_to_index)
            ).to(device)
            with torch.autocast(
                device_type=device.type, dtype=torch.float16, enabled=amp
            ):
                _, pair_logits = model(images[positions], pair_ids)
            real_probabilities.extend(pair_logits.sigmoid().float().cpu().tolist())
            real_labels.extend(pair_labels)

        synthetic_positions = []
        synthetic_candidates = []
        synthetic_batch_labels = []
        for position, record in enumerate(batch_records):
            target = record["target_text"]
            if (
                target is None
                or record["is_truncated"]
                or not 0 < len(target) <= MAX_TEXT_LENGTH
            ):
                continue
            synthetic_positions.extend((position, position))
            synthetic_candidates.append(target)
            synthetic_candidates.append(
                corrupt_text(target, characters, rng, hard=True)
            )
            synthetic_batch_labels.extend((1, 0))
        if synthetic_positions:
            positions = torch.tensor(synthetic_positions, device=device)
            synthetic_ids = torch.from_numpy(
                encode_texts(synthetic_candidates, token_to_index)
            ).to(device)
            with torch.autocast(
                device_type=device.type, dtype=torch.float16, enabled=amp
            ):
                _, synthetic_logits = model(images[positions], synthetic_ids)
            synthetic_probabilities.extend(
                synthetic_logits.sigmoid().float().cpu().tolist()
            )
            synthetic_labels.extend(synthetic_batch_labels)
    return {
        "visual_labels": np.asarray(visual_labels, dtype=np.int64),
        "visual_probabilities": np.asarray(visual_probabilities, dtype=np.float64),
        "real_labels": np.asarray(real_labels, dtype=np.int64),
        "real_probabilities": np.asarray(real_probabilities, dtype=np.float64),
        "synthetic_labels": np.asarray(synthetic_labels, dtype=np.int64),
        "synthetic_probabilities": np.asarray(
            synthetic_probabilities, dtype=np.float64
        ),
    }


def save_checkpoint(model: Verifier, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    state = {
        name: tensor.detach().cpu().contiguous()
        for name, tensor in model.state_dict().items()
    }
    temporary = path.with_suffix(".tmp.safetensors")
    save_file(
        state,
        temporary,
        metadata={
            "architecture": "KoharuComicOnomatopoeiaVerifier-v1",
            "input": "100x100 RGB crop plus up to 25 COO characters",
            "meaningful_cer": str(MEANINGFUL_CER),
            "unusable_cer": str(UNUSABLE_CER),
        },
    )
    os.replace(temporary, path)


def summarized_metrics(
    values: dict[str, np.ndarray],
    visual_threshold: float,
    match_threshold: float,
) -> dict[str, Any]:
    result = {
        "visual": binary_metrics(
            values["visual_labels"], values["visual_probabilities"], visual_threshold
        ),
        "synthetic_match": binary_metrics(
            values["synthetic_labels"],
            values["synthetic_probabilities"],
            match_threshold,
        ),
    }
    if len(values["real_labels"]):
        result["recognizer_match"] = binary_metrics(
            values["real_labels"], values["real_probabilities"], match_threshold
        )
    return result


def main() -> None:
    args = parse_args()
    if not torch.cuda.is_available() and args.device.startswith("cuda"):
        raise RuntimeError(
            "CUDA was requested but the CUDA PyTorch build is unavailable"
        )
    random.seed(args.seed)
    np.random.seed(args.seed)
    torch.manual_seed(args.seed)
    torch.cuda.manual_seed_all(args.seed)
    torch.backends.cudnn.benchmark = True
    device = torch.device(args.device)
    amp = device.type == "cuda" and not args.no_amp
    args.output.mkdir(parents=True, exist_ok=True)

    tokens, token_to_index = load_tokens()
    characters = tokens[5:]
    records: dict[str, list[dict[str, Any]]] = {}
    crops: dict[str, np.ndarray] = {}
    predictions: dict[str, dict[str, dict[str, Any]]] = {}
    for split in ("train", "val", "test"):
        full_records = load_records(args.dataset, split)
        crops[split] = ensure_crop_cache(
            args.dataset, split, full_records, args.workers
        )
        records[split] = (
            full_records[: args.limit] if args.limit is not None else full_records
        )
        if args.limit is not None:
            crops[split] = crops[split][: len(records[split])]
        predictions[split] = load_predictions(
            args.recognizer_output / "predictions" / f"{split}.jsonl"
        )

    model = Verifier().to(device)
    optimizer = torch.optim.AdamW(
        model.parameters(), lr=args.learning_rate, weight_decay=args.weight_decay
    )
    scheduler = torch.optim.lr_scheduler.CosineAnnealingLR(
        optimizer, T_max=args.epochs, eta_min=args.learning_rate * 0.01
    )
    scaler = torch.amp.GradScaler(device.type, enabled=amp)
    rng = np.random.default_rng(args.seed)
    checkpoint = args.output / "model.safetensors"
    history = []
    best_score = -math.inf
    started = time.time()

    for epoch in range(1, args.epochs + 1):
        losses = train_epoch(
            model,
            records["train"],
            crops["train"],
            predictions["train"],
            token_to_index,
            characters,
            optimizer,
            scaler,
            args.batch_size,
            device,
            amp,
            rng,
            epoch,
        )
        validation = evaluate(
            model,
            records["val"],
            crops["val"],
            predictions["val"],
            token_to_index,
            characters,
            args.batch_size,
            device,
            amp,
            args.seed + epoch,
            f"verifier validation {epoch}",
        )
        visual_threshold = best_f1_threshold(
            validation["visual_labels"], validation["visual_probabilities"]
        )
        match_labels = (
            validation["real_labels"]
            if len(validation["real_labels"])
            else validation["synthetic_labels"]
        )
        match_probabilities = (
            validation["real_probabilities"]
            if len(validation["real_labels"])
            else validation["synthetic_probabilities"]
        )
        match_threshold = best_f1_threshold(match_labels, match_probabilities)
        metrics = summarized_metrics(validation, visual_threshold, match_threshold)
        score = float(metrics["visual"]["auroc"]) + float(
            metrics.get("recognizer_match", metrics["synthetic_match"])["auroc"]
        )
        entry = {
            "epoch": epoch,
            "losses": losses,
            "learning_rate": optimizer.param_groups[0]["lr"],
            "metrics": metrics,
        }
        history.append(entry)
        print(json.dumps(entry, ensure_ascii=False))
        if score > best_score:
            best_score = score
            save_checkpoint(model, checkpoint)
        scheduler.step()

    state = load_file(checkpoint, device="cpu")
    model.load_state_dict(state, strict=True)
    model.to(device)
    validation = evaluate(
        model,
        records["val"],
        crops["val"],
        predictions["val"],
        token_to_index,
        characters,
        args.batch_size,
        device,
        amp,
        args.seed + 10_000,
        "best verifier validation",
    )
    visual_threshold = best_f1_threshold(
        validation["visual_labels"], validation["visual_probabilities"]
    )
    match_labels = (
        validation["real_labels"]
        if len(validation["real_labels"])
        else validation["synthetic_labels"]
    )
    match_probabilities = (
        validation["real_probabilities"]
        if len(validation["real_labels"])
        else validation["synthetic_probabilities"]
    )
    match_threshold = best_f1_threshold(match_labels, match_probabilities)
    test = evaluate(
        model,
        records["test"],
        crops["test"],
        predictions["test"],
        token_to_index,
        characters,
        args.batch_size,
        device,
        amp,
        args.seed + 20_000,
        "best verifier test",
    )
    metrics = {
        "schema_version": 1,
        "device": str(device),
        "gpu": torch.cuda.get_device_name(device) if device.type == "cuda" else None,
        "amp": amp,
        "seed": args.seed,
        "epochs": args.epochs,
        "batch_size": args.batch_size,
        "learning_rate": args.learning_rate,
        "parameters": sum(parameter.numel() for parameter in model.parameters()),
        "meaningful_cer": MEANINGFUL_CER,
        "unusable_cer": UNUSABLE_CER,
        "thresholds": {
            "is_onomatopoeia": visual_threshold,
            "ocr_meaningful": match_threshold,
        },
        "validation": summarized_metrics(validation, visual_threshold, match_threshold),
        "test": summarized_metrics(test, visual_threshold, match_threshold),
        "history": history,
        "elapsed_seconds": time.time() - started,
    }
    atomic_json(args.output / "metrics.json", metrics)
    atomic_json(
        args.output / "config.json",
        {
            "architecture": "KoharuComicOnomatopoeiaVerifier-v1",
            "image_size": 100,
            "max_text_length": MAX_TEXT_LENGTH,
            "num_tokens": 187,
            "meaningful_cer": MEANINGFUL_CER,
            "unusable_cer": UNUSABLE_CER,
            "thresholds": metrics["thresholds"],
        },
    )
    print(json.dumps(metrics, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
