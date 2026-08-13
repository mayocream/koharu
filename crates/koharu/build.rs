fn main() {
    std::env::var_os("DEP_KOHARU_TORCH_SHIM")
        .expect("koharu-torch-sys did not provide its native shim");
    tauri_build::build();
}
