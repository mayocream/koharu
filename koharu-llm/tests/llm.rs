use std::path::PathBuf;

use strum::IntoEnumIterator;

use koharu_llm::{GenerateOptions, Language, Llm, ModelId};

async fn initialize_runtime() -> anyhow::Result<()> {
    koharu_runtime::initialize().await?;
    Ok(())
}

#[tokio::test]
#[ignore] // Ignored because it requires downloading multiple large models.
async fn llm_generates_text_for_all_models() -> anyhow::Result<()> {
    let prompt = r#"ã“ã‚“ã«ã¡ã¯ã€‚
ãƒ†ã‚¹ãƒˆã§ã™ã€‚
ã•ã‚ˆãªã‚‰ã€‚"#;

    let model_dir = dirs::data_local_dir()
        .map(|path| path.join("Koharu"))
        .unwrap_or(PathBuf::from("."))
        .join("models");

    koharu_http::hf_hub::set_cache_dir(model_dir)?;
    initialize_runtime().await?;
    koharu_llm::sys::initialize()?;

    for model in ModelId::iter() {
        let mut llm = Llm::load(model, false).await?;
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
