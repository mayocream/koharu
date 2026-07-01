use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};

const DEFAULT_DEVICE_AUTH_INTERVAL_SECONDS: u64 = 5;

#[derive(Serialize)]
pub(super) struct UserCodeRequest<'a> {
    pub(super) client_id: &'a str,
}

#[derive(Deserialize)]
pub(super) struct UserCodeResponse {
    pub(super) device_auth_id: String,
    #[serde(alias = "usercode")]
    pub(super) user_code: String,
    #[serde(
        default = "default_interval",
        deserialize_with = "deserialize_interval"
    )]
    pub(super) interval: u64,
}

#[derive(Serialize)]
pub(super) struct TokenPollRequest<'a> {
    pub(super) device_auth_id: &'a str,
    pub(super) user_code: &'a str,
}

#[derive(Deserialize)]
pub(super) struct TokenPollSuccessResponse {
    pub(super) authorization_code: String,
    pub(super) code_challenge: String,
    pub(super) code_verifier: String,
}

#[derive(Serialize)]
pub(super) struct TokenExchangeRequest<'a> {
    pub(super) grant_type: &'a str,
    pub(super) code: &'a str,
    pub(super) redirect_uri: String,
    pub(super) client_id: &'a str,
    pub(super) code_verifier: &'a str,
}

#[derive(Serialize)]
pub(super) struct TokenRefreshRequest<'a> {
    pub(super) client_id: &'a str,
    pub(super) grant_type: &'a str,
    pub(super) refresh_token: &'a str,
}

#[derive(Deserialize)]
pub(super) struct TokenExchangeResponse {
    pub(super) id_token: Option<String>,
    pub(super) access_token: String,
    pub(super) refresh_token: Option<String>,
    pub(super) token_type: Option<String>,
    pub(super) expires_in: Option<u64>,
    pub(super) scope: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct TokenRefreshResponse {
    pub(super) id_token: Option<String>,
    pub(super) access_token: Option<String>,
    pub(super) refresh_token: Option<String>,
    pub(super) token_type: Option<String>,
    pub(super) expires_in: Option<u64>,
    pub(super) scope: Option<String>,
}

fn default_interval() -> u64 {
    DEFAULT_DEVICE_AUTH_INTERVAL_SECONDS
}

fn deserialize_interval<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Interval {
        String(String),
        Number(u64),
    }

    match Interval::deserialize(deserializer)? {
        Interval::String(value) => value.trim().parse().map_err(de::Error::custom),
        Interval::Number(value) => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_device_code_interval_from_string_or_number() {
        let from_string: UserCodeResponse = serde_json::from_value(serde_json::json!({
            "device_auth_id": "device",
            "user_code": "ABCD-1234",
            "interval": "7"
        }))
        .unwrap();
        assert_eq!(from_string.interval, 7);

        let from_number: UserCodeResponse = serde_json::from_value(serde_json::json!({
            "device_auth_id": "device",
            "usercode": "ABCD-1234",
            "interval": 3
        }))
        .unwrap();
        assert_eq!(from_number.interval, 3);
        assert_eq!(from_number.user_code, "ABCD-1234");
    }
}
