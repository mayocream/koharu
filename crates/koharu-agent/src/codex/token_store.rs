use anyhow::{Context as _, Result, anyhow};
use koharu_secrets::{ExposeSecret as _, SecretString};
use uuid::Uuid;

use super::auth::Tokens;

const ACTIVE_KEY: &str = "agent_codex_active";
const PREFIX: &str = "agent_codex";
const CHUNK_UTF16_UNITS: usize = 1000;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TokenStore;

impl TokenStore {
    pub(super) fn load(self) -> Result<Option<Tokens>> {
        let Some(generation) = get(ACTIVE_KEY)? else {
            return Ok(None);
        };
        let access = self
            .field(generation.expose_secret(), "access")?
            .context("stored Codex credentials are missing the access token")?;
        let refresh = self
            .field(generation.expose_secret(), "refresh")?
            .context("stored Codex credentials are missing the refresh token")?;
        let expires_at_ms = self
            .field(generation.expose_secret(), "expires_at_ms")?
            .context("stored Codex credentials are missing their expiry")?
            .parse()
            .context("stored Codex credential expiry is invalid")?;
        Ok(Some(Tokens {
            access,
            refresh,
            id: self.field(generation.expose_secret(), "id")?,
            expires_at_ms,
        }))
    }

    pub(super) fn store(self, tokens: &Tokens) -> Result<()> {
        let previous = get(ACTIVE_KEY)?;
        let generation = Uuid::new_v4().simple().to_string();
        self.set_field(&generation, "access", &tokens.access)?;
        self.set_field(&generation, "refresh", &tokens.refresh)?;
        self.set_field(
            &generation,
            "expires_at_ms",
            &tokens.expires_at_ms.to_string(),
        )?;
        if let Some(id) = tokens.id.as_deref() {
            self.set_field(&generation, "id", id)?;
        }
        koharu_secrets::set(ACTIVE_KEY, &SecretString::from(generation.clone()))?;

        if let Some(previous) = previous {
            let previous = previous.expose_secret();
            if previous != generation {
                let _ = self.delete_generation(previous);
            }
        }
        Ok(())
    }

    pub(super) fn delete(self) -> Result<()> {
        let active = get(ACTIVE_KEY)?;
        koharu_secrets::delete(ACTIVE_KEY)?;
        if let Some(active) = active {
            self.delete_generation(active.expose_secret())?;
        }
        Ok(())
    }

    fn field(self, generation: &str, field: &str) -> Result<Option<String>> {
        let count_key = count_key(generation, field);
        let Some(count) = get(&count_key)? else {
            return Ok(None);
        };
        let count = count
            .expose_secret()
            .parse::<usize>()
            .with_context(|| format!("stored Codex {field} chunk count is invalid"))?;
        let mut value = String::new();
        for index in 0..count {
            let chunk = get(&chunk_key(generation, field, index))?.ok_or_else(|| {
                anyhow!("stored Codex {field} is missing chunk {index} of {count}")
            })?;
            value.push_str(chunk.expose_secret());
        }
        Ok(Some(value))
    }

    fn set_field(self, generation: &str, field: &str, value: &str) -> Result<()> {
        let chunks = split(value);
        for (index, chunk) in chunks.iter().enumerate() {
            koharu_secrets::set(
                &chunk_key(generation, field, index),
                &SecretString::from((*chunk).to_owned()),
            )?;
        }
        koharu_secrets::set(
            &count_key(generation, field),
            &SecretString::from(chunks.len().to_string()),
        )?;
        Ok(())
    }

    fn delete_generation(self, generation: &str) -> Result<()> {
        for field in ["access", "refresh", "id", "expires_at_ms"] {
            let count_key = count_key(generation, field);
            let count = get(&count_key)?
                .and_then(|value| value.expose_secret().parse::<usize>().ok())
                .unwrap_or_default();
            koharu_secrets::delete(&count_key)?;
            for index in 0..count {
                koharu_secrets::delete(&chunk_key(generation, field, index))?;
            }
        }
        Ok(())
    }
}

fn get(key: &str) -> Result<Option<SecretString>> {
    koharu_secrets::get(key).with_context(|| format!("failed to read secret {key}"))
}

fn count_key(generation: &str, field: &str) -> String {
    format!("{PREFIX}_{generation}_{field}_chunks")
}

fn chunk_key(generation: &str, field: &str, index: usize) -> String {
    format!("{PREFIX}_{generation}_{field}_{index}")
}

fn split(value: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut units = 0;
    for (index, character) in value.char_indices() {
        let width = character.len_utf16();
        if units + width > CHUNK_UTF16_UNITS && index > start {
            chunks.push(&value[start..index]);
            start = index;
            units = 0;
        }
        units += width;
    }
    if start < value.len() {
        chunks.push(&value[start..]);
    }
    chunks
}
