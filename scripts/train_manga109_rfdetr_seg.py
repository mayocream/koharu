#!/usr/bin/env python3
"""Train RF-DETR-Seg-2XL on the Manga109 Segmentation RF-DETR view.

This entry point intentionally selects checkpoints by segmentation mAP rather
than RF-DETR's default bounding-box mAP.  It uses RF-DETR's public Lightning
training components and is pinned operationally to rfdetr 1.7.x.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import torch


def configure_windows_utf8_console() -> None:
    """Keep Rich metric tables printable from non-UTF-8 Windows shells."""

    if os.name != "nt":
        return
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if reconfigure is not None:
            reconfigure(encoding="utf-8", errors="backslashreplace")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--dataset-dir",
        type=Path,
        default=Path("/workspace/koharu/data/manga109-segmentation-rfdetr"),
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("/workspace/koharu/runs/rfdetr-seg-2xl-768"),
    )
    parser.add_argument(
        "--pretrain-weights",
        type=Path,
        help="Initialize model weights without resuming optimizer or scheduler state.",
    )
    parser.add_argument(
        "--resume-checkpoint",
        type=Path,
        help="Resume model, optimizer, scheduler, loop, and epoch state from a Lightning checkpoint.",
    )
    parser.add_argument("--resolution", type=int, default=768)
    parser.add_argument("--epochs", type=int, default=36)
    parser.add_argument("--batch-size", type=int, default=4)
    parser.add_argument("--grad-accum-steps", type=int, default=1)
    parser.add_argument("--devices", type=int, default=4)
    parser.add_argument("--num-workers", type=int, default=12)
    parser.add_argument("--eval-interval", type=int, default=1)
    parser.add_argument(
        "--limit-train-batches",
        type=int,
        help="Run only this many training batches per epoch (diagnostic smoke tests only).",
    )
    parser.add_argument(
        "--limit-val-batches",
        type=int,
        help="Run only this many validation batches (diagnostic smoke tests only).",
    )
    parser.add_argument("--num-select", type=int, default=160)
    parser.add_argument("--eval-max-dets", type=int, default=160)
    parser.add_argument("--ddp-timeout-minutes", type=int, default=120)
    parser.add_argument("--checkpoint-interval", type=int, default=1)
    parser.add_argument(
        "--periodic-checkpoint-interval",
        type=int,
        default=6,
        help="Save an atomic resumable RF-DETR checkpoint every N completed epochs.",
    )
    parser.add_argument(
        "--skip-distributed-validation",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Disable validation during DDP fit and run one-GPU validation after training.",
    )
    parser.add_argument(
        "--skip-final-validation",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Stop after writing the final checkpoint (diagnostic smoke tests only).",
    )
    parser.add_argument(
        "--eval-only-checkpoint",
        type=Path,
        help=argparse.SUPPRESS,
    )
    parser.add_argument("--warmup-epochs", type=float, default=1.0)
    parser.add_argument("--lr", type=float, default=1e-4)
    parser.add_argument("--lr-encoder", type=float, default=1.5e-4)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument(
        "--run-test", action=argparse.BooleanOptionalAction, default=True
    )
    parser.add_argument(
        "--use-ema", action=argparse.BooleanOptionalAction, default=True
    )
    parser.add_argument(
        "--augmentation-backend",
        choices=("cpu", "auto", "gpu"),
        default="gpu",
    )
    parser.add_argument(
        "--typography-distillation",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Enable the PP-DocLayoutV3-assisted dense typography branch.",
    )
    parser.add_argument("--typography-head-channels", type=int, default=96)
    parser.add_argument(
        "--typography-output-scale", type=int, choices=(1, 2, 4), default=4
    )
    parser.add_argument("--typography-dilation", type=int, default=2)
    parser.add_argument(
        "--typography-loss-weight",
        type=float,
        default=0.75,
        help=(
            "Weight of the dense typography loss. Set to 0 for a controlled "
            "direct-pseudo-only ablation that keeps RF-DETR target filtering unchanged."
        ),
    )
    parser.add_argument("--teacher-positive-weight", type=float, default=0.30)
    parser.add_argument("--dense-negative-weight", type=float, default=0.25)
    parser.add_argument("--dense-head-warmup-epochs", type=int, default=1)
    parser.add_argument("--db-slope", type=float, default=20.0)
    return parser.parse_args()


def validate_args(args: argparse.Namespace) -> None:
    block_size = 24  # Seg-2XL: patch_size=12, num_windows=2.
    if args.resolution <= 0 or args.resolution % block_size:
        raise ValueError(f"--resolution must be a positive multiple of {block_size}")
    if args.epochs <= 0 or args.batch_size <= 0 or args.grad_accum_steps <= 0:
        raise ValueError("epochs, batch size, and accumulation steps must be positive")
    if args.num_select <= 0 or args.eval_max_dets <= 0:
        raise ValueError("num-select and eval-max-dets must be positive")
    if args.ddp_timeout_minutes <= 0:
        raise ValueError("--ddp-timeout-minutes must be positive")
    if args.periodic_checkpoint_interval <= 0:
        raise ValueError("--periodic-checkpoint-interval must be positive")
    if args.limit_train_batches is not None and args.limit_train_batches <= 0:
        raise ValueError("--limit-train-batches must be positive")
    if args.limit_val_batches is not None and args.limit_val_batches <= 0:
        raise ValueError("--limit-val-batches must be positive")
    if args.num_select < args.eval_max_dets:
        raise ValueError(
            "--num-select must be greater than or equal to --eval-max-dets"
        )
    for split in ("train", "valid", "test"):
        split_dir = args.dataset_dir / split
        annotation = split_dir / "_annotations.coco.json"
        if not split_dir.is_dir() or not annotation.is_file():
            raise FileNotFoundError(f"missing COCO split or annotation: {annotation}")
    if args.pretrain_weights is not None and not args.pretrain_weights.is_file():
        raise FileNotFoundError(
            f"pretrained checkpoint not found: {args.pretrain_weights}"
        )
    if args.resume_checkpoint is not None and not args.resume_checkpoint.is_file():
        raise FileNotFoundError(
            f"resume checkpoint not found: {args.resume_checkpoint}"
        )
    if args.typography_distillation:
        marker = args.dataset_dir / ".manga109-pp-doclayout-distillation-view"
        if not marker.is_file():
            raise FileNotFoundError(
                "--typography-distillation requires the generated PP teacher view; "
                f"missing marker: {marker}"
            )
        if args.typography_head_channels <= 0:
            raise ValueError("--typography-head-channels must be positive")
        if args.typography_dilation < 0 or args.dense_head_warmup_epochs < 0:
            raise ValueError(
                "typography dilation and warmup epochs must be non-negative"
            )
        if args.typography_loss_weight < 0 or args.db_slope <= 0:
            raise ValueError(
                "typography loss weight must be non-negative and DB slope positive"
            )
        if not 0.0 < args.teacher_positive_weight <= 1.0:
            raise ValueError("--teacher-positive-weight must be in (0, 1]")
        if not 0.0 < args.dense_negative_weight <= 1.0:
            raise ValueError("--dense-negative-weight must be in (0, 1]")
    if not torch.cuda.is_available():
        raise RuntimeError("CUDA is required for this launch configuration")
    if torch.cuda.device_count() < args.devices:
        raise RuntimeError(
            f"requested {args.devices} GPUs, but PyTorch sees {torch.cuda.device_count()}"
        )


def set_training_resolution(model_config: object, resolution: int) -> None:
    """Apply the same formula-derived positional encoding update as RFDETR.train."""
    old_resolution = model_config.resolution
    old_derived_pe = old_resolution // model_config.patch_size
    if model_config.positional_encoding_size == old_derived_pe:
        model_config.positional_encoding_size = resolution // model_config.patch_size
    model_config.resolution = resolution


def build_training_modules(
    args: argparse.Namespace,
    model_config: object,
    train_config: object,
) -> tuple[object, object, object | None]:
    from rfdetr.training import RFDETRDataModule, RFDETRModelModule

    if not args.typography_distillation:
        return (
            RFDETRModelModule(model_config, train_config),
            RFDETRDataModule(model_config, train_config),
            None,
        )

    from rfdetr_typography_distillation import (
        TypographyDistillationConfig,
        estimated_head_parameters,
        make_distillation_datamodule,
        make_distillation_model_module,
    )

    typography_config = TypographyDistillationConfig(
        feature_channels=int(model_config.hidden_dim),
        feature_levels=len(model_config.projector_scale),
        head_channels=args.typography_head_channels,
        output_scale=args.typography_output_scale,
        dilation=args.typography_dilation,
        loss_weight=args.typography_loss_weight,
        teacher_positive_weight=args.teacher_positive_weight,
        negative_weight=args.dense_negative_weight,
        head_warmup_epochs=args.dense_head_warmup_epochs,
        db_slope=args.db_slope,
    )
    module_class = make_distillation_model_module(RFDETRModelModule)
    datamodule_class = make_distillation_datamodule(RFDETRDataModule)
    module = module_class(model_config, train_config, typography_config)
    datamodule = datamodule_class(model_config, train_config)
    metadata = {
        **typography_config.__dict__,
        "head_parameters": estimated_head_parameters(typography_config),
        "head_restored_from_checkpoint": bool(module.typography_head_restored),
        "gold_classes": ["text", "onomatopoeia"],
        "teacher": "PP-DocLayoutV3 normal-text polygons",
        "direct_pseudo_marker": 3,
    }
    return module, datamodule, metadata


def typography_child_arguments(args: argparse.Namespace) -> list[str]:
    if not args.typography_distillation:
        return []
    return [
        "--typography-distillation",
        "--typography-head-channels",
        str(args.typography_head_channels),
        "--typography-output-scale",
        str(args.typography_output_scale),
        "--typography-dilation",
        str(args.typography_dilation),
        "--typography-loss-weight",
        str(args.typography_loss_weight),
        "--teacher-positive-weight",
        str(args.teacher_positive_weight),
        "--dense-negative-weight",
        str(args.dense_negative_weight),
        "--dense-head-warmup-epochs",
        str(args.dense_head_warmup_epochs),
        "--db-slope",
        str(args.db_slope),
    ]


def write_launch_metadata(
    args: argparse.Namespace,
    model_config: object,
    train_config: object,
    typography_metadata: object | None = None,
) -> None:
    if os.environ.get("LOCAL_RANK", "0") != "0":
        return
    args.output_dir.mkdir(parents=True, exist_ok=True)
    payload = {
        "created_at": datetime.now(timezone.utc).isoformat(),
        "argv": sys.argv,
        "python": sys.version,
        "platform": platform.platform(),
        "torch": torch.__version__,
        "cuda": torch.version.cuda,
        "gpu_count": torch.cuda.device_count(),
        "gpus": [
            torch.cuda.get_device_name(i) for i in range(torch.cuda.device_count())
        ],
        "model_config": model_config.model_dump(),
        "train_config": train_config.model_dump(),
        "checkpoint_metric": "validation segmentation mAP@[.50:.95]",
        "typography_distillation": typography_metadata,
    }
    (args.output_dir / "launch.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, default=str) + "\n",
        encoding="utf-8",
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as checkpoint_file:
        for chunk in iter(lambda: checkpoint_file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def checkpoint_epoch(path: Path) -> int:
    payload = torch.load(path, map_location="cpu", weights_only=False)
    return int(payload.get("epoch", -1))


def configure_segmentation_checkpoint(trainer: object, use_ema: bool) -> object:
    from rfdetr.training.callbacks import BestModelCallback

    best_callbacks = [
        cb for cb in trainer.callbacks if isinstance(cb, BestModelCallback)
    ]
    if len(best_callbacks) != 1:
        raise RuntimeError(
            f"expected one BestModelCallback, found {len(best_callbacks)}"
        )
    best_callback = best_callbacks[0]
    best_callback.monitor = "val/segm_mAP_50_95"
    best_callback._monitor_ema = "val/ema_segm_mAP_50_95" if use_ema else None
    return best_callback


def final_validation(args: argparse.Namespace) -> None:
    from rfdetr import RFDETRSeg2XLarge
    from rfdetr.training import build_trainer

    checkpoint_path = args.eval_only_checkpoint.resolve()
    if not checkpoint_path.is_file():
        raise FileNotFoundError(f"evaluation checkpoint not found: {checkpoint_path}")

    wrapper = RFDETRSeg2XLarge(pretrain_weights=str(checkpoint_path))
    set_training_resolution(wrapper.model_config, args.resolution)
    wrapper.model_config.num_select = args.num_select
    wrapper.model_config.model_name = type(wrapper).__name__
    if args.typography_distillation:
        wrapper.model_config.compile = False
    wrapper._align_num_classes_from_dataset(str(args.dataset_dir.resolve()))

    train_config = wrapper.get_train_config(
        dataset_dir=str(args.dataset_dir.resolve()),
        dataset_file="roboflow",
        output_dir=str(args.output_dir.resolve()),
        epochs=1,
        batch_size=args.batch_size,
        devices=1,
        strategy="auto",
        num_workers=args.num_workers,
        pin_memory=True,
        persistent_workers=True,
        prefetch_factor=2,
        augmentation_backend="cpu",
        eval_interval=1,
        eval_max_dets=args.eval_max_dets,
        log_per_class_metrics=True,
        compute_val_loss=False,
        use_ema=False,
        run_test=False,
        tensorboard=True,
        wandb=False,
        progress_bar=None,
        seed=args.seed,
    )
    module, datamodule, typography_metadata = build_training_modules(
        args,
        wrapper.model_config,
        train_config,
    )
    trainer = build_trainer(
        train_config,
        wrapper.model_config,
        num_sanity_val_steps=0,
        devices=1,
        strategy="auto",
    )
    configure_segmentation_checkpoint(trainer, use_ema=False)
    results = trainer.validate(module, datamodule=datamodule)
    metrics = results[0] if results else {}
    serializable_metrics = {
        str(key): float(value.item() if isinstance(value, torch.Tensor) else value)
        for key, value in metrics.items()
        if isinstance(value, (int, float, torch.Tensor))
    }
    validation_payload = {
        "completed_at": datetime.now(timezone.utc).isoformat(),
        "checkpoint": checkpoint_path.name,
        "checkpoint_epoch": checkpoint_epoch(checkpoint_path),
        "checkpoint_sha256": sha256_file(checkpoint_path),
        "dataset": str(args.dataset_dir.resolve()),
        "validation_mode": "single_gpu_after_ddp_fit",
        "typography_distillation": typography_metadata,
        "metrics": serializable_metrics,
    }
    metrics_path = args.output_dir / "final_validation.json"
    metrics_path.write_text(
        json.dumps(validation_payload, ensure_ascii=False, indent=2, sort_keys=True)
        + "\n",
        encoding="utf-8",
    )
    completion_payload = {
        "status": "complete",
        "completed_at": validation_payload["completed_at"],
        "checkpoint": checkpoint_path.name,
        "checkpoint_epoch": validation_payload["checkpoint_epoch"],
        "checkpoint_sha256": validation_payload["checkpoint_sha256"],
        "validation_metrics": metrics_path.name,
    }
    (args.output_dir / "TRAINING_COMPLETE.json").write_text(
        json.dumps(completion_payload, ensure_ascii=False, indent=2, sort_keys=True)
        + "\n",
        encoding="utf-8",
    )
    print(json.dumps(completion_payload, indent=2), flush=True)


def main() -> None:
    configure_windows_utf8_console()
    args = parse_args()
    validate_args(args)
    # Module construction happens before Lightning's on_fit_start seed hook.
    # Seed here as well so paired dense-loss ablations initialize the auxiliary
    # branch identically before the training RNG is reset by RF-DETR.
    torch.manual_seed(args.seed)
    torch.set_float32_matmul_precision("high")

    if args.eval_only_checkpoint is not None:
        final_validation(args)
        return

    from pytorch_lightning.callbacks import Callback
    from pytorch_lightning.strategies import DDPStrategy
    from rfdetr import RFDETRSeg2XLarge
    from rfdetr.training import build_trainer

    class PeriodicRFDETRCheckpoint(Callback):
        """Atomically save the current EMA weights without validation collectives."""

        def __init__(
            self, best_callback: object, output_dir: Path, interval: int
        ) -> None:
            self.best_callback = best_callback
            self.output_dir = output_dir
            self.interval = interval

        def on_train_epoch_end(self, trainer: object, pl_module: object) -> None:
            completed_epochs = int(trainer.current_epoch) + 1
            if completed_epochs % self.interval != 0 and completed_epochs < int(
                trainer.max_epochs
            ):
                return
            if trainer.is_global_zero:
                temporary = self.output_dir / "checkpoint_latest.tmp.pth"
                latest = self.output_dir / "checkpoint_latest.pth"
                self.best_callback._current_pl_module = pl_module
                self.best_callback._save_checkpoint(trainer, str(temporary))
                os.replace(temporary, latest)
                print(
                    json.dumps(
                        {
                            "checkpoint": latest.name,
                            "checkpoint_epoch": int(trainer.current_epoch),
                            "global_step": int(trainer.global_step),
                        }
                    ),
                    flush=True,
                )
            trainer.strategy.barrier("periodic_rfdetr_checkpoint")

    model_kwargs = {}
    if args.pretrain_weights is not None:
        model_kwargs["pretrain_weights"] = str(args.pretrain_weights.resolve())
    wrapper = RFDETRSeg2XLarge(**model_kwargs)
    set_training_resolution(wrapper.model_config, args.resolution)
    # The strict Manga109 Segmentation view has at most 145 objects per page. Keeping 160 ranked
    # candidates preserves headroom without evaluating the architecture default
    # of 300 full-resolution masks for every validation image.
    wrapper.model_config.num_select = args.num_select
    wrapper.model_config.model_name = type(wrapper).__name__
    if args.typography_distillation:
        # The dense branch consumes backbone features through a forward hook;
        # keep this experiment eager so torch.compile cannot elide the tap.
        wrapper.model_config.compile = False

    training_strategy = "auto" if args.devices == 1 else "ddp"
    train_config = wrapper.get_train_config(
        dataset_dir=str(args.dataset_dir.resolve()),
        dataset_file="roboflow",
        output_dir=str(args.output_dir.resolve()),
        epochs=args.epochs,
        batch_size=args.batch_size,
        grad_accum_steps=args.grad_accum_steps,
        devices=args.devices,
        strategy=training_strategy,
        num_workers=args.num_workers,
        pin_memory=True,
        persistent_workers=True,
        prefetch_factor=2,
        augmentation_backend=args.augmentation_backend,
        lr=args.lr,
        lr_encoder=args.lr_encoder,
        lr_scheduler="cosine",
        lr_min_factor=0.05,
        warmup_epochs=args.warmup_epochs,
        checkpoint_interval=args.checkpoint_interval,
        skip_best_epochs=0,
        eval_interval=args.eval_interval,
        eval_max_dets=args.eval_max_dets,
        log_per_class_metrics=True,
        compute_val_loss=False,
        use_ema=args.use_ema,
        run_test=args.run_test,
        tensorboard=True,
        wandb=False,
        progress_bar=None,
        seed=args.seed,
        notes={
            "dataset": (
                "Manga109 Segmentation strict RF-DETR view with PP-DocLayoutV3 dense distillation"
                if args.typography_distillation
                else "Manga109 Segmentation v2.0.0 TextSeg-refined RF-DETR view"
            ),
            "classes": ["text", "onomatopoeia", "bubble", "panel"],
            "checkpoint_selection": "segmentation mAP@[.50:.95]",
            "typography_distillation": args.typography_distillation,
        },
    )

    wrapper._align_num_classes_from_dataset(str(args.dataset_dir.resolve()))
    module, datamodule, typography_metadata = build_training_modules(
        args,
        wrapper.model_config,
        train_config,
    )
    # COCO mask evaluation is expensive and the full validation pass already
    # runs at every configured interval. Skip Lightning's redundant startup
    # sanity validation so all ranks begin training immediately.
    trainer_kwargs = {
        "num_sanity_val_steps": 0,
        "check_val_every_n_epoch": args.eval_interval,
        "strategy": (
            "auto"
            if args.devices == 1
            else DDPStrategy(
                find_unused_parameters=True,
                timeout=timedelta(minutes=args.ddp_timeout_minutes),
            )
        ),
    }
    if args.limit_train_batches is not None:
        trainer_kwargs["limit_train_batches"] = args.limit_train_batches
    if args.limit_val_batches is not None:
        trainer_kwargs["limit_val_batches"] = args.limit_val_batches
    if args.skip_distributed_validation:
        trainer_kwargs["limit_val_batches"] = 0
    trainer = build_trainer(train_config, wrapper.model_config, **trainer_kwargs)

    # RF-DETR 1.7 tracks the best checkpoint by box AP even for segmentation
    # variants. Change only the monitor keys before fit so all RF-DETR checkpoint
    # serialization and EMA handling remain intact.
    best_callback = configure_segmentation_checkpoint(trainer, args.use_ema)
    if args.skip_distributed_validation:
        trainer.callbacks.append(
            PeriodicRFDETRCheckpoint(
                best_callback,
                args.output_dir,
                args.periodic_checkpoint_interval,
            )
        )

    write_launch_metadata(
        args,
        wrapper.model_config,
        train_config,
        typography_metadata,
    )
    if os.environ.get("LOCAL_RANK", "0") == "0":
        print(
            json.dumps(
                {
                    "dataset": str(args.dataset_dir.resolve()),
                    "output": str(args.output_dir.resolve()),
                    "resolution": args.resolution,
                    "epochs": args.epochs,
                    "micro_batch_per_gpu": args.batch_size,
                    "global_batch": args.batch_size
                    * args.grad_accum_steps
                    * args.devices,
                    "devices": args.devices,
                    "num_select": args.num_select,
                    "eval_max_dets": args.eval_max_dets,
                    "ddp_timeout_minutes": args.ddp_timeout_minutes,
                    "best_metric": best_callback.monitor,
                    "best_ema_metric": best_callback._monitor_ema,
                    "distributed_validation": not args.skip_distributed_validation,
                    "periodic_checkpoint_interval": args.periodic_checkpoint_interval,
                    "typography_distillation": typography_metadata,
                },
                indent=2,
            ),
            flush=True,
        )

    resume_checkpoint = (
        str(args.resume_checkpoint.resolve())
        if args.resume_checkpoint is not None
        else train_config.resume or None
    )
    trainer.fit(module, datamodule, ckpt_path=resume_checkpoint)

    if not args.skip_distributed_validation or not trainer.is_global_zero:
        return

    latest_path = args.output_dir / "checkpoint_latest.pth"
    final_path = args.output_dir / "checkpoint_best_total.pth"
    if not latest_path.is_file():
        raise RuntimeError(f"final periodic checkpoint was not written: {latest_path}")
    final_epoch = checkpoint_epoch(latest_path)
    if final_epoch != args.epochs - 1:
        raise RuntimeError(
            f"final checkpoint epoch mismatch: expected {args.epochs - 1}, got {final_epoch}"
        )
    shutil.copy2(latest_path, final_path)

    if args.skip_final_validation:
        print(
            json.dumps(
                {
                    "status": "checkpoint_complete_validation_skipped",
                    "checkpoint": final_path.name,
                    "checkpoint_epoch": final_epoch,
                    "checkpoint_sha256": sha256_file(final_path),
                },
                indent=2,
            ),
            flush=True,
        )
        return

    # Run validation in a clean process after DDP has torn down. Keeping this
    # parent process alive also prevents the external watcher from treating the
    # short train/eval transition as premature termination.
    command = [
        sys.executable,
        str(Path(__file__).resolve()),
        "--dataset-dir",
        str(args.dataset_dir.resolve()),
        "--output-dir",
        str(args.output_dir.resolve()),
        "--resolution",
        str(args.resolution),
        "--batch-size",
        str(args.batch_size),
        "--devices",
        "1",
        "--num-workers",
        str(args.num_workers),
        "--num-select",
        str(args.num_select),
        "--eval-max-dets",
        str(args.eval_max_dets),
        "--no-run-test",
        "--no-use-ema",
        "--eval-only-checkpoint",
        str(final_path.resolve()),
    ]
    command.extend(typography_child_arguments(args))
    child_env = os.environ.copy()
    for name in (
        "LOCAL_RANK",
        "RANK",
        "WORLD_SIZE",
        "GROUP_RANK",
        "ROLE_RANK",
        "LOCAL_WORLD_SIZE",
        "MASTER_ADDR",
        "MASTER_PORT",
        "TORCHELASTIC_RUN_ID",
        "TORCHELASTIC_RESTART_COUNT",
        "TORCHELASTIC_MAX_RESTARTS",
    ):
        child_env.pop(name, None)
    subprocess.run(command, check=True, env=child_env)


if __name__ == "__main__":
    main()
