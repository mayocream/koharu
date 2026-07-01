use koharu_runtime::SecretStore;

use super::error::{CodexError, Result};
use super::tokens::CodexTokens;

pub const DEFAULT_SECRET_SERVICE: &str = "koharu";
pub const DEFAULT_TOKEN_SECRET_KEY: &str = "codex_oauth_tokens";

const SECRET_CHUNK_UTF16_UNITS: usize = 1000;
const TOKEN_FIELDS: [TokenField; 6] = [
    TokenField::IdToken,
    TokenField::AccessToken,
    TokenField::RefreshToken,
    TokenField::TokenType,
    TokenField::ExpiresIn,
    TokenField::Scope,
];

#[derive(Debug, Clone)]
pub struct TokenStore {
    secrets: SecretStore,
    key: String,
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::new(DEFAULT_SECRET_SERVICE, DEFAULT_TOKEN_SECRET_KEY)
    }
}

impl TokenStore {
    pub fn new(service: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            secrets: SecretStore::new(service),
            key: key.into(),
        }
    }

    pub fn load(&self) -> Result<Option<CodexTokens>> {
        if let Some(tokens) = self.load_chunked()? {
            return Ok(Some(tokens));
        }

        let Some(raw) = self
            .secrets
            .get(&self.key)
            .map_err(CodexError::SecretStore)?
        else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&raw)?))
    }

    pub fn store(&self, tokens: &CodexTokens) -> Result<()> {
        let expires_in = tokens.expires_in.map(|value| value.to_string());

        self.set_field(TokenField::AccessToken, Some(&tokens.access_token))?;
        self.set_field(TokenField::IdToken, tokens.id_token.as_deref())?;
        self.set_field(TokenField::RefreshToken, tokens.refresh_token.as_deref())?;
        self.set_field(TokenField::TokenType, tokens.token_type.as_deref())?;
        self.set_field(TokenField::ExpiresIn, expires_in.as_deref())?;
        self.set_field(TokenField::Scope, tokens.scope.as_deref())?;

        self.secrets
            .delete(&self.key)
            .map_err(CodexError::SecretStore)
    }

    pub fn delete(&self) -> Result<()> {
        self.secrets
            .delete(&self.key)
            .map_err(CodexError::SecretStore)?;
        for field in TOKEN_FIELDS {
            self.delete_field(field)?;
        }
        Ok(())
    }

    fn load_chunked(&self) -> Result<Option<CodexTokens>> {
        let Some(access_token) = self.get_field(TokenField::AccessToken)? else {
            return Ok(None);
        };

        let expires_in = match self.get_field(TokenField::ExpiresIn)? {
            Some(value) => Some(value.parse::<u64>().map_err(|err| {
                CodexError::InvalidStoredToken(format!("expires_in is not a u64: {err}"))
            })?),
            None => None,
        };

        Ok(Some(CodexTokens {
            id_token: self.get_field(TokenField::IdToken)?,
            access_token,
            refresh_token: self.get_field(TokenField::RefreshToken)?,
            token_type: self.get_field(TokenField::TokenType)?,
            expires_in,
            scope: self.get_field(TokenField::Scope)?,
        }))
    }

    fn get_field(&self, field: TokenField) -> Result<Option<String>> {
        let chunks_key = self.field_chunks_key(field);
        let Some(chunk_count) = self
            .secrets
            .get(&chunks_key)
            .map_err(CodexError::SecretStore)?
        else {
            return Ok(None);
        };
        let field_name = field.name();
        let chunk_count = chunk_count.parse::<usize>().map_err(|err| {
            CodexError::InvalidStoredToken(format!("{field_name} chunk count is invalid: {err}"))
        })?;

        let mut value = String::new();
        for index in 0..chunk_count {
            let chunk_key = self.field_chunk_key(field, index);
            let chunk = self
                .secrets
                .get(&chunk_key)
                .map_err(CodexError::SecretStore)?
                .ok_or_else(|| {
                    CodexError::InvalidStoredToken(format!(
                        "{field_name} is missing chunk {index} of {chunk_count}"
                    ))
                })?;
            value.push_str(&chunk);
        }

        Ok(Some(value))
    }

    fn set_field(&self, field: TokenField, value: Option<&str>) -> Result<()> {
        self.delete_field(field)?;

        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(());
        };

        let chunks = split_secret_chunks(value);
        for (index, chunk) in chunks.iter().enumerate() {
            let chunk_key = self.field_chunk_key(field, index);
            self.secrets
                .set(&chunk_key, chunk)
                .map_err(CodexError::SecretStore)?;
        }
        let chunks_key = self.field_chunks_key(field);
        self.secrets
            .set(&chunks_key, &chunks.len().to_string())
            .map_err(CodexError::SecretStore)
    }

    fn delete_field(&self, field: TokenField) -> Result<()> {
        let chunks_key = self.field_chunks_key(field);
        let chunk_count = self
            .secrets
            .get(&chunks_key)
            .map_err(CodexError::SecretStore)?
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();

        self.secrets
            .delete(&chunks_key)
            .map_err(CodexError::SecretStore)?;
        for index in 0..chunk_count {
            let chunk_key = self.field_chunk_key(field, index);
            self.secrets
                .delete(&chunk_key)
                .map_err(CodexError::SecretStore)?;
        }
        Ok(())
    }

    fn field_chunks_key(&self, field: TokenField) -> String {
        format!("{}_{}_chunks", self.key, field.name())
    }

    fn field_chunk_key(&self, field: TokenField, index: usize) -> String {
        format!("{}_{}_{}", self.key, field.name(), index)
    }
}

#[derive(Debug, Clone, Copy)]
enum TokenField {
    IdToken,
    AccessToken,
    RefreshToken,
    TokenType,
    ExpiresIn,
    Scope,
}

impl TokenField {
    fn name(self) -> &'static str {
        match self {
            Self::IdToken => "id_token",
            Self::AccessToken => "access_token",
            Self::RefreshToken => "refresh_token",
            Self::TokenType => "token_type",
            Self::ExpiresIn => "expires_in",
            Self::Scope => "scope",
        }
    }
}

fn split_secret_chunks(value: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut chunk_start = 0;
    let mut chunk_units = 0;

    for (index, ch) in value.char_indices() {
        let units = ch.len_utf16();
        if chunk_units + units > SECRET_CHUNK_UTF16_UNITS && index > chunk_start {
            chunks.push(&value[chunk_start..index]);
            chunk_start = index;
            chunk_units = 0;
        }

        chunk_units += units;
    }

    if chunk_start < value.len() {
        chunks.push(&value[chunk_start..]);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_stay_under_utf16_limit_and_reassemble() {
        let value = "a".repeat(SECRET_CHUNK_UTF16_UNITS * 3 + 27);

        let chunks = split_secret_chunks(&value);

        assert!(chunks.len() > 1);
        assert_eq!(chunks.concat(), value);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.encode_utf16().count() <= SECRET_CHUNK_UTF16_UNITS)
        );
    }

    #[test]
    fn chunks_do_not_split_multibyte_chars() {
        let value: String =
            std::iter::repeat_n('\u{1F600}', SECRET_CHUNK_UTF16_UNITS + 1).collect();

        let chunks = split_secret_chunks(&value);

        assert!(chunks.len() > 1);
        assert_eq!(chunks.concat(), value);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.encode_utf16().count() <= SECRET_CHUNK_UTF16_UNITS)
        );
    }
}
