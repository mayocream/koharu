use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCode {
    pub verification_url: String,
    pub user_code: String,
    device_auth_id: String,
    interval: Duration,
}

impl DeviceCode {
    pub(super) fn new(
        verification_url: String,
        user_code: String,
        device_auth_id: String,
        interval_seconds: u64,
    ) -> Self {
        Self {
            verification_url,
            user_code,
            device_auth_id,
            interval: Duration::from_secs(interval_seconds.max(1)),
        }
    }

    pub fn device_auth_id(&self) -> &str {
        &self.device_auth_id
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAuthorization {
    pub authorization_code: String,
    pub code_verifier: String,
    pub code_challenge: String,
}
