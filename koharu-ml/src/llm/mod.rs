mod model;
mod prompt;

pub use model::{GenerateOptions, Llm};
pub use prompt::{ChatMessage, ChatRole};

macro_rules! define_llms {
    (
        $(
            $name:ident => {
                id = $id:literal,
                repo = $repo:literal,
                filename = $weights:literal,
                tokenizer_repo = $tokenizer_repo:literal
            }
        ),* $(,)?
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString, strum::EnumIter)]
        pub enum ModelId {
            $(
                #[strum(serialize = $id)]
                $name,
            )*
        }

        $(
            #[allow(non_snake_case)]
            mod $name {
                use crate::define_models;

                define_models! {
                    Model => ($repo, $weights),
                    Tokenizer => ($tokenizer_repo, "tokenizer.json"),
                }
            }
        )*

        impl ModelId {
            pub async fn get(&self) -> anyhow::Result<(std::path::PathBuf, std::path::PathBuf)> {
                match self {
                    $(
                        ModelId::$name => {
                            let weights = $name::Manifest::Model.get().await?;
                            let tokenizer = $name::Manifest::Tokenizer.get().await?;
                            Ok((weights, tokenizer))
                        }
                    ),*
                }
            }
        }

        pub async fn prefetch() -> anyhow::Result<()> {
            use futures::stream::{self, StreamExt, TryStreamExt};
            let models = [
                $(ModelId::$name),*
            ];
            let len = models.len();
            stream::iter(models)
                .map(|manifest| async move {
                    manifest.get().await
                })
                .buffer_unordered(len)
                .try_collect::<Vec<_>>()
                .await?;
            Ok(())
        }
    };
}

define_llms! {
    VntlLlama3_8Bv2 => {
        id = "vntl-llama3-8b-v2",
        repo = "lmg-anon/vntl-llama3-8b-v2-gguf",
        filename = "vntl-llama3-8b-v2-hf-q8_0.gguf",
        tokenizer_repo = "rinna/llama-3-youko-8b"
    },
    SakuraGalTransl7Bv3_7 => {
        id = "sakura-galtransl-7b-v3.7",
        repo = "SakuraLLM/Sakura-GalTransl-7B-v3.7",
        filename = "Sakura-Galtransl-7B-v3.7.gguf",
        tokenizer_repo = "Qwen/Qwen2.5-1.5B-Instruct"
    },
}
