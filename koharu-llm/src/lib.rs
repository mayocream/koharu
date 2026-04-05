pub mod language;
mod model;
pub mod paddleocr_vl;
pub mod prompt;
pub mod providers;
pub mod safe;
pub mod sys;

use std::path::PathBuf;

use koharu_runtime::RuntimeManager;
use strum::{EnumProperty, IntoEnumIterator};

pub use language::{Language, language_from_tag, supported_locales};
pub use model::{GenerateOptions, Llm};
pub use prompt::{ChatMessage, ChatRole};

/// Suppress all llama.cpp / ggml / mtmd / clip native log output.
/// Must be called after `LlamaBackend::init()`.
pub fn suppress_native_logs() {
    unsafe extern "C" fn void_log(
        _: sys::ggml_log_level,
        _: *const std::os::raw::c_char,
        _: *mut std::os::raw::c_void,
    ) {
    }
    unsafe {
        // Suppress llama.cpp + ggml logs
        sys::llama_log_set(Some(void_log), std::ptr::null_mut());
        // Suppress mtmd / clip logs (separate logger)
        sys::mtmd_log_set(Some(void_log), std::ptr::null_mut());
        // Suppress mtmd-helper logs (yet another separate logger)
        sys::mtmd_helper_log_set(Some(void_log), std::ptr::null_mut());
    }
}

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
        serialize = "lfm2.5-1.2b-instruct",
        props(
            repo = "LiquidAI/LFM2.5-1.2B-Instruct-GGUF",
            filename = "LFM2.5-1.2B-Instruct-Q8_0.gguf",
            languages = "en-US,ar-SA,zh-CN,fr-FR,de-DE,ja-JP,ko-KR,pt-PT,es-ES"
        )
    )]
    Lfm2_5_1_2bInstruct,
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
            languages = "zh-CN,en-US,fr-FR,pt-PT,pt-BR,es-ES,ja-JP,tr-TR,ru-RU,ar-SA,ko-KR,th-TH,it-IT,de-DE,vi-VN,ms-MY,id-ID,fil-PH,hi-IN,zh-TW,pl-PL,cs-CZ,nl-NL,km-KH,my-MM,fa-IR,gu-IN,ur-PK,te-IN,mr-IN,he-IL,bn-BD,ta-IN,uk-UA,bo-CN,kk-KZ,mn-MN,ug-CN,yue-HK"
        )
    )]
    HunyuanMT7B,
    #[strum(
        serialize = "gemma4-e2b-it",
        props(
            repo = "unsloth/gemma-4-E2B-it-GGUF",
            filename = "gemma-4-e2b-it-Q8_0.gguf",
            languages = "*"
        )
    )]
    Gemma4E2bIt,
    #[strum(
        serialize = "gemma4-e4b-it",
        props(
            repo = "unsloth/gemma-4-E4B-it-GGUF",
            filename = "gemma-4-e4b-it-Q8_0.gguf",
            languages = "*"
        )
    )]
    Gemma4E4bIt,
    #[strum(
        serialize = "gemma4-26b-a4b-it",
        props(
            repo = "unsloth/gemma-4-26B-A4B-it-GGUF",
            filename = "gemma-4-26B-A4B-it-Q8_0.gguf",
            languages = "*"
        )
    )]
    Gemma4_26bA4bIt,
    #[strum(
        serialize = "gemma4-31b-it",
        props(
            repo = "unsloth/gemma-4-31B-it-GGUF",
            filename = "gemma-4-31B-it-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Gemma4_31bIt,
    #[strum(
        serialize = "qwen3.5-0.8b",
        props(
            repo = "unsloth/Qwen3.5-0.8B-GGUF",
            filename = "Qwen3.5-0.8B-Q8_0.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_0_8b,
    #[strum(
        serialize = "qwen3.5-2b",
        props(
            repo = "unsloth/Qwen3.5-2B-GGUF",
            filename = "Qwen3.5-2B-Q8_0.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_2b,
    #[strum(
        serialize = "qwen3.5-4b",
        props(
            repo = "unsloth/Qwen3.5-4B-GGUF",
            filename = "Qwen3.5-4B-Q8_0.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_4b,
    #[strum(
        serialize = "qwen3.5-9b",
        props(
            repo = "unsloth/Qwen3.5-9B-GGUF",
            filename = "Qwen3.5-9B-Q8_0.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_9b,
    #[strum(
        serialize = "qwen3.5-27b",
        props(
            repo = "unsloth/Qwen3.5-27B-GGUF",
            filename = "Qwen3.5-27B-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_27b,
    #[strum(
        serialize = "qwen3.5-35b-a3b",
        props(
            repo = "unsloth/Qwen3.5-35B-A3B-GGUF",
            filename = "Qwen3.5-35B-A3B-Q8_0.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_35bA3b,
}

impl ModelId {
    fn property(&self, name: &str) -> &'static str {
        self.get_str(name).expect("missing model property")
    }

    pub async fn get(&self, runtime: &RuntimeManager) -> anyhow::Result<PathBuf> {
        runtime
            .downloads()
            .huggingface_model(self.property("repo"), self.property("filename"))
            .await
    }

    pub fn default_generate_options(&self) -> GenerateOptions {
        match self {
            // LFM2.5: temp=0.1, top_k=50, repeat=1.05
            Self::Lfm2_5_1_2bInstruct => GenerateOptions {
                temperature: 0.1,
                top_k: Some(50),
                repeat_penalty: 1.05,
                ..Default::default()
            },
            // Gemma 4: temp=1.0, top_p=0.95, top_k=64
            Self::Gemma4E2bIt | Self::Gemma4E4bIt | Self::Gemma4_26bA4bIt | Self::Gemma4_31bIt => {
                GenerateOptions {
                    temperature: 1.0,
                    top_k: Some(64),
                    top_p: Some(0.95),
                    repeat_penalty: 1.0,
                    ..Default::default()
                }
            }
            // Qwen3.5 non-thinking: temp=1.0, top_k=20, top_p=1.0, presence=2.0
            Self::Qwen3_5_0_8b
            | Self::Qwen3_5_2b
            | Self::Qwen3_5_4b
            | Self::Qwen3_5_9b
            | Self::Qwen3_5_27b
            | Self::Qwen3_5_35bA3b => GenerateOptions {
                temperature: 1.0,
                top_k: Some(20),
                top_p: Some(1.0),
                min_p: Some(0.0),
                presence_penalty: 2.0,
                repeat_penalty: 1.0,
                ..Default::default()
            },
            // Default for other models
            _ => GenerateOptions::default(),
        }
    }

    pub fn languages(&self) -> Vec<Language> {
        let langs = self.property("languages");
        if langs == "*" {
            return Language::iter().collect();
        }
        langs
            .split(',')
            .map(|tag| Language::parse(tag).expect("invalid model language tag"))
            .collect()
    }
}

pub async fn prefetch(runtime: &RuntimeManager) -> anyhow::Result<()> {
    use futures::stream::{self, StreamExt, TryStreamExt};

    stream::iter(ModelId::iter())
        .map(|model| {
            let runtime = runtime.clone();
            async move { model.get(&runtime).await }
        })
        .buffer_unordered(num_cpus::get())
        .try_collect::<Vec<_>>()
        .await?;
    Ok(())
}
