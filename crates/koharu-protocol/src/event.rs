use serde::Serialize;
use specta::Type;

use crate::{
    AgentLoginEvent, AgentRunEvent, AppError, CanvasState, Download, Job, ModelResources,
    ProjectInfo, StartupState, WindowState,
};

#[derive(Clone, Debug, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    StartupReady { startup: Box<StartupState> },
    StartupFailed { error: AppError },
    Canvas { state: CanvasState },
    Job { job: Job },
    Download { download: Download },
    Resources { resources: ModelResources },
    Project { project: Option<ProjectInfo> },
    AgentLogin { event: AgentLoginEvent },
    AgentRun { event: AgentRunEvent },
    WindowState { state: WindowState },
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct ServerEvent {
    #[specta(type = f64)]
    pub sequence: u64,
    pub event: AppEvent,
}
