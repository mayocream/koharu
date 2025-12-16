mod model;
pub mod prompt;
mod quantized_hunyuan_dense;
mod quantized_lfm2;
mod tokenizer;

pub use model::{GenerateOptions, Llm};
pub use prompt::{ChatMessage, ChatRole};

macro_rules! define_llms {
    (
        $(
            $name:ident => {
                id = $id:literal,
                repo = $repo:literal,
                filename = $weights:literal
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
                    Model => ($repo, $weights)
                }
            }
        )*

        impl ModelId {
            pub async fn get(&self) -> anyhow::Result<std::path::PathBuf> {
                match self {
                    $(
                        ModelId::$name => {
                            let weights = $name::Manifest::Model.get().await?;
                            Ok(weights)
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
            stream::iter(models)
                .map(|manifest| async move {
                    manifest.get().await
                })
                .buffer_unordered(num_cpus::get())
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
        filename = "vntl-llama3-8b-v2-hf-q8_0.gguf"
    },
    Lfm2_350mEnjpMt => {
        id = "lfm2-350m-enjp-mt",
        repo = "LiquidAI/LFM2-350M-ENJP-MT-GGUF",
        filename = "LFM2-350M-ENJP-MT-Q8_0.gguf"
    },
    SakuraGalTransl7Bv3_7 => {
        id = "sakura-galtransl-7b-v3.7",
        repo = "SakuraLLM/Sakura-GalTransl-7B-v3.7",
        filename = "Sakura-Galtransl-7B-v3.7.gguf"
    },
    Sakura1_5bQwen2_5v1_0 => {
        id = "sakura-1.5b-qwen2.5-v1.0",
        repo = "shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX",
        filename = "sakura-1.5b-qwen2.5-v1.0-Q5KS.gguf"
    },
    // Mungert/Hunyuan-MT-7B-GGUF
    HunyuanMT7B => {
        id = "hunyuan-mt-7b",
        repo = "Mungert/Hunyuan-MT-7B-GGUF",
        filename = "Hunyuan-MT-7B-q6_k_m.gguf"
    },
}
