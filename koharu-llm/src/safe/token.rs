//! Safe wrappers around `llama_token_data` and `llama_token_data_array`.

use std::fmt::Debug;
use std::fmt::Display;

#[path = "token/data.rs"]
pub mod data;
#[path = "token/data_array.rs"]
pub mod data_array;
#[path = "token/logit_bias.rs"]
pub mod logit_bias;

/// A safe wrapper for `llama_token`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[allow(clippy::module_name_repetitions)]
pub struct LlamaToken(pub crate::sys::llama_token);

impl Display for LlamaToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl LlamaToken {
    /// Create a new `LlamaToken` from a i32.
    ///
    /// ```
    /// # use llama_cpp_2::token::LlamaToken;
    /// let token = LlamaToken::new(0);
    /// assert_eq!(token, LlamaToken(0));
    /// ```
    #[must_use]
    pub fn new(token_id: i32) -> Self {
        Self(token_id)
    }
}
