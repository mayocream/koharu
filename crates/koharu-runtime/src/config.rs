use anyhow::Result;
use serde::{Deserialize, Serialize};

const HTTP_SECTION: &str = "http";

/// HTTP settings shared by runtime package and model downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpConfig {
    #[serde(rename = "connect_timeout")]
    pub connect_timeout_secs: u64,
    #[serde(rename = "read_timeout")]
    pub read_timeout_secs: u64,
    pub max_retries: u32,
}

impl HttpConfig {
    pub fn load() -> Result<koharu_config::Config<Self>> {
        koharu_config::load(HTTP_SECTION)
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            connect_timeout_secs: 20,
            read_timeout_secs: 300,
            max_retries: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_defaults_are_stable() {
        let config = HttpConfig::default();
        assert_eq!(config.connect_timeout_secs, 20);
        assert_eq!(config.read_timeout_secs, 300);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn http_field_names_match_the_shared_toml_section() {
        let value = toml::Value::try_from(HttpConfig::default()).unwrap();
        let table = value.as_table().unwrap();
        assert!(table.contains_key("connect_timeout"));
        assert!(table.contains_key("read_timeout"));
        assert!(!table.contains_key("connect_timeout_secs"));
    }
}
