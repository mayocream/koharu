pub struct ApiModelInfo {
    pub id: &'static str,
    pub name: &'static str,
}

pub struct ApiProviderInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub models: &'static [ApiModelInfo],
}

pub const OPENAI: ApiProviderInfo = ApiProviderInfo {
    id: "openai",
    name: "OpenAI",
    models: &[ApiModelInfo {
        id: "gpt-5-mini",
        name: "GPT-5 mini",
    }],
};

pub const GEMINI: ApiProviderInfo = ApiProviderInfo {
    id: "gemini",
    name: "Gemini",
    models: &[ApiModelInfo {
        id: "gemini-3.1-flash-lite-preview",
        name: "Gemini 3.1 Flash-Lite Preview",
    }],
};

pub const CLAUDE: ApiProviderInfo = ApiProviderInfo {
    id: "claude",
    name: "Claude",
    models: &[ApiModelInfo {
        id: "claude-haiku-4-5",
        name: "Claude Haiku 4.5",
    }],
};

pub const ALL_API_PROVIDERS: &[&ApiProviderInfo] = &[&OPENAI, &GEMINI, &CLAUDE];

/// Parse a namespaced model ID like `"openai:gpt-5-mini"` into its provider
/// and model-id parts. Returns `None` if the ID is not in the expected format
/// or the provider / model are not recognised.
pub fn find_api_model(id: &str) -> Option<(&'static ApiProviderInfo, &'static str)> {
    let (provider_id, model_id) = id.split_once(':')?;
    let provider = ALL_API_PROVIDERS
        .iter()
        .copied()
        .find(|p| p.id == provider_id)?;
    let model = provider.models.iter().find(|m| m.id == model_id)?;
    Some((provider, model.id))
}
