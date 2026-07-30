use anyhow::{Context as _, Result, bail};
use koharu_scene::{Authored, LanguageTag, Origin, SourceText, Translation};
use koharu_translator::{
    Language, LocalModel, LocalTranslator, Providers, RemoteProvider, RemoteTranslator,
    TranslationRequest, Translator,
};

use super::{finish, generation, producer};
use crate::{NodeInput, NodeOutput, Stage};

pub(super) struct Model {
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
    pub(super) async fn load(device: koharu_ml::Device, config: &Providers) -> Result<Self> {
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

    pub(super) async fn run(&self, input: NodeInput) -> Result<NodeOutput> {
        let locale = LanguageTag::new(input.options.target_language.clone())?;
        let target = input
            .options
            .target_language
            .parse::<Language>()
            .context("invalid translation target language")?;
        let mut targets = Vec::new();
        for entity in input.scene.entities_with::<SourceText>("default")? {
            let id = entity.id();
            if !input.scope.contains_entity(&input.scene, id)? {
                continue;
            }
            let source = entity
                .component::<SourceText>("default")?
                .expect("entities_with returned an entity with source text");
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

        let segments = if targets.is_empty() {
            Vec::new()
        } else {
            let mut request =
                TranslationRequest::new(targets.iter().map(|(_, source)| source.as_str()), target);
            if let Some(instructions) = &input.options.translation_instructions {
                request = request.with_instructions(instructions);
            }
            match &self.backend {
                Backend::Local(translator) => translator.translate(request).await?.segments,
                Backend::Remote { provider, client } => {
                    remote(provider, client)?.translate(request).await?.segments
                }
            }
        };
        if input.cancellation.is_cancelled() {
            bail!("translation was cancelled");
        }
        let model_name = match &self.backend {
            Backend::Local(_) => "local-translation",
            Backend::Remote { provider, .. } => provider_name(provider),
        };
        let generation = generation(producer(Stage::Translation), model_name)?;
        let mut edit = input.scene.edit_as(generation.clone());
        for ((entity, _), text) in targets.into_iter().zip(segments) {
            edit.set_translation(
                entity,
                &locale,
                Translation {
                    text: Authored::generated(text, generation.clone()),
                },
            )?;
        }
        finish(edit)
    }
}

fn remote(provider: &Providers, client: &reqwest::Client) -> Result<RemoteTranslator> {
    let provider = match provider {
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
