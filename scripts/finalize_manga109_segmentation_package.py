# /// script
# requires-python = ">=3.12,<3.15"
# ///
"""Validate and checksum the annotation-only Hugging Face package."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PACKAGE = ROOT / "data" / "manga109-segmentation"
RASTER_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp", ".gif", ".bmp", ".tif", ".tiff"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, default=DEFAULT_PACKAGE)
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_text(path: Path, value: str) -> None:
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(value, encoding="utf-8", newline="\n")
    os.replace(temporary, path)


def main() -> None:
    args = parse_args()
    package = args.package.resolve()
    if not (package / ".manga109-segmentation-dataset").is_file():
        raise ValueError(f"not a generated Manga109 Segmentation package: {package}")
    rasters = [path for path in package.rglob("*") if path.suffix.lower() in RASTER_EXTENSIONS]
    if rasters:
        raise ValueError(f"package contains prohibited raster files: {rasters[:10]}")
    required = [
        package / "README.md",
        package / "build.json",
        *(package / "annotations" / f"{split}.coco.json" for split in ("train", "validation", "test")),
        *(package / "review" / f"{split}.jsonl" for split in ("train", "validation", "test")),
    ]
    missing = [path for path in required if not path.is_file()]
    if missing:
        raise FileNotFoundError(f"package files missing: {missing}")
    files = sorted(
        (
            path
            for path in package.rglob("*")
            if path.is_file()
            and path.name not in {"checksums.sha256", "package_manifest.json"}
            and path.name != ".manga109-segmentation-dataset"
        ),
        key=lambda path: path.relative_to(package).as_posix(),
    )
    entries = [
        {
            "path": path.relative_to(package).as_posix(),
            "bytes": path.stat().st_size,
            "sha256": sha256(path),
        }
        for path in files
    ]
    atomic_text(
        package / "checksums.sha256",
        "".join(f"{entry['sha256']}  {entry['path']}\n" for entry in entries),
    )
    manifest = {
        "schema_version": 1,
        "annotation_only": True,
        "contains_raster_images": False,
        "files": entries,
        "total_bytes": sum(entry["bytes"] for entry in entries),
    }
    atomic_text(
        package / "package_manifest.json",
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
    )
    print(json.dumps(manifest, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
