use koharu_ml::llm::{GenerateOptions, Llm, ModelId};
use strum::IntoEnumIterator;

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::instrument;

    #[tokio::test]
    #[instrument(level = "debug", skip_all)]
    async fn llm_generates_text_for_all_models() -> anyhow::Result<()> {
        let prompt = "こんにちは、テストです。";

        for model in ModelId::iter() {
            let mut llm = Llm::load(model, false).await?;
            let opts = GenerateOptions {
                max_tokens: 32,
                temperature: 0.0,
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
        }

        Ok(())
    }
}
