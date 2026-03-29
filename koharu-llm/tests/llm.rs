use std::path::PathBuf;
use std::sync::Arc;

use directories::ProjectDirs;
use strum::IntoEnumIterator;

use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_llm::{GenerateOptions, Language, Llm, ModelId};

async fn initialize_runtime() -> anyhow::Result<()> {
    let runtime_dir = ProjectDirs::from("rs", "Koharu", "Koharu")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or(PathBuf::from("."))
        .join("runtime");
    koharu_runtime::initialize(&runtime_dir).await?;
    Ok(())
}

#[tokio::test]
#[ignore] // Ignored because it requires downloading multiple large models.
async fn llm_generates_text_for_all_models() -> anyhow::Result<()> {
    let prompt = r#"ã“ã‚“ã«ã¡ã¯ã€‚
ãƒ†ã‚¹ãƒˆã§ã™ã€‚
ã•ã‚ˆãªã‚‰ã€‚"#;

    let model_dir = ProjectDirs::from("rs", "Koharu", "Koharu")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or(PathBuf::from("."))
        .join("models");
    let runtime_dir = ProjectDirs::from("rs", "Koharu", "Koharu")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or(PathBuf::from("."))
        .join("runtime");

    initialize_runtime().await?;
    koharu_llm::sys::initialize(&runtime_dir)?;
    let backend = Arc::new(LlamaBackend::init()?);

    for model in ModelId::iter() {
        let mut llm =
            Llm::load(model, false, Arc::clone(&backend), &runtime_dir, &model_dir).await?;
        let opts = GenerateOptions {
            max_tokens: 100,
            temperature: 0.3,
            top_k: None,
            top_p: None,
            seed: 1,
            split_prompt: false,
            repeat_penalty: 1.0,
            repeat_last_n: 64,
        };

        let generated = llm.generate(prompt, &opts, Language::English)?;
        assert!(
            !generated.trim().is_empty(),
            "model {model:?} should return some text"
        );
    }

    Ok(())
}
