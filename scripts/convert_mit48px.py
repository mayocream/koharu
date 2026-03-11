#!/usr/bin/env python3
"""
Convert mit48px OCR weights to safetensors for Candle.
"""

import argparse
import json
import shutil
from pathlib import Path

from huggingface_hub import hf_hub_download
from safetensors.torch import save_file
import torch


MODEL_REPO = "zyddnys/manga-image-translator"
MODEL_FILENAME = "ocr_ar_48px.ckpt"
DICT_FILENAME = "alphabet-all-v7.txt"


def parse_args() -> argparse.Namespace:
    default_output = Path.home() / ".cache" / "Koharu" / "models" / "mit48px-ocr"
    parser = argparse.ArgumentParser(description="Convert mit48px OCR checkpoint to safetensors.")
    parser.add_argument(
        "--checkpoint",
        type=Path,
        default=None,
        help="Optional local checkpoint path. Defaults to downloading ocr_ar_48px.ckpt.",
    )
    parser.add_argument(
        "--dictionary",
        type=Path,
        default=None,
        help="Optional local dictionary path. Defaults to downloading alphabet-all-v7.txt.",
    )
    parser.add_argument(
        "-o",
        "--output-dir",
        type=Path,
        default=default_output,
        help=f"Output directory (default: {default_output})",
    )
    return parser.parse_args()


def load_state_dict(checkpoint_path: Path) -> dict[str, torch.Tensor]:
    state = torch.load(checkpoint_path, map_location="cpu")
    if isinstance(state, dict) and "state_dict" in state and isinstance(state["state_dict"], dict):
        state = state["state_dict"]
    if not isinstance(state, dict):
        raise RuntimeError("Unexpected checkpoint format")
    tensor_map = {}
    for key, value in state.items():
        if not isinstance(value, torch.Tensor):
            raise RuntimeError(f"Unexpected non-tensor entry for key {key!r}")
        tensor_map[key] = value.detach().cpu().contiguous().clone()
    return tensor_map


def main() -> None:
    args = parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    checkpoint_path = args.checkpoint or Path(
        hf_hub_download(repo_id=MODEL_REPO, filename=MODEL_FILENAME)
    )
    dictionary_path = args.dictionary or Path(
        hf_hub_download(repo_id=MODEL_REPO, filename=DICT_FILENAME)
    )

    state_dict = load_state_dict(checkpoint_path)
    save_file(state_dict, str(args.output_dir / "model.safetensors"))
    shutil.copyfile(dictionary_path, args.output_dir / DICT_FILENAME)

    config = {
        "text_height": 48,
        "max_width": 8100,
        "embd_dim": 320,
        "num_heads": 4,
        "encoder_layers": 4,
        "decoder_layers": 5,
        "beam_size_default": 5,
        "max_seq_length_default": 255,
        "pad_token_id": 0,
        "bos_token_id": 1,
        "eos_token_id": 2,
        "space_token": "<SP>",
        "dictionary_file": DICT_FILENAME,
    }
    with open(args.output_dir / "config.json", "w", encoding="utf-8") as fp:
        json.dump(config, fp, ensure_ascii=False, indent=2)
        fp.write("\n")

    print(f"Saved {len(state_dict)} tensors to {args.output_dir / 'model.safetensors'}")
    print(f"Saved dictionary to {args.output_dir / DICT_FILENAME}")
    print(f"Saved config to {args.output_dir / 'config.json'}")


if __name__ == "__main__":
    main()
