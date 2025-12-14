use std::path::PathBuf;

use koharu_ml::llm::{GenerateOptions, Llm, ModelId};
use strum::IntoEnumIterator;

#[tokio::test]
async fn llm_generates_text_for_all_models() -> anyhow::Result<()> {
    let prompt = r#"こんにちは。
テストです。
さよなら。"#;

    let model_dir = dirs::data_local_dir()
        .map(|path| path.join("Koharu"))
        .unwrap_or(PathBuf::from("."))
        .join("models");

    koharu_ml::set_cache_dir(model_dir)?;

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

        let generated = llm.generate(prompt, &opts)?;
        assert!(
            !generated.trim().is_empty(),
            "model {model:?} should return some text"
        );
        // output should have three lines
        // let line_count = generated.lines().count();
        // assert!(line_count == 3, "model {model:?} should return exactly 3 lines, got {line_count}: {generated}");
    }

    Ok(())
}
