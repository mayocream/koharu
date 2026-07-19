use anyhow::Result;
use koharu_secrets::{ExposeSecret, SecretString};

use crate::RemoteProviderKind;

#[derive(Clone, Debug, Default)]
pub struct TranslationCredentials {
    pub openai: SecretString,
    pub gemini: SecretString,
    pub claude: SecretString,
    pub deepseek: SecretString,
    pub openai_compatible: SecretString,
    pub openrouter: SecretString,
    pub lm_studio: SecretString,
    pub deepl: SecretString,
    pub google_cloud_translation: SecretString,
    pub caiyun: SecretString,
}

impl TranslationCredentials {
    pub fn load() -> Result<Self> {
        let mut credentials = Self::default();
        for (provider, value) in PROVIDERS.into_iter().zip(credentials.values_mut()) {
            *value = koharu_secrets::get(provider.id())?.unwrap_or_default();
        }
        Ok(credentials)
    }

    pub fn save(&self) -> Result<()> {
        for (provider, value) in PROVIDERS.into_iter().zip(self.values()) {
            if value.expose_secret().trim().is_empty() {
                koharu_secrets::delete(provider.id())?;
            } else {
                koharu_secrets::set(provider.id(), value)?;
            }
        }
        Ok(())
    }

    fn values(&self) -> [&SecretString; 10] {
        [
            &self.openai,
            &self.gemini,
            &self.claude,
            &self.deepseek,
            &self.openai_compatible,
            &self.openrouter,
            &self.lm_studio,
            &self.deepl,
            &self.google_cloud_translation,
            &self.caiyun,
        ]
    }

    fn values_mut(&mut self) -> [&mut SecretString; 10] {
        [
            &mut self.openai,
            &mut self.gemini,
            &mut self.claude,
            &mut self.deepseek,
            &mut self.openai_compatible,
            &mut self.openrouter,
            &mut self.lm_studio,
            &mut self.deepl,
            &mut self.google_cloud_translation,
            &mut self.caiyun,
        ]
    }
}

const PROVIDERS: [RemoteProviderKind; 10] = [
    RemoteProviderKind::OpenAi,
    RemoteProviderKind::Gemini,
    RemoteProviderKind::Claude,
    RemoteProviderKind::DeepSeek,
    RemoteProviderKind::OpenAiCompatible,
    RemoteProviderKind::OpenRouter,
    RemoteProviderKind::LmStudio,
    RemoteProviderKind::DeepL,
    RemoteProviderKind::GoogleCloudTranslation,
    RemoteProviderKind::Caiyun,
];
