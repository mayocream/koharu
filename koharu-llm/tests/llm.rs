use std::sync::Arc;

use strum::IntoEnumIterator;

use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_llm::{GenerateOptions, Language, Llm, ModelId};
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

#[tokio::test]
#[ignore] // Ignored because it requires downloading multiple large models.
async fn llm_generates_text_for_all_models() -> anyhow::Result<()> {
    let prompt = r#"ã“ã‚“ã«ã¡ã¯ã€‚
ãƒ†ã‚¹ãƒˆã§ã™ã€‚
ã•ã‚ˆãªã‚‰ã€‚"#;

    let app_data_root = default_app_data_root();

    let runtime = RuntimeManager::new(app_data_root, ComputePolicy::PreferGpu)?;
    runtime.prepare().await?;
    koharu_llm::sys::initialize(&runtime)?;
    let backend = Arc::new(LlamaBackend::init()?);

    for model in ModelId::iter() {
        let mut llm = Llm::load(&runtime, model, false, Arc::clone(&backend)).await?;
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
