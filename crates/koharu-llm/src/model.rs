use std::{
    fmt,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use koharu_llama::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{
        AddBos, LlamaChatMessage, LlamaModel,
        params::{LlamaModelParams, LlamaSplitMode},
    },
    sampling::LlamaSampler,
    token::LlamaToken,
};
use koharu_runtime::package::{Package, PreloadablePackage, llama_cpp::LlamaCpp};

use crate::{BuiltinModel, ModelSource};

const DEFAULT_GPU_LAYERS: u32 = 1000;
const DEFAULT_MAX_TOKENS: usize = 512;
const DEFAULT_SEED: u32 = 299_792_458;
const DEFAULT_MAX_UBATCH: u32 = 512;
const SAKURA_QWEN_CORRECT_EOS_ID: i32 = 151_645;

pub async fn init() -> Result<()> {
    init_with_runtime(LlamaRuntime::default()).await
}

pub async fn init_with_runtime(runtime: LlamaRuntime) -> Result<()> {
    prepare_runtime(runtime).await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum LlamaRuntime {
    #[default]
    Auto,
    Cpu,
    Cuda,
    Vulkan,
}

impl LlamaRuntime {
    pub fn package(self) -> Result<LlamaCpp> {
        match self {
            Self::Auto => Ok(LlamaCpp::for_current_target()),
            Self::Cpu => cpu_package(),
            Self::Cuda => cuda_package(),
            Self::Vulkan => vulkan_package(),
        }
    }
}

async fn preload_runtime(package: LlamaCpp) -> Result<PathBuf> {
    let package_dir = package.resolve().await?;
    package.preload().await?;
    Ok(package_dir)
}

async fn prepare_runtime(runtime: LlamaRuntime) -> Result<PathBuf> {
    let runtime_dir = preload_runtime(runtime.package()?).await?;
    LlamaBackend::load_all_backends_from_path(&runtime_dir)
        .map_err(|err| anyhow::anyhow!("failed to load llama.cpp backends: {err}"))?;
    Ok(runtime_dir)
}

fn cpu_package() -> Result<LlamaCpp> {
    if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        Ok(LlamaCpp::WindowsArm64Cpu)
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok(LlamaCpp::WindowsX64Cpu)
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Ok(LlamaCpp::LinuxArm64Cpu)
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok(LlamaCpp::LinuxX64Cpu)
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Ok(LlamaCpp::MacosX64)
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok(LlamaCpp::MacosArm64)
    } else {
        bail!("unsupported llama.cpp CPU runtime for this target")
    }
}

fn cuda_package() -> Result<LlamaCpp> {
    LlamaCpp::cuda_for_current_target()
}

fn vulkan_package() -> Result<LlamaCpp> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok(LlamaCpp::WindowsX64Vulkan)
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Ok(LlamaCpp::LinuxArm64Vulkan)
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok(LlamaCpp::LinuxX64Vulkan)
    } else {
        bail!("unsupported llama.cpp Vulkan runtime for this target")
    }
}

#[derive(Debug, Clone)]
pub struct LoadOptions {
    pub runtime: LlamaRuntime,
    pub cpu_only: bool,
    pub gpu_layers: u32,
    pub use_mmap: bool,
    pub use_mlock: bool,
    pub split_mode: LlamaSplitMode,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            runtime: LlamaRuntime::default(),
            cpu_only: false,
            gpu_layers: DEFAULT_GPU_LAYERS,
            use_mmap: true,
            use_mlock: false,
            split_mode: LlamaSplitMode::Layer,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GenerationOptions {
    pub max_tokens: usize,
    pub temperature: f32,
    pub top_k: Option<usize>,
    pub top_p: Option<f32>,
    pub min_p: Option<f32>,
    pub seed: u32,
    pub repeat_penalty: f32,
    pub repeat_last_n: usize,
    pub frequency_penalty: f32,
    pub presence_penalty: f32,
    pub split_prompt: bool,
    pub n_ctx: Option<NonZeroU32>,
    pub n_batch: Option<u32>,
    pub n_ubatch: Option<u32>,
    pub n_threads: Option<i32>,
    pub n_threads_batch: Option<i32>,
}

impl Default for GenerationOptions {
    fn default() -> Self {
        Self {
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: 0.1,
            top_k: None,
            top_p: None,
            min_p: None,
            seed: DEFAULT_SEED,
            repeat_penalty: 1.1,
            repeat_last_n: 64,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            split_prompt: false,
            n_ctx: None,
            n_batch: None,
            n_ubatch: None,
            n_threads: None,
            n_threads_batch: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmBuilder {
    source: ModelSource,
    load_options: LoadOptions,
}

impl LlmBuilder {
    pub fn new(source: impl Into<ModelSource>) -> Self {
        Self {
            source: source.into(),
            load_options: LoadOptions::default(),
        }
    }

    pub fn with_load_options(mut self, load_options: LoadOptions) -> Self {
        self.load_options = load_options;
        self
    }

    pub fn with_cpu_only(mut self, cpu_only: bool) -> Self {
        self.load_options.cpu_only = cpu_only;
        self
    }

    pub fn with_gpu_layers(mut self, gpu_layers: u32) -> Self {
        self.load_options.gpu_layers = gpu_layers;
        self
    }

    pub fn with_runtime(mut self, runtime: LlamaRuntime) -> Self {
        self.load_options.runtime = runtime;
        self
    }

    pub async fn load(self) -> Result<LlmModel> {
        prepare_runtime(self.load_options.runtime)
            .await
            .context("failed to preload llama.cpp runtime")?;
        let backend = Arc::new(
            LlamaBackend::init()
                .map_err(|err| anyhow::anyhow!("failed to init llama.cpp: {err}"))?,
        );
        self.load_resolved_with_backend(backend).await
    }

    pub async fn load_with_backend(self, backend: Arc<LlamaBackend>) -> Result<LlmModel> {
        prepare_runtime(self.load_options.runtime)
            .await
            .context("failed to preload llama.cpp runtime")?;
        self.load_resolved_with_backend(backend).await
    }

    async fn load_resolved_with_backend(self, backend: Arc<LlamaBackend>) -> Result<LlmModel> {
        let model_path = self.source.resolve().await?;
        let source = self.source;
        let load_options = self.load_options;

        tokio::task::spawn_blocking(move || {
            LlmModel::load_from_path(source, model_path, backend, load_options)
        })
        .await
        .context("failed to join llama.cpp model loading task")?
    }
}

pub struct LlmModel {
    source: ModelSource,
    backend: Arc<LlamaBackend>,
    model: LlamaModel,
    eos_token: LlamaToken,
}

impl fmt::Debug for LlmModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmModel")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl LlmModel {
    pub async fn load(source: impl Into<ModelSource>) -> Result<Self> {
        LlmBuilder::new(source).load().await
    }

    pub async fn load_with_options(
        source: impl Into<ModelSource>,
        load_options: LoadOptions,
    ) -> Result<Self> {
        LlmBuilder::new(source)
            .with_load_options(load_options)
            .load()
            .await
    }

    fn load_from_path(
        source: ModelSource,
        model_path: PathBuf,
        backend: Arc<LlamaBackend>,
        load_options: LoadOptions,
    ) -> Result<Self> {
        ensure_model_path(&model_path)?;

        let model_params = model_params(&backend, &load_options);
        let model = LlamaModel::load_from_file(backend.as_ref(), &model_path, &model_params)
            .with_context(|| format!("failed to load GGUF model {}", model_path.display()))?;

        let eos_token = eos_token_for(&source, &model);

        Ok(Self {
            source,
            backend,
            model,
            eos_token,
        })
    }

    pub fn source(&self) -> &ModelSource {
        &self.source
    }

    pub fn default_generation_options(&self) -> GenerationOptions {
        self.source.default_generation_options()
    }

    pub fn tokenize(&self, prompt: &str) -> Result<Vec<LlamaToken>> {
        self.model
            .str_to_token(prompt, AddBos::Never)
            .context("failed to tokenize prompt")
    }

    pub fn render_chat_prompt(&self, messages: &[ChatMessage]) -> Result<String> {
        let llama_messages = messages
            .iter()
            .map(|message| {
                LlamaChatMessage::new(message.role.as_str().to_owned(), message.content.clone())
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to build llama.cpp chat messages")?;

        match self.model.chat_template(None) {
            Ok(template) => self
                .model
                .apply_chat_template(&template, &llama_messages, true)
                .context("failed to apply GGUF chat template"),
            Err(_) => Ok(render_fallback_chat_prompt(messages)),
        }
    }

    pub fn generate(&mut self, prompt: &str, options: &GenerationOptions) -> Result<Generation> {
        self.generate_with_callback(prompt, options, |_| Ok(GenerationControl::Continue))
    }

    pub fn chat(
        &mut self,
        messages: &[ChatMessage],
        options: &GenerationOptions,
    ) -> Result<Generation> {
        let prompt = self.render_chat_prompt(messages)?;
        self.generate(&prompt, options)
    }

    pub fn chat_with_callback<F>(
        &mut self,
        messages: &[ChatMessage],
        options: &GenerationOptions,
        on_token: F,
    ) -> Result<Generation>
    where
        F: FnMut(TokenChunk<'_>) -> Result<GenerationControl>,
    {
        let prompt = self.render_chat_prompt(messages)?;
        self.generate_with_callback(&prompt, options, on_token)
    }

    pub fn generate_with_callback<F>(
        &mut self,
        prompt: &str,
        options: &GenerationOptions,
        mut on_token: F,
    ) -> Result<Generation>
    where
        F: FnMut(TokenChunk<'_>) -> Result<GenerationControl>,
    {
        if options.max_tokens == 0 {
            return Ok(Generation::empty(FinishReason::Length));
        }

        let prompt_tokens = self.tokenize(prompt)?;
        if prompt_tokens.is_empty() {
            bail!("prompt produced no tokens");
        }

        let mut ctx = self
            .model
            .new_context(
                self.backend.as_ref(),
                context_params(prompt_tokens.len(), options)?,
            )
            .context("failed to create llama.cpp context")?;
        let mut sampler = build_sampler(options);
        sampler.accept_many(&prompt_tokens);

        let prompt_start = Instant::now();
        let mut next_token = if options.split_prompt {
            process_prompt_split(&mut ctx, &prompt_tokens, &mut sampler)?
        } else {
            process_prompt_batch(&mut ctx, &prompt_tokens, &mut sampler)?
        };
        let prompt_duration = prompt_start.elapsed();

        if self.should_stop(next_token) {
            return Ok(Generation {
                text: String::new(),
                prompt_tokens: prompt_tokens.len(),
                generated_tokens: 0,
                prompt_duration,
                generation_duration: Duration::ZERO,
                finish_reason: FinishReason::StopToken,
            });
        }

        let generation_start = Instant::now();
        let mut generated = String::new();
        let mut generated_tokens = 0usize;
        let mut position = i32::try_from(prompt_tokens.len()).context("prompt is too long")?;
        let mut batch = LlamaBatch::new(1, 1);
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut finish_reason = FinishReason::Length;

        while generated_tokens < options.max_tokens {
            sampler.accept(next_token);
            let piece = decode_token(&self.model, next_token, &mut decoder)?;
            generated.push_str(&piece);
            generated_tokens += 1;

            let chunk = TokenChunk {
                token: next_token,
                piece: &piece,
                generated_tokens,
            };
            if matches!(on_token(chunk)?, GenerationControl::Stop) {
                finish_reason = FinishReason::Callback;
                break;
            }

            if generated_tokens >= options.max_tokens {
                break;
            }

            batch.clear();
            batch
                .add(next_token, position, &[0], true)
                .context("failed to build generation batch")?;
            ctx.decode(&mut batch)
                .context("failed to decode generated token")?;
            position += 1;

            next_token = sampler.sample(&ctx, batch.n_tokens() - 1);
            if self.should_stop(next_token) {
                finish_reason = FinishReason::StopToken;
                break;
            }
        }

        Ok(Generation {
            text: generated,
            prompt_tokens: prompt_tokens.len(),
            generated_tokens,
            prompt_duration,
            generation_duration: generation_start.elapsed(),
            finish_reason,
        })
    }

    fn should_stop(&self, token: LlamaToken) -> bool {
        token == self.eos_token || self.model.is_eog_token(token)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(ChatRole::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(ChatRole::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(ChatRole::Assistant, content)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

impl ChatRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Generation {
    pub text: String,
    pub prompt_tokens: usize,
    pub generated_tokens: usize,
    pub prompt_duration: Duration,
    pub generation_duration: Duration,
    pub finish_reason: FinishReason,
}

impl Generation {
    fn empty(finish_reason: FinishReason) -> Self {
        Self {
            text: String::new(),
            prompt_tokens: 0,
            generated_tokens: 0,
            prompt_duration: Duration::ZERO,
            generation_duration: Duration::ZERO,
            finish_reason,
        }
    }

    pub fn prompt_tokens_per_second(&self) -> f64 {
        rate(self.prompt_tokens, self.prompt_duration)
    }

    pub fn generated_tokens_per_second(&self) -> f64 {
        rate(self.generated_tokens, self.generation_duration)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishReason {
    StopToken,
    Length,
    Callback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationControl {
    Continue,
    Stop,
}

#[derive(Debug, Clone, Copy)]
pub struct TokenChunk<'a> {
    pub token: LlamaToken,
    pub piece: &'a str,
    pub generated_tokens: usize,
}

fn model_params(backend: &LlamaBackend, options: &LoadOptions) -> LlamaModelParams {
    let gpu_layers = if options.cpu_only || !backend.supports_gpu_offload() {
        0
    } else {
        options.gpu_layers
    };

    LlamaModelParams::default()
        .with_n_gpu_layers(gpu_layers)
        .with_use_mmap(options.use_mmap)
        .with_use_mlock(options.use_mlock)
        .with_split_mode(options.split_mode)
}

fn context_params(prompt_tokens: usize, options: &GenerationOptions) -> Result<LlamaContextParams> {
    let required_ctx = prompt_tokens
        .saturating_add(options.max_tokens)
        .saturating_add(1)
        .max(1);
    let n_ctx = match options.n_ctx {
        Some(n_ctx) if usize::try_from(n_ctx.get()).unwrap_or(usize::MAX) >= required_ctx => n_ctx,
        Some(_) => {
            bail!("n_ctx is too small: need at least {required_ctx} tokens for prompt + generation")
        }
        None => NonZeroU32::new(u32::try_from(required_ctx).context("context size exceeds u32")?)
            .expect("required context is non-zero"),
    };

    let n_batch = options
        .n_batch
        .unwrap_or_else(|| u32::try_from(prompt_tokens.max(1)).unwrap_or(u32::MAX))
        .max(1);
    let n_ubatch = options
        .n_ubatch
        .unwrap_or_else(|| n_batch.min(DEFAULT_MAX_UBATCH))
        .max(1)
        .min(n_batch);

    let mut params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_n_batch(n_batch)
        .with_n_ubatch(n_ubatch);

    if let Some(n_threads) = options.n_threads {
        params = params.with_n_threads(n_threads);
    }
    if let Some(n_threads_batch) = options.n_threads_batch {
        params = params.with_n_threads_batch(n_threads_batch);
    }

    Ok(params)
}

fn build_sampler(options: &GenerationOptions) -> LlamaSampler {
    let mut samplers = Vec::new();

    let has_repeat =
        (options.repeat_penalty - 1.0).abs() >= f32::EPSILON && options.repeat_last_n > 0;
    let has_frequency = options.frequency_penalty.abs() >= f32::EPSILON;
    let has_presence = options.presence_penalty.abs() >= f32::EPSILON;
    if has_repeat || has_frequency || has_presence {
        samplers.push(LlamaSampler::penalties(
            i32::try_from(options.repeat_last_n).unwrap_or(i32::MAX),
            if has_repeat {
                options.repeat_penalty
            } else {
                1.0
            },
            options.frequency_penalty,
            options.presence_penalty,
        ));
    }

    if options.temperature <= 0.0 {
        samplers.push(LlamaSampler::greedy());
        return LlamaSampler::chain_simple(samplers);
    }

    if let Some(top_k) = options.top_k.filter(|value| *value > 0) {
        samplers.push(LlamaSampler::top_k(
            i32::try_from(top_k).unwrap_or(i32::MAX),
        ));
    }
    if let Some(top_p) = options.top_p {
        samplers.push(LlamaSampler::top_p(top_p, 1));
    }
    if let Some(min_p) = options.min_p.filter(|value| *value > 0.0) {
        samplers.push(LlamaSampler::min_p(min_p, 1));
    }

    samplers.push(LlamaSampler::temp(options.temperature));
    samplers.push(LlamaSampler::dist(options.seed));
    LlamaSampler::chain_simple(samplers)
}

fn process_prompt_batch(
    ctx: &mut koharu_llama::context::LlamaContext<'_>,
    prompt_tokens: &[LlamaToken],
    sampler: &mut LlamaSampler,
) -> Result<LlamaToken> {
    let mut batch = LlamaBatch::new(prompt_tokens.len(), 1);
    batch
        .add_sequence(prompt_tokens, 0, false)
        .context("failed to build prompt batch")?;
    ctx.decode(&mut batch)
        .context("failed to decode prompt batch")?;
    Ok(sampler.sample(ctx, batch.n_tokens() - 1))
}

fn process_prompt_split(
    ctx: &mut koharu_llama::context::LlamaContext<'_>,
    prompt_tokens: &[LlamaToken],
    sampler: &mut LlamaSampler,
) -> Result<LlamaToken> {
    let last_index = prompt_tokens.len() - 1;

    for (index, token) in prompt_tokens.iter().copied().enumerate() {
        let mut batch = LlamaBatch::new(1, 1);
        batch
            .add(
                token,
                i32::try_from(index).context("prompt is too long")?,
                &[0],
                index == last_index,
            )
            .context("failed to build split prompt batch")?;
        ctx.decode(&mut batch)
            .with_context(|| format!("failed to decode prompt token {index}"))?;

        if index == last_index {
            return Ok(sampler.sample(ctx, batch.n_tokens() - 1));
        }
    }

    bail!("split prompt processing did not produce a final token")
}

fn eos_token_for(source: &ModelSource, model: &LlamaModel) -> LlamaToken {
    match source {
        ModelSource::Builtin(BuiltinModel::Sakura1_5bQwen2_5v1_0) => {
            LlamaToken::new(SAKURA_QWEN_CORRECT_EOS_ID)
        }
        _ => model.token_eos(),
    }
}

fn decode_token(
    model: &LlamaModel,
    token: LlamaToken,
    decoder: &mut encoding_rs::Decoder,
) -> Result<String> {
    model
        .token_to_piece(token, decoder, true, None)
        .context("failed to decode generated token")
}

fn render_fallback_chat_prompt(messages: &[ChatMessage]) -> String {
    let mut prompt = String::new();
    for message in messages {
        prompt.push_str(message.role.as_str());
        prompt.push_str(": ");
        prompt.push_str(&message.content);
        prompt.push('\n');
    }
    prompt.push_str("assistant: ");
    prompt
}

fn ensure_model_path(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("model file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("model path is not a file: {}", path.display());
    }
    Ok(())
}

fn rate(tokens: usize, duration: Duration) -> f64 {
    if duration.as_secs_f64() > 0.0 {
        tokens as f64 / duration.as_secs_f64()
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::{GenerationOptions, context_params};

    #[test]
    fn context_params_rejects_too_small_context() {
        let options = GenerationOptions {
            max_tokens: 10,
            n_ctx: NonZeroU32::new(4),
            ..Default::default()
        };
        assert!(context_params(5, &options).is_err());
    }

    #[test]
    fn context_params_uses_required_context_by_default() {
        let options = GenerationOptions {
            max_tokens: 10,
            ..Default::default()
        };
        let params = context_params(5, &options).unwrap();
        assert_eq!(params.n_ctx(), NonZeroU32::new(16));
    }
}
