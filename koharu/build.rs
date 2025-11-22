use std::path::Path;

#[tokio::main]
async fn main() {
    // Pre-download dynamic libraries
    {
        let workspace_dir = Path::new(env!("CARGO_WORKSPACE_DIR"));
        let profile = std::env::var("PROFILE").unwrap();
        let out_dir = workspace_dir.join("target").join(&profile);

        koharu_runtime::ensure_dylibs(out_dir).await.unwrap();
    }

    tauri_build::build();
}
