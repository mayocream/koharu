use std::fmt;
use std::time::Instant;

use koharu_runtime::{RuntimeHttpClient, RuntimeHttpConfig};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;

use super::config::CodexConfig;
use super::device::{DeviceAuthorization, DeviceCode};
use super::error::{CodexError, Result};
use super::image::{CodexImageGenerationRequest, CodexInputImage, image_response_stream_result};
use super::requests::{
    TokenExchangeRequest, TokenExchangeResponse, TokenPollRequest, TokenPollSuccessResponse,
    TokenRefreshRequest, TokenRefreshResponse, UserCodeRequest, UserCodeResponse,
};
use super::token_store::TokenStore;
use super::tokens::CodexTokens;
use crate::provider::{AiImageProvider, AiImageRequest, AiImageResult};

const USER_AGENT: &str = concat!("koharu-ai/", env!("CARGO_PKG_VERSION"));

#[derive(Clone)]
pub struct CodexClient {
    http_client: RuntimeHttpClient,
    config: CodexConfig,
    token_store: TokenStore,
}

impl fmt::Debug for CodexClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexClient")
            .field("config", &self.config)
            .field("token_store", &self.token_store)
            .finish_non_exhaustive()
    }
}

impl Default for CodexClient {
    fn default() -> Self {
        Self::new(CodexConfig::default())
    }
}

impl CodexClient {
    pub fn new(config: CodexConfig) -> Self {
        Self::try_new(config).expect("failed to build default runtime HTTP client")
    }

    pub fn try_new(config: CodexConfig) -> Result<Self> {
        Self::with_runtime_http_config(config, RuntimeHttpConfig::default())
    }

    pub fn with_runtime_http_config(config: CodexConfig, http: RuntimeHttpConfig) -> Result<Self> {
        let http_client = http.build_client().map_err(CodexError::RuntimeHttpClient)?;
        Ok(Self::with_http_client(config, http_client))
    }

    pub fn with_http_client(config: CodexConfig, http_client: RuntimeHttpClient) -> Self {
        Self {
            http_client,
            config,
            token_store: TokenStore::default(),
        }
    }

    pub fn with_token_store(mut self, token_store: TokenStore) -> Self {
        self.token_store = token_store;
        self
    }

    pub fn config(&self) -> &CodexConfig {
        &self.config
    }

    pub fn token_store(&self) -> &TokenStore {
        &self.token_store
    }

    pub async fn request_device_code(&self) -> Result<DeviceCode> {
        let endpoint = self.config.accounts_endpoint("deviceauth/usercode");
        let response = self
            .http_client
            .post(&endpoint)
            .json(&UserCodeRequest {
                client_id: &self.config.client_id,
            })
            .send()
            .await?;
        let body: UserCodeResponse = ensure_success(&endpoint, response).await?.json().await?;

        Ok(DeviceCode::new(
            self.config.issuer_endpoint("codex/device"),
            body.user_code,
            body.device_auth_id,
            body.interval,
        ))
    }

    pub async fn poll_device_code_once(
        &self,
        device_code: &DeviceCode,
    ) -> Result<Option<DeviceAuthorization>> {
        let endpoint = self.config.accounts_endpoint("deviceauth/token");
        let response = self
            .http_client
            .post(&endpoint)
            .json(&TokenPollRequest {
                device_auth_id: device_code.device_auth_id(),
                user_code: &device_code.user_code,
            })
            .send()
            .await?;

        if response.status() == StatusCode::FORBIDDEN || response.status() == StatusCode::NOT_FOUND
        {
            return Ok(None);
        }

        let body: TokenPollSuccessResponse =
            ensure_success(&endpoint, response).await?.json().await?;

        Ok(Some(DeviceAuthorization {
            authorization_code: body.authorization_code,
            code_verifier: body.code_verifier,
            code_challenge: body.code_challenge,
        }))
    }

    pub async fn poll_device_code(&self, device_code: &DeviceCode) -> Result<DeviceAuthorization> {
        let started_at = Instant::now();
        loop {
            if let Some(authorization) = self.poll_device_code_once(device_code).await? {
                return Ok(authorization);
            }

            let elapsed = started_at.elapsed();
            if elapsed >= self.config.device_auth_timeout {
                return Err(CodexError::DeviceCodeTimeout(
                    self.config.device_auth_timeout,
                ));
            }

            let remaining = self.config.device_auth_timeout - elapsed;
            tokio::time::sleep(device_code.interval().min(remaining)).await;
        }
    }

    pub async fn exchange_code_for_tokens(
        &self,
        authorization_code: &str,
        code_verifier: &str,
    ) -> Result<CodexTokens> {
        let endpoint = self.config.issuer_endpoint("oauth/token");
        let response = self
            .http_client
            .post(&endpoint)
            .form(&TokenExchangeRequest {
                grant_type: "authorization_code",
                code: authorization_code,
                redirect_uri: self.config.device_callback_uri(),
                client_id: &self.config.client_id,
                code_verifier,
            })
            .send()
            .await?;
        let body: TokenExchangeResponse = ensure_success(&endpoint, response).await?.json().await?;

        Ok(CodexTokens {
            id_token: body.id_token,
            access_token: body.access_token,
            refresh_token: body.refresh_token,
            token_type: body.token_type,
            expires_in: body.expires_in,
            scope: body.scope,
        })
    }

    pub async fn exchange_device_authorization(
        &self,
        authorization: &DeviceAuthorization,
    ) -> Result<CodexTokens> {
        self.exchange_code_for_tokens(
            &authorization.authorization_code,
            &authorization.code_verifier,
        )
        .await
    }

    pub async fn refresh_tokens(&self, tokens: &CodexTokens) -> Result<CodexTokens> {
        let refresh_token = tokens
            .refresh_token
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(CodexError::MissingRefreshToken)?;
        let endpoint = self.config.issuer_endpoint("oauth/token");
        let response = self
            .http_client
            .post(&endpoint)
            .json(&TokenRefreshRequest {
                client_id: &self.config.client_id,
                grant_type: "refresh_token",
                refresh_token,
            })
            .send()
            .await?;
        let body: TokenRefreshResponse = ensure_success(&endpoint, response).await?.json().await?;

        Ok(tokens.refreshed_with(body))
    }

    pub async fn refresh_stored_tokens(&self) -> Result<CodexTokens> {
        let tokens = self.load_tokens()?;
        let refreshed = self.refresh_tokens(&tokens).await?;
        self.token_store.store(&refreshed)?;
        Ok(refreshed)
    }

    pub async fn complete_device_code_login(
        &self,
        device_code: &DeviceCode,
    ) -> Result<CodexTokens> {
        let authorization = self.poll_device_code(device_code).await?;
        let tokens = self.exchange_device_authorization(&authorization).await?;
        self.token_store.store(&tokens)?;
        Ok(tokens)
    }

    pub fn load_tokens(&self) -> Result<CodexTokens> {
        self.token_store.load()?.ok_or(CodexError::MissingToken)
    }

    pub async fn create_response_raw<T: Serialize + ?Sized>(
        &self,
        request: &T,
    ) -> Result<reqwest::Response> {
        let tokens = self.load_tokens()?;
        let response = self.send_response_request(&tokens, request).await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return ensure_success(&self.config.responses_url, response).await;
        }

        let refreshed = self.refresh_tokens(&tokens).await?;
        self.token_store.store(&refreshed)?;
        self.create_response_raw_with_tokens(&refreshed, request)
            .await
    }

    pub async fn create_response_raw_with_tokens<T: Serialize + ?Sized>(
        &self,
        tokens: &CodexTokens,
        request: &T,
    ) -> Result<reqwest::Response> {
        let response = self.send_response_request(tokens, request).await?;
        ensure_success(&self.config.responses_url, response).await
    }

    pub async fn create_response_json<T: Serialize + ?Sized>(&self, request: &T) -> Result<Value> {
        self.create_response_raw(request)
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    pub async fn create_response_json_with_tokens<T: Serialize + ?Sized>(
        &self,
        tokens: &CodexTokens,
        request: &T,
    ) -> Result<Value> {
        self.create_response_raw_with_tokens(tokens, request)
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    async fn send_response_request<T: Serialize + ?Sized>(
        &self,
        tokens: &CodexTokens,
        request: &T,
    ) -> Result<reqwest::Response> {
        let mut builder = self
            .http_client
            .post(&self.config.responses_url)
            .bearer_auth(&tokens.access_token)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .json(request);

        if let Some(account_id) = tokens.chatgpt_account_id() {
            builder = builder.header("chatgpt-account-id", account_id);
        }

        Ok(builder.send().await?)
    }
}

async fn ensure_success(endpoint: &str, response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
    Err(CodexError::HttpStatus {
        endpoint: endpoint.to_owned(),
        status,
        body,
    })
}

#[async_trait::async_trait]
impl AiImageProvider for CodexClient {
    async fn generate_image(&self, request: AiImageRequest) -> anyhow::Result<AiImageResult> {
        let action = request.action.unwrap_or_else(|| {
            if request.input_image.is_some() {
                "edit".to_string()
            } else {
                "generate".to_string()
            }
        });

        let mut codex_request = CodexImageGenerationRequest::new(request.model, request.prompt)
            .with_instructions(request.instructions)
            .with_quality(request.quality)
            .with_action(action);
        if let Some(size) = request.size {
            codex_request = codex_request.with_size(size);
        }
        if let Some(image) = request.input_image {
            codex_request = codex_request
                .with_input_image(CodexInputImage::new(image.data_url).with_detail(image.detail));
        }

        let response = self.create_response_raw(&codex_request).await?;
        let result = image_response_stream_result(response).await?;
        let image_url = result.image_url.ok_or_else(|| {
            let response_text = result.response_text.as_deref().unwrap_or("none");
            anyhow::anyhow!(
                "Codex returned no image URL or image result. Response text: {response_text}"
            )
        })?;
        Ok(AiImageResult { image_url })
    }
}
