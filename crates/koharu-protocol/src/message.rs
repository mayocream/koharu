use koharu_agent::{Config as AgentConfig, RunId};
use koharu_pipeline::{Operation, PipelineConfig, Scope};
use koharu_renderer::TypesettingConfig;
use koharu_scene::{EntityId, Revision};
use koharu_translator::Model;
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::*;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(transparent)]
pub struct RequestId(Uuid);

impl RequestId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}
impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

/// Flat browser-to-desktop envelope: `{ id, command, payload }`.
#[derive(Clone, Debug, Deserialize, Type)]
pub struct Request {
    pub id: RequestId,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Clone, Debug, Deserialize, Type)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum Command {
    GetStartup {},
    GetProject {},
    GetPages {},
    GetPage {},
    ListProjects {},
    CreateProject {
        name: String,
    },
    OpenProject {
        name: String,
    },
    DeleteProject {
        name: String,
    },
    CloseProject {},
    ImportPages {
        source: PageImportSource,
    },
    SelectPage {
        page: EntityId,
    },
    RenamePage {
        page: EntityId,
        label: String,
    },
    DeletePages {
        pages: Vec<EntityId>,
    },
    MovePage {
        page: EntityId,
        index: u32,
    },
    SetSourceText {
        layer: EntityId,
        text: String,
    },
    SetTranslation {
        layer: EntityId,
        text: Option<String>,
    },
    SetTypography {
        updates: Vec<TypographyUpdate>,
    },
    SetGeometry {
        updates: Vec<GeometryUpdate>,
    },
    SetVisibility {
        layers: Vec<EntityId>,
        visible: Option<bool>,
        opacity: Option<f32>,
    },
    DeleteLayers {
        layers: Vec<EntityId>,
    },
    MoveLayer {
        layer: EntityId,
        parent: EntityId,
        index: u32,
    },
    Undo {},
    Redo {},
    Process {
        scope: Scope,
        operation: Operation,
    },
    StopJob {
        job: JobId,
    },
    ExportPages {
        pages: Vec<EntityId>,
        format: ExportFormat,
    },
    GetThumbnail {
        page: EntityId,
    },
    GetFonts {},
    GetFontPreview {
        #[serde(rename = "familyName")]
        family_name: String,
    },
    SavePreferences {
        pipeline: Box<PipelineConfig>,
        providers: Box<ProviderPreferences>,
        typesetting: Box<TypesettingConfig>,
    },
    GetPreferences {},
    GetTranslationModels {},
    SetZoom {
        zoom: f32,
    },
    SetCanvasView {
        zoom: f64,
        translation: [f64; 2],
    },
    FitCanvas {},
    AddPointText {
        point: Point,
    },
    AddTextBox {
        frame: CanvasFrame,
    },
    BeginPaint {
        layer: Option<EntityId>,
        point: Point,
        brush: PaintBrush,
    },
    ExtendPaint {
        points: Vec<Point>,
    },
    FinishPaint {},
    CancelPaint {},
    BeginErase {
        layer: EntityId,
        point: Point,
        diameter: f32,
    },
    ExtendErase {
        points: Vec<Point>,
    },
    FinishErase {},
    CancelErase {},
    BeginTransform {
        elements: Vec<TransformFrame>,
    },
    UpdateTransform {
        frame: u32,
        elements: Vec<TransformFrame>,
    },
    PreviewOpacity {
        element: EntityId,
        opacity: Option<f32>,
    },
    FinishTransform {},
    CancelTransform {},
    BeginInpaint {
        point: Point,
        diameter: f32,
    },
    ExtendInpaint {
        points: Vec<Point>,
    },
    FinishInpaint {},
    CancelInpaint {},
    SampleColor {
        point: Point,
    },
    SetViewport {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        dpr: f64,
        background: [u8; 3],
    },
    GetAgentStatus {},
    LoginAgent {},
    LogoutAgent {},
    SaveAgentConfig {
        config: AgentConfig,
    },
    RunAgent {
        prompt: String,
    },
    CancelAgent {
        run: RunId,
    },
    WindowMinimize {},
    WindowToggleMaximize {},
    WindowClose {},
    WindowBeginDrag {},
    OpenExternal {
        url: String,
    },
    GetVersion {},
    CheckUpdate {},
    InstallUpdate {
        version: String,
    },
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(untagged)]
pub enum CommandResult {
    Unit(()),
    Startup(StartupState),
    Project(Option<ProjectInfo>),
    Projects(Vec<ProjectSummary>),
    Pages(Vec<PageSummary>),
    Page(Option<Page>),
    PageValue(Page),
    Canvas(CanvasState),
    LayerCommit(LayerCommit),
    Revision(Option<Revision>),
    Job(JobId),
    OptionalJob(Option<JobId>),
    Binary(BinaryPayload),
    Fonts(Vec<FontFamily>),
    Preferences(Preferences),
    Models(Vec<Model>),
    Color([u8; 4]),
    AgentStatus(AgentStatus),
    AgentConfig(AgentConfig),
    AgentRun(RunId),
    WindowState(WindowState),
    Version(String),
    OptionalUpdate(Option<UpdateInfo>),
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct Response {
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<CommandResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
}

impl Response {
    pub fn success(id: RequestId, result: CommandResult) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }
    pub fn failure(id: RequestId, error: AppError) -> Self {
        Self {
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerMessage {
    Response {
        id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Box<CommandResult>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<AppError>,
    },
    Event {
        #[specta(type = f64)]
        sequence: u64,
        event: crate::AppEvent,
    },
}

impl From<Response> for ServerMessage {
    fn from(value: Response) -> Self {
        Self::Response {
            id: value.id,
            result: value.result.map(Box::new),
            error: value.error,
        }
    }
}
impl From<ServerEvent> for ServerMessage {
    fn from(value: ServerEvent) -> Self {
        Self::Event {
            sequence: value.sequence,
            event: value.event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flat_wire_requests_cover_no_arg_scalar_and_structured_payloads() {
        let no_arg: Request =
            serde_json::from_value(json!({"id": Uuid::new_v4(), "command": "undo", "payload": {}}))
                .unwrap();
        assert!(matches!(no_arg.command, Command::Undo {}));
        let scalar: Request = serde_json::from_value(json!({"id": Uuid::new_v4(), "command": "create_project", "payload": {"name": "Volume 1"}})).unwrap();
        assert!(matches!(scalar.command, Command::CreateProject { name } if name == "Volume 1"));
        let structured: Request = serde_json::from_value(json!({"id": Uuid::new_v4(), "command": "set_canvas_view", "payload": {"zoom": 2.0, "translation": [4.0, 8.0]}})).unwrap();
        assert!(matches!(structured.command, Command::SetCanvasView { zoom, .. } if zoom == 2.0));
    }

    #[test]
    fn binary_result_uses_attachment_metadata_not_json_bytes() {
        let response: ServerMessage = Response::success(
            RequestId::new(),
            CommandResult::Binary(BinaryPayload {
                attachment: "thumbnail".into(),
            }),
        )
        .into();
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["result"]["attachment"], "thumbnail");
        assert!(value.to_string().find("bytes").is_none());
    }
}
