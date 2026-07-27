# /// script
# requires-python = ">=3.12,<3.15"
# dependencies = [
#   "einops>=0.8.1",
#   "huggingface-hub>=0.30",
#   "numpy>=2.0",
#   "opencv-python-headless>=4.10",
#   "pillow>=11.0",
#   "pyclipper>=1.3",
#   "safetensors>=0.5",
#   "shapely>=2.0",
#   "torch>=2.7",
#   "torchsummary>=1.5",
#   "torchvision>=0.22",
#   "tqdm>=4.67",
# ]
# ///
"""Generate PyTorch teacher predictions for Manga109 Segmentation.

The runner deliberately writes one atomic record per page so a long Manga109
pass can resume without replaying completed inference. It uses PyTorch for both
teachers and loads the split SafeTensors published in these repositories:

* mayocream/coo-comic-onomatopoeia-safetensors (MTSv3)
* mayocream/comic-text-detector (YOLOv5 + U-Net + DBNet)

MTSv3's original MaskTextSpotter-v3 checkout requires legacy compiled PyTorch
operators that are not used by its detection-only COO checkpoint. The compact
module tree below preserves the checkpoint parameter paths and forward order
for the ResNet-50/FPN/segmentation path while avoiding those unused operators.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib
import json
import math
import os
import sys
import time
from collections import OrderedDict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

import cv2
import numpy as np
import pyclipper
import torch
import torch.nn.functional as F
from huggingface_hub import hf_hub_download
from safetensors.torch import load_file
from torch import nn
from torchvision.models import resnet50
from tqdm import tqdm


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_IMAGES_ROOT = REPOSITORY_ROOT / "data" / "Manga109_released_2026_05_21" / "images"
DEFAULT_OUTPUT = REPOSITORY_ROOT / "data" / "koharu-manga-layoutseg" / "teachers"
DEFAULT_COMIC_TEXT_SOURCE = REPOSITORY_ROOT / "temp" / "comic-text-detector"

MTSV3_REPOSITORY = "mayocream/coo-comic-onomatopoeia-safetensors"
MTSV3_FILENAME = "mtsv3/model.safetensors"
COMIC_TEXT_REPOSITORY = "mayocream/comic-text-detector"
COMIC_TEXT_FILENAMES = {
    "yolo": "yolo-v5.safetensors",
    "unet": "unet.safetensors",
    "dbnet": "dbnet.safetensors",
}

# Exact upstream settings from the reported-best COO test configuration.
MTSV3_MIN_SIZE = 1440
MTSV3_MAX_SIZE = 4000
MTSV3_SIZE_DIVISIBILITY = 32
MTSV3_PIXEL_MEAN = (102.9801, 115.9465, 122.7717)
MTSV3_BINARY_THRESHOLD = 0.1
MTSV3_BOX_THRESHOLD = 0.1
MTSV3_MINIMUM_SIDE = 5.0
MTSV3_POLYGON_EXPAND_RATIO = 3.0
MTSV3_BOX_EXPAND_RATIO = 1.5
MTSV3_TOP_N = 1000


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--images-root", type=Path, default=DEFAULT_IMAGES_ROOT)
    parser.add_argument(
        "--manifest",
        type=Path,
        help="Optional JSONL with an `image` path relative to --images-root.",
    )
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--comic-text-source", type=Path, default=DEFAULT_COMIC_TEXT_SOURCE)
    parser.add_argument(
        "--teachers",
        choices=("both", "mtsv3", "comic-text"),
        default="both",
    )
    parser.add_argument("--device", default="cuda:0")
    parser.add_argument("--limit", type=int)
    parser.add_argument(
        "--num-shards",
        type=int,
        default=1,
        help="Number of disjoint page shards for concurrent workers.",
    )
    parser.add_argument(
        "--shard-index",
        type=int,
        default=0,
        help="Zero-based shard handled by this worker.",
    )
    parser.add_argument("--overwrite", action="store_true")
    parser.add_argument(
        "--comic-text-size",
        type=int,
        default=1280,
        help="Square inference size for Comic Text Detector.",
    )
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
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, path)


def atomic_write_png(path: Path, image: np.ndarray) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.stem}.{os.getpid()}.tmp.png")
    if not cv2.imwrite(str(temporary), image):
        raise OSError(f"failed to write {temporary}")
    os.replace(temporary, path)


def json_value(value: Any) -> Any:
    if isinstance(value, np.ndarray):
        return value.tolist()
    if isinstance(value, np.generic):
        return value.item()
    if isinstance(value, dict):
        return {str(key): json_value(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [json_value(item) for item in value]
    return value


def load_manifest(images_root: Path, manifest: Path | None) -> list[Path]:
    if manifest is None:
        paths = [
            path.relative_to(images_root)
            for path in images_root.glob("*/*")
            if path.is_file() and path.suffix.lower() in {".jpg", ".jpeg", ".png", ".webp"}
        ]
        return sorted(paths, key=lambda path: path.as_posix())

    paths: list[Path] = []
    with manifest.open(encoding="utf-8") as file:
        for line_number, line in enumerate(file, 1):
            if not line.strip():
                continue
            record = json.loads(line)
            if not isinstance(record, dict) or not isinstance(record.get("image"), str):
                raise ValueError(f"{manifest}:{line_number}: expected an object with string `image`")
            relative = Path(record["image"])
            if relative.is_absolute() or ".." in relative.parts:
                raise ValueError(f"{manifest}:{line_number}: image must be a safe relative path")
            paths.append(relative)
    return paths


class FrozenBatchNorm2d(nn.Module):
    """Checkpoint-exact Mask R-CNN Benchmark frozen batch normalization."""

    def __init__(self, channels: int) -> None:
        super().__init__()
        self.register_buffer("weight", torch.ones(channels))
        self.register_buffer("bias", torch.zeros(channels))
        self.register_buffer("running_mean", torch.zeros(channels))
        self.register_buffer("running_var", torch.ones(channels))

    def forward(self, values: torch.Tensor) -> torch.Tensor:
        scale = self.weight * self.running_var.rsqrt()
        bias = self.bias - self.running_mean * scale
        return values * scale.reshape(1, -1, 1, 1) + bias.reshape(1, -1, 1, 1)


class MtsResNetBody(nn.Module):
    """ResNet-50 body matching Mask R-CNN Benchmark's stride-in-1x1 variant."""

    def __init__(self) -> None:
        super().__init__()
        base = resnet50(weights=None, norm_layer=FrozenBatchNorm2d)
        # torchvision's ResNet-v1.5 puts the downsampling stride in the 3x3
        # convolution; MTSv3 uses the Caffe2 stride-in-1x1 variant.
        for layer in (base.layer2, base.layer3, base.layer4):
            first = layer[0]
            first.conv1.stride = (2, 2)
            first.conv2.stride = (1, 1)
        stem = nn.Module()
        stem.add_module("conv1", base.conv1)
        stem.add_module("bn1", base.bn1)
        self.stem = stem
        self.layer1 = base.layer1
        self.layer2 = base.layer2
        self.layer3 = base.layer3
        self.layer4 = base.layer4

    def forward(self, values: torch.Tensor) -> list[torch.Tensor]:
        values = self.stem.conv1(values)
        values = self.stem.bn1(values)
        values = F.relu_(values)
        values = F.max_pool2d(values, kernel_size=3, stride=2, padding=1)
        outputs = []
        for layer in (self.layer1, self.layer2, self.layer3, self.layer4):
            values = layer(values)
            outputs.append(values)
        return outputs


class MtsFpn(nn.Module):
    # Ported from MaskTextSpotter-v3's FPN at COO commit d8028f0:
    # https://github.com/ku21fan/COO-Comic-Onomatopoeia/blob/d8028f015b8ce99a4dd798427342f97087529357/MTSv3/maskrcnn_benchmark/modeling/backbone/fpn.py
    def __init__(self) -> None:
        super().__init__()
        in_channels = (256, 512, 1024, 2048)
        for index, channels in enumerate(in_channels, 1):
            self.add_module(f"fpn_inner{index}", nn.Conv2d(channels, 256, 1))
            self.add_module(f"fpn_layer{index}", nn.Conv2d(256, 256, 3, padding=1))

    def forward(self, features: list[torch.Tensor]) -> tuple[torch.Tensor, ...]:
        last_inner = self.fpn_inner4(features[-1])
        results = [self.fpn_layer4(last_inner)]
        for feature, index in zip(features[:-1][::-1], (3, 2, 1)):
            inner_top_down = F.interpolate(last_inner, scale_factor=2, mode="nearest")
            inner_lateral = getattr(self, f"fpn_inner{index}")(feature)
            last_inner = inner_lateral + inner_top_down
            results.insert(0, getattr(self, f"fpn_layer{index}")(last_inner))
        results.append(F.max_pool2d(results[-1], 1, 2, 0))
        return tuple(results)


class MtsSegHead(nn.Module):
    # Ported from the COO detection-only segmentation head at commit d8028f0:
    # https://github.com/ku21fan/COO-Comic-Onomatopoeia/blob/d8028f015b8ce99a4dd798427342f97087529357/MTSv3/maskrcnn_benchmark/modeling/segmentation/segmentation.py
    def __init__(self) -> None:
        super().__init__()
        self.fpn_out5 = nn.Sequential(
            nn.Conv2d(256, 64, 3, padding=1, bias=False),
            nn.Upsample(scale_factor=8, mode="nearest"),
        )
        self.fpn_out4 = nn.Sequential(
            nn.Conv2d(256, 64, 3, padding=1, bias=False),
            nn.Upsample(scale_factor=4, mode="nearest"),
        )
        self.fpn_out3 = nn.Sequential(
            nn.Conv2d(256, 64, 3, padding=1, bias=False),
            nn.Upsample(scale_factor=2, mode="nearest"),
        )
        self.fpn_out2 = nn.Conv2d(256, 64, 3, padding=1, bias=False)
        self.seg_out = nn.Sequential(
            nn.Sequential(
                nn.Conv2d(256, 64, 3, padding=1, bias=False),
                nn.BatchNorm2d(64),
                nn.ReLU(inplace=True),
            ),
            nn.ConvTranspose2d(64, 64, 2, 2),
            nn.BatchNorm2d(64),
            nn.ReLU(inplace=True),
            nn.ConvTranspose2d(64, 1, 2, 2),
            nn.Sigmoid(),
        )

    def forward(self, features: tuple[torch.Tensor, ...]) -> torch.Tensor:
        fused = torch.cat(
            (
                self.fpn_out5(features[-2]),
                self.fpn_out4(features[-3]),
                self.fpn_out3(features[-4]),
                self.fpn_out2(features[-5]),
            ),
            dim=1,
        )
        return self.seg_out(fused)


class MtsProposal(nn.Module):
    def __init__(self) -> None:
        super().__init__()
        self.head = MtsSegHead()


class MtsV3Model(nn.Module):
    def __init__(self) -> None:
        super().__init__()
        self.backbone = nn.Sequential(
            OrderedDict((("body", MtsResNetBody()), ("fpn", MtsFpn())))
        )
        self.proposal = MtsProposal()

    def forward(self, values: torch.Tensor) -> torch.Tensor:
        return self.proposal.head(self.backbone(values))


@dataclass
class ResizedImage:
    tensor: torch.Tensor
    resized_width: int
    resized_height: int
    original_width: int
    original_height: int


def resize_short_side(width: int, height: int, minimum: int, maximum: int) -> tuple[int, int]:
    short = min(width, height)
    long = max(width, height)
    size = minimum
    if long / short * size > maximum:
        size = round(maximum * short / long)
    if width < height:
        resized_width = size
        resized_height = int(size * height / width)
    else:
        resized_height = size
        resized_width = int(size * width / height)
    return resized_width, resized_height


def mts_preprocess(image: np.ndarray, device: torch.device) -> ResizedImage:
    original_height, original_width = image.shape[:2]
    resized_width, resized_height = resize_short_side(
        original_width, original_height, MTSV3_MIN_SIZE, MTSV3_MAX_SIZE
    )
    resized = cv2.resize(image, (resized_width, resized_height), interpolation=cv2.INTER_LINEAR)
    values = torch.from_numpy(np.ascontiguousarray(resized.transpose(2, 0, 1))).float()
    mean = torch.tensor(MTSV3_PIXEL_MEAN).reshape(3, 1, 1)
    values = values - mean
    padded_height = math.ceil(resized_height / MTSV3_SIZE_DIVISIBILITY) * MTSV3_SIZE_DIVISIBILITY
    padded_width = math.ceil(resized_width / MTSV3_SIZE_DIVISIBILITY) * MTSV3_SIZE_DIVISIBILITY
    padded = torch.zeros((1, 3, padded_height, padded_width), dtype=torch.float32)
    padded[0, :, :resized_height, :resized_width] = values
    return ResizedImage(
        tensor=padded.to(device, non_blocking=True),
        resized_width=resized_width,
        resized_height=resized_height,
        original_width=original_width,
        original_height=original_height,
    )


def unclip(points: np.ndarray, ratio: float) -> list[np.ndarray]:
    contour = points.reshape(-1, 2)
    area = abs(cv2.contourArea(contour.astype(np.float32)))
    perimeter = cv2.arcLength(contour.astype(np.float32), True)
    if perimeter <= 0:
        return []
    offset = pyclipper.PyclipperOffset()
    offset.AddPath(contour.astype(np.int64).tolist(), pyclipper.JT_ROUND, pyclipper.ET_CLOSEDPOLYGON)
    return [np.asarray(path, dtype=np.float32) for path in offset.Execute(area * ratio / perimeter)]


def mini_box(contour: np.ndarray) -> tuple[np.ndarray, float]:
    rectangle = cv2.minAreaRect(contour.astype(np.float32))
    points = sorted(cv2.boxPoints(rectangle).tolist(), key=lambda point: point[0])
    first, fourth = (0, 1) if points[1][1] > points[0][1] else (1, 0)
    second, third = (2, 3) if points[3][1] > points[2][1] else (3, 2)
    return np.asarray([points[first], points[second], points[third], points[fourth]], dtype=np.float32), min(rectangle[1])


def polygon_score(probability: np.ndarray, box: np.ndarray) -> float:
    height, width = probability.shape
    local = box.copy()
    minimum_x = int(np.clip(np.floor(local[:, 0].min()), 0, width - 1))
    maximum_x = int(np.clip(np.ceil(local[:, 0].max()), 0, width - 1))
    minimum_y = int(np.clip(np.floor(local[:, 1].min()), 0, height - 1))
    maximum_y = int(np.clip(np.ceil(local[:, 1].max()), 0, height - 1))
    if maximum_x < minimum_x or maximum_y < minimum_y:
        return 0.0
    mask = np.zeros((maximum_y - minimum_y + 1, maximum_x - minimum_x + 1), dtype=np.uint8)
    local[:, 0] -= minimum_x
    local[:, 1] -= minimum_y
    cv2.fillPoly(mask, [local.astype(np.int32)], 1)
    return float(cv2.mean(probability[minimum_y : maximum_y + 1, minimum_x : maximum_x + 1], mask)[0])


def mts_postprocess(probability: np.ndarray, resized: ResizedImage) -> list[dict[str, Any]]:
    bitmap = (probability > MTSV3_BINARY_THRESHOLD).astype(np.uint8) * 255
    contours, _ = cv2.findContours(bitmap, cv2.RETR_LIST, cv2.CHAIN_APPROX_NONE)
    x_scale = resized.original_width / resized.resized_width
    y_scale = resized.original_height / resized.resized_height
    detections: list[dict[str, Any]] = []
    for contour in contours:
        box, shortest_side = mini_box(contour)
        if shortest_side < MTSV3_MINIMUM_SIDE:
            continue
        score = polygon_score(probability, box)
        if score < MTSV3_BOX_THRESHOLD:
            continue
        approximation = cv2.approxPolyDP(contour, 0.01 * cv2.arcLength(contour, True), True).reshape(-1, 2)
        if len(approximation) <= 2:
            continue
        polygons = unclip(approximation, MTSV3_POLYGON_EXPAND_RATIO)
        expanded_boxes = unclip(box, MTSV3_BOX_EXPAND_RATIO)
        if len(polygons) != 1 or len(expanded_boxes) != 1:
            continue
        polygon = polygons[0]
        expanded_box, _ = mini_box(expanded_boxes[0].reshape(-1, 1, 2))
        polygon[:, 0] = np.clip(polygon[:, 0], 0, resized.resized_width) * x_scale
        polygon[:, 1] = np.clip(polygon[:, 1], 0, resized.resized_height) * y_scale
        expanded_box[:, 0] = np.clip(np.round(expanded_box[:, 0]), 0, resized.resized_width) * x_scale
        expanded_box[:, 1] = np.clip(np.round(expanded_box[:, 1]), 0, resized.resized_height) * y_scale
        minimum = expanded_box.min(axis=0)
        maximum = expanded_box.max(axis=0)
        detections.append(
            {
                "polygon": polygon.tolist(),
                "rotated_box": expanded_box.tolist(),
                "bbox_xyxy": [float(minimum[0]), float(minimum[1]), float(maximum[0]), float(maximum[1])],
                "score": score,
            }
        )
    detections.sort(key=lambda detection: detection["score"], reverse=True)
    return detections[:MTSV3_TOP_N]


class MtsV3Teacher:
    def __init__(self, device: torch.device) -> None:
        self.device = device
        weights = Path(hf_hub_download(MTSV3_REPOSITORY, MTSV3_FILENAME))
        state = {
            key.removeprefix("module."): value
            for key, value in load_file(weights, device="cpu").items()
        }
        self.model = MtsV3Model()
        model_keys = set(self.model.state_dict())
        state_keys = set(state)
        if model_keys != state_keys:
            raise RuntimeError(
                f"MTSv3 parameter mismatch: missing={sorted(model_keys-state_keys)}, "
                f"unexpected={sorted(state_keys-model_keys)}"
            )
        self.model.load_state_dict(state, strict=True)
        self.model.eval().to(device)
        self.weights = weights

    @torch.inference_mode()
    def predict(self, image: np.ndarray) -> tuple[np.ndarray, list[dict[str, Any]]]:
        resized = mts_preprocess(image, self.device)
        probability = self.model(resized.tensor)[0, 0, : resized.resized_height, : resized.resized_width]
        probability = probability.float().cpu().numpy()
        detections = mts_postprocess(probability, resized)
        probability_original = cv2.resize(
            probability,
            (resized.original_width, resized.original_height),
            interpolation=cv2.INTER_LINEAR,
        )
        return np.clip(np.round(probability_original * 255), 0, 255).astype(np.uint8), detections


def load_comic_text_modules(source: Path) -> tuple[Any, Any, Any]:
    """Load the pinned upstream Python modules without copying or editing them."""
    # The pinned upstream predates NumPy 2.0. Keep compatibility local to this
    # isolated runner rather than modifying the read-only upstream checkout.
    if not hasattr(np, "bool8"):
        np.bool8 = np.bool_  # type: ignore[attr-defined]
    if not hasattr(np, "float_"):
        np.float_ = np.float64  # type: ignore[attr-defined]
    source_string = str(source.resolve())
    if source_string not in sys.path:
        sys.path.insert(0, source_string)
    basemodel = importlib.import_module("basemodel")
    inference = importlib.import_module("inference")
    textmask = importlib.import_module("utils.textmask")
    return basemodel, inference, textmask


def comic_text_yolo_config() -> dict[str, Any]:
    # YOLOv5s topology used by comic-text-detector, with its two block classes.
    return {
        "nc": 2,
        "depth_multiple": 0.33,
        "width_multiple": 0.50,
        "anchors": [
            [10, 13, 16, 30, 33, 23],
            [30, 61, 62, 45, 59, 119],
            [116, 90, 156, 198, 373, 326],
        ],
        "backbone": [
            [-1, 1, "Conv", [64, 6, 2, 2]],
            [-1, 1, "Conv", [128, 3, 2]],
            [-1, 3, "C3", [128]],
            [-1, 1, "Conv", [256, 3, 2]],
            [-1, 6, "C3", [256]],
            [-1, 1, "Conv", [512, 3, 2]],
            [-1, 9, "C3", [512]],
            [-1, 1, "Conv", [1024, 3, 2]],
            [-1, 3, "C3", [1024]],
            [-1, 1, "SPPF", [1024, 5]],
        ],
        "head": [
            [-1, 1, "Conv", [512, 1, 1]],
            [-1, 1, "nn.Upsample", [None, 2, "nearest"]],
            [[-1, 6], 1, "Concat", [1]],
            [-1, 3, "C3", [512, False]],
            [-1, 1, "Conv", [256, 1, 1]],
            [-1, 1, "nn.Upsample", [None, 2, "nearest"]],
            [[-1, 4], 1, "Concat", [1]],
            [-1, 3, "C3", [256, False]],
            [-1, 1, "Conv", [256, 3, 2]],
            [[-1, 14], 1, "Concat", [1]],
            [-1, 3, "C3", [512, False]],
            [-1, 1, "Conv", [512, 3, 2]],
            [[-1, 10], 1, "Concat", [1]],
            [-1, 3, "C3", [1024, False]],
            [[17, 20, 23], 1, "Detect", [2, 3]],
        ],
    }


class ComicTextSplitModel(nn.Module):
    def __init__(self, source: Path, device: torch.device) -> None:
        super().__init__()
        source = source.resolve()
        if not (source / "basemodel.py").is_file():
            raise FileNotFoundError(f"comic-text-detector source checkout is missing: {source}")
        basemodel, _, _ = load_comic_text_modules(source)

        # Keep the complete detector for block predictions while also exposing
        # its five backbone feature maps to the segmentation heads.
        self.backbone = basemodel.Model(comic_text_yolo_config())
        self.text_seg = basemodel.UnetHead(act="leaky")
        self.text_det = basemodel.DBHead(64, act="leaky")

        paths = {
            name: Path(hf_hub_download(COMIC_TEXT_REPOSITORY, filename))
            for name, filename in COMIC_TEXT_FILENAMES.items()
        }
        for module, name in (
            (self.backbone, "yolo"),
            (self.text_seg, "unet"),
            (self.text_det, "dbnet"),
        ):
            state = load_file(paths[name], device="cpu")
            missing, unexpected = module.load_state_dict(state, strict=False)
            if missing or unexpected:
                raise RuntimeError(
                    f"Comic Text Detector {name} parameter mismatch: "
                    f"missing={missing}, unexpected={unexpected}"
                )
        self.backbone.out_indices = [1, 3, 5, 7, 9]
        self.eval().to(device)
        self.weight_paths = paths

    def forward(self, values: torch.Tensor) -> tuple[torch.Tensor, torch.Tensor, torch.Tensor]:
        blocks, features = self.backbone(values, detect=True)
        mask, features = self.text_seg(*features, forward_mode=2)
        lines = self.text_det(*features, step_eval=False)
        return blocks[0], mask, lines


class ComicTextTeacher:
    def __init__(self, source: Path, device: torch.device, size: int) -> None:
        source = source.resolve()
        _, inference, textmask = load_comic_text_modules(source)
        network = ComicTextSplitModel(source, device)
        detector = inference.TextDetector.__new__(inference.TextDetector)
        detector.net = network
        detector.backend = "torch"
        detector.input_size = (size, size)
        detector.device = str(device)
        detector.half = False
        detector.conf_thresh = 0.4
        detector.nms_thresh = 0.35
        detector.seg_rep = inference.SegDetectorRepresenter(thresh=0.3)
        self.detector = detector
        self.refine_mode = textmask.REFINEMASK_ANNOTATION
        self.network = network

    def predict(self, image: np.ndarray) -> tuple[np.ndarray, list[dict[str, Any]]]:
        _, refined, blocks = self.detector(
            image,
            refine_mode=self.refine_mode,
            keep_undetected_mask=True,
        )
        records = [json_value(block.to_dict()) for block in blocks]
        return refined, records


def output_paths(output: Path, relative: Path) -> dict[str, Path]:
    stem = relative.with_suffix("")
    return {
        "record": output / "records" / stem.with_suffix(".json"),
        "mtsv3": output / "masks" / stem.with_name(f"{stem.name}.mtsv3.png"),
        "comic_text": output / "masks" / stem.with_name(f"{stem.name}.comic-text.png"),
    }


def record_complete(paths: dict[str, Path], teachers: str) -> bool:
    if not paths["record"].is_file():
        return False
    if teachers in {"both", "mtsv3"} and not paths["mtsv3"].is_file():
        return False
    if teachers in {"both", "comic-text"} and not paths["comic_text"].is_file():
        return False
    return True


def main() -> None:
    args = parse_args()
    images_root = args.images_root.resolve()
    output = args.output.resolve()
    if not images_root.is_dir():
        raise FileNotFoundError(images_root)
    if args.comic_text_size <= 0 or args.comic_text_size % 64:
        raise ValueError("--comic-text-size must be a positive multiple of 64")
    if args.num_shards <= 0 or not 0 <= args.shard_index < args.num_shards:
        raise ValueError("require --num-shards > 0 and 0 <= --shard-index < --num-shards")
    device = torch.device(args.device)
    if device.type == "cuda" and not torch.cuda.is_available():
        raise RuntimeError("CUDA was requested but PyTorch cannot access it")

    relative_images = load_manifest(images_root, args.manifest)
    if args.limit is not None:
        relative_images = relative_images[: args.limit]
    relative_images = relative_images[args.shard_index :: args.num_shards]
    for relative in relative_images:
        if not (images_root / relative).is_file():
            raise FileNotFoundError(images_root / relative)

    mtsv3 = MtsV3Teacher(device) if args.teachers in {"both", "mtsv3"} else None
    comic_text = (
        ComicTextTeacher(args.comic_text_source, device, args.comic_text_size)
        if args.teachers in {"both", "comic-text"}
        else None
    )
    model_metadata: dict[str, Any] = {
        "schema_version": 1,
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "torch": torch.__version__,
        "device": str(device),
        "gpu": torch.cuda.get_device_name(device) if device.type == "cuda" else None,
        "teachers": args.teachers,
        "mtsv3": (
            {
                "repository": MTSV3_REPOSITORY,
                "filename": MTSV3_FILENAME,
                "sha256": sha256(mtsv3.weights),
                "minimum_size": MTSV3_MIN_SIZE,
                "maximum_size": MTSV3_MAX_SIZE,
            }
            if mtsv3 is not None
            else None
        ),
        "comic_text": (
            {
                "repository": COMIC_TEXT_REPOSITORY,
                "files": {
                    name: {"filename": path.name, "sha256": sha256(path)}
                    for name, path in comic_text.network.weight_paths.items()
                },
                "inference_size": args.comic_text_size,
            }
            if comic_text is not None
            else None
        ),
    }
    output.mkdir(parents=True, exist_ok=True)
    if args.num_shards == 1 or args.shard_index == 0:
        atomic_write_json(output / "teacher_models.json", model_metadata)

    completed = 0
    failed = 0
    elapsed_total = 0.0
    for relative in tqdm(relative_images, desc="PyTorch teacher inference", unit="page"):
        paths = output_paths(output, relative)
        if not args.overwrite and record_complete(paths, args.teachers):
            completed += 1
            continue
        started = time.perf_counter()
        image_path = images_root / relative
        image = cv2.imread(str(image_path), cv2.IMREAD_COLOR)
        if image is None:
            raise OSError(f"failed to decode {image_path}")
        height, width = image.shape[:2]
        payload: dict[str, Any] = {
            "schema_version": 1,
            "image": relative.as_posix(),
            "width": width,
            "height": height,
            "teachers": {},
        }
        try:
            if mtsv3 is not None:
                probability, detections = mtsv3.predict(image)
                atomic_write_png(paths["mtsv3"], probability)
                payload["teachers"]["mtsv3"] = {
                    "probability_mask": paths["mtsv3"].relative_to(output).as_posix(),
                    "detections": detections,
                }
            if comic_text is not None:
                refined, blocks = comic_text.predict(image)
                atomic_write_png(paths["comic_text"], refined)
                payload["teachers"]["comic_text"] = {
                    "refined_mask": paths["comic_text"].relative_to(output).as_posix(),
                    "blocks": blocks,
                }
            payload["elapsed_seconds"] = time.perf_counter() - started
            atomic_write_json(paths["record"], payload)
            completed += 1
            elapsed_total += payload["elapsed_seconds"]
        except Exception as error:
            failed += 1
            error_payload = {
                **payload,
                "status": "error",
                "error_type": type(error).__name__,
                "error": str(error),
                "elapsed_seconds": time.perf_counter() - started,
            }
            atomic_write_json(paths["record"], error_payload)
            raise

    summary = {
        **model_metadata,
        "images_root": str(images_root),
        "manifest": str(args.manifest.resolve()) if args.manifest else None,
        "requested": len(relative_images),
        "completed": completed,
        "failed": failed,
        "inference_elapsed_seconds": elapsed_total,
    }
    summary["num_shards"] = args.num_shards
    summary["shard_index"] = args.shard_index
    summary_name = (
        "summary.json"
        if args.num_shards == 1
        else f"summary-shard-{args.shard_index:03d}-of-{args.num_shards:03d}.json"
    )
    atomic_write_json(output / summary_name, summary)
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
