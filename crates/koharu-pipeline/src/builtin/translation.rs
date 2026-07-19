use anyhow::{Context as _, Result, bail};
use async_trait::async_trait;
use koharu_scene::{Command, ElementChange};
use koharu_translator::{
    Language, LocalModel, LocalTranslator, Providers, RemoteProvider, RemoteTranslator,
    TranslationRequest, Translator,
};

use crate::{Artifact, Context, Processor};

pub(super) struct TranslationProcessor {
    config: Providers,
    backend: Backend,
}

enum Backend {
    Local(LocalTranslator),
    Remote(reqwest::Client),
}

impl TranslationProcessor {
    pub(super) async fn load(device: koharu_ml::Device, config: &Providers) -> Result<Self> {
        let backend = match config {
            Providers::Local(config) => {
                let model = config
                    .model
                    .parse::<LocalModel>()
                    .with_context(|| format!("unknown local translator '{}'", config.model))?;
                Backend::Local(LocalTranslator::load(device, model).await?)
            }
            _ => Backend::Remote(reqwest::Client::new()),
        };
        Ok(Self {
            config: config.clone(),
            backend,
        })
    }

    fn remote_translator(&self, client: &reqwest::Client) -> Result<RemoteTranslator> {
        let provider = match &self.config {
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
}

#[async_trait]
impl Processor for TranslationProcessor {
    fn name(&self) -> &'static str {
        match self.config {
            Providers::Local(_) => "LocalTranslator",
            Providers::OpenAi(_) => "OpenAI",
            Providers::Gemini(_) => "Gemini",
            Providers::Claude(_) => "Claude",
            Providers::DeepSeek(_) => "DeepSeek",
            Providers::OpenAiCompatible(_) => "OpenAI-compatible",
            Providers::OpenRouter(_) => "OpenRouter",
            Providers::LmStudio(_) => "LM Studio",
            Providers::DeepL(_) => "DeepL",
            Providers::GoogleCloudTranslation(_) => "Google Cloud Translation",
            Providers::Caiyun(_) => "Caiyun",
        }
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[Artifact::SourceText, Artifact::CooText]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[Artifact::Translation]
    }

    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let targets = context
            .pages()
            .iter()
            .flat_map(|page| {
                page.texts().filter_map(|(element, text)| {
                    let source = text.source.as_ref()?;
                    (context.includes_element(page.id, element.id, element.frame)
                        && !source.text.trim().is_empty())
                    .then(|| (page.id, element.id, source.text.clone()))
                })
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return Ok(context.commands());
        }
        let target = context
            .target_language()
            .parse::<Language>()
            .context("invalid translation target language")?;
        let translator: &dyn Translator = match &self.backend {
            Backend::Local(translator) => translator,
            Backend::Remote(client) => {
                let translator = self.remote_translator(client)?;
                return translate(context, &targets, target, &translator).await;
            }
        };
        translate(context, &targets, target, translator).await
    }
}

async fn translate(
    context: &Context,
    targets: &[(koharu_scene::PageId, koharu_scene::ElementId, String)],
    target: Language,
    translator: &dyn Translator,
) -> Result<koharu_scene::Commands> {
    let mut request =
        TranslationRequest::new(targets.iter().map(|(_, _, source)| source.as_str()), target);
    if let Some(instructions) = context.instructions() {
        request = request.with_instructions(instructions);
    }
    let translation = translator.translate(request).await?;
    let mut commands = context.commands();
    for ((page, element, _), translation) in targets.iter().zip(translation.segments) {
        commands.push(Command::EditElement {
            page: *page,
            element: *element,
            edit: ElementChange::Translation(Some(translation)),
        });
    }
    Ok(commands)
}
