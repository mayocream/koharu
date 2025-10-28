#!/usr/bin/env python3
"""Download CUDA libraries for Rust project"""

import sys
import subprocess
import shutil
import argparse
from pathlib import Path
import tempfile

# https://docs.nvidia.com/cuda/cuda-quick-start-guide/index.html
# https://docs.nvidia.com/deeplearning/cudnn/installation/latest/windows.html#installing-cudnn-with-pip
PACKAGES = [
    "nvidia-cuda-runtime-cu12",
    "nvidia-cudnn-cu12",
    "nvidia-cublas-cu12",
    "nvidia-cufft-cu12",
]


def main():
    parser = argparse.ArgumentParser(description="Download CUDA libraries")
    parser.add_argument(
        "-o",
        "--output",
        default="libs",
        help="Output directory (default: libs)",
    )
    args = parser.parse_args()

    output = Path(args.output)
    output.mkdir(exist_ok=True)

    with tempfile.TemporaryDirectory() as tmp:
        venv = Path(tmp) / "venv"

        # Create venv and install packages
        subprocess.run([sys.executable, "-m", "venv", str(venv)], check=True)
        pip = venv / ("Scripts/pip.exe" if sys.platform == "win32" else "bin/pip")

        for pkg in PACKAGES:
            print(f"Installing {pkg}...")
            subprocess.run([str(pip), "install", "-q", pkg], check=True)

        # Find and copy libraries
        if sys.platform == "win32":
            site = venv / "Lib/site-packages"
            pattern = "**/*.dll"
        else:
            site = next((venv / "lib").glob("python3.*")) / "site-packages"
            pattern = "**/*.so*"

        for lib in site.glob(pattern):
            if "nvidia" in str(lib):
                shutil.copy2(lib, output)
                print(f"Copied {lib.name}")

    print(f"\nDone! Libraries in {output}")


if __name__ == "__main__":
    main()
