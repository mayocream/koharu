fn main() {
    // Ensure CUDA library search path is visible when flash-attn is enabled.
    #[cfg(feature = "flash-attn")]
    {
        use std::env;
        use std::path::PathBuf;

        println!("cargo:rerun-if-env-changed=CUDA_PATH");
        println!("cargo:rerun-if-env-changed=CUDA_HOME");

        let cuda_root = env::var_os("CUDA_PATH").or_else(|| env::var_os("CUDA_HOME"));
        if let Some(root) = cuda_root {
            let mut lib_dir = PathBuf::from(root);
            if cfg!(target_os = "windows") {
                lib_dir.push("lib");
                lib_dir.push("x64");
            } else {
                lib_dir.push("lib64");
            }
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
        }
    }
}
