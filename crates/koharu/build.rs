fn main() {
    std::env::var_os("DEP_KOHARU_TORCH_SHIM")
        .expect("koharu-torch-sys did not provide its runtime shim");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=icons/icon.ico");
        winresource::WindowsResource::new()
            .set_icon("icons/icon.ico")
            .compile()
            .expect("failed to embed the Windows application icon");
    }
}
