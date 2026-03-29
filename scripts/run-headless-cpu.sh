#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ ! -x "$ROOT/target/release/koharu" ]; then
  echo "Binary not found: $ROOT/target/release/koharu" >&2
  echo "Build first with scripts/bootstrap-ubuntu22-headless-cpu.sh" >&2
  exit 1
fi

PORT="${PORT:-4000}"

exec "$ROOT/target/release/koharu" --cpu --headless --port "$PORT"
