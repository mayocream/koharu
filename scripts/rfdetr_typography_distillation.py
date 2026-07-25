"""Auxiliary dense typography supervision for RF-DETR 1.7.

This module intentionally depends only on PyTorch at import time.  Factory
functions receive RF-DETR's Lightning classes from the training entry point so
the target/head math can be tested without installing the full RF-DETR stack.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import torch
import torch.nn.functional as F
from torch import nn

GOLD_MARKER = 0
WEAK_POSITIVE_MARKER = 1
IGNORE_MARKER = 2
PSEUDO_INSTANCE_MARKER = 3
INSTANCE_FIELDS = frozenset(
    {"labels", "boxes", "masks", "area", "iscrowd", "keypoints"}
)


@dataclass(frozen=True)
class TypographyDistillationConfig:
    feature_channels: int
    feature_levels: int
    head_channels: int = 96
    output_scale: int = 4
    dilation: int = 2
    loss_weight: float = 0.75
    teacher_positive_weight: float = 0.30
    negative_weight: float = 0.25
    head_warmup_epochs: int = 1
    db_slope: float = 20.0
    typography_labels: tuple[int, ...] = (0, 1)

    def validate(self) -> None:
        if (
            self.feature_channels <= 0
            or self.feature_levels <= 0
            or self.head_channels <= 0
        ):
            raise ValueError("feature/head dimensions must be positive")
        if self.output_scale not in {1, 2, 4}:
            raise ValueError("output_scale must be one of 1, 2, or 4")
        if self.dilation < 0:
            raise ValueError("dilation must be non-negative")
        if self.loss_weight < 0 or self.db_slope <= 0:
            raise ValueError("loss_weight must be non-negative and db_slope positive")
        if not 0.0 < self.teacher_positive_weight <= 1.0:
            raise ValueError("teacher_positive_weight must be in (0, 1]")
        if not 0.0 < self.negative_weight <= 1.0:
            raise ValueError("negative_weight must be in (0, 1]")
        if self.head_warmup_epochs < 0:
            raise ValueError("head_warmup_epochs must be non-negative")
        if not self.typography_labels:
            raise ValueError("at least one typography label is required")


class ConvNormActivation(nn.Sequential):
    def __init__(
        self, in_channels: int, out_channels: int, kernel_size: int = 3
    ) -> None:
        groups = min(16, out_channels)
        while out_channels % groups:
            groups -= 1
        super().__init__(
            nn.Conv2d(
                in_channels,
                out_channels,
                kernel_size,
                padding=kernel_size // 2,
                bias=False,
            ),
            nn.GroupNorm(groups, out_channels),
            nn.SiLU(inplace=True),
        )


class DenseTypographyHead(nn.Module):
    """Fuse RF-DETR pyramid features into region, threshold, and ink maps."""

    def __init__(
        self,
        in_channels: int,
        feature_levels: int,
        hidden_channels: int,
        output_scale: int,
    ) -> None:
        super().__init__()
        self.output_scale = output_scale
        self.lateral = nn.ModuleList(
            nn.Conv2d(in_channels, hidden_channels, 1) for _ in range(feature_levels)
        )
        self.refine = nn.Sequential(
            ConvNormActivation(hidden_channels, hidden_channels),
            ConvNormActivation(hidden_channels, hidden_channels),
        )
        self.output = nn.Conv2d(hidden_channels, 3, 1)
        self.reset_parameters()

    def reset_parameters(self) -> None:
        for module in self.modules():
            if isinstance(module, nn.Conv2d):
                nn.init.kaiming_normal_(
                    module.weight, mode="fan_out", nonlinearity="relu"
                )
                if module.bias is not None:
                    nn.init.zeros_(module.bias)
        # Sparse-positive priors for region and ink; threshold starts at 0.5.
        with torch.no_grad():
            self.output.bias.copy_(torch.tensor((-2.2, 0.0, -2.2)))

    def forward(self, features: Sequence[torch.Tensor]) -> dict[str, torch.Tensor]:
        if len(features) != len(self.lateral):
            raise ValueError(
                f"expected {len(self.lateral)} feature levels, got {len(features)}"
            )
        target_size = features[0].shape[-2:]
        fused = None
        for feature, projection in zip(features, self.lateral, strict=True):
            value = projection(feature)
            if value.shape[-2:] != target_size:
                value = F.interpolate(
                    value, size=target_size, mode="bilinear", align_corners=False
                )
            fused = value if fused is None else fused + value
        fused = self.refine(fused / len(features))
        logits = self.output(fused)
        if self.output_scale != 1:
            # Upsample only the three output maps. Keeping the hidden feature at
            # P4 resolution avoids a large 96-channel allocation at 1152 px.
            logits = F.interpolate(
                logits,
                scale_factor=self.output_scale,
                mode="bilinear",
                align_corners=False,
            )
        region_logits, threshold_logits, ink_logits = logits.split(1, dim=1)
        return {
            "region_logits": region_logits,
            "threshold_logits": threshold_logits,
            "ink_logits": ink_logits,
        }


def _labels_in(labels: torch.Tensor, selected: tuple[int, ...]) -> torch.Tensor:
    keep = torch.zeros_like(labels, dtype=torch.bool)
    for label in selected:
        keep |= labels == label
    return keep


def _union_masks(
    masks: torch.Tensor,
    selected: torch.Tensor,
    output_size: tuple[int, int],
) -> torch.Tensor:
    if masks.ndim != 3:
        raise ValueError(
            f"target masks must have [N,H,W] shape, got {tuple(masks.shape)}"
        )
    if selected.numel() != masks.shape[0]:
        raise ValueError("mask selector length does not match instance masks")
    if not bool(selected.any()):
        return torch.zeros((1, *output_size), dtype=torch.float32, device=masks.device)
    union = masks[selected].any(dim=0, keepdim=True).float()
    # Max pooling preserves very small typography while reducing target size.
    return F.adaptive_max_pool2d(union, output_size)


def _dilate(mask: torch.Tensor, radius: int) -> torch.Tensor:
    if radius == 0:
        return mask
    kernel = radius * 2 + 1
    return F.max_pool2d(mask, kernel_size=kernel, stride=1, padding=radius)


def build_typography_targets(
    targets: Sequence[dict[str, torch.Tensor]],
    valid_pixels: torch.Tensor,
    output_size: tuple[int, int],
    config: TypographyDistillationConfig,
) -> dict[str, torch.Tensor]:
    """Build gold ink, mixed region, and uncertainty-aware weight maps."""

    valid = F.interpolate(
        valid_pixels.unsqueeze(1).float(),
        size=output_size,
        mode="nearest",
    )
    regions: list[torch.Tensor] = []
    inks: list[torch.Tensor] = []
    region_weights: list[torch.Tensor] = []
    ink_weights: list[torch.Tensor] = []

    for index, target in enumerate(targets):
        labels = target["labels"]
        masks = target["masks"]
        markers = target.get("iscrowd")
        if markers is None:
            markers = torch.zeros_like(labels)
        typography = _labels_in(labels, config.typography_labels)
        gold_selector = typography & (markers == GOLD_MARKER)
        weak_selector = (markers == WEAK_POSITIVE_MARKER) | (
            markers == PSEUDO_INSTANCE_MARKER
        )
        ignore_selector = markers == IGNORE_MARKER

        gold_ink = _union_masks(masks, gold_selector, output_size)
        gold_region = _dilate(gold_ink, config.dilation)
        weak_region = _union_masks(masks, weak_selector, output_size)
        ignore_region = _union_masks(masks, ignore_selector, output_size)
        region = torch.maximum(gold_region, weak_region)

        valid_i = valid[index]
        region_weight = valid_i * config.negative_weight
        region_weight = torch.where(
            ignore_region.bool() & ~region.bool(),
            torch.zeros_like(region_weight),
            region_weight,
        )
        region_weight = torch.where(
            weak_region.bool(),
            torch.full_like(region_weight, config.teacher_positive_weight),
            region_weight,
        )
        region_weight = torch.where(
            gold_region.bool(), torch.ones_like(region_weight), region_weight
        )

        # PP polygons describe filled layout regions, not glyph ink.  Ignore all
        # teacher-covered pixels for the ink branch unless gold overrides them.
        teacher_coverage = weak_region.bool() | ignore_region.bool()
        ink_weight = valid_i * config.negative_weight
        ink_weight = torch.where(
            teacher_coverage & ~gold_ink.bool(),
            torch.zeros_like(ink_weight),
            ink_weight,
        )
        ink_weight = torch.where(
            gold_ink.bool(), torch.ones_like(ink_weight), ink_weight
        )

        regions.append(region)
        inks.append(gold_ink)
        region_weights.append(region_weight)
        ink_weights.append(ink_weight)

    return {
        "region": torch.stack(regions),
        "ink": torch.stack(inks),
        "region_weight": torch.stack(region_weights),
        "ink_weight": torch.stack(ink_weights),
    }


def weighted_bce_with_logits(
    logits: torch.Tensor,
    target: torch.Tensor,
    weight: torch.Tensor,
) -> torch.Tensor:
    loss = F.binary_cross_entropy_with_logits(
        logits.float(), target.float(), reduction="none"
    )
    return (loss * weight).sum() / weight.sum().clamp_min(1.0)


def weighted_dice_loss(
    logits: torch.Tensor,
    target: torch.Tensor,
    weight: torch.Tensor,
) -> torch.Tensor:
    probability = logits.float().sigmoid()
    target = target.float()
    probability = probability.flatten(1)
    target = target.flatten(1)
    weight = weight.flatten(1)
    intersection = (probability * target * weight).sum(dim=1)
    denominator = (probability * weight).sum(dim=1) + (target * weight).sum(dim=1)
    return (1.0 - (2.0 * intersection + 1.0) / (denominator + 1.0)).mean()


def typography_losses(
    outputs: dict[str, torch.Tensor],
    targets: dict[str, torch.Tensor],
    db_slope: float,
) -> tuple[torch.Tensor, dict[str, torch.Tensor]]:
    region_logits = outputs["region_logits"]
    threshold_logits = outputs["threshold_logits"]
    ink_logits = outputs["ink_logits"]
    region = targets["region"]
    ink = targets["ink"]
    region_weight = targets["region_weight"]
    ink_weight = targets["ink_weight"]

    binary_logits = db_slope * (region_logits.sigmoid() - threshold_logits.sigmoid())
    region_bce = weighted_bce_with_logits(region_logits, region, region_weight)
    region_dice = weighted_dice_loss(region_logits, region, region_weight)
    binary_bce = weighted_bce_with_logits(binary_logits, region, region_weight)
    binary_dice = weighted_dice_loss(binary_logits, region, region_weight)
    ink_bce = weighted_bce_with_logits(ink_logits, ink, ink_weight)
    ink_dice = weighted_dice_loss(ink_logits, ink, ink_weight)
    threshold_prior = (
        (threshold_logits.sigmoid() - 0.5).abs() * region_weight
    ).sum() / region_weight.sum().clamp_min(1.0)

    components = {
        "dense_region_bce": region_bce,
        "dense_region_dice": region_dice,
        "dense_binary_bce": binary_bce,
        "dense_binary_dice": binary_dice,
        "dense_ink_bce": ink_bce,
        "dense_ink_dice": ink_dice,
        "dense_threshold_prior": threshold_prior,
    }
    total = (
        region_bce
        + region_dice
        + binary_bce
        + binary_dice
        + ink_bce
        + ink_dice
        + 0.05 * threshold_prior
    )
    return total, components


def dense_batch_metrics(
    region_logits: torch.Tensor,
    target: torch.Tensor,
    valid_weight: torch.Tensor,
) -> dict[str, torch.Tensor]:
    predicted = region_logits.sigmoid() >= 0.5
    truth = target.bool()
    valid = valid_weight > 0
    true_positive = (predicted & truth & valid).sum().float()
    false_positive = (predicted & ~truth & valid).sum().float()
    false_negative = (~predicted & truth & valid).sum().float()
    precision = true_positive / (true_positive + false_positive).clamp_min(1.0)
    recall = true_positive / (true_positive + false_negative).clamp_min(1.0)
    f1 = 2.0 * precision * recall / (precision + recall).clamp_min(1e-6)
    return {
        "dense_precision": precision,
        "dense_recall": recall,
        "dense_f1": f1,
    }


def filter_teacher_instances(targets: Sequence[dict[str, Any]]) -> list[dict[str, Any]]:
    """Keep gold and novel pseudo instances for RF-DETR matching."""

    filtered_targets: list[dict[str, Any]] = []
    for target in targets:
        labels = target["labels"]
        markers = target.get("iscrowd")
        if markers is None:
            filtered_targets.append(target)
            continue
        keep = (markers == GOLD_MARKER) | (markers == PSEUDO_INSTANCE_MARKER)
        filtered: dict[str, Any] = {}
        for key, value in target.items():
            if (
                key in INSTANCE_FIELDS
                and isinstance(value, torch.Tensor)
                and value.shape[:1] == labels.shape[:1]
            ):
                filtered[key] = value[keep]
            else:
                filtered[key] = value
        filtered["iscrowd"] = torch.zeros_like(markers[keep])
        filtered_targets.append(filtered)
    return filtered_targets


class IncludeCrowdTeacherAnnotations:
    """Adapt RF-DETR's COCO converter without changing its augmentation code."""

    def __init__(self, base_converter: Any) -> None:
        self.base_converter = base_converter

    def __call__(
        self, image: Any, target: dict[str, Any]
    ) -> tuple[Any, dict[str, torch.Tensor]]:
        width, height = image.size
        annotations = target["annotations"]
        markers = torch.tensor(
            [int(annotation.get("iscrowd", 0)) for annotation in annotations],
            dtype=torch.int64,
        )
        boxes = torch.as_tensor(
            [annotation["bbox"] for annotation in annotations],
            dtype=torch.float32,
        ).reshape(-1, 4)
        boxes[:, 2:] += boxes[:, :2]
        boxes[:, 0::2].clamp_(min=0, max=width)
        boxes[:, 1::2].clamp_(min=0, max=height)
        keep = (boxes[:, 3] > boxes[:, 1]) & (boxes[:, 2] > boxes[:, 0])

        inclusive_target = dict(target)
        inclusive_target["annotations"] = [
            {**annotation, "iscrowd": 0} for annotation in annotations
        ]
        image, converted = self.base_converter(image, inclusive_target)
        converted["iscrowd"] = markers[keep]
        if converted["iscrowd"].shape != converted["labels"].shape:
            raise RuntimeError("teacher markers lost alignment during COCO conversion")
        return image, converted


def make_distillation_datamodule(base_class: type) -> type:
    class TypographyDistillationDataModule(base_class):
        def __init__(self, *args: Any, **kwargs: Any) -> None:
            super().__init__(*args, **kwargs)
            self._teacher_converter_installed = False

        def setup(self, stage: str) -> None:
            super().setup(stage)
            if stage != "fit" or self._teacher_converter_installed:
                return
            dataset = self._dataset_train
            if dataset is None or not hasattr(dataset, "prepare"):
                raise RuntimeError(
                    "RF-DETR training dataset does not expose a COCO converter"
                )
            dataset.prepare = IncludeCrowdTeacherAnnotations(dataset.prepare)
            self._teacher_converter_installed = True

    TypographyDistillationDataModule.__name__ = "TypographyDistillationDataModule"
    return TypographyDistillationDataModule


def _load_typography_head_from_checkpoint(
    head: nn.Module,
    checkpoint: str | Path | None,
) -> bool:
    if checkpoint is None:
        return False
    path = Path(checkpoint)
    if not path.is_file():
        return False
    payload = torch.load(path, map_location="cpu", weights_only=False)
    state = payload.get("model", payload.get("state_dict", payload))
    if not isinstance(state, dict):
        return False
    prefixes = (
        "typography_head.",
        "model.typography_head.",
        "model._orig_mod.typography_head.",
    )
    head_state: dict[str, torch.Tensor] = {}
    for key, value in state.items():
        for prefix in prefixes:
            if str(key).startswith(prefix):
                head_state[str(key).removeprefix(prefix)] = value
                break
    if not head_state:
        return False
    head.load_state_dict(head_state, strict=True)
    return True


def make_distillation_model_module(base_class: type) -> type:
    class TypographyDistillationModelModule(base_class):
        def __init__(
            self,
            model_config: Any,
            train_config: Any,
            typography_config: TypographyDistillationConfig,
        ) -> None:
            typography_config.validate()
            super().__init__(model_config, train_config)
            self.typography_config = typography_config
            detector = getattr(self.model, "_orig_mod", self.model)
            detector.typography_head = DenseTypographyHead(
                typography_config.feature_channels,
                typography_config.feature_levels,
                typography_config.head_channels,
                typography_config.output_scale,
            )
            self._typography_features: list[torch.Tensor] | None = None
            detector.backbone.register_forward_hook(self._capture_backbone_features)
            self.typography_head_restored = _load_typography_head_from_checkpoint(
                detector.typography_head,
                getattr(model_config, "pretrain_weights", None),
            )

        def _detector(self) -> nn.Module:
            return getattr(self.model, "_orig_mod", self.model)

        def _capture_backbone_features(
            self,
            module: nn.Module,
            inputs: tuple[Any, ...],
            output: Any,
        ) -> None:
            del module, inputs
            features = output[0]
            self._typography_features = [feature.tensors for feature in features]

        def _dense_forward(
            self, detach_features: bool = False
        ) -> dict[str, torch.Tensor]:
            features = self._typography_features
            self._typography_features = None
            if features is None:
                raise RuntimeError("RF-DETR backbone hook did not capture features")
            if detach_features:
                features = [feature.detach() for feature in features]
            return self._detector().typography_head(features)

        def training_step(
            self, batch: tuple[Any, Sequence[dict[str, Any]]], batch_idx: int
        ) -> torch.Tensor:
            del batch_idx
            samples, targets = batch
            pseudo_instances = sum(
                int(
                    (
                        target.get("iscrowd", torch.zeros_like(target["labels"]))
                        == PSEUDO_INSTANCE_MARKER
                    ).sum()
                )
                for target in targets
            )
            detector_targets = filter_teacher_instances(targets)
            outputs = self.model(samples, detector_targets)
            warmup = int(self.current_epoch) < self.typography_config.head_warmup_epochs

            if warmup:
                rf_loss_dict: dict[str, torch.Tensor] = {}
                rf_loss = outputs["pred_logits"].new_zeros(())
            else:
                rf_loss_dict = self.criterion(outputs, detector_targets)
                weight_dict = self.criterion.weight_dict
                rf_loss = sum(
                    rf_loss_dict[key] * weight_dict[key]
                    for key in rf_loss_dict
                    if key in weight_dict
                )

            dense_outputs = self._dense_forward(detach_features=warmup)
            output_size = dense_outputs["region_logits"].shape[-2:]
            dense_targets = build_typography_targets(
                targets,
                ~samples.mask,
                output_size,
                self.typography_config,
            )
            dense_loss, dense_components = typography_losses(
                dense_outputs,
                dense_targets,
                self.typography_config.db_slope,
            )
            total_loss = rf_loss + self.typography_config.loss_weight * dense_loss
            batch_size = len(targets)
            log_on_step = bool(self.train_config.train_log_on_step)
            sync_dist = bool(self.train_config.train_log_sync_dist)
            logs = {f"train/{key}": value for key, value in rf_loss_dict.items()}
            logs.update(
                {f"train/{key}": value for key, value in dense_components.items()}
            )
            logs["train/rf_loss"] = rf_loss
            logs["train/dense_loss"] = dense_loss
            self.log_dict(
                logs,
                on_step=log_on_step,
                on_epoch=True,
                sync_dist=sync_dist,
                batch_size=batch_size,
            )
            self.log(
                "train/loss",
                total_loss,
                prog_bar=True,
                on_step=log_on_step,
                on_epoch=True,
                sync_dist=sync_dist,
                batch_size=batch_size,
            )
            self.log(
                "train/dense_warmup",
                float(warmup),
                on_step=False,
                on_epoch=True,
                sync_dist=False,
                batch_size=batch_size,
            )
            self.log(
                "train/pseudo_instances_per_batch",
                float(pseudo_instances),
                on_step=log_on_step,
                on_epoch=True,
                sync_dist=sync_dist,
                batch_size=batch_size,
            )
            optimizer = self.optimizers()
            if isinstance(optimizer, list):
                optimizer = optimizer[0]
            group_lrs = [
                group["lr"] for group in optimizer.param_groups if "lr" in group
            ]
            if group_lrs:
                self.log(
                    "train/lr",
                    group_lrs[0],
                    prog_bar=True,
                    on_step=True,
                    on_epoch=False,
                )
                self.log(
                    "train/lr_min",
                    min(group_lrs),
                    prog_bar=True,
                    on_step=True,
                    on_epoch=False,
                )
                self.log(
                    "train/lr_max",
                    max(group_lrs),
                    prog_bar=True,
                    on_step=True,
                    on_epoch=False,
                )
            return total_loss / self.trainer.accumulate_grad_batches

        def validation_step(
            self, batch: tuple[Any, Sequence[dict[str, Any]]], batch_idx: int
        ) -> dict[str, Any]:
            samples, targets = batch
            detector_targets = filter_teacher_instances(targets)
            results = super().validation_step((samples, detector_targets), batch_idx)
            dense_outputs = self._dense_forward()
            dense_targets = build_typography_targets(
                targets,
                ~samples.mask,
                dense_outputs["region_logits"].shape[-2:],
                self.typography_config,
            )
            dense_loss, dense_components = typography_losses(
                dense_outputs,
                dense_targets,
                self.typography_config.db_slope,
            )
            metrics = dense_batch_metrics(
                dense_outputs["region_logits"],
                dense_targets["region"],
                dense_targets["region_weight"],
            )
            batch_size = len(targets)
            self.log(
                "val/dense_loss",
                dense_loss,
                on_epoch=True,
                sync_dist=True,
                batch_size=batch_size,
            )
            for name, value in metrics.items():
                self.log(
                    f"val/{name}",
                    value,
                    on_epoch=True,
                    sync_dist=True,
                    batch_size=batch_size,
                )
            for name, value in dense_components.items():
                self.log(
                    f"val/{name}",
                    value,
                    on_epoch=True,
                    sync_dist=True,
                    batch_size=batch_size,
                )
            return results

        def test_step(
            self, batch: tuple[Any, Sequence[dict[str, Any]]], batch_idx: int
        ) -> dict[str, Any]:
            samples, targets = batch
            detector_targets = filter_teacher_instances(targets)
            results = super().test_step((samples, detector_targets), batch_idx)
            # Clear hook output; test metrics remain RF-DETR's official COCO metrics.
            self._typography_features = None
            return results

    TypographyDistillationModelModule.__name__ = "TypographyDistillationModelModule"
    return TypographyDistillationModelModule


def estimated_head_parameters(config: TypographyDistillationConfig) -> int:
    config.validate()
    head = DenseTypographyHead(
        config.feature_channels,
        config.feature_levels,
        config.head_channels,
        config.output_scale,
    )
    return sum(parameter.numel() for parameter in head.parameters())


__all__ = [
    "DenseTypographyHead",
    "IncludeCrowdTeacherAnnotations",
    "TypographyDistillationConfig",
    "build_typography_targets",
    "dense_batch_metrics",
    "estimated_head_parameters",
    "filter_teacher_instances",
    "make_distillation_datamodule",
    "make_distillation_model_module",
    "typography_losses",
]
