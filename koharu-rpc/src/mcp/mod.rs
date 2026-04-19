//! MCP server exposing Koharu operations as tools.
//!
//! Built on rmcp 1.5's `#[tool_router]` + streamable HTTP transport. Mount
//! via [`mount`] onto an existing axum `Router`; sessions and routing are
//! handled by `StreamableHttpService`.
//!
//! **Tools exposed:**
//!   - `koharu.apply` — apply an `Op` to the active scene
//!   - `koharu.undo` / `koharu.redo`
//!   - `koharu.open_project` / `koharu.close_project`
//!   - `koharu.start_pipeline` — kick off a pipeline run
//!
//! More tools can be added by extending the `#[tool_router]` impl.

use std::sync::Arc;

use camino::Utf8PathBuf;
use koharu_app::{
    App,
    pipeline::{PipelineRunOptions, PipelineSpec, Scope},
};
use koharu_core::{Op, PageId};
use rmcp::handler::server::wrapper::{Json as JsonOutput, Parameters};
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use uuid::Uuid;

/// Server state handed to each tool call. Carries the shared `App`.
#[derive(Clone)]
pub struct KoharuServer {
    app: Arc<App>,
}

impl KoharuServer {
    pub fn new(app: Arc<App>) -> Self {
        Self { app }
    }
}

// ---------------------------------------------------------------------------
// Tool I/O schemas
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInput {
    /// The `Op` value to apply.
    pub op: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutput {
    pub epoch: u64,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UndoOutput {
    pub epoch: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectInput {
    pub path: String,
    /// If set, create the project with this name instead of opening an existing one.
    pub create_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectOutput {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct StartPipelineInput {
    pub steps: Vec<String>,
    pub pages: Option<Vec<PageId>>,
    pub target_language: Option<String>,
    pub system_prompt: Option<String>,
    pub default_font: Option<String>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartPipelineOutput {
    pub job_id: String,
}

// ---------------------------------------------------------------------------
// Tool router
// ---------------------------------------------------------------------------

#[tool_router]
impl KoharuServer {
    #[tool(name = "koharu.apply", description = "Apply an Op to the active scene")]
    async fn apply(
        &self,
        Parameters(input): Parameters<ApplyInput>,
    ) -> Result<JsonOutput<ApplyOutput>, rmcp::ErrorData> {
        let op: Op = serde_json::from_value(input.op).map_err(err)?;
        let epoch = self.app.apply(op).map_err(err)?;
        Ok(JsonOutput(ApplyOutput { epoch }))
    }

    #[tool(name = "koharu.undo", description = "Undo the most recent op")]
    async fn undo(&self) -> Result<JsonOutput<UndoOutput>, rmcp::ErrorData> {
        let epoch = self.app.undo().map_err(err)?;
        Ok(JsonOutput(UndoOutput { epoch }))
    }

    #[tool(name = "koharu.redo", description = "Redo the most recent undo")]
    async fn redo(&self) -> Result<JsonOutput<UndoOutput>, rmcp::ErrorData> {
        let epoch = self.app.redo().map_err(err)?;
        Ok(JsonOutput(UndoOutput { epoch }))
    }

    #[tool(
        name = "koharu.open_project",
        description = "Open or create a Koharu project directory"
    )]
    async fn open_project(
        &self,
        Parameters(input): Parameters<OpenProjectInput>,
    ) -> Result<JsonOutput<OpenProjectOutput>, rmcp::ErrorData> {
        let path = Utf8PathBuf::from(input.path);
        let session = self
            .app
            .open_project(path, input.create_name)
            .await
            .map_err(err)?;
        Ok(JsonOutput(OpenProjectOutput {
            name: session.scene.read().project.name.clone(),
            path: session.dir.to_string(),
        }))
    }

    #[tool(
        name = "koharu.close_project",
        description = "Close the active project"
    )]
    async fn close_project(&self) -> Result<JsonOutput<serde_json::Value>, rmcp::ErrorData> {
        self.app.close_project().await.map_err(err)?;
        Ok(JsonOutput(serde_json::Value::Null))
    }

    #[tool(
        name = "koharu.start_pipeline",
        description = "Kick off a pipeline run; returns a job id"
    )]
    async fn start_pipeline(
        &self,
        Parameters(input): Parameters<StartPipelineInput>,
    ) -> Result<JsonOutput<StartPipelineOutput>, rmcp::ErrorData> {
        let session = self
            .app
            .current_session()
            .ok_or_else(|| rmcp::ErrorData::invalid_request("no project open", None))?;
        let spec = PipelineSpec {
            scope: match input.pages {
                Some(pages) => Scope::Pages(pages),
                None => Scope::WholeProject,
            },
            steps: input.steps,
            options: PipelineRunOptions {
                target_language: input.target_language,
                system_prompt: input.system_prompt,
                default_font: input.default_font,
                region: None,
            },
        };
        let job_id = Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let registry = self.app.registry.clone();
        let runtime = self.app.runtime.clone();
        let llm = self.app.llm.clone();
        let renderer = self.app.renderer.clone();
        let cpu = self.app.cpu_only();
        tokio::spawn(async move {
            let _ = koharu_app::pipeline::run(
                session, registry, runtime, cpu, llm, renderer, spec, cancel, None,
            )
            .await;
        });
        Ok(JsonOutput(StartPipelineOutput { job_id }))
    }
}

fn err(e: impl std::fmt::Display) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(e.to_string(), None)
}

#[tool_handler]
impl ServerHandler for KoharuServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        let mut implementation = rmcp::model::Implementation::default();
        implementation.name = "koharu".into();
        implementation.version = env!("CARGO_PKG_VERSION").into();
        info.server_info = implementation;
        info
    }
}

// ---------------------------------------------------------------------------
// Axum mount
// ---------------------------------------------------------------------------

/// Mount the MCP endpoint at `/mcp` on `router`.
pub fn mount(router: axum::Router, app: Arc<App>) -> axum::Router {
    let manager = Arc::new(LocalSessionManager::default());
    let factory = {
        let app = app.clone();
        move || -> Result<KoharuServer, std::io::Error> { Ok(KoharuServer::new(app.clone())) }
    };
    let service =
        StreamableHttpService::new(factory, manager, StreamableHttpServerConfig::default());
    router.nest_service("/mcp", service)
}
