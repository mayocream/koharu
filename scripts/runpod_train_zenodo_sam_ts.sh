#!/usr/bin/env bash
set -uo pipefail

ROOT=/workspace/koharu
RUN_NAME=sam-ts-l-textseg-zenodo-b300-full
OUTPUT="$ROOT/runs/$RUN_NAME"
STDOUT_LOG="$ROOT/runs/$RUN_NAME.stdout.log"
EXIT_FILE="$ROOT/runs/$RUN_NAME.wrapper.exit"
PID_FILE="$ROOT/runs/$RUN_NAME.wrapper.pid"
VENV="$ROOT/.venv"

mkdir -p "$ROOT/runs" "$ROOT/models/hi-sam"
echo "$$" > "$PID_FILE"
trap 'status=$?; printf "%s\n" "$status" > "$EXIT_FILE"' EXIT
set -e

cd "$ROOT"
python --version
nvidia-smi
if [[ ! -x "$VENV/bin/python" ]]; then
  python -m venv --system-site-packages "$VENV"
fi
PYTHON="$VENV/bin/python"
"$PYTHON" -m pip install --quiet --disable-pip-version-check \
  --index-url https://download.pytorch.org/whl/cu130 \
  torchvision==0.24.1
"$PYTHON" -m pip install --quiet --disable-pip-version-check \
  einops matplotlib opencv-python-headless

download_checkpoint() {
  local filename="$1"
  local expected="$2"
  local destination="$ROOT/models/hi-sam/$filename"
  if [[ ! -f "$destination" ]]; then
    curl --fail --location --retry 5 --retry-delay 3 \
      "https://huggingface.co/GoGiants1/Hi-SAM/resolve/main/$filename?download=true" \
      --output "$destination.download"
    mv "$destination.download" "$destination"
  fi
  printf '%s  %s\n' "$expected" "$destination" | sha256sum --check --status
}

download_checkpoint \
  sam_tss_l_textseg.pth \
  1a7399fd5b031383a3776b4375332d23b952be616a735b545b3abb7eb89d063f
download_checkpoint \
  sam_vit_l_0b3195.pth \
  3adcc4315b642a4d2101128f611684e8734c41232a17c648ed1693702a49a622

"$PYTHON" - <<'PY'
import cv2
import matplotlib
import torch
import torchvision

print(
    {
        "torch": torch.__version__,
        "torchvision": torchvision.__version__,
        "cuda": torch.version.cuda,
        "gpu": torch.cuda.get_device_name(0),
        "opencv": cv2.__version__,
        "matplotlib": matplotlib.__version__,
    },
    flush=True,
)
PY

"$PYTHON" scripts/train_zenodo_sam_ts.py \
  --dataset data/manga109-zenodo-sam-ts \
  --hi-sam-root temp/Hi-SAM \
  --checkpoint models/hi-sam/sam_tss_l_textseg.pth \
  --sam-checkpoint models/hi-sam/sam_vit_l_0b3195.pth \
  --output "$OUTPUT" \
  --epochs 20 \
  --batch-size 16 \
  --workers 8 \
  --learning-rate 1e-5 \
  --min-learning-rate 1e-6 \
  --warmup-epochs 1 \
  --minimum-epochs 8 \
  --early-stopping-patience 5 \
  --weight-decay 0.05 \
  --seed 42 \
  --log-every 10 \
  --amp-dtype bfloat16 \
  > "$STDOUT_LOG" 2>&1

test -s "$OUTPUT/sam_tss_l_zenodo_best.pth"
test -s "$OUTPUT/sam_tss_l_zenodo_final.pth"
test -s "$OUTPUT/checkpoint_latest.pth"
test -s "$OUTPUT/TRAINING_COMPLETE.json"
