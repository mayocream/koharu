fn main() {
    // Alias for the long `target_os` list needing the Wayland subsurface path.
    println!("cargo::rustc-check-cfg=cfg(wayland_platform)");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(
        target_os.as_str(),
        "linux" | "dragonfly" | "freebsd" | "netbsd" | "openbsd"
    ) {
        println!("cargo::rustc-cfg=wayland_platform");
    }
    std::env::var_os("DEP_KOHARU_TORCH_SHIM")
        .expect("koharu-torch-sys did not provide its runtime shim");
}
