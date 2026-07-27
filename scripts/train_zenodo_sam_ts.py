"""Fine-tune SAM-TS on the prepared Zenodo Manga109 text masks.

The upstream Hi-SAM model and losses are imported from an untouched checkout.
This launcher supplies a Windows-compatible single-GPU loop and consumes the
prepared 80% native-crop / 20% full-page JSONL manifest directly.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import random
import sys
import time
from collections import Counter
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import numpy as np
import torch
import torch.utils.checkpoint
from PIL import Image, ImageEnhance
from torch.utils.data import DataLoader, Dataset

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DATASET = ROOT / "data" / "manga109-zenodo-sam-ts"
DEFAULT_HI_SAM = ROOT / "temp" / "Hi-SAM"
DEFAULT_CHECKPOINT = ROOT / "models" / "hi-sam" / "sam_tss_l_textseg.pth"
DEFAULT_SAM = ROOT / "models" / "hi-sam" / "sam_vit_l_0b3195.pth"
DEFAULT_OUTPUT = ROOT / "runs" / "sam-ts-l-textseg-zenodo-1epoch-test"
TEXTSEG_SHA256 = "1a7399fd5b031383a3776b4375332d23b952be616a735b545b3abb7eb89d063f"
SAM_VIT_L_SHA256 = "3adcc4315b642a4d2101128f611684e8734c41232a17c648ed1693702a49a622"
IMAGE_SIZE = 1024


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=DEFAULT_DATASET)
    parser.add_argument("--hi-sam-root", type=Path, default=DEFAULT_HI_SAM)
    parser.add_argument("--checkpoint", type=Path, default=DEFAULT_CHECKPOINT)
    parser.add_argument("--sam-checkpoint", type=Path, default=DEFAULT_SAM)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--epochs", type=int, default=20)
    parser.add_argument("--batch-size", type=int, default=1)
    parser.add_argument("--workers", type=int, default=2)
    parser.add_argument("--learning-rate", type=float, default=1e-5)
    parser.add_argument("--min-learning-rate", type=float, default=1e-6)
    parser.add_argument("--warmup-epochs", type=int, default=1)
    parser.add_argument("--early-stopping-patience", type=int, default=5)
    parser.add_argument("--minimum-epochs", type=int, default=8)
    parser.add_argument("--weight-decay", type=float, default=0.05)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--log-every", type=int, default=25)
    parser.add_argument("--max-train-samples", type=int)
    parser.add_argument("--max-val-samples", type=int)
    parser.add_argument(
        "--amp-dtype", choices=("bfloat16", "float16", "none"), default="bfloat16"
    )
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def validate_inputs(args: argparse.Namespace) -> None:
    required = (
        args.dataset / "manifest.json",
        args.dataset / "manifests" / "train_mixed_80crop_20full.jsonl",
        args.dataset / "manifests" / "val.jsonl",
        args.hi_sam_root / "hi_sam" / "modeling" / "build.py",
        args.checkpoint,
        args.sam_checkpoint,
    )
    missing = [str(path) for path in required if not path.is_file()]
    if missing:
        raise FileNotFoundError(f"missing required inputs: {missing}")
    if sha256(args.checkpoint) != TEXTSEG_SHA256:
        raise ValueError("SAM-TS-L TextSeg checkpoint SHA-256 mismatch")
    if sha256(args.sam_checkpoint) != SAM_VIT_L_SHA256:
        raise ValueError("SAM ViT-L checkpoint SHA-256 mismatch")
    if args.epochs <= 0 or args.batch_size <= 0 or args.workers < 0:
        raise ValueError(
            "epochs and batch size must be positive; workers cannot be negative"
        )
    if not 0 < args.min_learning_rate <= args.learning_rate:
        raise ValueError("learning rates must satisfy 0 < minimum <= peak")
    if not 0 <= args.warmup_epochs < args.epochs:
        raise ValueError(
            "warmup epochs must be non-negative and less than total epochs"
        )
    if args.minimum_epochs <= 0 or args.minimum_epochs > args.epochs:
        raise ValueError("minimum epochs must be between one and total epochs")
    if args.early_stopping_patience <= 0:
        raise ValueError("early-stopping patience must be positive")
    if args.output.exists():
        raise FileExistsError(f"output already exists: {args.output}")
    source_manifest = json.loads(
        (args.dataset / "manifest.json").read_text(encoding="utf-8")
    )
    if source_manifest.get("uses_manga109_segmentation") is not False:
        raise ValueError("dataset does not explicitly reject manga109-segmentation")
    policy = source_manifest.get("training_sampling_policy", {})
    if policy.get("crop_probability") != 0.8:
        raise ValueError("dataset is not configured for 80% native crops")


def read_jsonl(path: Path, limit: int | None = None) -> list[dict[str, Any]]:
    records = [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    return records if limit is None else records[:limit]


class ManifestDataset(Dataset[dict[str, Any]]):
    def __init__(
        self, root: Path, records: list[dict[str, Any]], augment: bool
    ) -> None:
        self.root = root
        self.records = records
        self.augment = augment

    def __len__(self) -> int:
        return len(self.records)

    def __getitem__(self, index: int) -> dict[str, Any]:
        record = self.records[index]
        with Image.open(self.root / record["image"]) as source:
            image = source.convert("RGB")
        with Image.open(self.root / record["mask"]) as source:
            mask = source.convert("L")
        if image.size != mask.size:
            raise ValueError(f"image/mask size mismatch for {record['image']}")

        if image.size != (IMAGE_SIZE, IMAGE_SIZE):
            scale = IMAGE_SIZE / max(image.size)
            resized = tuple(max(1, round(axis * scale)) for axis in image.size)
            image = image.resize(resized, Image.Resampling.BILINEAR)
            mask = mask.resize(resized, Image.Resampling.NEAREST)
            image_canvas = Image.new("RGB", (IMAGE_SIZE, IMAGE_SIZE), (128, 128, 128))
            mask_canvas = Image.new("L", (IMAGE_SIZE, IMAGE_SIZE), 0)
            image_canvas.paste(image, (0, 0))
            mask_canvas.paste(mask, (0, 0))
            image, mask = image_canvas, mask_canvas

        if self.augment:
            brightness = 0.9 + random.random() * 0.2
            contrast = 0.9 + random.random() * 0.2
            image = ImageEnhance.Brightness(image).enhance(brightness)
            image = ImageEnhance.Contrast(image).enhance(contrast)

        image_array = np.array(image, dtype=np.float32, copy=True)
        mask_array = (np.array(mask, copy=True) > 127).astype(np.float32) * 255.0
        if not np.any(mask_array):
            raise ValueError(f"empty transformed mask for {record['mask']}")
        return {
            "image": torch.from_numpy(image_array).permute(2, 0, 1),
            "label": torch.from_numpy(mask_array).unsqueeze(0),
            "sample_type": record["sample_type"],
            "source_image": record["source_image"],
        }


def seed_worker(worker_id: int) -> None:
    worker_seed = torch.initial_seed() % (2**32)
    np.random.seed(worker_seed)
    random.seed(worker_seed)


def make_loader(
    dataset: ManifestDataset,
    batch_size: int,
    workers: int,
    shuffle: bool,
    seed: int,
) -> DataLoader[dict[str, Any]]:
    generator = torch.Generator().manual_seed(seed)
    return DataLoader(
        dataset,
        batch_size=batch_size,
        shuffle=shuffle,
        num_workers=workers,
        pin_memory=True,
        persistent_workers=workers > 0,
        worker_init_fn=seed_worker,
        generator=generator,
        drop_last=False,
    )


def install_checkpoint_compatibility() -> None:
    original = torch.utils.checkpoint.checkpoint

    def checkpoint(function: Any, *args: Any, **kwargs: Any) -> Any:
        kwargs.setdefault("use_reentrant", False)
        return original(function, *args, **kwargs)

    torch.utils.checkpoint.checkpoint = checkpoint


def build_model(args: argparse.Namespace, output: Path) -> torch.nn.Module:
    sys.path.insert(0, str(args.hi_sam_root))
    install_checkpoint_compatibility()
    from hi_sam.modeling.build import model_registry

    pretrained_dir = output / "pretrained_checkpoint"
    pretrained_dir.mkdir(parents=True)
    local_sam = pretrained_dir / "sam_vit_l_0b3195.pth"
    os.link(args.sam_checkpoint, local_sam)
    model_args = SimpleNamespace(
        checkpoint=str(args.checkpoint),
        model_type="vit_l",
        attn_layers=1,
        prompt_len=12,
        hier_det=False,
    )
    previous_cwd = Path.cwd()
    os.chdir(output)
    try:
        model = model_registry["vit_l"](args=model_args)
    finally:
        os.chdir(previous_cwd)
    return model


def optimizer_for(
    model: torch.nn.Module, learning_rate: float, weight_decay: float
) -> torch.optim.Optimizer:
    decoder = []
    remaining = []
    for name, parameter in model.named_parameters():
        if not parameter.requires_grad:
            continue
        (decoder if "mask_decoder" in name else remaining).append(parameter)
    if not decoder or not remaining:
        raise ValueError("unexpected trainable parameter grouping")
    return torch.optim.AdamW(
        [
            {"params": remaining, "lr": learning_rate},
            {"params": decoder, "lr": learning_rate},
        ],
        lr=learning_rate,
        betas=(0.9, 0.999),
        weight_decay=weight_decay,
    )


def amp_settings(name: str) -> tuple[bool, torch.dtype | None]:
    if name == "bfloat16":
        return True, torch.bfloat16
    if name == "float16":
        return True, torch.float16
    return False, None


def cosine_learning_rate(
    update: int,
    total_updates: int,
    warmup_updates: int,
    peak: float,
    minimum: float,
) -> float:
    if warmup_updates and update < warmup_updates:
        return peak * (update + 1) / warmup_updates
    decay_updates = max(total_updates - warmup_updates - 1, 1)
    progress = min(max((update - warmup_updates) / decay_updates, 0.0), 1.0)
    cosine = 0.5 * (1.0 + math.cos(math.pi * progress))
    return minimum + (peak - minimum) * cosine


def forward_loss(
    model: torch.nn.Module,
    images: torch.Tensor,
    labels: torch.Tensor,
    loss_masks: Any,
    loss_iou_mse: Any,
) -> tuple[torch.Tensor, dict[str, float]]:
    batched_input = [
        {
            "image": image.contiguous(),
            "original_size": image.shape[-2:],
        }
        for image in images
    ]
    outputs = model(batched_input, multimask_output=False)
    up_logits, up_masks, up_iou, hr_logits, hr_masks, hr_iou = outputs
    focal, dice = loss_masks(up_logits, labels / 255.0, len(up_logits))
    focal_hr, dice_hr = loss_masks(hr_logits, labels / 255.0, len(hr_logits))
    mse = loss_iou_mse(up_iou, up_masks, labels)
    mse_hr = loss_iou_mse(hr_iou, hr_masks, labels)
    loss = focal * 20 + dice + mse + focal_hr * 20 + dice_hr + mse_hr
    parts = {
        "loss": float(loss.detach()),
        "focal": float((focal * 20).detach()),
        "dice": float(dice.detach()),
        "iou_mse": float(mse.detach()),
        "focal_hr": float((focal_hr * 20).detach()),
        "dice_hr": float(dice_hr.detach()),
        "iou_mse_hr": float(mse_hr.detach()),
    }
    return loss, parts


def train_epoch(
    model: torch.nn.Module,
    loader: DataLoader[dict[str, Any]],
    optimizer: torch.optim.Optimizer,
    device: torch.device,
    amp_enabled: bool,
    amp_dtype: torch.dtype | None,
    log_path: Path,
    log_every: int,
    epoch: int,
    total_epochs: int,
    warmup_epochs: int,
    peak_learning_rate: float,
    min_learning_rate: float,
    loss_masks: Any,
    loss_iou_mse: Any,
) -> dict[str, float]:
    model.train()
    running: Counter[str] = Counter()
    started = time.perf_counter()
    scaler = torch.amp.GradScaler("cuda", enabled=amp_dtype == torch.float16)
    with log_path.open("a", encoding="utf-8", newline="\n") as log_file:
        for step, batch in enumerate(loader, start=1):
            global_update = (epoch - 1) * len(loader) + step - 1
            learning_rate = cosine_learning_rate(
                global_update,
                total_epochs * len(loader),
                warmup_epochs * len(loader),
                peak_learning_rate,
                min_learning_rate,
            )
            for group in optimizer.param_groups:
                group["lr"] = learning_rate
            images = batch["image"].to(device, non_blocking=True)
            labels = batch["label"].to(device, non_blocking=True)
            optimizer.zero_grad(set_to_none=True)
            with torch.autocast(
                device_type="cuda",
                dtype=amp_dtype,
                enabled=amp_enabled,
            ):
                loss, parts = forward_loss(
                    model, images, labels, loss_masks, loss_iou_mse
                )
            if not torch.isfinite(loss):
                raise FloatingPointError(f"non-finite loss at step {step}: {parts}")
            scaler.scale(loss).backward()
            scaler.unscale_(optimizer)
            grad_norm = torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
            scaler.step(optimizer)
            scaler.update()
            for key, value in parts.items():
                running[key] += value

            if step == 1 or step % log_every == 0 or step == len(loader):
                elapsed = time.perf_counter() - started
                seconds_per_step = elapsed / step
                event = {
                    "epoch": epoch,
                    "step": step,
                    "steps": len(loader),
                    **parts,
                    "grad_norm": float(grad_norm),
                    "learning_rate": learning_rate,
                    "seconds_per_step": seconds_per_step,
                    "eta_seconds": seconds_per_step * (len(loader) - step),
                    "gpu_memory_gib": torch.cuda.max_memory_allocated() / 2**30,
                }
                log_file.write(json.dumps(event, separators=(",", ":")) + "\n")
                log_file.flush()
                print(json.dumps(event), flush=True)

    elapsed = time.perf_counter() - started
    return {
        **{key: value / len(loader) for key, value in running.items()},
        "seconds": elapsed,
        "seconds_per_step": elapsed / len(loader),
        "learning_rate": optimizer.param_groups[0]["lr"],
    }


def evaluate(
    model: torch.nn.Module,
    loader: DataLoader[dict[str, Any]],
    device: torch.device,
    amp_enabled: bool,
    amp_dtype: torch.dtype | None,
) -> dict[str, float]:
    model.eval()
    counts = {
        name: 0 for name in ("low_tp", "low_fp", "low_fn", "hr_tp", "hr_fp", "hr_fn")
    }
    started = time.perf_counter()
    with torch.inference_mode():
        for batch in loader:
            images = batch["image"].to(device, non_blocking=True)
            labels = batch["label"].to(device, non_blocking=True) > 127
            inputs = [
                {"image": image.contiguous(), "original_size": image.shape[-2:]}
                for image in images
            ]
            with torch.autocast(
                device_type="cuda", dtype=amp_dtype, enabled=amp_enabled
            ):
                _, low, _, _, high, _ = model(inputs, multimask_output=False)
            for prefix, prediction in (("low", low), ("hr", high)):
                prediction = prediction.bool()
                counts[f"{prefix}_tp"] += int((prediction & labels).sum())
                counts[f"{prefix}_fp"] += int((prediction & ~labels).sum())
                counts[f"{prefix}_fn"] += int((~prediction & labels).sum())

    metrics: dict[str, float] = {}
    for prefix in ("low", "hr"):
        tp = counts[f"{prefix}_tp"]
        fp = counts[f"{prefix}_fp"]
        fn = counts[f"{prefix}_fn"]
        metrics[f"{prefix}_iou"] = tp / max(tp + fp + fn, 1)
        metrics[f"{prefix}_fscore"] = 2 * tp / max(2 * tp + fp + fn, 1)
    metrics["seconds"] = time.perf_counter() - started
    return metrics


def trainable_state_dict(model: torch.nn.Module) -> dict[str, torch.Tensor]:
    names = {
        name for name, parameter in model.named_parameters() if parameter.requires_grad
    }
    return {
        name: value.detach().cpu()
        for name, value in model.state_dict().items()
        if name in names
    }


def atomic_torch_save(value: Any, destination: Path) -> None:
    temporary = destination.with_suffix(destination.suffix + ".tmp")
    torch.save(value, temporary)
    temporary.replace(destination)


def main() -> None:
    args = parse_args()
    for name in ("dataset", "hi_sam_root", "checkpoint", "sam_checkpoint", "output"):
        setattr(args, name, getattr(args, name).resolve())
    validate_inputs(args)

    random.seed(args.seed)
    np.random.seed(args.seed)
    torch.manual_seed(args.seed)
    torch.cuda.manual_seed_all(args.seed)
    torch.backends.cuda.matmul.allow_tf32 = True
    torch.backends.cudnn.allow_tf32 = True
    torch.set_float32_matmul_precision("high")
    if not torch.cuda.is_available():
        raise RuntimeError("CUDA is required for SAM-TS-L fine-tuning")
    device = torch.device("cuda:0")
    amp_enabled, amp_dtype = amp_settings(args.amp_dtype)

    train_records = read_jsonl(
        args.dataset / "manifests" / "train_mixed_80crop_20full.jsonl",
        args.max_train_samples,
    )
    val_records = read_jsonl(
        args.dataset / "manifests" / "val.jsonl", args.max_val_samples
    )
    composition = Counter(record["sample_type"] for record in train_records)
    if args.max_train_samples is None and composition != {
        "native_crop": 1436,
        "full_page": 359,
    }:
        raise ValueError(f"unexpected 80/20 training composition: {composition}")

    args.output.mkdir(parents=True)
    config = {
        **vars(args),
        "dataset": str(args.dataset),
        "hi_sam_root": str(args.hi_sam_root),
        "checkpoint": str(args.checkpoint),
        "sam_checkpoint": str(args.sam_checkpoint),
        "output": str(args.output),
        "train_samples": len(train_records),
        "val_samples": len(val_records),
        "composition": dict(composition),
        "device": torch.cuda.get_device_name(0),
        "torch": torch.__version__,
    }
    (args.output / "run_config.json").write_text(
        json.dumps(config, indent=2, default=str) + "\n", encoding="utf-8"
    )

    train_loader = make_loader(
        ManifestDataset(args.dataset, train_records, augment=True),
        args.batch_size,
        args.workers,
        True,
        args.seed,
    )
    val_loader = make_loader(
        ManifestDataset(args.dataset, val_records, augment=False),
        1,
        args.workers,
        False,
        args.seed,
    )
    model = build_model(args, args.output).to(device)
    trainable = sum(
        parameter.numel() for parameter in model.parameters() if parameter.requires_grad
    )
    total = sum(parameter.numel() for parameter in model.parameters())
    print(
        json.dumps(
            {
                "event": "model_ready",
                "trainable_parameters": trainable,
                "total_parameters": total,
                "train_batches": len(train_loader),
                "val_batches": len(val_loader),
            }
        ),
        flush=True,
    )
    optimizer = optimizer_for(model, args.learning_rate, args.weight_decay)

    sys.path.insert(0, str(args.hi_sam_root))
    from hi_sam.modeling.loss import loss_iou_mse, loss_masks

    baseline = evaluate(model, val_loader, device, amp_enabled, amp_dtype)
    print(json.dumps({"event": "baseline_validation", **baseline}), flush=True)

    epoch_results = []
    best_score = -1.0
    best_epoch = 0
    epochs_without_improvement = 0
    stopped_early = False
    for epoch in range(1, args.epochs + 1):
        train_metrics = train_epoch(
            model,
            train_loader,
            optimizer,
            device,
            amp_enabled,
            amp_dtype,
            args.output / "train_log.jsonl",
            args.log_every,
            epoch,
            args.epochs,
            args.warmup_epochs,
            args.learning_rate,
            args.min_learning_rate,
            loss_masks,
            loss_iou_mse,
        )
        val_metrics = evaluate(model, val_loader, device, amp_enabled, amp_dtype)
        result = {"epoch": epoch, "train": train_metrics, "validation": val_metrics}
        epoch_results.append(result)
        score = max(val_metrics["low_iou"], val_metrics["hr_iou"])
        improved = score > best_score + 1e-5
        if improved:
            best_score = score
            best_epoch = epoch
            epochs_without_improvement = 0
        else:
            epochs_without_improvement += 1
        print(
            json.dumps(
                {
                    "event": "epoch_complete",
                    **result,
                    "selection_iou": score,
                    "best_iou": best_score,
                    "best_epoch": best_epoch,
                    "epochs_without_improvement": epochs_without_improvement,
                }
            ),
            flush=True,
        )

        learned_state = trainable_state_dict(model)
        if improved:
            atomic_torch_save(
                learned_state,
                args.output / "sam_tss_l_zenodo_best.pth",
            )
        atomic_torch_save(
            {
                "model": learned_state,
                "optimizer": optimizer.state_dict(),
                "epoch": epoch,
                "global_update": epoch * len(train_loader),
                "config": config,
                "metrics": result,
            },
            args.output / "checkpoint_latest.pth",
        )
        partial = {
            "status": "training",
            "baseline": baseline,
            "epochs": epoch_results,
            "best_epoch": best_epoch,
            "best_iou": best_score,
        }
        (args.output / "metrics_partial.json").write_text(
            json.dumps(partial, indent=2) + "\n", encoding="utf-8"
        )
        if (
            epoch >= args.minimum_epochs
            and epochs_without_improvement >= args.early_stopping_patience
        ):
            stopped_early = True
            print(
                json.dumps(
                    {
                        "event": "early_stopping",
                        "epoch": epoch,
                        "best_epoch": best_epoch,
                        "best_iou": best_score,
                    }
                ),
                flush=True,
            )
            break

    atomic_torch_save(
        trainable_state_dict(model), args.output / "sam_tss_l_zenodo_final.pth"
    )
    summary = {
        "status": "complete",
        "baseline": baseline,
        "epochs": epoch_results,
        "best_epoch": best_epoch,
        "best_iou": best_score,
        "stopped_early": stopped_early,
        "peak_gpu_memory_gib": torch.cuda.max_memory_allocated() / 2**30,
    }
    (args.output / "metrics.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    (args.output / "TRAINING_COMPLETE.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps({"event": "training_complete", **summary}), flush=True)


if __name__ == "__main__":
    main()
