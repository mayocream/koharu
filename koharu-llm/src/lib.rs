pub mod api;
pub mod facade;
pub mod language;
mod model;
pub mod paddleocr_vl;
pub mod prompt;
pub mod providers;
pub mod safe;
pub mod sys;

use std::path::PathBuf;

use strum::{EnumProperty, IntoEnumIterator};

use koharu_http::download::HubAssetSpec;
pub use language::{Language, language_from_tag, supported_locales};
pub use model::{GenerateOptions, Llm};
pub use prompt::{ChatMessage, ChatRole};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::Display,
    strum::EnumString,
    strum::EnumIter,
    strum::EnumProperty,
)]
pub enum ModelId {
    #[strum(
        serialize = "vntl-llama3-8b-v2",
        props(
            repo = "lmg-anon/vntl-llama3-8b-v2-gguf",
            filename = "vntl-llama3-8b-v2-hf-q8_0.gguf",
            languages = "en-US"
        )
    )]
    VntlLlama3_8Bv2,
    #[strum(
        serialize = "lfm2-350m-enjp-mt",
        props(
            repo = "LiquidAI/LFM2-350M-ENJP-MT-GGUF",
            filename = "LFM2-350M-ENJP-MT-Q8_0.gguf",
            languages = "en-US"
        )
    )]
    Lfm2_350mEnjpMt,
    #[strum(
        serialize = "sakura-galtransl-7b-v3.7",
        props(
            repo = "SakuraLLM/Sakura-GalTransl-7B-v3.7",
            filename = "Sakura-Galtransl-7B-v3.7.gguf",
            languages = "zh-CN"
        )
    )]
    SakuraGalTransl7Bv3_7,
    #[strum(
        serialize = "sakura-1.5b-qwen2.5-v1.0",
        props(
            repo = "shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX",
            filename = "sakura-1.5b-qwen2.5-v1.0-Q5KS.gguf",
            languages = "zh-CN"
        )
    )]
    Sakura1_5bQwen2_5v1_0,
    #[strum(
        serialize = "hunyuan-mt-7b",
        props(
            repo = "Mungert/Hunyuan-MT-7B-GGUF",
            filename = "Hunyuan-MT-7B-q6_k_m.gguf",
            languages = "zh-CN,en-US,fr-FR,pt-PT,es-ES,ja-JP,tr-TR,ru-RU,ar-SA,ko-KR,th-TH,it-IT,de-DE,vi-VN,ms-MY,id-ID,fil-PH,hi-IN,zh-TW,pl-PL,cs-CZ,nl-NL,km-KH,my-MM,fa-IR,gu-IN,ur-PK,te-IN,mr-IN,he-IL,bn-BD,ta-IN,uk-UA,bo-CN,kk-KZ,mn-MN,ug-CN,yue-HK"
        )
    )]
    HunyuanMT7B,
}

impl ModelId {
    fn property(&self, name: &str) -> &'static str {
        self.get_str(name).expect("missing model property")
    }

    pub fn repo(&self) -> &'static str {
        self.property("repo")
    }

    pub fn filename(&self) -> &'static str {
        self.property("filename")
    }

    pub async fn get(&self) -> anyhow::Result<PathBuf> {
        koharu_http::download::model(self.repo(), self.filename()).await
    }

    pub fn languages(&self) -> Vec<Language> {
        self.property("languages")
            .split(',')
            .map(|tag| Language::parse(tag).expect("invalid model language tag"))
            .collect()
    }
}

pub fn local_model_assets() -> Vec<(ModelId, HubAssetSpec)> {
    ModelId::iter()
        .map(|model| {
            (
                model,
                HubAssetSpec {
                    repo: model.repo(),
                    filename: model.filename(),
                },
            )
        })
        .collect()
}

pub async fn prefetch() -> anyhow::Result<()> {
    use futures::stream::{self, StreamExt, TryStreamExt};

    stream::iter(ModelId::iter())
        .map(|model| async move { model.get().await })
        .buffer_unordered(num_cpus::get())
        .try_collect::<Vec<_>>()
        .await?;
    Ok(())
}
