use anyhow::{Context as _, Result, anyhow, bail};
use async_trait::async_trait;
use koharu_scene::{Command, ElementChange};
use koharu_translator::{
    ApiKey, CaiyunConfig, ClaudeConfig, DeepLConfig, DeepSeekConfig, GeminiConfig,
    GoogleCloudConfig, Language, LocalModel, LocalTranslator, OpenAiCompatibleConfig, OpenAiConfig,
    RemoteGenerationOptions, RemoteProvider, RemoteProviderKind, RemoteTranslator,
    TranslationRequest, Translator,
};

use crate::{
    ChatTranslationConfig, Context, OpenAiCompatibleTranslationConfig, Processor, Stage,
    TranslationModel,
};

pub(super) struct TranslationProcessor {
    config: TranslationModel,
    backend: Backend,
}

enum Backend {
    Local(LocalTranslator),
    Remote(reqwest::Client),
}

impl TranslationProcessor {
    pub(super) async fn load(device: koharu_ml::Device, config: &TranslationModel) -> Result<Self> {
        validate(config)?;
        let backend = match config {
            TranslationModel::Local(config) => {
                let model = config.local_model.parse::<LocalModel>().with_context(|| {
                    format!("unknown local translator '{}'", config.local_model)
                })?;
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
        let (provider, generation) = match &self.config {
            TranslationModel::OpenAi(config) => (
                RemoteProvider::OpenAi(OpenAiConfig::new(
                    api_key(RemoteProviderKind::OpenAi)?,
                    &config.remote_model,
                )),
                generation(config),
            ),
            TranslationModel::Gemini(config) => (
                RemoteProvider::Gemini(GeminiConfig::new(
                    api_key(RemoteProviderKind::Gemini)?,
                    &config.remote_model,
                )),
                generation(config),
            ),
            TranslationModel::Claude(config) => (
                RemoteProvider::Claude(ClaudeConfig::new(
                    api_key(RemoteProviderKind::Claude)?,
                    &config.remote_model,
                )),
                generation(config),
            ),
            TranslationModel::DeepSeek(config) => (
                RemoteProvider::DeepSeek(DeepSeekConfig::new(
                    api_key(RemoteProviderKind::DeepSeek)?,
                    &config.remote_model,
                )),
                generation(config),
            ),
            TranslationModel::OpenAiCompatible(config) => {
                let mut provider =
                    OpenAiCompatibleConfig::new(config.base_url.clone(), &config.remote_model);
                if let Some(key) = ApiKey::load(RemoteProviderKind::OpenAiCompatible)? {
                    provider = provider.with_api_key(key);
                }
                (
                    RemoteProvider::OpenAiCompatible(provider),
                    RemoteGenerationOptions {
                        temperature: config.temperature,
                        max_tokens: config.max_tokens,
                    },
                )
            }
            TranslationModel::DeepL(config) => {
                let mut provider = DeepLConfig::new(api_key(RemoteProviderKind::DeepL)?);
                if let Some(base_url) = &config.base_url {
                    provider = provider.with_base_url(base_url.clone());
                }
                (RemoteProvider::DeepL(provider), Default::default())
            }
            TranslationModel::GoogleCloudTranslation => (
                RemoteProvider::GoogleCloudTranslation(GoogleCloudConfig::new(api_key(
                    RemoteProviderKind::GoogleCloudTranslation,
                )?)),
                Default::default(),
            ),
            TranslationModel::Caiyun => (
                RemoteProvider::Caiyun(CaiyunConfig::new(api_key(RemoteProviderKind::Caiyun)?)),
                Default::default(),
            ),
            TranslationModel::Local(_) => bail!("local translator has no remote provider"),
        };
        Ok(RemoteTranslator::with_client(client.clone(), provider)
            .with_generation_options(generation))
    }
}

#[async_trait]
impl Processor for TranslationProcessor {
    fn name(&self) -> &'static str {
        match self.config {
            TranslationModel::Local(_) => "LocalTranslator",
            TranslationModel::OpenAi(_) => "OpenAI",
            TranslationModel::Gemini(_) => "Gemini",
            TranslationModel::Claude(_) => "Claude",
            TranslationModel::DeepSeek(_) => "DeepSeek",
            TranslationModel::OpenAiCompatible(_) => "OpenAI-compatible",
            TranslationModel::DeepL(_) => "DeepL",
            TranslationModel::GoogleCloudTranslation => "Google Cloud Translation",
            TranslationModel::Caiyun => "Caiyun",
        }
    }

    fn stage(&self) -> Stage {
        Stage::Translation
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
            .ok_or_else(|| anyhow!("translation requires a target language"))?
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

fn api_key(provider: RemoteProviderKind) -> Result<ApiKey> {
    ApiKey::load(provider)?.ok_or_else(|| anyhow!("{} API key is not configured", provider.id()))
}

const fn generation(config: &ChatTranslationConfig) -> RemoteGenerationOptions {
    RemoteGenerationOptions {
        temperature: config.temperature,
        max_tokens: config.max_tokens,
    }
}

fn validate(config: &TranslationModel) -> Result<()> {
    match config {
        TranslationModel::Local(config) if config.local_model.trim().is_empty() => {
            bail!("local_model must not be empty")
        }
        TranslationModel::OpenAi(config)
        | TranslationModel::Gemini(config)
        | TranslationModel::Claude(config)
        | TranslationModel::DeepSeek(config) => {
            validate_generation(&config.remote_model, config.temperature, config.max_tokens)?
        }
        TranslationModel::OpenAiCompatible(config) => validate_compatible(config)?,
        _ => {}
    }
    Ok(())
}

fn validate_generation(
    remote_model: &str,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
) -> Result<()> {
    if remote_model.trim().is_empty() {
        bail!("remote_model must not be empty");
    }
    if temperature.is_some_and(|value| !value.is_finite()) {
        bail!("temperature must be finite");
    }
    if max_tokens == Some(0) {
        bail!("max_tokens must be positive");
    }
    Ok(())
}

fn validate_compatible(config: &OpenAiCompatibleTranslationConfig) -> Result<()> {
    validate_generation(&config.remote_model, config.temperature, config.max_tokens)?;
    Ok(())
}
