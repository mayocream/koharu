// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    ort::init()
        .with_execution_providers([
            #[cfg(feature = "cuda")]
            ort::execution_providers::CUDAExecutionProvider::default().build(),
        ])
        .commit()?;
    koharu_lib::run()
}
