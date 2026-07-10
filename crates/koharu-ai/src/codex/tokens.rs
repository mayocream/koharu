use std::fmt;

use serde::{Deserialize, Serialize};

use super::jwt::chatgpt_account_id_from_jwt;
use super::requests::TokenRefreshResponse;

#[derive(Clone, Serialize, Deserialize)]
pub struct CodexTokens {
    pub id_token: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: Option<String>,
    pub expires_in: Option<u64>,
    pub scope: Option<String>,
}

impl fmt::Debug for CodexTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexTokens")
            .field("id_token", &self.id_token.as_ref().map(|_| "[REDACTED]"))
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .finish()
    }
}

impl CodexTokens {
    pub fn chatgpt_account_id(&self) -> Option<String> {
        self.id_token
            .as_deref()
            .and_then(chatgpt_account_id_from_jwt)
            .or_else(|| chatgpt_account_id_from_jwt(&self.access_token))
    }

    pub(super) fn refreshed_with(&self, refresh: TokenRefreshResponse) -> Self {
        Self {
            id_token: refresh.id_token.or_else(|| self.id_token.clone()),
            access_token: refresh
                .access_token
                .unwrap_or_else(|| self.access_token.clone()),
            refresh_token: refresh.refresh_token.or_else(|| self.refresh_token.clone()),
            token_type: refresh.token_type.or_else(|| self.token_type.clone()),
            expires_in: refresh.expires_in.or(self.expires_in),
            scope: refresh.scope.or_else(|| self.scope.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_debug_redacts_secrets() {
        let tokens = CodexTokens {
            id_token: Some("id-token".to_string()),
            access_token: "access-token".to_string(),
            refresh_token: Some("refresh-token".to_string()),
            token_type: Some("Bearer".to_string()),
            expires_in: Some(3600),
            scope: None,
        };

        let debug = format!("{tokens:?}");
        assert!(!debug.contains("id-token"));
        assert!(!debug.contains("access-token"));
        assert!(!debug.contains("refresh-token"));
        assert!(debug.contains("[REDACTED]"));
    }
}
