use anyhow::{Context as _, Result, bail};
use async_trait::async_trait;
use koharu_scene::{Authored, LanguageTag, Origin, SourceText, Translation};
use koharu_translator::{
    Language, LocalModel, LocalTranslator, Providers, RemoteProvider, RemoteTranslator,
    TranslationRequest, Translator,
};

use super::{ModelRef, StageInput, StageProcessor, finish, generation, observe_page_hierarchy};
use crate::ModelCell;

const PRODUCER: &str = "dev.koharu.pipeline.translation";

pub(super) struct Processor {
    config: koharu_translator::TranslationConfig,
    device: koharu_ml::Device,
    model: ModelCell<Model>,
}

impl Processor {
    pub(super) fn new(
        config: koharu_translator::TranslationConfig,
        device: koharu_ml::Device,
    ) -> Result<Self> {
        config
            .target_language
            .parse::<Language>()
            .with_context(|| format!("unsupported target language {}", config.target_language))?;
        if let Providers::Local(local) = &config.model {
            local
                .model
                .parse::<LocalModel>()
                .with_context(|| format!("unknown local translator '{}'", local.model))?;
        }
        if config
            .instructions
            .as_ref()
            .is_some_and(|value| value.contains('\0') || value.len() > 1024 * 1024)
        {
            bail!("translation instructions are too large or contain NUL");
        }

        Ok(Self {
            config,
            device,
            model: ModelCell::new(),
        })
    }
}

#[async_trait]
impl StageProcessor for Processor {
    fn model(&self) -> ModelRef<'_> {
        ModelRef::new(provider_name(&self.config.model), &self.model)
    }

    async fn load(&self) -> Result<()> {
        self.model
            .ensure(|| Model::load(self.device.clone(), &self.config.model))
            .await
    }

    async fn process(&self, input: StageInput) -> Result<koharu_scene::ScenePatch> {
        self.model
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("translation model is not loaded"))?
            .run(input, &self.config)
            .await
    }
}

struct Model {
    backend: Backend,
}

enum Backend {
    Local(LocalTranslator),
    Remote {
        provider: Providers,
        client: reqwest::Client,
    },
}

impl Model {
    async fn load(device: koharu_ml::Device, config: &Providers) -> Result<Self> {
        let backend = match config {
            Providers::Local(config) => {
                let model = config
                    .model
                    .parse::<LocalModel>()
                    .with_context(|| format!("unknown local translator '{}'", config.model))?;
                Backend::Local(LocalTranslator::load(device, model).await?)
            }
            provider => Backend::Remote {
                provider: provider.clone(),
                client: reqwest::Client::new(),
            },
        };
        Ok(Self { backend })
    }

    async fn run(
        &self,
        input: StageInput,
        options: &koharu_translator::TranslationConfig,
    ) -> Result<koharu_scene::ScenePatch> {
        let locale = LanguageTag::new(options.target_language.clone())?;
        let target = options
            .target_language
            .parse::<Language>()
            .context("invalid translation target language")?;
        let targets = targets(&input, &locale)?;

        let segments = if targets.is_empty() {
            Vec::new()
        } else {
            let mut request =
                TranslationRequest::new(targets.iter().map(|(_, source)| source.as_str()), target);
            if let Some(instructions) = &options.instructions {
                request = request.with_instructions(instructions);
            }
            match &self.backend {
                Backend::Local(translator) => translator.translate(request).await?.segments,
                Backend::Remote { provider, client } => {
                    remote(provider, client)?.translate(request).await?.segments
                }
            }
        };
        let model_name = match &self.backend {
            Backend::Local(_) => "local-translation",
            Backend::Remote { provider, .. } => provider_name(provider),
        };
        let generation = generation(PRODUCER, model_name)?;
        let mut edit = input.scene.edit_as(generation.clone());
        for entity in observe_page_hierarchy(&mut edit, &input.scene, input.page)? {
            edit.observe::<SourceText>(entity, "default")?;
            edit.observe::<Translation>(entity, locale.as_str())?;
        }
        for ((entity, source), text) in targets.into_iter().zip(segments) {
            edit.set_translation(
                entity,
                &locale,
                Translation {
                    text: Authored::generated(translated_text(&source, text), generation.clone()),
                },
            )?;
        }
        finish(edit)
    }
}

fn translated_text(source: &str, translated: String) -> String {
    if source.trim() == "…" {
        "…".to_owned()
    } else {
        translated
    }
}

fn targets(
    input: &StageInput,
    locale: &LanguageTag,
) -> Result<Vec<(koharu_scene::EntityId, String)>> {
    let mut targets = Vec::new();
    for entity in input.scene.subtree(input.page)? {
        let id = entity.id();
        if !input.contains_entity(id)? {
            continue;
        }
        let Some(source) = entity.component::<SourceText>("default")? else {
            continue;
        };
        if source.text.value.trim().is_empty() {
            continue;
        }
        if input
            .scene
            .component::<Translation>(id, locale.as_str())?
            .is_some_and(|value| matches!(value.text.origin, Origin::User))
        {
            continue;
        }
        targets.push((id, source.text.value));
    }
    Ok(targets)
}

fn remote(provider: &Providers, client: &reqwest::Client) -> Result<RemoteTranslator> {
    let provider = match provider {
        Providers::AtlasCloud(config) => RemoteProvider::AtlasCloud(config.clone()),
        Providers::OpenAi(config) => RemoteProvider::OpenAi(config.clone()),
        Providers::Gemini(config) => RemoteProvider::Gemini(config.clone()),
        Providers::Claude(config) => RemoteProvider::Claude(config.clone()),
        Providers::DeepSeek(config) => RemoteProvider::DeepSeek(config.clone()),
        Providers::OpenAiCompatible(config) => RemoteProvider::OpenAiCompatible(config.clone()),
        Providers::OpenRouter(config) => RemoteProvider::OpenRouter(config.clone()),
        Providers::LmStudio(config) => RemoteProvider::LmStudio(config.clone()),
        Providers::DeepL(config) => RemoteProvider::DeepL(config.clone()),
        Providers::GoogleCloudTranslation(config) => {
            RemoteProvider::GoogleCloudTranslation(config.clone())
        }
        Providers::Caiyun(config) => RemoteProvider::Caiyun(config.clone()),
        Providers::Local(_) => bail!("local translator has no remote provider"),
    };
    Ok(RemoteTranslator::with_client(client.clone(), provider))
}

fn provider_name(provider: &Providers) -> &'static str {
    match provider {
        Providers::Local(_) => "local-translation",
        Providers::AtlasCloud(_) => "atlas-cloud",
        Providers::OpenAi(_) => "openai",
        Providers::Gemini(_) => "gemini",
        Providers::Claude(_) => "claude",
        Providers::DeepSeek(_) => "deepseek",
        Providers::OpenAiCompatible(_) => "openai-compatible",
        Providers::OpenRouter(_) => "openrouter",
        Providers::LmStudio(_) => "lm-studio",
        Providers::DeepL(_) => "deepl",
        Providers::GoogleCloudTranslation(_) => "google-cloud-translation",
        Providers::Caiyun(_) => "caiyun",
    }
}

#[cfg(test)]
mod tests {
    use super::translated_text;

    #[test]
    fn an_ellipsis_survives_translation() {
        assert_eq!(translated_text(" … ", "--- -->".to_owned()), "…");
        assert_eq!(
            translated_text("text", "translation".to_owned()),
            "translation"
        );
    }
}
