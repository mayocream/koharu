//! utilities for working with session files

use crate::safe::context::LlamaContext;
use crate::safe::token::LlamaToken;
use std::ffi::{CString, NulError};
use std::path::{Path, PathBuf};

/// Failed to save a sequence state file
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum SaveSeqStateError {
    /// llama.cpp failed to save the sequence state file
    #[error("Failed to save sequence state file")]
    FailedToSave,

    /// null byte in string
    #[error("null byte in string {0}")]
    NullError(#[from] NulError),

    /// failed to convert path to str
    #[error("failed to convert path {0} to str")]
    PathToStrError(PathBuf),
}

/// Failed to load a sequence state file
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum LoadSeqStateError {
    /// llama.cpp failed to load the sequence state file
    #[error("Failed to load sequence state file")]
    FailedToLoad,

    /// null byte in string
    #[error("null byte in string {0}")]
    NullError(#[from] NulError),

    /// failed to convert path to str
    #[error("failed to convert path {0} to str")]
    PathToStrError(PathBuf),

    /// Insufficient max length
    #[error("max_length is not large enough to hold {n_out} (was {max_tokens})")]
    InsufficientMaxLength {
        /// The length of the loaded sequence
        n_out: usize,
        /// The maximum length
        max_tokens: usize,
    },
}

/// Failed to save a Session file
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum SaveSessionError {
    /// llama.cpp failed to save the session file
    #[error("Failed to save session file")]
    FailedToSave,

    /// null byte in string
    #[error("null byte in string {0}")]
    NullError(#[from] NulError),

    /// failed to convert path to str
    #[error("failed to convert path {0} to str")]
    PathToStrError(PathBuf),
}

/// Failed to load a Session file
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum LoadSessionError {
    /// llama.cpp failed to load the session file
    #[error("Failed to load session file")]
    FailedToLoad,

    /// null byte in string
    #[error("null byte in string {0}")]
    NullError(#[from] NulError),

    /// failed to convert path to str
    #[error("failed to convert path {0} to str")]
    PathToStrError(PathBuf),

    /// Insufficient max length
    #[error("max_length is not large enough to hold {n_out} (was {max_tokens})")]
    InsufficientMaxLength {
        /// The length of the session file
        n_out: usize,
        /// The maximum length
        max_tokens: usize,
    },
}

impl LlamaContext<'_> {
    /// Save the current session to a file.
    ///
    /// # Parameters
    ///
    /// * `path_session` - The file to save to.
    /// * `tokens` - The tokens to associate the session with. This should be a prefix of a sequence of tokens that the context has processed, so that the relevant KV caches are already filled.
    ///
    /// # Errors
    ///
    /// Fails if the path is not a valid utf8, is not a valid c string, or llama.cpp fails to save the session file.
    #[deprecated(since = "0.1.136", note = "Use `state_save_file` instead")]
    pub fn save_session_file(
        &self,
        path_session: impl AsRef<Path>,
        tokens: &[LlamaToken],
    ) -> Result<(), SaveSessionError> {
        let path = path_session.as_ref();
        let path = path
            .to_str()
            .ok_or_else(|| SaveSessionError::PathToStrError(path.to_path_buf()))?;

        let cstr = CString::new(path)?;

        if unsafe {
            crate::sys::llama_save_session_file(
                self.context.as_ptr(),
                cstr.as_ptr(),
                tokens.as_ptr().cast::<crate::sys::llama_token>(),
                tokens.len(),
            )
        } {
            Ok(())
        } else {
            Err(SaveSessionError::FailedToSave)
        }
    }
    /// Load a session file into the current context.
    ///
    /// You still need to pass the returned tokens to the context for inference to work. What this function buys you is that the KV caches are already filled with the relevant data.
    ///
    /// # Parameters
    ///
    /// * `path_session` - The file to load from. It must be a session file from a compatible context, otherwise the function will error.
    /// * `max_tokens` - The maximum token length of the loaded session. If the session was saved with a longer length, the function will error.
    ///
    /// # Errors
    ///
    /// Fails if the path is not a valid utf8, is not a valid c string, or llama.cpp fails to load the session file. (e.g. the file does not exist, is not a session file, etc.)
    #[deprecated(since = "0.1.136", note = "Use `state_load_file` instead")]
    pub fn load_session_file(
        &mut self,
        path_session: impl AsRef<Path>,
        max_tokens: usize,
    ) -> Result<Vec<LlamaToken>, LoadSessionError> {
        let path = path_session.as_ref();
        let path = path
            .to_str()
            .ok_or(LoadSessionError::PathToStrError(path.to_path_buf()))?;

        let cstr = CString::new(path)?;
        let mut tokens: Vec<LlamaToken> = Vec::with_capacity(max_tokens);
        let mut n_out = 0;

        // SAFETY: cast is valid as LlamaToken is repr(transparent)
        let tokens_out = tokens.as_mut_ptr().cast::<crate::sys::llama_token>();

        let load_session_success = unsafe {
            crate::sys::llama_load_session_file(
                self.context.as_ptr(),
                cstr.as_ptr(),
                tokens_out,
                max_tokens,
                &mut n_out,
            )
        };
        if load_session_success {
            if n_out > max_tokens {
                return Err(LoadSessionError::InsufficientMaxLength { n_out, max_tokens });
            }
            // SAFETY: we checked that n_out <= max_tokens and llama.cpp promises that n_out tokens will be written
            unsafe {
                tokens.set_len(n_out);
            }
            Ok(tokens)
        } else {
            Err(LoadSessionError::FailedToLoad)
        }
    }

    /// Save the full state to a file.
    ///
    /// This is the non-deprecated replacement for [`save_session_file`](Self::save_session_file).
    ///
    /// # Parameters
    ///
    /// * `path_session` - The file to save to.
    /// * `tokens` - The tokens to associate the state with. This should be a prefix of a sequence
    ///   of tokens that the context has processed, so that the relevant KV caches are already filled.
    ///
    /// # Errors
    ///
    /// Fails if the path is not a valid utf8, is not a valid c string, or llama.cpp fails to save
    /// the state file.
    pub fn state_save_file(
        &self,
        path_session: impl AsRef<Path>,
        tokens: &[LlamaToken],
    ) -> Result<(), SaveSessionError> {
        let path = path_session.as_ref();
        let path = path
            .to_str()
            .ok_or_else(|| SaveSessionError::PathToStrError(path.to_path_buf()))?;

        let cstr = CString::new(path)?;

        if unsafe {
            crate::sys::llama_state_save_file(
                self.context.as_ptr(),
                cstr.as_ptr(),
                tokens.as_ptr().cast::<crate::sys::llama_token>(),
                tokens.len(),
            )
        } {
            Ok(())
        } else {
            Err(SaveSessionError::FailedToSave)
        }
    }

    /// Load a state file into the current context.
    ///
    /// This is the non-deprecated replacement for [`load_session_file`](Self::load_session_file).
    ///
    /// You still need to pass the returned tokens to the context for inference to work. What this
    /// function buys you is that the KV caches are already filled with the relevant data.
    ///
    /// # Parameters
    ///
    /// * `path_session` - The file to load from. It must be a state file from a compatible context,
    ///   otherwise the function will error.
    /// * `max_tokens` - The maximum token length of the loaded state. If the state was saved with a
    ///   longer length, the function will error.
    ///
    /// # Errors
    ///
    /// Fails if the path is not a valid utf8, is not a valid c string, or llama.cpp fails to load
    /// the state file.
    pub fn state_load_file(
        &mut self,
        path_session: impl AsRef<Path>,
        max_tokens: usize,
    ) -> Result<Vec<LlamaToken>, LoadSessionError> {
        let path = path_session.as_ref();
        let path = path
            .to_str()
            .ok_or(LoadSessionError::PathToStrError(path.to_path_buf()))?;

        let cstr = CString::new(path)?;
        let mut tokens: Vec<LlamaToken> = Vec::with_capacity(max_tokens);
        let mut n_out = 0;

        // SAFETY: cast is valid as LlamaToken is repr(transparent)
        let tokens_out = tokens.as_mut_ptr().cast::<crate::sys::llama_token>();

        let success = unsafe {
            crate::sys::llama_state_load_file(
                self.context.as_ptr(),
                cstr.as_ptr(),
                tokens_out,
                max_tokens,
                &mut n_out,
            )
        };
        if success {
            if n_out > max_tokens {
                return Err(LoadSessionError::InsufficientMaxLength { n_out, max_tokens });
            }
            // SAFETY: we checked that n_out <= max_tokens and llama.cpp promises that n_out tokens will be written
            unsafe {
                tokens.set_len(n_out);
            }
            Ok(tokens)
        } else {
            Err(LoadSessionError::FailedToLoad)
        }
    }

    /// Save state for a single sequence to a file.
    ///
    /// This enables saving state for individual sequences, which is useful for multi-sequence
    /// inference scenarios.
    ///
    /// # Parameters
    ///
    /// * `filepath` - The file to save to.
    /// * `seq_id` - The sequence ID whose state to save.
    /// * `tokens` - The tokens to associate with the saved state.
    ///
    /// # Errors
    ///
    /// Fails if the path is not a valid utf8, is not a valid c string, or llama.cpp fails to save
    /// the sequence state file.
    ///
    /// # Returns
    ///
    /// The number of bytes written on success.
    pub fn state_seq_save_file(
        &self,
        filepath: impl AsRef<Path>,
        seq_id: i32,
        tokens: &[LlamaToken],
    ) -> Result<usize, SaveSeqStateError> {
        let path = filepath.as_ref();
        let path = path
            .to_str()
            .ok_or_else(|| SaveSeqStateError::PathToStrError(path.to_path_buf()))?;

        let cstr = CString::new(path)?;

        let bytes_written = unsafe {
            crate::sys::llama_state_seq_save_file(
                self.context.as_ptr(),
                cstr.as_ptr(),
                seq_id,
                tokens.as_ptr().cast::<crate::sys::llama_token>(),
                tokens.len(),
            )
        };

        if bytes_written == 0 {
            Err(SaveSeqStateError::FailedToSave)
        } else {
            Ok(bytes_written)
        }
    }

    /// Load state for a single sequence from a file.
    ///
    /// This enables loading state for individual sequences, which is useful for multi-sequence
    /// inference scenarios.
    ///
    /// # Parameters
    ///
    /// * `filepath` - The file to load from.
    /// * `dest_seq_id` - The destination sequence ID to load the state into.
    /// * `max_tokens` - The maximum number of tokens to read.
    ///
    /// # Errors
    ///
    /// Fails if the path is not a valid utf8, is not a valid c string, or llama.cpp fails to load
    /// the sequence state file.
    ///
    /// # Returns
    ///
    /// A tuple of `(tokens, bytes_read)` on success.
    pub fn state_seq_load_file(
        &mut self,
        filepath: impl AsRef<Path>,
        dest_seq_id: i32,
        max_tokens: usize,
    ) -> Result<(Vec<LlamaToken>, usize), LoadSeqStateError> {
        let path = filepath.as_ref();
        let path = path
            .to_str()
            .ok_or(LoadSeqStateError::PathToStrError(path.to_path_buf()))?;

        let cstr = CString::new(path)?;
        let mut tokens: Vec<LlamaToken> = Vec::with_capacity(max_tokens);
        let mut n_out = 0;

        // SAFETY: cast is valid as LlamaToken is repr(transparent)
        let tokens_out = tokens.as_mut_ptr().cast::<crate::sys::llama_token>();

        let bytes_read = unsafe {
            crate::sys::llama_state_seq_load_file(
                self.context.as_ptr(),
                cstr.as_ptr(),
                dest_seq_id,
                tokens_out,
                max_tokens,
                &mut n_out,
            )
        };

        if bytes_read == 0 {
            return Err(LoadSeqStateError::FailedToLoad);
        }

        if n_out > max_tokens {
            return Err(LoadSeqStateError::InsufficientMaxLength { n_out, max_tokens });
        }

        // SAFETY: we checked that n_out <= max_tokens and llama.cpp promises that n_out tokens will be written
        unsafe {
            tokens.set_len(n_out);
        }

        Ok((tokens, bytes_read))
    }

    /// Returns the maximum size in bytes of the state (rng, logits, embedding
    /// and `kv_cache`) - will often be smaller after compacting tokens
    #[must_use]
    pub fn get_state_size(&self) -> usize {
        unsafe { crate::sys::llama_get_state_size(self.context.as_ptr()) }
    }

    /// Copies the state to the specified destination address.
    ///
    /// Returns the number of bytes copied
    ///
    /// # Safety
    ///
    /// Destination needs to have allocated enough memory.
    pub unsafe fn copy_state_data(&self, dest: *mut u8) -> usize {
        unsafe { crate::sys::llama_copy_state_data(self.context.as_ptr(), dest) }
    }

    /// Set the state reading from the specified address
    /// Returns the number of bytes read
    ///
    /// # Safety
    ///
    /// help wanted: not entirely sure what the safety requirements are here.
    pub unsafe fn set_state_data(&mut self, src: &[u8]) -> usize {
        unsafe { crate::sys::llama_set_state_data(self.context.as_ptr(), src.as_ptr()) }
    }
}
