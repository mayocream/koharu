#!/bin/bash
# Build script for Intel Arc Vulkan support

set -e

echo "=== Koharu Intel Arc Vulkan Build ==="
echo ""

# Detect OS
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    OS="linux"
    echo "[*] Detected Linux"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    OS="macos"
    echo "[*] Detected macOS (use Metal for best performance)"
elif [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "cygwin" ]]; then
    OS="windows"
    echo "[*] Detected Windows"
else
    echo "[!] Unsupported OS: $OSTYPE"
    exit 1
fi

# Check prerequisites
echo "[*] Checking prerequisites..."

if ! command -v rustc &> /dev/null; then
    echo "[!] Rust not found. Install from https://rustup.rs/"
    exit 1
fi

echo "[+] Rust version: $(rustc --version)"

if ! command -v cargo &> /dev/null; then
    echo "[!] Cargo not found"
    exit 1
fi

echo "[+] Cargo version: $(cargo --version)"

# Check for Vulkan SDK (optional but recommended)
if command -v vulkaninfo &> /dev/null; then
    echo "[+] Vulkan SDK found"
else
    echo "[!] Vulkan SDK not found (optional, drivers may be sufficient)"
fi

echo ""
echo "[*] Building with Vulkan support..."

if [ "$1" == "--dev" ]; then
    echo "[*] Debug build"
    cargo build -p koharu --features vulkan
else
    echo "[*] Release build"
    cargo build --release -p koharu --features vulkan
fi

echo ""
echo "[+] Build complete!"
echo ""

if [ "$OS" == "windows" ]; then
    echo "[*] Executable: target\\release\\koharu.exe"
elif [ "$OS" == "linux" ] || [ "$OS" == "macos" ]; then
    echo "[*] Executable: target/release/koharu"
fi

echo "[*] Run with debug info: RUST_LOG=debug ./target/release/koharu --debug"
