//! Safe wrapper around `llama_context`.

use std::fmt::{Debug, Formatter};
use std::num::NonZeroI32;
use std::ptr::NonNull;
use std::slice;

use crate::safe::llama_batch::LlamaBatch;
use crate::safe::model::{LlamaLoraAdapter, LlamaModel};
use crate::safe::timing::LlamaTimings;
use crate::safe::token::LlamaToken;
use crate::safe::token::data::LlamaTokenData;
use crate::safe::token::data_array::LlamaTokenDataArray;
use crate::safe::{
    DecodeError, EmbeddingsError, EncodeError, LlamaLoraAdapterRemoveError,
    LlamaLoraAdapterSetError,
};

#[path = "context/kv_cache.rs"]
pub mod kv_cache;
#[path = "context/params.rs"]
pub mod params;
#[path = "context/session.rs"]
pub mod session;

/// Safe wrapper around `llama_context`.
#[allow(clippy::module_name_repetitions)]
pub struct LlamaContext<'a> {
    pub(crate) context: NonNull<crate::sys::llama_context>,
    /// a reference to the contexts model.
    pub model: &'a LlamaModel,
    initialized_logits: Vec<i32>,
    embeddings_enabled: bool,
}

impl Debug for LlamaContext<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlamaContext")
            .field("context", &self.context)
            .finish()
    }
}

impl<'model> LlamaContext<'model> {
    pub(crate) fn new(
        llama_model: &'model LlamaModel,
        llama_context: NonNull<crate::sys::llama_context>,
        embeddings_enabled: bool,
    ) -> Self {
        Self {
            context: llama_context,
            model: llama_model,
            initialized_logits: Vec::new(),
            embeddings_enabled,
        }
    }

    /// Gets the max number of logical tokens that can be submitted to decode. Must be greater than or equal to [`Self::n_ubatch`].
    #[must_use]
    pub fn n_batch(&self) -> u32 {
        unsafe { crate::sys::llama_n_batch(self.context.as_ptr()) }
    }

    /// Gets the max number of physical tokens (hardware level) to decode in batch. Must be less than or equal to [`Self::n_batch`].
    #[must_use]
    pub fn n_ubatch(&self) -> u32 {
        unsafe { crate::sys::llama_n_ubatch(self.context.as_ptr()) }
    }

    /// Gets the size of the context.
    #[must_use]
    pub fn n_ctx(&self) -> u32 {
        unsafe { crate::sys::llama_n_ctx(self.context.as_ptr()) }
    }

    /// Decodes the batch.
    ///
    /// # Errors
    ///
    /// - `DecodeError` if the decoding failed.
    ///
    /// # Panics
    ///
    /// - the returned [`std::ffi::c_int`] from llama-cpp does not fit into a i32 (this should never happen on most systems)
    pub fn decode(&mut self, batch: &mut LlamaBatch) -> Result<(), DecodeError> {
        let result = unsafe { crate::sys::llama_decode(self.context.as_ptr(), batch.llama_batch) };

        match NonZeroI32::new(result) {
            None => {
                self.initialized_logits
                    .clone_from(&batch.initialized_logits);
                Ok(())
            }
            Some(error) => Err(DecodeError::from(error)),
        }
    }

    /// Encodes the batch.
    ///
    /// # Errors
    ///
    /// - `EncodeError` if the decoding failed.
    ///
    /// # Panics
    ///
    /// - the returned [`std::ffi::c_int`] from llama-cpp does not fit into a i32 (this should never happen on most systems)
    pub fn encode(&mut self, batch: &mut LlamaBatch) -> Result<(), EncodeError> {
        let result = unsafe { crate::sys::llama_encode(self.context.as_ptr(), batch.llama_batch) };

        match NonZeroI32::new(result) {
            None => {
                self.initialized_logits
                    .clone_from(&batch.initialized_logits);
                Ok(())
            }
            Some(error) => Err(EncodeError::from(error)),
        }
    }

    /// Get the embeddings for the `i`th sequence in the current context.
    ///
    /// # Returns
    ///
    /// A slice containing the embeddings for the last decoded batch.
    /// The size corresponds to the `n_embd` parameter of the context's model.
    ///
    /// # Errors
    ///
    /// - When the current context was constructed without enabling embeddings.
    /// - If the current model had a pooling type of [`crate::sys::LLAMA_POOLING_TYPE_NONE`]
    /// - If the given sequence index exceeds the max sequence id.
    ///
    /// # Panics
    ///
    /// * `n_embd` does not fit into a usize
    pub fn embeddings_seq_ith(&self, i: i32) -> Result<&[f32], EmbeddingsError> {
        if !self.embeddings_enabled {
            return Err(EmbeddingsError::NotEnabled);
        }

        let n_embd =
            usize::try_from(self.model.n_embd()).expect("n_embd does not fit into a usize");

        unsafe {
            let embedding = crate::sys::llama_get_embeddings_seq(self.context.as_ptr(), i);

            // Technically also possible whenever `i >= max(batch.n_seq)`, but can't check that here.
            if embedding.is_null() {
                Err(EmbeddingsError::NonePoolType)
            } else {
                Ok(slice::from_raw_parts(embedding, n_embd))
            }
        }
    }

    /// Get the embeddings for the `i`th token in the current context.
    ///
    /// # Returns
    ///
    /// A slice containing the embeddings for the last decoded batch of the given token.
    /// The size corresponds to the `n_embd` parameter of the context's model.
    ///
    /// # Errors
    ///
    /// - When the current context was constructed without enabling embeddings.
    /// - When the given token didn't have logits enabled when it was passed.
    /// - If the given token index exceeds the max token id.
    ///
    /// # Panics
    ///
    /// * `n_embd` does not fit into a usize
    pub fn embeddings_ith(&self, i: i32) -> Result<&[f32], EmbeddingsError> {
        if !self.embeddings_enabled {
            return Err(EmbeddingsError::NotEnabled);
        }

        let n_embd =
            usize::try_from(self.model.n_embd()).expect("n_embd does not fit into a usize");

        unsafe {
            let embedding = crate::sys::llama_get_embeddings_ith(self.context.as_ptr(), i);
            // Technically also possible whenever `i >= batch.n_tokens`, but no good way of checking `n_tokens` here.
            if embedding.is_null() {
                Err(EmbeddingsError::LogitsNotEnabled)
            } else {
                Ok(slice::from_raw_parts(embedding, n_embd))
            }
        }
    }

    /// Get the logits for the last token in the context.
    ///
    /// # Returns
    /// An iterator over unsorted `LlamaTokenData` containing the
    /// logits for the last token in the context.
    ///
    /// # Panics
    ///
    /// - underlying logits data is null
    pub fn candidates(&self) -> impl Iterator<Item = LlamaTokenData> + '_ {
        (0_i32..).zip(self.get_logits()).map(|(i, logit)| {
            let token = LlamaToken::new(i);
            LlamaTokenData::new(token, *logit, 0_f32)
        })
    }

    /// Get the token data array for the last token in the context.
    ///
    /// This is a convience method that implements:
    /// ```ignore
    /// LlamaTokenDataArray::from_iter(ctx.candidates(), false)
    /// ```
    ///
    /// # Panics
    ///
    /// - underlying logits data is null
    #[must_use]
    pub fn token_data_array(&self) -> LlamaTokenDataArray {
        LlamaTokenDataArray::from_iter(self.candidates(), false)
    }

    /// Token logits obtained from the last call to `decode()`.
    /// The logits for which `batch.logits[i] != 0` are stored contiguously
    /// in the order they have appeared in the batch.
    /// Rows: number of tokens for which `batch.logits[i] != 0`
    /// Cols: `n_vocab`
    ///
    /// # Returns
    ///
    /// A slice containing the logits for the last decoded token.
    /// The size corresponds to the `n_vocab` parameter of the context's model.
    ///
    /// # Panics
    ///
    /// - `n_vocab` does not fit into a usize
    /// - token data returned is null
    #[must_use]
    pub fn get_logits(&self) -> &[f32] {
        let data = unsafe { crate::sys::llama_get_logits(self.context.as_ptr()) };
        assert!(!data.is_null(), "logits data for last token is null");
        let len = usize::try_from(self.model.n_vocab()).expect("n_vocab does not fit into a usize");

        unsafe { slice::from_raw_parts(data, len) }
    }

    /// Get the logits for the ith token in the context.
    ///
    /// # Panics
    ///
    /// - logit `i` is not initialized.
    pub fn candidates_ith(&self, i: i32) -> impl Iterator<Item = LlamaTokenData> + '_ {
        (0_i32..).zip(self.get_logits_ith(i)).map(|(i, logit)| {
            let token = LlamaToken::new(i);
            LlamaTokenData::new(token, *logit, 0_f32)
        })
    }

    /// Get the token data array for the ith token in the context.
    ///
    /// This is a convience method that implements:
    /// ```ignore
    /// LlamaTokenDataArray::from_iter(ctx.candidates_ith(i), false)
    /// ```
    ///
    /// # Panics
    ///
    /// - logit `i` is not initialized.
    #[must_use]
    pub fn token_data_array_ith(&self, i: i32) -> LlamaTokenDataArray {
        LlamaTokenDataArray::from_iter(self.candidates_ith(i), false)
    }

    /// Get the logits for the ith token in the context.
    ///
    /// # Panics
    ///
    /// - `i` is greater than `n_ctx`
    /// - `n_vocab` does not fit into a usize
    /// - logit `i` is not initialized.
    #[must_use]
    pub fn get_logits_ith(&self, i: i32) -> &[f32] {
        assert!(
            self.initialized_logits.contains(&i),
            "logit {i} is not initialized. only {:?} is",
            self.initialized_logits
        );
        assert!(
            self.n_ctx() > u32::try_from(i).expect("i does not fit into a u32"),
            "n_ctx ({}) must be greater than i ({})",
            self.n_ctx(),
            i
        );

        let data = unsafe { crate::sys::llama_get_logits_ith(self.context.as_ptr(), i) };
        let len = usize::try_from(self.model.n_vocab()).expect("n_vocab does not fit into a usize");

        unsafe { slice::from_raw_parts(data, len) }
    }

    /// Reset the timings for the context.
    pub fn reset_timings(&mut self) {
        unsafe { crate::sys::llama_perf_context_reset(self.context.as_ptr()) }
    }

    /// Returns the timings for the context.
    pub fn timings(&mut self) -> LlamaTimings {
        let timings = unsafe { crate::sys::llama_perf_context(self.context.as_ptr()) };
        LlamaTimings { timings }
    }

    /// Sets a lora adapter.
    ///
    /// # Errors
    ///
    /// See [`LlamaLoraAdapterSetError`] for more information.
    pub fn lora_adapter_set(
        &self,
        adapter: &mut LlamaLoraAdapter,
        scale: f32,
    ) -> Result<(), LlamaLoraAdapterSetError> {
        let mut adapters = [adapter.lora_adapter.as_ptr()];
        let mut scales = [scale];
        let err_code = unsafe {
            crate::sys::llama_set_adapters_lora(
                self.context.as_ptr(),
                adapters.as_mut_ptr(),
                1,
                scales.as_mut_ptr(),
            )
        };
        if err_code != 0 {
            return Err(LlamaLoraAdapterSetError::ErrorResult(err_code));
        }

        tracing::debug!("Set lora adapter");
        Ok(())
    }

    /// Remove all lora adapters.
    ///
    /// Note: The upstream API now replaces all adapters at once via
    /// `llama_set_adapters_lora`. This clears all adapters from the context.
    ///
    /// # Errors
    ///
    /// See [`LlamaLoraAdapterRemoveError`] for more information.
    pub fn lora_adapter_remove(
        &self,
        _adapter: &mut LlamaLoraAdapter,
    ) -> Result<(), LlamaLoraAdapterRemoveError> {
        let err_code = unsafe {
            crate::sys::llama_set_adapters_lora(
                self.context.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            )
        };
        if err_code != 0 {
            return Err(LlamaLoraAdapterRemoveError::ErrorResult(err_code));
        }

        tracing::debug!("Remove lora adapter");
        Ok(())
    }
}

impl Drop for LlamaContext<'_> {
    fn drop(&mut self) {
        unsafe { crate::sys::llama_free(self.context.as_ptr()) }
    }
}
