#!/usr/bin/env bash
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

apt-get update
apt-get install -y --no-install-recommends \
  build-essential \
  ca-certificates \
  clang \
  cmake \
  curl \
  file \
  fonts-noto-cjk \
  libayatana-appindicator3-dev \
  libclang-dev \
  librsvg2-dev \
  libssl-dev \
  libwebkit2gtk-4.1-dev \
  libxdo-dev \
  patchelf \
  pkg-config \
  wget

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi

if ! command -v bun >/dev/null 2>&1; then
  curl -fsSL https://bun.sh/install | bash
fi

source "$HOME/.cargo/env"
export PATH="$HOME/.bun/bin:$PATH"

bun install
bun --cwd ui build
cargo build --release -p koharu --no-default-features

target/release/koharu --cpu --download

cat <<'EOF'

Build completed.

Run headless UI:
  target/release/koharu --cpu --headless --port 4000

Open:
  http://127.0.0.1:4000/

EOF
