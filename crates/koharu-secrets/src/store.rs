use std::sync::Once;

use keyring::Entry;

static INIT_CREDENTIAL_STORE: Once = Once::new();

/// Service-scoped access to Koharu's platform-backed string secret storage.
#[derive(Debug, Clone)]
pub struct SecretStore {
    service: String,
}

impl SecretStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// Load a secret by key, returning `None` when no credential exists.
    pub fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        get_secret(&self.service, key)
    }

    /// Store a secret by key. Use `delete` to clear an existing credential.
    pub fn set(&self, key: &str, secret: &str) -> anyhow::Result<()> {
        set_secret(&self.service, key, secret)
    }

    /// Clear a secret by key. Missing credentials are treated as success.
    pub fn delete(&self, key: &str) -> anyhow::Result<()> {
        delete_secret(&self.service, key)
    }
}

pub fn get_secret(service: &str, key: &str) -> anyhow::Result<Option<String>> {
    let entry = secret_entry(service, key)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn set_secret(service: &str, key: &str, secret: &str) -> anyhow::Result<()> {
    secret_entry(service, key)?.set_password(secret)?;
    Ok(())
}

pub fn delete_secret(service: &str, key: &str) -> anyhow::Result<()> {
    match secret_entry(service, key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn secret_entry(service: &str, key: &str) -> anyhow::Result<Entry> {
    INIT_CREDENTIAL_STORE.call_once(crate::platform::configure);
    Ok(Entry::new(service, key)?)
}
