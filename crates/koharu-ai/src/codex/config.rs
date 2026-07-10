use std::time::Duration;

pub const DEFAULT_ISSUER_URL: &str = "https://auth.openai.com";
pub const DEFAULT_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEFAULT_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";

const DEVICE_AUTH_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
pub struct CodexConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub responses_url: String,
    pub device_auth_timeout: Duration,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            issuer_url: DEFAULT_ISSUER_URL.to_string(),
            client_id: DEFAULT_CLIENT_ID.to_string(),
            responses_url: DEFAULT_RESPONSES_URL.to_string(),
            device_auth_timeout: DEVICE_AUTH_TIMEOUT,
        }
    }
}

impl CodexConfig {
    pub(super) fn issuer(&self) -> &str {
        self.issuer_url.trim_end_matches('/')
    }

    pub(super) fn accounts_endpoint(&self, path: &str) -> String {
        format!("{}/api/accounts/{path}", self.issuer())
    }

    pub(super) fn issuer_endpoint(&self, path: &str) -> String {
        format!("{}/{path}", self.issuer())
    }

    pub(super) fn device_callback_uri(&self) -> String {
        self.issuer_endpoint("deviceauth/callback")
    }
}
