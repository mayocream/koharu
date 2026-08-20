#!/usr/bin/env bash

llama=$(gh api 'repos/mayocream/koharu/releases?per_page=100' \
    --jq 'map(select(.tag_name | startswith("llama.cpp-"))) | first | .tag_name' | sed 's/llama.cpp-//')
diffusion=$(gh api 'repos/mayocream/koharu/releases?per_page=100' \
    --jq 'map(select(.tag_name | startswith("stable-diffusion.cpp-"))) | first | .tag_name' | sed 's/stable-diffusion.cpp-//')

while read -r source target; do
    curl -sL "https://raw.githubusercontent.com/ggml-org/llama.cpp/$llama/$source" -o "$target"
done <<'EOF'
include/llama.h crates/koharu-llama-sys/include/llama.h
ggml/include/gguf.h crates/koharu-llama-sys/include/gguf.h
ggml/include/ggml.h crates/koharu-llama-sys/include/ggml.h
ggml/include/ggml-alloc.h crates/koharu-llama-sys/include/ggml-alloc.h
ggml/include/ggml-backend.h crates/koharu-llama-sys/include/ggml-backend.h
ggml/include/ggml-cpu.h crates/koharu-llama-sys/include/ggml-cpu.h
ggml/include/ggml-opt.h crates/koharu-llama-sys/include/ggml-opt.h
tools/mtmd/mtmd.h crates/koharu-llama-sys/include/mtmd.h
tools/mtmd/mtmd-helper.h crates/koharu-llama-sys/include/mtmd-helper.h
EOF

curl -sL \
    "https://raw.githubusercontent.com/leejet/stable-diffusion.cpp/$diffusion/include/stable-diffusion.h" \
    -o crates/koharu-diffusion-sys/include/stable-diffusion.h

printf 'llama.cpp: %s\nstable-diffusion.cpp: %s\n' "$llama" "$diffusion"
