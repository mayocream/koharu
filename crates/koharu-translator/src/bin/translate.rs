use std::{env, str::FromStr};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use koharu_secrets::{ExposeSecret, SecretString};
use koharu_translator::{
    CaiyunConfig, ClaudeConfig, DeepLConfig, DeepSeekConfig, GeminiConfig, GoogleCloudConfig,
    Language, LmStudioConfig, LocalModel, LocalTranslator, OpenAiCompatibleConfig, OpenAiConfig,
    OpenRouterConfig, RemoteProvider, RemoteProviderKind, RemoteTranslator, TranslationContext,
    TranslationRequest, Translator,
};
use url::Url;

#[derive(Debug, Parser)]
#[command(about = "Run a local or hosted Koharu translation backend")]
struct Args {
    /// Translation backend to exercise.
    #[arg(long, value_enum)]
    provider: Provider,

    /// Local catalog ID or hosted model ID.
    #[arg(long)]
    model: Option<String>,

    /// API key. Stored in Koharu's credential store; when omitted, the
    /// provider-specific environment variable is stored or the current secret is used.
    #[arg(long)]
    api_key: Option<String>,

    /// Base URL for OpenAI-compatible APIs, LM Studio, or an optional DeepL override.
    #[arg(long)]
    base_url: Option<Url>,

    /// Source language tag or English name. Omit to use automatic detection.
    #[arg(long)]
    source: Option<Language>,

    /// Target language tag or English name.
    #[arg(long)]
    target: Language,

    /// Additional instructions for LLM translators.
    #[arg(long)]
    instructions: Option<String>,

    /// Earlier translation pair in SOURCE=TRANSLATION form. May be repeated.
    #[arg(long, value_name = "SOURCE=TRANSLATION")]
    context: Vec<ContextPair>,

    /// Force CPU inference for a local model.
    #[arg(long)]
    cpu: bool,

    /// Print the translated segment array as JSON.
    #[arg(long)]
    json: bool,

    /// Segments to translate. Quote segments containing whitespace.
    #[arg(required = true, num_args = 1..)]
    segments: Vec<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Provider {
    Local,
    #[value(name = "openai")]
    OpenAi,
    Gemini,
    Claude,
    #[value(name = "deepseek")]
    DeepSeek,
    OpenAiCompatible,
    #[value(name = "openrouter")]
    OpenRouter,
    #[value(name = "lm-studio")]
    LmStudio,
    #[value(name = "deepl")]
    DeepL,
    GoogleCloudTranslation,
    Caiyun,
}

#[derive(Debug, Clone)]
struct ContextPair(TranslationContext);

impl FromStr for ContextPair {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let (source, translation) = value
            .split_once('=')
            .ok_or_else(|| "context must use SOURCE=TRANSLATION form".to_owned())?;
        Ok(Self(TranslationContext::new(source, translation)))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let translator: Box<dyn Translator> = match args.provider {
        Provider::Local => {
            koharu_ml::init_llama().await?;
            let model = required_model(&args)?.parse::<LocalModel>()?;
            let device = koharu_ml::device(args.cpu);
            Box::new(LocalTranslator::load(device, model).await?)
        }
        provider => Box::new(RemoteTranslator::new(remote_provider(provider, &args)?)),
    };

    let mut request = TranslationRequest::new(args.segments, args.target)
        .with_context(args.context.into_iter().map(|ContextPair(context)| context));
    if let Some(source) = args.source {
        request = request.with_source_language(source);
    }
    if let Some(instructions) = args.instructions {
        request = request.with_instructions(instructions);
    }

    let translation = translator.translate(request).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&translation.segments)?);
    } else {
        for (index, segment) in translation.segments.iter().enumerate() {
            println!("[{}] {segment}", index + 1);
        }
    }
    Ok(())
}

fn remote_provider(provider: Provider, args: &Args) -> Result<RemoteProvider> {
    Ok(match provider {
        Provider::OpenAi => {
            prepare_secret(args, RemoteProviderKind::OpenAi, "OPENAI_API_KEY", true)?;
            RemoteProvider::OpenAi(OpenAiConfig::new(required_model(args)?))
        }
        Provider::Gemini => {
            prepare_secret(args, RemoteProviderKind::Gemini, "GEMINI_API_KEY", true)?;
            RemoteProvider::Gemini(GeminiConfig::new(required_model(args)?))
        }
        Provider::Claude => {
            prepare_secret(args, RemoteProviderKind::Claude, "ANTHROPIC_API_KEY", true)?;
            RemoteProvider::Claude(ClaudeConfig::new(required_model(args)?))
        }
        Provider::DeepSeek => {
            prepare_secret(args, RemoteProviderKind::DeepSeek, "DEEPSEEK_API_KEY", true)?;
            RemoteProvider::DeepSeek(DeepSeekConfig::new(required_model(args)?))
        }
        Provider::OpenAiCompatible => {
            prepare_secret(
                args,
                RemoteProviderKind::OpenAiCompatible,
                "OPENAI_COMPATIBLE_API_KEY",
                false,
            )?;
            let base_url = args
                .base_url
                .clone()
                .context("--base-url is required for open-ai-compatible")?;
            RemoteProvider::OpenAiCompatible(OpenAiCompatibleConfig::new(
                base_url,
                required_model(args)?,
            ))
        }
        Provider::OpenRouter => {
            prepare_secret(
                args,
                RemoteProviderKind::OpenRouter,
                "OPENROUTER_API_KEY",
                true,
            )?;
            RemoteProvider::OpenRouter(OpenRouterConfig::new(required_model(args)?))
        }
        Provider::LmStudio => {
            prepare_secret(
                args,
                RemoteProviderKind::LmStudio,
                "LM_STUDIO_API_TOKEN",
                false,
            )?;
            let base_url = args
                .base_url
                .clone()
                .unwrap_or_else(|| LmStudioConfig::default().base_url);
            RemoteProvider::LmStudio(LmStudioConfig::new(base_url, required_model(args)?))
        }
        Provider::DeepL => {
            prepare_secret(args, RemoteProviderKind::DeepL, "DEEPL_API_KEY", true)?;
            let mut config = DeepLConfig::default();
            if let Some(base_url) = args.base_url.clone() {
                config = config.with_base_url(base_url);
            }
            RemoteProvider::DeepL(config)
        }
        Provider::GoogleCloudTranslation => {
            prepare_secret(
                args,
                RemoteProviderKind::GoogleCloudTranslation,
                "GOOGLE_CLOUD_API_KEY",
                true,
            )?;
            RemoteProvider::GoogleCloudTranslation(GoogleCloudConfig::default())
        }
        Provider::Caiyun => {
            prepare_secret(args, RemoteProviderKind::Caiyun, "CAIYUN_API_KEY", true)?;
            RemoteProvider::Caiyun(CaiyunConfig::default())
        }
        Provider::Local => bail!("local provider must be constructed through the local path"),
    })
}

fn required_model(args: &Args) -> Result<&str> {
    args.model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
        .context("--model is required for this provider")
}

fn prepare_secret(
    args: &Args,
    provider: RemoteProviderKind,
    variable: &str,
    required: bool,
) -> Result<()> {
    let value = args
        .api_key
        .clone()
        .filter(|key| !key.trim().is_empty())
        .or_else(|| env::var(variable).ok().filter(|key| !key.trim().is_empty()));
    if let Some(value) = value {
        koharu_secrets::set(provider.id(), &SecretString::from(value))?;
    } else if required
        && koharu_secrets::get(provider.id())?
            .is_none_or(|value| value.expose_secret().trim().is_empty())
    {
        bail!("--api-key, {variable}, or a stored API key is required");
    }
    Ok(())
}
