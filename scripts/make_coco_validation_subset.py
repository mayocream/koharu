#!/usr/bin/env python3
"""Create a deterministic COCO subset for the RF-DETR validation loader.

The train and test directories are symlinked unchanged. Selected images from
the requested source split are exposed through ``valid`` because RF-DETR's
evaluation entry point reads that split. COCO image and annotation IDs remain
unchanged.
"""

from __future__ import annotations

import argparse
import json
import os
import random
from pathlib import Path


def link_file(source: Path, destination: Path) -> None:
    """Expose a read-only source file without duplicating its contents."""

    try:
        destination.symlink_to(source)
    except OSError:
        if os.name != "nt":
            raise
        os.link(source, destination)


def link_split(source: Path, destination: Path) -> None:
    """Link a split, with a minimal Windows fallback for validation-only views."""

    try:
        destination.symlink_to(source, target_is_directory=True)
    except OSError:
        if os.name != "nt":
            raise
        destination.mkdir()
        link_file(source / "_annotations.coco.json", destination / "_annotations.coco.json")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--count", type=int, default=256)
    parser.add_argument("--source-split", choices=("train", "valid", "test"), default="valid")
    parser.add_argument("--selection", choices=("even", "random"), default="even")
    parser.add_argument("--seed", type=int, default=0)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    source = args.source.resolve()
    destination = args.destination.resolve()
    if args.count <= 0:
        raise ValueError("--count must be positive")
    if destination == source or source in destination.parents:
        raise ValueError("destination must not equal or be inside source")

    annotation_path = source / args.source_split / "_annotations.coco.json"
    document = json.loads(annotation_path.read_text(encoding="utf-8"))
    images = sorted(document["images"], key=lambda image: image["file_name"])
    if args.count >= len(images):
        selected = images
    elif args.selection == "random":
        selected = sorted(
            random.Random(args.seed).sample(images, args.count),
            key=lambda image: image["file_name"],
        )
    else:
        selected = [images[index * len(images) // args.count] for index in range(args.count)]
    selected_ids = {image["id"] for image in selected}

    destination.mkdir(parents=True, exist_ok=True)
    for split in ("train", "test"):
        link = destination / split
        if link.exists() or link.is_symlink():
            raise FileExistsError(link)
        link_split(source / split, link)

    valid = destination / "valid"
    valid.mkdir()
    for image in selected:
        source_image = source / args.source_split / image["file_name"]
        if not source_image.is_file():
            raise FileNotFoundError(source_image)
        destination_image = valid / image["file_name"]
        destination_image.parent.mkdir(parents=True, exist_ok=True)
        link_file(source_image, destination_image)

    subset = dict(document)
    subset["images"] = selected
    subset["annotations"] = [
        annotation for annotation in document["annotations"] if annotation["image_id"] in selected_ids
    ]
    (valid / "_annotations.coco.json").write_text(
        json.dumps(subset, ensure_ascii=False, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    selection = {
        "source": str(source),
        "source_split": args.source_split,
        "selection": args.selection,
        "seed": args.seed,
        "source_images": len(images),
        "selected_images": len(selected),
        "selected_annotations": len(subset["annotations"]),
        "images": [
            {"id": image["id"], "file_name": image["file_name"]}
            for image in selected
        ],
    }
    (destination / "selection.json").write_text(
        json.dumps(selection, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {**selection, "images": None, "destination": str(destination)},
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
