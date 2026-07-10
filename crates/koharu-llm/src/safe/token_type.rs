//! Utilities for working with `llama_token_type` values.
use enumflags2::{BitFlags, bitflags};
use std::ops::{Deref, DerefMut};

/// A rust flavored equivalent of `llama_token_type`.
#[derive(Eq, PartialEq, Debug, Clone, Copy)]
#[bitflags]
#[repr(u32)]
#[allow(clippy::module_name_repetitions, missing_docs)]
pub enum LlamaTokenAttr {
    Unknown = crate::sys::LLAMA_TOKEN_ATTR_UNKNOWN as _,
    Unused = crate::sys::LLAMA_TOKEN_ATTR_UNUSED as _,
    Normal = crate::sys::LLAMA_TOKEN_ATTR_NORMAL as _,
    Control = crate::sys::LLAMA_TOKEN_ATTR_CONTROL as _,
    UserDefined = crate::sys::LLAMA_TOKEN_ATTR_USER_DEFINED as _,
    Byte = crate::sys::LLAMA_TOKEN_ATTR_BYTE as _,
    Normalized = crate::sys::LLAMA_TOKEN_ATTR_NORMALIZED as _,
    LStrip = crate::sys::LLAMA_TOKEN_ATTR_LSTRIP as _,
    RStrip = crate::sys::LLAMA_TOKEN_ATTR_RSTRIP as _,
    SingleWord = crate::sys::LLAMA_TOKEN_ATTR_SINGLE_WORD as _,
}

/// A set of `LlamaTokenAttrs`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LlamaTokenAttrs(pub BitFlags<LlamaTokenAttr>);

impl Deref for LlamaTokenAttrs {
    type Target = BitFlags<LlamaTokenAttr>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LlamaTokenAttrs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TryFrom<crate::sys::llama_token_type> for LlamaTokenAttrs {
    type Error = LlamaTokenTypeFromIntError;

    fn try_from(value: crate::sys::llama_vocab_type) -> Result<Self, Self::Error> {
        Ok(Self(BitFlags::from_bits(value as _).map_err(|e| {
            LlamaTokenTypeFromIntError::UnknownValue(e.invalid_bits())
        })?))
    }
}

/// An error type for `LlamaTokenType::try_from`.
#[derive(thiserror::Error, Debug, Eq, PartialEq)]
pub enum LlamaTokenTypeFromIntError {
    /// The value is not a valid `llama_token_type`.
    #[error("Unknown Value {0}")]
    UnknownValue(std::ffi::c_uint),
}
