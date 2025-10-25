#[allow(dead_code)]
pub fn find_onnxruntime_dylib() -> String {
    #[cfg(target_os = "windows")]
    let dylib_name = "onnxruntime.dll";
    #[cfg(target_os = "linux")]
    let dylib_name = "libonnxruntime.so";
    #[cfg(target_os = "macos")]
    let dylib_name = "libonnxruntime.dylib";

    // we assume the dylib is in the current working directory
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join(dylib_name)
        .to_string_lossy()
        .to_string()
}
