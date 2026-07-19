# /// script
# requires-python = ">=3.11,<3.15"
# dependencies = [
#   "ultralytics==8.4.43",
# ]
# ///
"""Compare the upstream and fine-tuned comic-mask checkpoints on the test split."""

from __future__ import annotations

import argparse
import json
import os
import shutil
from pathlib import Path
from typing import Any

from ultralytics import YOLO


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASELINE = (
    REPOSITORY_ROOT / "data" / "models" / "shadowb-comic-mask-yolo26s" / "best.pt"
)
DEFAULT_DATA = (
    REPOSITORY_ROOT / "data" / "datasets" / "comic-mask-yolo26s" / "dataset.yaml"
)
DEFAULT_PROJECT = REPOSITORY_ROOT / "runs" / "comic-mask-yolo26s-eval"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    parser.add_argument("--fine-tuned", type=Path, required=True)
    parser.add_argument("--data", type=Path, default=DEFAULT_DATA)
    parser.add_argument("--project", type=Path, default=DEFAULT_PROJECT)
    parser.add_argument("--core3-data", type=Path)
    parser.add_argument("--imgsz", type=int, default=1280)
    parser.add_argument("--batch", type=int, default=16)
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--device", default="0")
    return parser.parse_args()


def link_or_copy(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        return
    try:
        os.link(source, destination)
    except OSError:
        shutil.copy2(source, destination)


def prepare_core3_dataset(source_root: Path, output_root: Path) -> tuple[Path, Path]:
    source_images = source_root / "images" / "test"
    source_labels = source_root / "labels" / "test"
    output_images = output_root / "images" / "test"
    output_labels = output_root / "labels" / "test"

    for source in source_images.rglob("*"):
        if source.is_file():
            link_or_copy(source, output_images / source.relative_to(source_images))

    for source in source_labels.rglob("*.txt"):
        destination = output_labels / source.relative_to(source_labels)
        destination.parent.mkdir(parents=True, exist_ok=True)
        lines = [
            line
            for line in source.read_text(encoding="utf-8").splitlines()
            if line and line.split(maxsplit=1)[0] != "3"
        ]
        destination.write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8")

    common = (
        f"path: {output_root.resolve().as_posix()}\n"
        "train: images/test\n"
        "val: images/test\n"
        "test: images/test\n"
    )
    baseline_yaml = output_root / "baseline3.yaml"
    baseline_yaml.write_text(
        common
        + "names:\n"
        + "  0: frame\n"
        + "  1: dialogue_text\n"
        + "  2: balloon\n",
        encoding="utf-8",
    )
    fine_yaml = output_root / "fine4-core3.yaml"
    fine_yaml.write_text(
        common
        + "names:\n"
        + "  0: frame\n"
        + "  1: dialogue_text\n"
        + "  2: balloon\n"
        + "  3: onomatopoeia_text\n",
        encoding="utf-8",
    )
    return baseline_yaml, fine_yaml


def to_float(value: Any) -> float:
    return float(value.item() if hasattr(value, "item") else value)


def serialize_metrics(results: Any) -> dict[str, Any]:
    output: dict[str, Any] = {
        "aggregate": {key: to_float(value) for key, value in results.results_dict.items()},
        "speed_ms_per_image": {
            key: to_float(value) for key, value in results.speed.items()
        },
        "per_class": {},
    }
    class_ids = [int(value) for value in results.box.ap_class_index]
    for metric_name, metric in (("box", results.box), ("mask", results.seg)):
        for metric_index, class_id in enumerate(class_ids):
            precision, recall, map50, map50_95 = metric.class_result(metric_index)
            class_name = results.names[class_id]
            block = output["per_class"].setdefault(class_name, {})
            block[metric_name] = {
                "precision": to_float(precision),
                "recall": to_float(recall),
                "map50": to_float(map50),
                "map50_95": to_float(map50_95),
            }
    return output


def validate(
    model_path: Path,
    data_path: Path,
    project: Path,
    name: str,
    args: argparse.Namespace,
) -> dict[str, Any]:
    model = YOLO(model_path)
    results = model.val(
        data=data_path,
        split="test",
        imgsz=args.imgsz,
        batch=args.batch,
        device=args.device,
        workers=args.workers,
        project=project,
        name=name,
        exist_ok=False,
        plots=True,
    )
    return serialize_metrics(results)


def main() -> None:
    args = parse_args()
    source_root = args.data.resolve().parent
    core3_root = (
        args.core3_data.resolve()
        if args.core3_data
        else source_root.with_name(f"{source_root.name}-core3-test")
    )
    baseline_yaml, fine_yaml = prepare_core3_dataset(source_root, core3_root)
    args.project.mkdir(parents=True, exist_ok=True)

    report = {
        "baseline_core3": validate(
            args.baseline, baseline_yaml, args.project, "baseline-core3-test", args
        ),
        "fine_tuned_core3": validate(
            args.fine_tuned, fine_yaml, args.project, "fine-tuned-core3-test", args
        ),
        "fine_tuned_full4": validate(
            args.fine_tuned, args.data, args.project, "fine-tuned-full4-test", args
        ),
    }
    output = args.project / "metrics.json"
    output.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
