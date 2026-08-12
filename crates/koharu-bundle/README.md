# koharu-bundle

This crate is a small adapter around two upstream packagers:

- cef-rs prepares the pinned CEF runtime and the standard macOS helper apps.
- `tauri-bundler` creates the NSIS, AppImage, or macOS DMG package.

It deliberately does not maintain a second installer format, CEF file matrix,
plist generator, or package manifest.

cef-rs downloads its pinned `151.3.16` distribution during the build.

```text
cargo run -p koharu-bundle -- \
  --package nsis \
  --target x86_64-pc-windows-msvc \
  --executable target/x86_64-pc-windows-msvc/release/koharu.exe \
  --libraries target/release/koharu-torch.dll \
  --ui packages/koharu/out \
  --license LICENSE \
  --output target/bundle/windows \
  --version 0.64.2
```

Use `appimage` on Linux and `dmg` on macOS. On macOS, `tauri-bundler` signs and
notarizes the application before it creates the DMG when Apple credentials are
available.
