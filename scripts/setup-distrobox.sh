#!/bin/bash
set -e

CONTAINER_NAME="koharu-build"
IMAGE="ubuntu:24.04"

# Create the container if it doesn't exist
if ! distrobox list | grep -q "$CONTAINER_NAME"; then
    echo "Creating distrobox container $CONTAINER_NAME..."
    distrobox create -n "$CONTAINER_NAME" -i "$IMAGE" --nvidia --yes
fi

# Function to run commands inside the container
run_in_box() {
    distrobox enter "$CONTAINER_NAME" -- bash -c "$1"
}

echo "Fixing NVIDIA GPG key and sources list inside $CONTAINER_NAME..."
# Modernize GPG key handling and repository configuration for Ubuntu 24.04
run_in_box "
    # Import NVIDIA Container Toolkit keys BEFORE apt update
    sudo mkdir -p /usr/share/keyrings
    curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey | sudo gpg --dearmor --yes -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg
    
    # Handle both .list and .sources formats (Ubuntu 24.04 uses .sources by default)
    for f in /etc/apt/sources.list.d/nvidia-container-toolkit.*; do
        if [ -f \"\$f\" ]; then
            sudo sed -i 's/\\\$(ARCH)/amd64/g' \"\$f\"
        fi
    done

    # CUDA repository setup (using modern signed-by)
    wget -qO- https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2404/x86_64/3bf863cc.pub | sudo gpg --dearmor --yes -o /usr/share/keyrings/cuda-archive-keyring.gpg
    echo 'deb [signed-by=/usr/share/keyrings/cuda-archive-keyring.gpg] https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2404/x86_64/ /' | sudo tee /etc/apt/sources.list.d/cuda.list
    
    wget -q https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2404/x86_64/cuda-ubuntu2404.pin && \
    sudo mv cuda-ubuntu2404.pin /etc/apt/preferences.d/cuda-repository-pin-600
"

echo "Updating and installing build dependencies inside $CONTAINER_NAME..."
run_in_box "sudo apt update && sudo apt install -y \
    build-essential \
    cmake \
    pkg-config \
    curl \
    wget \
    file \
    libxdo-dev \
    libssl-dev \
    libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libglib2.0-dev \
    libclang-dev \
    cuda-toolkit-13-0"

echo "Configuring CUDA environment inside $CONTAINER_NAME..."
run_in_box "echo 'export PATH=/usr/local/cuda-13.0/bin\${PATH:+:\${PATH}}' | sudo tee /etc/profile.d/cuda.sh && \
    echo 'export LD_LIBRARY_PATH=/usr/local/cuda-13.0/lib64\${LD_LIBRARY_PATH:+:\${LD_LIBRARY_PATH}}' | sudo tee -a /etc/profile.d/cuda.sh && \
    echo 'export CUDA_HOME=/usr/local/cuda-13.0' | sudo tee -a /etc/profile.d/cuda.sh && \
    echo \"export NVCC_PREPEND_FLAGS='-ccbin gcc'\" | sudo tee -a /etc/profile.d/cuda.sh && \
    echo 'export CUDA_COMPUTE_CAP=86' | sudo tee -a /etc/profile.d/cuda.sh"

# Symlink CUDA tools for universal access and add to bash.bashrc
run_in_box "
    sudo ln -sf /usr/local/cuda-13.0/bin/nvcc /usr/local/bin/nvcc
    sudo ln -sf /usr/local/cuda-13.0/bin/ptxas /usr/local/bin/ptxas
    sudo ln -sf /usr/local/cuda-13.0/bin/fatbinary /usr/local/bin/fatbinary
    sudo ln -sf /usr/local/cuda-13.0/bin/cuobjdump /usr/local/bin/cuobjdump
    
    # Ensure they are also in the global bashrc for any non-login shell
    cat <<EOF | sudo tee -a /etc/bash.bashrc > /dev/null
export PATH=/usr/local/cuda-13.0/bin\${PATH:+:\${PATH}}
export CUDA_HOME=/usr/local/cuda-13.0
export LD_LIBRARY_PATH=/usr/local/cuda-13.0/lib64\${LD_LIBRARY_PATH:+:\${LD_LIBRARY_PATH}}
EOF
"

echo "Installing Rustup inside $CONTAINER_NAME..."
run_in_box "if ! command -v rustc &> /dev/null; then curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y; fi"

echo "Installing Bun inside $CONTAINER_NAME..."
run_in_box "if ! command -v bun &> /dev/null; then curl -fsSL https://bun.sh/install | bash; fi"

echo "Setup complete! To build Koharu, run:"
echo "distrobox enter $CONTAINER_NAME -- bash -l -c \"bun install && bun run build\""
