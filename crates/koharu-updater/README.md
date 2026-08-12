# koharu-updater

This crate is Koharu's repository-owned updater. Its download and platform
installation paths are adapted from `tauri-plugin-updater` 2.10.1 at commit
`d6a3898001a4bcc659e045f9501498751b77dbe6`.

Koharu does not consume a Tauri updater manifest. The crate reads published
GitHub Releases, takes the product version from the release tag, ignores the
`llama.cpp-*` and `stable-diffusion.cpp-*` release streams, and selects one
hard-coded package name for each supported build:

- Windows x86-64: `Koharu_<version>_x64-setup.exe`
- Linux x86-64: `Koharu_<version>_amd64.AppImage`
- macOS Apple Silicon: `Koharu_<version>_aarch64.app.tar.gz`

Package signatures are intentionally not part of this updater contract. The
GitHub HTTPS transport is the download trust boundary; Windows and macOS also
retain their existing platform code-signing checks when the package is opened.
