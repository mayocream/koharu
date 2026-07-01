use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use candle_core::{Device, Tensor};

use super::latents::prepare_text_ids;

pub const PROMPT_EMBED_SEQ_LEN: usize = 512;
pub const PROMPT_EMBED_DIM: usize = 7680;

const PROMPT_SAFETENSORS: &[u8] = include_bytes!("prompt.safetensors");

#[derive(Debug, Clone)]
pub struct Flux2PromptEmbeddings {
    pub prompt_embeds: Tensor,
    pub text_ids: Tensor,
}

#[derive(Debug)]
pub struct Flux2PromptEmbedder {
    device: Device,
    cache: Mutex<Option<Arc<Flux2PromptEmbeddings>>>,
}

impl Flux2PromptEmbedder {
    pub fn new(device: &Device) -> Self {
        Self {
            device: device.clone(),
            cache: Mutex::new(None),
        }
    }

    pub fn encode_prompt(&self) -> Result<Arc<Flux2PromptEmbeddings>> {
        if let Some(cached) = self.cache.lock().expect("prompt cache poisoned").as_ref() {
            return Ok(cached.clone());
        }

        let mut tensors = candle_core::safetensors::load_buffer(PROMPT_SAFETENSORS, &self.device)?;
        let prompt_embeds = tensors
            .remove("prompt_embeds")
            .context("prompt.safetensors is missing prompt_embeds")?;
        let dims = prompt_embeds.dims3()?;
        if dims != (1, PROMPT_EMBED_SEQ_LEN, PROMPT_EMBED_DIM) {
            bail!(
                "prompt_embeds shape {:?} does not match expected {:?}",
                dims,
                (1, PROMPT_EMBED_SEQ_LEN, PROMPT_EMBED_DIM)
            );
        }
        let text_ids = prepare_text_ids(1, PROMPT_EMBED_SEQ_LEN, &self.device)?;
        let embeddings = Arc::new(Flux2PromptEmbeddings {
            prompt_embeds,
            text_ids,
        });
        *self.cache.lock().expect("prompt cache poisoned") = Some(embeddings.clone());
        Ok(embeddings)
    }
}
