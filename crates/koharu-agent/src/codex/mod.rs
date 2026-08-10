mod auth;
mod catalog;
mod protocol;
mod stream;
mod token_store;

use anyhow::{Result, bail};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use specta::Type;

use crate::{Control, Reasoning};

pub use auth::Account;
use auth::Auth;
pub(crate) use protocol::{Request, function_output, message, project_context};
pub(crate) use stream::{Delta, Turn};

const RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";

#[derive(Clone, Debug, Serialize, Type)]
pub struct CodexModel {
    pub id: String,
    pub name: String,
    pub reasoning: Vec<Reasoning>,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoginEvent {
    Progress {
        message: String,
    },
    DeviceCode {
        verification_url: String,
        user_code: String,
    },
}

#[derive(Clone, Debug)]
pub struct Codex {
    client: Client,
    auth: Auth,
}

impl Codex {
    pub fn new() -> Result<Self> {
        let client = koharu_runtime::http_client()?;
        Ok(Self {
            auth: Auth::new(client.clone()),
            client,
        })
    }

    pub fn account(&self) -> Result<Option<Account>> {
        self.auth.account()
    }

    #[tracing::instrument(skip_all)]
    pub async fn login_device<F>(&self, control: &Control, publish: F) -> Result<Account>
    where
        F: FnMut(LoginEvent),
    {
        self.auth.login_device(control, publish).await
    }

    pub fn logout(&self) -> Result<()> {
        self.auth.logout()
    }

    #[tracing::instrument(skip_all)]
    pub async fn models(&self) -> Result<Vec<CodexModel>> {
        catalog::models(&self.client, &self.auth).await
    }

    pub(crate) async fn respond<F>(
        &self,
        request: &Request,
        control: &Control,
        publish: F,
    ) -> Result<Turn>
    where
        F: FnMut(Delta),
    {
        control.ensure_running()?;
        let session = self.auth.session().await?;
        let mut response = self.send(request, &session, control).await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            let session = self.auth.force_refresh().await?;
            response = self.send(request, &session, control).await?;
        }
        if !response.status().is_success() {
            let status = response.status();
            let mut body = response.text().await.unwrap_or_default();
            body.truncate(16 * 1024);
            bail!("Codex returned {status}: {body}");
        }
        stream::read(response, control, publish).await
    }

    async fn send(
        &self,
        request: &Request,
        session: &auth::Session,
        control: &Control,
    ) -> Result<reqwest::Response> {
        let request_id = request.prompt_cache_key.clone();
        let send = self
            .client
            .post(RESPONSES_URL)
            .bearer_auth(&session.access)
            .header("chatgpt-account-id", &session.account.id)
            .header("originator", "koharu")
            .header("OpenAI-Beta", "responses=experimental")
            .header("accept", "text/event-stream")
            .header("session_id", &request_id)
            .header("x-client-request-id", request_id)
            .json(request)
            .send();
        tokio::select! {
            response = send => Ok(response?),
            () = control.cancelled() => {
                control.ensure_running()?;
                unreachable!("cancelled control must fail ensure_running")
            }
        }
    }
}
