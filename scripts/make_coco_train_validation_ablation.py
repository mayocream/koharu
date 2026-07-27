#!/usr/bin/env python3
"""Create deterministic, hard-linked COCO train/validation ablation views."""

from __future__ import annotations

import argparse
import json
import os
import random
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--train-count", type=int, default=100)
    parser.add_argument("--validation-count", type=int, default=100)
    parser.add_argument("--train-seed", type=int, default=42)
    parser.add_argument("--validation-seed", type=int, default=20260724)
    return parser.parse_args()


def link_file(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    try:
        destination.symlink_to(source)
    except OSError:
        if os.name != "nt":
            raise
        os.link(source, destination)


def select_split(
    source: Path, split: str, count: int, seed: int
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    annotation_path = source / split / "_annotations.coco.json"
    document = json.loads(annotation_path.read_text(encoding="utf-8"))
    images = sorted(document["images"], key=lambda image: image["file_name"])
    if count <= 0 or count > len(images):
        raise ValueError(f"invalid count {count} for {split} with {len(images)} images")
    selected = sorted(
        random.Random(seed).sample(images, count),
        key=lambda image: image["file_name"],
    )
    return document, selected


def write_split(
    source: Path,
    source_split: str,
    destination: Path,
    output_split: str,
    document: dict[str, Any],
    selected: list[dict[str, Any]],
) -> int:
    output = destination / output_split
    output.mkdir()
    selected_ids = {image["id"] for image in selected}
    annotations = [
        annotation
        for annotation in document["annotations"]
        if annotation["image_id"] in selected_ids
    ]
    for image in selected:
        source_image = source / source_split / image["file_name"]
        if not source_image.is_file():
            raise FileNotFoundError(source_image)
        link_file(source_image, output / image["file_name"])
    subset = dict(document)
    subset["images"] = selected
    subset["annotations"] = annotations
    (output / "_annotations.coco.json").write_text(
        json.dumps(subset, ensure_ascii=False, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    return len(annotations)


def main() -> None:
    args = parse_args()
    source = args.source.resolve()
    destination = args.destination.resolve()
    if destination.exists():
        raise FileExistsError(destination)
    if destination == source or source in destination.parents:
        raise ValueError("destination must not equal or be inside source")

    train_document, train_images = select_split(
        source, "train", args.train_count, args.train_seed
    )
    validation_document, validation_images = select_split(
        source, "test", args.validation_count, args.validation_seed
    )
    destination.mkdir(parents=True)
    train_annotations = write_split(
        source,
        "train",
        destination,
        "train",
        train_document,
        train_images,
    )
    validation_annotations = write_split(
        source,
        "test",
        destination,
        "valid",
        validation_document,
        validation_images,
    )
    write_split(
        source,
        "test",
        destination,
        "test",
        validation_document,
        validation_images,
    )
    marker = source / ".manga109-pp-doclayout-distillation-view"
    if not marker.is_file():
        raise FileNotFoundError(marker)
    link_file(marker, destination / marker.name)

    manifest = {
        "source": str(source),
        "train": {
            "source_split": "train",
            "seed": args.train_seed,
            "images": len(train_images),
            "annotations": train_annotations,
            "selection": [
                {"id": image["id"], "file_name": image["file_name"]}
                for image in train_images
            ],
        },
        "validation": {
            "source_split": "test",
            "seed": args.validation_seed,
            "images": len(validation_images),
            "annotations": validation_annotations,
            "selection": [
                {"id": image["id"], "file_name": image["file_name"]}
                for image in validation_images
            ],
        },
    }
    (destination / "ablation_selection.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {
                "destination": str(destination),
                "train_images": len(train_images),
                "train_annotations": train_annotations,
                "validation_images": len(validation_images),
                "validation_annotations": validation_annotations,
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
