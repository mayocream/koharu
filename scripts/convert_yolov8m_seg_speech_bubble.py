#!/usr/bin/env python3
"""
Convert kitsumed/yolov8m_seg-speech-bubble weights to safetensors for Candle.
"""

import argparse
import json
import shutil
from pathlib import Path

from huggingface_hub import HfApi, hf_hub_download, model_info
from safetensors.torch import save_file
import torch
from ultralytics import YOLO


SOURCE_REPO = "kitsumed/yolov8m_seg-speech-bubble"
SOURCE_MODEL_FILENAME = "model.pt"
SOURCE_CONFIG_FILENAME = "config.yaml"
TARGET_REPO = "mayocream/yolov8m_seg-speech-bubble"


def parse_args() -> argparse.Namespace:
    default_output = Path("temp") / "speech-bubble-convert" / "export"
    parser = argparse.ArgumentParser(
        description="Convert YOLOv8m speech bubble segmentation weights to safetensors."
    )
    parser.add_argument(
        "--checkpoint",
        type=Path,
        default=None,
        help="Optional local model.pt path. Defaults to downloading from Hugging Face.",
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=None,
        help="Optional local config.yaml path. Defaults to downloading from Hugging Face.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=default_output,
        help=f"Output directory (default: {default_output})",
    )
    parser.add_argument(
        "--repo-id",
        default=TARGET_REPO,
        help=f"Target Hugging Face repo for --upload (default: {TARGET_REPO})",
    )
    parser.add_argument(
        "--upload",
        action="store_true",
        help="Upload the converted bundle to Hugging Face after conversion.",
    )
    parser.add_argument(
        "--private",
        action="store_true",
        help="Create the target Hugging Face repo as private when used with --upload.",
    )
    return parser.parse_args()


def normalize_class_names(names: object) -> list[str]:
    if isinstance(names, dict):
        return [str(name) for _, name in sorted(names.items(), key=lambda item: int(item[0]))]
    if isinstance(names, (list, tuple)):
        return [str(name) for name in names]
    raise RuntimeError(f"Unexpected class names value: {names!r}")


def build_model_card(repo_id: str, class_names: list[str], config: dict[str, object]) -> str:
    tags = [
        "candle",
        "yolo",
        "image-segmentation",
        "comic",
        "manga",
        "speech-bubble",
    ]
    tags_block = "\n".join(f"- {tag}" for tag in tags)
    classes_block = "\n".join(f"- `{name}`" for name in class_names)
    return f"""---
license: gpl-3.0
library_name: candle
base_model: {SOURCE_REPO}
tags:
{tags_block}
---

# {repo_id}

This repository contains a Candle-compatible `safetensors` conversion of
[`{SOURCE_REPO}`](https://huggingface.co/{SOURCE_REPO}).

Files:

- `model.safetensors`: converted floating-point checkpoint with the original Ultralytics tensor names
- `config.json`: Candle loader metadata for `koharu-ml`
- `config.yaml`: original upstream Ultralytics config

Model metadata:

- Variant: `YOLOv8{config["variant"]}-seg`
- Input size: `{config["input_size"]}`
- Classes:
{classes_block}
"""


def main() -> None:
    args = parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    checkpoint_path = args.checkpoint or Path(
        hf_hub_download(repo_id=SOURCE_REPO, filename=SOURCE_MODEL_FILENAME)
    )
    source_config_path = args.config or Path(
        hf_hub_download(repo_id=SOURCE_REPO, filename=SOURCE_CONFIG_FILENAME)
    )

    model = YOLO(str(checkpoint_path))
    inner = model.model.float().eval()
    head = inner.model[-1]
    class_names = normalize_class_names(inner.names)

    tensor_map: dict[str, torch.Tensor] = {}
    for key, value in inner.state_dict().items():
        if not isinstance(value, torch.Tensor) or not value.is_floating_point():
            continue
        tensor_map[key] = value.detach().cpu().contiguous().clone()

    config = {
        "model_type": "yolov8-seg",
        "variant": str(inner.yaml.get("scale", "m")),
        "input_size": 640,
        "num_classes": int(head.nc),
        "num_masks": int(head.nm),
        "num_prototypes": int(head.npr),
        "reg_max": int(head.reg_max),
        "class_names": class_names,
        "default_confidence_threshold": 0.25,
        "default_nms_threshold": 0.45,
        "mask_threshold": 0.5,
        "letterbox_color": 114,
        "source_repo": SOURCE_REPO,
        "source_model_filename": SOURCE_MODEL_FILENAME,
    }

    save_file(tensor_map, str(args.output_dir / "model.safetensors"))
    shutil.copyfile(source_config_path, args.output_dir / "config.yaml")
    with open(args.output_dir / "config.json", "w", encoding="utf-8") as fp:
        json.dump(config, fp, ensure_ascii=False, indent=2)
        fp.write("\n")

    repo_info = model_info(SOURCE_REPO)
    with open(args.output_dir / "README.md", "w", encoding="utf-8") as fp:
        fp.write(build_model_card(args.repo_id, class_names, config))
        fp.write("\n")
        fp.write("\n")
        fp.write(f"Upstream revision: `{repo_info.sha}`\n")

    print(f"Saved {len(tensor_map)} tensors to {args.output_dir / 'model.safetensors'}")
    print(f"Saved config to {args.output_dir / 'config.json'}")
    print(f"Saved upstream config to {args.output_dir / 'config.yaml'}")
    print(f"Saved README to {args.output_dir / 'README.md'}")

    if args.upload:
        api = HfApi()
        api.create_repo(repo_id=args.repo_id, repo_type="model", private=args.private, exist_ok=True)
        api.upload_folder(
            folder_path=str(args.output_dir),
            repo_id=args.repo_id,
            repo_type="model",
            commit_message=f"Add Candle safetensors conversion from {SOURCE_REPO}",
        )
        print(f"Uploaded converted bundle to https://huggingface.co/{args.repo_id}")


if __name__ == "__main__":
    main()
