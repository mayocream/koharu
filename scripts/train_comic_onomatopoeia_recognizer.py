# /// script
# requires-python = ">=3.12,<3.13"
# dependencies = [
#   "huggingface-hub==1.4.1",
#   "numpy==2.4.1",
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
"""Fine-tune and evaluate COO's exact TRBA+2D recognizer on joined manifests.

The architecture and training forward pass are imported from the commit-pinned
COO checkout. The resulting Safetensors keeps the original ``module.*`` names,
so it can be loaded directly by koharu-ml's strict Rust port.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import random
import subprocess
import sys
import time
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import numpy as np
import torch
from huggingface_hub import hf_hub_download
from safetensors.torch import load_file, save_file
from torch import nn
from tqdm import tqdm

from comic_onomatopoeia_training import (
    DEFAULT_DATASET,
    EOS_INDEX,
    MAX_TEXT_LENGTH,
    REPOSITORY_ROOT,
    SOS_INDEX,
    atomic_json,
    batched_indices,
    character_error_rate,
    encode_recognizer_targets,
    ensure_crop_cache,
    load_records,
    load_tokens,
    normalize_text,
    torch_images,
)


UPSTREAM_COMMIT = "d8028f015b8ce99a4dd798427342f97087529357"
HF_REPOSITORY = "mayocream/coo-comic-onomatopoeia-safetensors"
HF_WEIGHTS = "trba-rot-sar-hardroi-2d/model.safetensors"
DEFAULT_OUTPUT = (
    REPOSITORY_ROOT / "data" / "models" / "comic-onomatopoeia" / "trba-finetuned"
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=DEFAULT_DATASET)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--upstream-root", type=Path)
    parser.add_argument("--epochs", type=int, default=3)
    parser.add_argument("--batch-size", type=int, default=192)
    parser.add_argument("--eval-batch-size", type=int, default=128)
    parser.add_argument("--learning-rate", type=float, default=1e-4)
    parser.add_argument("--workers", type=int, default=16)
    parser.add_argument("--seed", type=int, default=20260720)
    parser.add_argument("--device", default="cuda")
    parser.add_argument("--no-amp", action="store_true")
    parser.add_argument("--skip-predictions", action="store_true")
    parser.add_argument(
        "--limit",
        type=int,
        help="Limit each split for smoke testing; never use for reported metrics.",
    )
    return parser.parse_args()


def find_upstream(requested: Path | None) -> Path:
    candidates = []
    if requested is not None:
        candidates.append(requested)
    candidates.extend(
        [
            REPOSITORY_ROOT / "temp" / "COO-Comic-Onomatopoeia",
            Path(os.environ.get("LOCALAPPDATA", ""))
            / "Temp"
            / "koharu-coo-upstream-d8028f0",
            REPOSITORY_ROOT
            / "data"
            / "models"
            / "upstream"
            / "COO-Comic-Onomatopoeia-d8028f0",
        ]
    )
    for candidate in candidates:
        trba = candidate / "TRBA"
        if (trba / "model.py").is_file() and (trba / "modules").is_dir():
            return candidate.resolve()

    destination = candidates[-1]
    destination.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            "git",
            "clone",
            "--filter=blob:none",
            "https://github.com/ku21fan/COO-Comic-Onomatopoeia.git",
            str(destination),
        ],
        check=True,
    )
    subprocess.run(
        ["git", "checkout", "--detach", UPSTREAM_COMMIT],
        cwd=destination,
        check=True,
    )
    return destination.resolve()


def import_upstream_model(upstream_root: Path) -> type[nn.Module]:
    trba = upstream_root / "TRBA"
    sys.path.insert(0, str(trba))
    spec = importlib.util.spec_from_file_location(
        "coo_upstream_trba_model", trba / "model.py"
    )
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to import upstream TRBA from {trba}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.Model


def model_options() -> SimpleNamespace:
    return SimpleNamespace(
        Transformation="TPS",
        FeatureExtraction="ResNet",
        SequenceModeling="BiLSTM",
        Prediction="Attn",
        num_fiducial=20,
        imgH=100,
        imgW=100,
        input_channel=3,
        output_channel=512,
        hidden_size=256,
        num_class=187,
        batch_max_length=MAX_TEXT_LENGTH,
        twoD=True,
    )


def load_pretrained(model: nn.Module) -> Path:
    path = Path(hf_hub_download(repo_id=HF_REPOSITORY, filename=HF_WEIGHTS))
    state = load_file(path, device="cpu")
    if all(name.startswith("module.") for name in state):
        state = {name.removeprefix("module."): tensor for name, tensor in state.items()}
    model.load_state_dict(state, strict=True)
    return path


def save_checkpoint(model: nn.Module, path: Path, metadata: dict[str, str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    state = {
        f"module.{name}": tensor.detach().cpu().contiguous()
        for name, tensor in model.state_dict().items()
    }
    temporary = path.with_suffix(".tmp.safetensors")
    save_file(state, temporary, metadata=metadata)
    os.replace(temporary, path)


def load_checkpoint(model: nn.Module, path: Path) -> None:
    state = load_file(path, device="cpu")
    state = {name.removeprefix("module."): tensor for name, tensor in state.items()}
    model.load_state_dict(state, strict=True)


def recognition_indices(records: list[dict[str, Any]]) -> np.ndarray:
    return np.asarray(
        [
            index
            for index, record in enumerate(records)
            if record["label_is_onomatopoeia"]
            and not record["is_truncated"]
            and record["target_text"]
            and len(record["target_text"]) <= MAX_TEXT_LENGTH
        ],
        dtype=np.int64,
    )


def rotate_training_images(
    images: torch.Tensor,
    records: list[dict[str, Any]],
    indices: np.ndarray,
) -> torch.Tensor:
    rotate = []
    for position, index in enumerate(indices.tolist()):
        record = records[index]
        left, top, right, bottom = record["crop_box_xyxy"]
        if len(record["target_text"]) >= 3 and bottom - top > right - left:
            rotate.append(position)
    if rotate:
        positions = torch.tensor(rotate, device=images.device)
        images[positions] = torch.rot90(images[positions], 1, dims=(2, 3))
    return images


def decode_predictions(
    logits: torch.Tensor,
    tokens: list[str],
    original_batch_size: int,
) -> tuple[list[str], list[float], list[int]]:
    probabilities = logits.softmax(dim=-1)
    maximum_probabilities, indices = probabilities.max(dim=-1)
    indices = indices.cpu().tolist()
    maximum_probabilities = maximum_probabilities.float().cpu().tolist()
    candidates: list[list[tuple[str, float, int]]] = [
        [] for _ in range(original_batch_size)
    ]
    for rotation, degrees in enumerate((0, 90, 270)):
        offset = rotation * original_batch_size
        for position in range(original_batch_size):
            token_indices = indices[offset + position]
            token_probabilities = maximum_probabilities[offset + position]
            try:
                end = token_indices.index(EOS_INDEX)
            except ValueError:
                end = len(token_indices)
            confidence = float(np.prod(token_probabilities[:end])) if end else 0.0
            text = "".join(tokens[index] for index in token_indices[:end])
            candidates[position].append((text, confidence, degrees))
    texts = []
    confidences = []
    rotations = []
    for values in candidates:
        best = max(range(3), key=lambda index: values[index][1])
        text, confidence, degrees = values[best]
        texts.append(text)
        confidences.append(confidence)
        rotations.append(degrees)
    return texts, confidences, rotations


@torch.inference_mode()
def predict_batch(
    model: nn.Module,
    images: torch.Tensor,
    records: list[dict[str, Any]],
    indices: np.ndarray,
    tokens: list[str],
    amp: bool,
) -> tuple[list[str], list[float], list[int]]:
    rotated_90 = images.clone()
    rotated_270 = images.clone()
    vertical_positions = []
    for position, index in enumerate(indices.tolist()):
        left, top, right, bottom = records[index]["crop_box_xyxy"]
        if bottom - top > right - left:
            vertical_positions.append(position)
    if vertical_positions:
        positions = torch.tensor(vertical_positions, device=images.device)
        rotated_90[positions] = torch.rot90(images[positions], 1, dims=(2, 3))
        rotated_270[positions] = torch.rot90(images[positions], 3, dims=(2, 3))
    sar_images = torch.cat((images, rotated_90, rotated_270), dim=0)
    starts = torch.full(
        (len(sar_images),), SOS_INDEX, dtype=torch.long, device=images.device
    )
    with torch.autocast(
        device_type=images.device.type, dtype=torch.float16, enabled=amp
    ):
        logits = model(sar_images, starts, is_train=False)
    return decode_predictions(logits, tokens, len(images))


@torch.inference_mode()
def evaluate(
    model: nn.Module,
    records: list[dict[str, Any]],
    crops: np.ndarray,
    indices: np.ndarray,
    tokens: list[str],
    batch_size: int,
    device: torch.device,
    amp: bool,
    description: str,
) -> dict[str, float | int]:
    model.eval()
    exact = 0
    normalized_edit_distance = 0.0
    total_cer = 0.0
    for batch in tqdm(
        batched_indices(indices, batch_size),
        total=(len(indices) + batch_size - 1) // batch_size,
        desc=description,
        unit="batch",
    ):
        images = torch_images(crops, batch, device)
        predictions, _, _ = predict_batch(model, images, records, batch, tokens, amp)
        for prediction, index in zip(predictions, batch.tolist(), strict=True):
            target = records[index]["target_text"]
            prediction_normalized = normalize_text(prediction)
            target_normalized = normalize_text(target)
            distance = character_error_rate(prediction, target)
            exact += prediction_normalized == target_normalized
            total_cer += distance
            normalized_edit_distance += max(0.0, 1.0 - distance)
    return {
        "samples": int(len(indices)),
        "accuracy": exact / max(1, len(indices)),
        "mean_character_error_rate": total_cer / max(1, len(indices)),
        "normalized_edit_distance": normalized_edit_distance / max(1, len(indices)),
    }


def train_epoch(
    model: nn.Module,
    records: list[dict[str, Any]],
    crops: np.ndarray,
    indices: np.ndarray,
    token_to_index: dict[str, int],
    optimizer: torch.optim.Optimizer,
    scheduler: torch.optim.lr_scheduler.LRScheduler,
    scaler: torch.amp.GradScaler,
    batch_size: int,
    device: torch.device,
    amp: bool,
    rng: np.random.Generator,
    epoch: int,
) -> float:
    model.train()
    criterion = nn.CrossEntropyLoss(ignore_index=0)
    total_loss = 0.0
    batches = 0
    iterator = batched_indices(indices, batch_size, rng)
    for batch in tqdm(
        iterator,
        total=(len(indices) + batch_size - 1) // batch_size,
        desc=f"recognizer epoch {epoch}",
        unit="batch",
    ):
        images = rotate_training_images(
            torch_images(crops, batch, device), records, batch
        )
        labels = [records[index]["target_text"] for index in batch.tolist()]
        encoded = torch.from_numpy(
            encode_recognizer_targets(labels, token_to_index)
        ).to(device)
        optimizer.zero_grad(set_to_none=True)
        with torch.autocast(device_type=device.type, dtype=torch.float16, enabled=amp):
            logits = model(images, encoded[:, :-1], is_train=True)
            loss = criterion(
                logits.reshape(-1, logits.shape[-1]), encoded[:, 1:].reshape(-1)
            )
        scaler.scale(loss).backward()
        scaler.unscale_(optimizer)
        nn.utils.clip_grad_norm_(model.parameters(), 5.0)
        scaler.step(optimizer)
        scaler.update()
        scheduler.step()
        total_loss += float(loss.detach())
        batches += 1
    return total_loss / max(1, batches)


@torch.inference_mode()
def write_predictions(
    model: nn.Module,
    dataset: Path,
    split: str,
    records: list[dict[str, Any]],
    crops: np.ndarray,
    tokens: list[str],
    batch_size: int,
    device: torch.device,
    amp: bool,
    output: Path,
) -> None:
    model.eval()
    path = output / "predictions" / f"{split}.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp.jsonl")
    indices = np.arange(len(records), dtype=np.int64)
    with temporary.open("w", encoding="utf-8", newline="\n") as handle:
        for batch in tqdm(
            batched_indices(indices, batch_size),
            total=(len(indices) + batch_size - 1) // batch_size,
            desc=f"predict {split}",
            unit="batch",
        ):
            images = torch_images(crops, batch, device)
            predictions, confidences, rotations = predict_batch(
                model, images, records, batch, tokens, amp
            )
            for index, prediction, confidence, rotation in zip(
                batch.tolist(), predictions, confidences, rotations, strict=True
            ):
                target = records[index]["target_text"]
                value = {
                    "sample_id": records[index]["sample_id"],
                    "prediction": prediction,
                    "confidence": confidence,
                    "rotation_degrees": rotation,
                    "normalized_cer": (
                        character_error_rate(prediction, target)
                        if target is not None and not records[index]["is_truncated"]
                        else None
                    ),
                }
                handle.write(
                    json.dumps(value, ensure_ascii=False, separators=(",", ":"))
                )
                handle.write("\n")
    os.replace(temporary, path)


def main() -> None:
    args = parse_args()
    if not torch.cuda.is_available() and args.device.startswith("cuda"):
        raise RuntimeError(
            "CUDA was requested but the CUDA PyTorch build is unavailable"
        )
    if args.epochs < 1 or args.batch_size < 1:
        raise ValueError("epochs and batch size must be positive")

    random.seed(args.seed)
    np.random.seed(args.seed)
    torch.manual_seed(args.seed)
    torch.cuda.manual_seed_all(args.seed)
    torch.backends.cudnn.benchmark = True
    device = torch.device(args.device)
    amp = device.type == "cuda" and not args.no_amp
    args.output.mkdir(parents=True, exist_ok=True)

    upstream_root = find_upstream(args.upstream_root)
    model_class = import_upstream_model(upstream_root)
    model = model_class(model_options()).to(device)
    pretrained_path = load_pretrained(model)
    tokens, token_to_index = load_tokens()

    records: dict[str, list[dict[str, Any]]] = {}
    crops: dict[str, np.ndarray] = {}
    indices: dict[str, np.ndarray] = {}
    for split in ("train", "val", "test"):
        split_records = load_records(args.dataset, split)
        if args.limit is not None:
            split_records = split_records[: args.limit]
        records[split] = split_records
        crops[split] = ensure_crop_cache(
            args.dataset, split, load_records(args.dataset, split), args.workers
        )
        if args.limit is not None:
            crops[split] = crops[split][: len(split_records)]
        indices[split] = recognition_indices(split_records)

    started = time.time()
    baseline = evaluate(
        model,
        records["val"],
        crops["val"],
        indices["val"],
        tokens,
        args.eval_batch_size,
        device,
        amp,
        "baseline validation",
    )
    print(f"baseline validation: {json.dumps(baseline, ensure_ascii=False)}")

    optimizer = torch.optim.Adam(model.parameters(), lr=args.learning_rate)
    steps_per_epoch = (len(indices["train"]) + args.batch_size - 1) // args.batch_size
    scheduler = torch.optim.lr_scheduler.OneCycleLR(
        optimizer,
        max_lr=args.learning_rate,
        total_steps=steps_per_epoch * args.epochs,
        div_factor=20,
        final_div_factor=1000,
        cycle_momentum=False,
    )
    scaler = torch.amp.GradScaler(device.type, enabled=amp)
    rng = np.random.default_rng(args.seed)
    history = []
    best_accuracy = -1.0
    checkpoint = args.output / "model.safetensors"
    for epoch in range(1, args.epochs + 1):
        train_loss = train_epoch(
            model,
            records["train"],
            crops["train"],
            indices["train"],
            token_to_index,
            optimizer,
            scheduler,
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
            indices["val"],
            tokens,
            args.eval_batch_size,
            device,
            amp,
            f"validation epoch {epoch}",
        )
        entry = {"epoch": epoch, "train_loss": train_loss, "validation": validation}
        history.append(entry)
        print(json.dumps(entry, ensure_ascii=False))
        if float(validation["accuracy"]) > best_accuracy:
            best_accuracy = float(validation["accuracy"])
            save_checkpoint(
                model,
                checkpoint,
                {
                    "architecture": "COO TRBA_Rot+SAR+HardROIhalf+2D",
                    "upstream_commit": UPSTREAM_COMMIT,
                    "training_dataset": "COO joined to MangaSeg/Manga109",
                },
            )

    load_checkpoint(model, checkpoint)
    model.to(device)
    final_validation = evaluate(
        model,
        records["val"],
        crops["val"],
        indices["val"],
        tokens,
        args.eval_batch_size,
        device,
        amp,
        "best validation",
    )
    final_test = evaluate(
        model,
        records["test"],
        crops["test"],
        indices["test"],
        tokens,
        args.eval_batch_size,
        device,
        amp,
        "best test",
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
        "upstream_commit": UPSTREAM_COMMIT,
        "pretrained_weights": str(pretrained_path),
        "samples": {split: len(indices[split]) for split in indices},
        "baseline_validation": baseline,
        "best_validation": final_validation,
        "test": final_test,
        "history": history,
        "elapsed_seconds": time.time() - started,
    }
    atomic_json(args.output / "metrics.json", metrics)

    if not args.skip_predictions:
        for split in ("train", "val", "test"):
            write_predictions(
                model,
                args.dataset,
                split,
                records[split],
                crops[split],
                tokens,
                args.eval_batch_size,
                device,
                amp,
                args.output,
            )
    print(json.dumps(metrics, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
