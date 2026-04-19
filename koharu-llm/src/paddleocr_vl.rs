use std::ffi::CString;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use image::DynamicImage;
use koharu_runtime::RuntimeManager;
use minijinja::context;
use serde::{Deserialize, Serialize};

use crate::jinja;
use crate::safe::context::params::{LlamaAttentionType, LlamaContextParams};
use crate::safe::llama_backend::LlamaBackend;
use crate::safe::llama_batch::LlamaBatch;
use crate::safe::model::LlamaModel;
use crate::safe::model::params::LlamaModelParams;
use crate::safe::mtmd::{
    MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputChunkType, MtmdInputText,
};
use crate::safe::sampling::LlamaSampler;
use crate::safe::token::LlamaToken;

const HF_REPO: &str = "PaddlePaddle/PaddleOCR-VL-1.5-GGUF";
const MODEL_FILENAME: &str = "PaddleOCR-VL-1.5.gguf";
const MMPROJ_FILENAME: &str = "PaddleOCR-VL-1.5-mmproj.gguf";
const DEFAULT_MEDIA_MARKER: &str = "<__media__>";
const DEFAULT_GPU_LAYERS: u32 = 1000;
const DEFAULT_MAX_NEW_TOKENS: usize = 128;
const MAX_UBATCH: u32 = 512;

koharu_runtime::declare_hf_model_package!(
    id: "model:paddleocr-vl:weights",
    repo: HF_REPO,
    file: MODEL_FILENAME,
    bootstrap: false,
    order: 120,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:paddleocr-vl:mmproj",
    repo: HF_REPO,
    file: MMPROJ_FILENAME,
    bootstrap: false,
    order: 121,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaddleOcrVlTask {
    Ocr,
    Table,
    Formula,
    Chart,
    Spotting,
    Seal,
}

impl PaddleOcrVlTask {
    fn prompt(self) -> &'static str {
        match self {
            Self::Ocr => "OCR:",
            Self::Table => "Table Recognition:",
            Self::Formula => "Formula Recognition:",
            Self::Chart => "Chart Recognition:",
            Self::Spotting => "Spotting:",
            Self::Seal => "Seal Recognition:",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaddleOcrVlOutput {
    pub task: PaddleOcrVlTask,
    pub text: String,
    pub token_ids: Vec<u32>,
    pub original_width: u32,
    pub original_height: u32,
    pub processed_width: u32,
    pub processed_height: u32,
    pub grid_thw: [u32; 3],
    pub num_image_tokens: usize,
}

struct ModelFiles {
    model: PathBuf,
    mmproj: PathBuf,
}

struct RenderedPrompt {
    text: String,
    add_special: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PromptMessage {
    role: &'static str,
    content: String,
}

pub struct PaddleOcrVl {
    backend: Arc<LlamaBackend>,
    model: LlamaModel,
    chat_template: String,
    bos_token: String,
    eos_token_text: String,
    mtmd: MtmdContext,
    eos_token: LlamaToken,
}

impl PaddleOcrVl {
    pub async fn load(
        runtime: &RuntimeManager,
        cpu: bool,
        backend: Arc<LlamaBackend>,
    ) -> Result<Self> {
        let files = download_model_files(runtime).await?;
        let runtime = runtime.clone();
        tokio::task::spawn_blocking(move || Self::load_from_files(&runtime, files, cpu, backend))
            .await
            .context("failed to join PaddleOCR-VL loading task")?
    }

    pub fn load_from_dir(
        runtime: &RuntimeManager,
        dir: impl AsRef<Path>,
        cpu: bool,
        backend: Arc<LlamaBackend>,
    ) -> Result<Self> {
        let files = resolve_local_model_files(dir.as_ref())?;
        Self::load_from_files(runtime, files, cpu, backend)
    }

    fn load_from_files(
        runtime: &RuntimeManager,
        files: ModelFiles,
        cpu: bool,
        backend: Arc<LlamaBackend>,
    ) -> Result<Self> {
        crate::sys::initialize(runtime)
            .context("failed to initialize llama.cpp runtime bindings")?;

        let model_params = model_params(cpu, backend.as_ref());
        let model = LlamaModel::load_from_file(backend.as_ref(), &files.model, &model_params)
            .with_context(|| format!("unable to load model from `{}`", files.model.display()))?;
        let eos_token = model.token_eos();
        let chat_template = model
            .meta_val_str("tokenizer.ggml.chat_template")
            .or_else(|_| model.meta_val_str("tokenizer.chat_template"))
            .context("missing embedded PaddleOCR-VL chat template")?;
        let bos_token = token_text(&model, model.token_bos());
        let eos_token_text = token_text(&model, eos_token);
        let mmproj_path = files
            .mmproj
            .to_str()
            .with_context(|| format!("invalid mmproj path `{}`", files.mmproj.display()))?;
        let mtmd = MtmdContext::init_from_file(
            mmproj_path,
            &model,
            &MtmdContextParams {
                use_gpu: !cpu && backend.as_ref().supports_gpu_offload(),
                print_timings: false,
                n_threads: num_cpus::get().try_into().unwrap_or(i32::MAX),
                media_marker: CString::new(DEFAULT_MEDIA_MARKER)
                    .expect("default media marker contains no null bytes"),
            },
        )
        .context("unable to initialize multimodal projector")?;

        if !mtmd.support_vision() {
            bail!("loaded PaddleOCR-VL projector does not advertise vision support");
        }

        tracing::info!(
            has_encoder = unsafe { crate::sys::llama_model_has_encoder(model.model.as_ptr()) },
            has_decoder = unsafe { crate::sys::llama_model_has_decoder(model.model.as_ptr()) },
            decoder_start_token = model.decode_start_token().0,
            "loaded PaddleOCR-VL model capabilities"
        );

        Ok(Self {
            backend,
            model,
            chat_template,
            bos_token,
            eos_token_text,
            mtmd,
            eos_token,
        })
    }

    pub fn inference(
        &mut self,
        image: &DynamicImage,
        task: PaddleOcrVlTask,
    ) -> Result<PaddleOcrVlOutput> {
        self.inference_with_max_new_tokens(image, task, DEFAULT_MAX_NEW_TOKENS)
    }

    pub fn inference_with_max_new_tokens(
        &mut self,
        image: &DynamicImage,
        task: PaddleOcrVlTask,
        max_new_tokens: usize,
    ) -> Result<PaddleOcrVlOutput> {
        let started = Instant::now();
        let original_width = image.width();
        let original_height = image.height();
        let bitmap = bitmap_from_image(image)?;
        let prompt = self.render_prompt(task)?;
        let chunks = self
            .mtmd
            .tokenize(
                MtmdInputText {
                    text: prompt.text,
                    add_special: prompt.add_special,
                    parse_special: true,
                },
                &[&bitmap],
            )
            .context("failed to tokenize multimodal OCR input")?;
        if chunks.is_empty() {
            bail!("multimodal tokenization produced no chunks");
        }

        let batch_tokens = max_chunk_tokens(&chunks).max(1);
        let prompt_positions =
            usize::try_from(chunks.total_positions()).context("prompt positions overflow usize")?;
        let prompt_total_tokens = chunks.total_tokens();
        let num_image_tokens = image_chunk_tokens(&chunks);
        let ctx_params = context_params(
            &self.mtmd,
            prompt_positions,
            prompt_total_tokens,
            batch_tokens,
            max_new_tokens,
        )?;
        let mut ctx = self
            .model
            .new_context(self.backend.as_ref(), ctx_params)
            .context("unable to create PaddleOCR-VL llama.cpp context")?;
        let n_batch = i32::try_from(batch_tokens).context("batch size exceeds i32")?;

        let prompt_started = Instant::now();
        let n_past = chunks
            .eval_chunks(&self.mtmd, &ctx, 0, 0, n_batch, true)
            .context("failed to evaluate multimodal prompt")?;
        let prompt_elapsed = prompt_started.elapsed();

        let generation_started = Instant::now();
        let mut sampler = LlamaSampler::greedy();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut token_ids = Vec::new();
        let mut text = String::new();

        if max_new_tokens > 0 {
            let decoder_start = self.decoder_start_token();
            let (mut next_token, mut position) = if let Some(decoder_start) = decoder_start {
                let mut batch = LlamaBatch::new(1, 1);
                batch
                    .add(decoder_start, 0, &[0], true)
                    .context("failed to build decoder start batch")?;
                ctx.decode(&mut batch)
                    .context("failed to decode PaddleOCR-VL decoder start token")?;
                (sampler.sample(&ctx, -1), 1)
            } else {
                (sampler.sample(&ctx, -1), n_past)
            };

            while token_ids.len() < max_new_tokens && !self.should_stop(next_token) {
                sampler.accept(next_token);
                token_ids
                    .push(u32::try_from(next_token.0).context("generated token id was negative")?);
                text.push_str(&decode_token(&self.model, next_token, &mut decoder)?);

                if token_ids.len() >= max_new_tokens {
                    break;
                }

                let mut batch = LlamaBatch::new(1, 1);
                batch
                    .add(next_token, position, &[0], true)
                    .context("failed to build generation batch")?;
                ctx.decode(&mut batch)
                    .context("failed to decode generated OCR token")?;
                position += 1;
                next_token = sampler.sample(&ctx, -1);
            }
        }

        tracing::debug!(
            task = ?task,
            original_width,
            original_height,
            prompt_total_tokens,
            prompt_positions,
            batch_tokens,
            num_image_tokens,
            prompt_ms = prompt_elapsed.as_millis(),
            generation_ms = generation_started.elapsed().as_millis(),
            total_ms = started.elapsed().as_millis(),
            "paddleocr-vl inference timings"
        );

        Ok(PaddleOcrVlOutput {
            task,
            text: text.trim().to_string(),
            token_ids,
            original_width,
            original_height,
            processed_width: original_width,
            processed_height: original_height,
            grid_thw: [0, 0, 0],
            num_image_tokens,
        })
    }

    pub fn inference_images(
        &mut self,
        images: &[DynamicImage],
        task: PaddleOcrVlTask,
        max_new_tokens: usize,
    ) -> Result<Vec<PaddleOcrVlOutput>> {
        let started = Instant::now();
        let mut outputs = Vec::with_capacity(images.len());
        for image in images {
            outputs.push(self.inference_with_max_new_tokens(image, task, max_new_tokens)?);
        }
        tracing::info!(
            images = images.len(),
            total_ms = started.elapsed().as_millis(),
            "paddleocr-vl batch timings"
        );
        Ok(outputs)
    }

    fn should_stop(&self, token: LlamaToken) -> bool {
        token == self.eos_token || self.model.is_eog_token(token)
    }

    fn render_prompt(&self, task: PaddleOcrVlTask) -> Result<RenderedPrompt> {
        let text = render_chat_prompt(
            &self.chat_template,
            &self.bos_token,
            &self.eos_token_text,
            task,
        )
        .context("failed to render PaddleOCR-VL prompt from embedded chat template")?;
        Ok(RenderedPrompt {
            text,
            add_special: false,
        })
    }

    fn decoder_start_token(&self) -> Option<LlamaToken> {
        let has_encoder = unsafe { crate::sys::llama_model_has_encoder(self.model.model.as_ptr()) };
        if !has_encoder {
            return None;
        }

        let decoder_start = self.model.decode_start_token();
        if decoder_start.0 >= 0 {
            Some(decoder_start)
        } else {
            Some(self.model.token_bos())
        }
    }
}

pub async fn prefetch(runtime: &RuntimeManager) -> Result<()> {
    download_model_files(runtime).await?;
    Ok(())
}

async fn download_model_files(runtime: &RuntimeManager) -> Result<ModelFiles> {
    let artifacts = runtime.downloads();
    let (model, mmproj) = tokio::try_join!(
        artifacts.huggingface_model(HF_REPO, MODEL_FILENAME),
        artifacts.huggingface_model(HF_REPO, MMPROJ_FILENAME),
    )?;

    Ok(ModelFiles { model, mmproj })
}

fn resolve_local_model_files(dir: &Path) -> Result<ModelFiles> {
    let preferred_model = dir.join(MODEL_FILENAME);
    let preferred_mmproj = dir.join(MMPROJ_FILENAME);
    if preferred_model.exists() && preferred_mmproj.exists() {
        return Ok(ModelFiles {
            model: preferred_model,
            mmproj: preferred_mmproj,
        });
    }

    let mut model = None;
    let mut mmproj = None;
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("unable to read PaddleOCR-VL model dir `{}`", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf"))
        {
            continue;
        }

        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if filename.contains("mmproj") {
            if mmproj.is_none() {
                mmproj = Some(path);
            }
        } else if model.is_none() {
            model = Some(path);
        }
    }

    Ok(ModelFiles {
        model: model.with_context(|| {
            format!(
                "missing `{MODEL_FILENAME}` (or any non-mmproj GGUF) in `{}`",
                dir.display()
            )
        })?,
        mmproj: mmproj.with_context(|| {
            format!(
                "missing `{MMPROJ_FILENAME}` (or any mmproj GGUF) in `{}`",
                dir.display()
            )
        })?,
    })
}

fn model_params(cpu: bool, backend: &LlamaBackend) -> LlamaModelParams {
    if !cpu && backend.supports_gpu_offload() {
        LlamaModelParams::default().with_n_gpu_layers(DEFAULT_GPU_LAYERS)
    } else {
        LlamaModelParams::default()
    }
}

fn context_params(
    mtmd: &MtmdContext,
    prompt_positions: usize,
    prompt_total_tokens: usize,
    batch_tokens: usize,
    max_tokens: usize,
) -> Result<LlamaContextParams> {
    let required_ctx = prompt_positions
        .saturating_add(max_tokens)
        .saturating_add(1)
        .max(
            prompt_total_tokens
                .saturating_add(max_tokens)
                .saturating_add(1),
        )
        .max(batch_tokens.saturating_add(1));
    let n_ctx = NonZeroU32::new(u32::try_from(required_ctx).context("context size exceeds u32")?)
        .expect("required context size is non-zero");
    let n_batch = u32::try_from(batch_tokens.max(1)).context("batch size exceeds u32")?;
    let n_ubatch = if mtmd.decode_use_non_causal() {
        n_batch
    } else {
        n_batch.min(MAX_UBATCH)
    };

    let mut params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_n_batch(n_batch)
        .with_n_ubatch(n_ubatch);
    if mtmd.decode_use_non_causal() {
        params = params.with_attention_type(LlamaAttentionType::NonCausal);
    }
    Ok(params)
}

fn bitmap_from_image(image: &DynamicImage) -> Result<MtmdBitmap> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    MtmdBitmap::from_image_data(width, height, &rgb.into_raw())
        .context("failed to create MTMD bitmap from image")
}

fn build_user_message_content(task: PaddleOcrVlTask) -> String {
    format!("{DEFAULT_MEDIA_MARKER}{}", task.prompt())
}

fn render_chat_prompt(
    chat_template: &str,
    bos_token: &str,
    eos_token: &str,
    task: PaddleOcrVlTask,
) -> Result<String> {
    let env = jinja::environment();
    let tmpl = env
        .template_from_str(chat_template)
        .map_err(anyhow::Error::msg)
        .context("failed to parse embedded PaddleOCR-VL chat template")?;
    tmpl.render(context! {
        messages => vec![PromptMessage {
            role: "user",
            content: build_user_message_content(task),
        }],
        bos_token => bos_token,
        eos_token => eos_token,
        add_generation_prompt => true,
    })
    .map_err(anyhow::Error::msg)
    .context("failed to evaluate embedded PaddleOCR-VL chat template")
}

fn max_chunk_tokens(chunks: &crate::safe::mtmd::MtmdInputChunks) -> usize {
    (0..chunks.len())
        .filter_map(|index| chunks.get(index))
        .map(|chunk| chunk.n_tokens())
        .max()
        .unwrap_or(0)
}

fn image_chunk_tokens(chunks: &crate::safe::mtmd::MtmdInputChunks) -> usize {
    (0..chunks.len())
        .filter_map(|index| chunks.get(index))
        .filter(|chunk| chunk.chunk_type() == MtmdInputChunkType::Image)
        .map(|chunk| chunk.n_tokens())
        .sum()
}

fn decode_token(
    model: &LlamaModel,
    token: LlamaToken,
    decoder: &mut encoding_rs::Decoder,
) -> Result<String> {
    model
        .token_to_piece(token, decoder, true, None)
        .context("failed to decode generated OCR token")
}

fn token_text(model: &LlamaModel, token: LlamaToken) -> String {
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    match model.token_to_piece(token, &mut decoder, true, None) {
        Ok(piece) if !piece.is_empty() => piece,
        _ => token.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_MEDIA_MARKER, PaddleOcrVlTask, build_user_message_content, render_chat_prompt,
    };

    #[test]
    fn user_message_places_media_marker_before_task() {
        assert_eq!(
            build_user_message_content(PaddleOcrVlTask::Ocr),
            format!("{DEFAULT_MEDIA_MARKER}OCR:")
        );
    }

    #[test]
    fn user_message_contents_match_paddleocr_tasks() {
        assert_eq!(
            build_user_message_content(PaddleOcrVlTask::Table),
            "<__media__>Table Recognition:"
        );
        assert_eq!(
            build_user_message_content(PaddleOcrVlTask::Formula),
            "<__media__>Formula Recognition:"
        );
        assert_eq!(
            build_user_message_content(PaddleOcrVlTask::Chart),
            "<__media__>Chart Recognition:"
        );
        assert_eq!(
            build_user_message_content(PaddleOcrVlTask::Spotting),
            "<__media__>Spotting:"
        );
        assert_eq!(
            build_user_message_content(PaddleOcrVlTask::Seal),
            "<__media__>Seal Recognition:"
        );
    }

    #[test]
    fn embedded_template_render_keeps_media_marker_in_user_turn() -> anyhow::Result<()> {
        let rendered = render_chat_prompt(
            "{{ bos_token }}{% for message in messages %}User: {{ message['content'] }}\n{% endfor %}{% if add_generation_prompt %}Assistant:\n{% endif %}",
            "<|begin_of_sentence|>",
            "</s>",
            PaddleOcrVlTask::Formula,
        )?;
        assert_eq!(
            rendered,
            "<|begin_of_sentence|>User: <__media__>Formula Recognition:\nAssistant:\n"
        );
        Ok(())
    }
}
