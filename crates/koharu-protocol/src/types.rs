use koharu_agent::{Account, CodexModel, Config, RunId};
use koharu_pipeline::Stage;
use koharu_scene::{EntityId, Revision};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

pub type AgentLoginEvent = koharu_agent::LoginEvent;
pub type AgentRunEvent = koharu_agent::Event;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct AppError {
    pub code: AppErrorCode,
    pub message: String,
}

impl AppError {
    #[must_use]
    pub fn new(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn internal(error: impl std::fmt::Display) -> Self {
        Self::new(AppErrorCode::Internal, error.to_string())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppErrorCode {
    InvalidRequest,
    NotReady,
    NoProject,
    Conflict,
    NotFound,
    Cancelled,
    Unavailable,
    Internal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct UpdateInfo {
    pub version: String,
    pub body: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct UpdateProgress {
    pub version: String,
    #[specta(type = f64)]
    pub downloaded: u64,
    #[specta(type = Option<f64>)]
    pub total: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
pub struct StartupState {
    pub preferences: Preferences,
    pub jobs: Vec<Job>,
    pub canvas: CanvasState,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct Preferences {
    pub pipeline: koharu_pipeline::PipelineConfig,
    pub providers: ProviderPreferences,
    pub typesetting: koharu_renderer::TypesettingConfig,
    pub languages: Vec<LanguageChoice>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            pipeline: koharu_pipeline::PipelineConfig::default(),
            providers: ProviderPreferences {
                entries: Vec::new(),
            },
            typesetting: koharu_renderer::TypesettingConfig::default(),
            languages: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct ProviderPreferences {
    pub entries: Vec<ProviderPreference>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct ProviderPreference {
    pub name: String,
    pub config: koharu_translator::ProviderConfig,
    pub credential: Option<CredentialInput>,
}

#[derive(Clone, Default, Deserialize, PartialEq, Serialize, Type)]
pub struct CredentialInput {
    pub configured: bool,
    pub value: Option<String>,
    pub clear: bool,
}

impl std::fmt::Debug for CredentialInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CredentialInput")
            .field("configured", &self.configured)
            .field("value", &self.value.as_ref().map(|_| "[REDACTED]"))
            .field("clear", &self.clear)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct LanguageChoice {
    pub tag: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename = "Frame")]
pub struct CanvasFrame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub angle_degrees: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct TransformFrame {
    pub element: EntityId,
    pub frame: CanvasFrame,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
pub struct CanvasState {
    pub zoom: f64,
    pub translation: [f64; 2],
    pub fitted: bool,
    pub element_frames: Vec<TransformFrame>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct ProjectInfo {
    pub name: String,
    pub revision: Revision,
    pub active_page: Option<EntityId>,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct ProjectSummary {
    pub name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct PageSummary {
    pub id: EntityId,
    pub label: String,
    pub size: PageSize,
    pub source_asset: Option<String>,
    #[specta(type = f64)]
    pub layer_count: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct Page {
    pub id: EntityId,
    pub label: String,
    pub size: PageSize,
    pub layers: Vec<Layer>,
    pub regions: Vec<AnalysisRegion>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Layer {
    Group {
        id: EntityId,
        parent: Option<EntityId>,
        visibility: LayerVisibility,
        name: String,
        role: Option<GroupRole>,
    },
    Text {
        id: EntityId,
        parent: Option<EntityId>,
        geometry: Option<Geometry>,
        visibility: LayerVisibility,
        content: Box<TextContent>,
        typography: Option<Typography>,
        layout: koharu_scene::TextLayoutKind,
        automatic_region: Option<EntityId>,
    },
    Raster {
        id: EntityId,
        parent: Option<EntityId>,
        visibility: LayerVisibility,
        image: Option<String>,
        name: String,
        kind: koharu_scene::RasterLayerKind,
    },
    Image {
        id: EntityId,
        parent: Option<EntityId>,
        geometry: Geometry,
        visibility: LayerVisibility,
        image: String,
    },
    Artwork {
        id: EntityId,
        parent: Option<EntityId>,
        geometry: Geometry,
        visibility: LayerVisibility,
        image: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GroupRole {
    Text,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct TextContent {
    pub id: EntityId,
    pub source: Option<SourceText>,
    pub translation: Option<Translation>,
    pub role: Option<String>,
    pub source_region: Option<EntityId>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct AnalysisRegion {
    pub id: EntityId,
    pub parent: Option<EntityId>,
    pub geometry: Geometry,
    pub kind: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct Geometry {
    pub points: Vec<Point>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct LayerVisibility {
    pub visible: bool,
    pub opacity: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct SourceText {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct Translation {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct Typography {
    pub preferred_font: Option<String>,
    pub font_weight: Option<u16>,
    pub font_style: Option<koharu_scene::FontStyle>,
    pub size: Option<f32>,
    pub auto_fit: bool,
    pub color: Option<[u8; 4]>,
    pub stroke_color: Option<[u8; 4]>,
    pub stroke_width: Option<f32>,
    pub alignment: Option<koharu_scene::TextAlignment>,
    pub writing_mode: Option<koharu_scene::WritingMode>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Type)]
pub struct GeometryUpdate {
    pub layer: EntityId,
    pub points: Option<Vec<Point>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Type)]
pub struct TypographyUpdate {
    pub layer: EntityId,
    pub typography: Typography,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum PageImportSource {
    Files,
    Folder,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Png,
    Psd,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Type)]
pub struct PaintBrush {
    pub diameter: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct LayerCommit {
    pub revision: Revision,
    pub layer: EntityId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct FontFamily {
    pub name: String,
    pub metadata: FontMetadata,
    pub sources: Vec<FontSource>,
    pub faces: Vec<FontFace>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct FontMetadata {
    pub primary_script: Option<String>,
    pub scripts: Vec<String>,
    pub languages: Vec<String>,
    pub category: Option<String>,
    pub classifications: Vec<String>,
    pub use_cases: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct FontFace {
    pub postscript_name: String,
    pub weight: u16,
    pub weight_range: Option<FontRange>,
    pub style: koharu_scene::FontStyle,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct FontRange {
    pub minimum: u16,
    pub maximum: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FontSource {
    System,
    Bundled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(transparent)]
pub struct JobId(Uuid);

impl JobId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
pub struct Job {
    pub id: JobId,
    pub state: JobState,
    #[specta(type = f64)]
    pub completed: usize,
    #[specta(type = f64)]
    pub total: usize,
    pub page: Option<EntityId>,
    pub stage: Option<Stage>,
    pub model: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Finished,
    Failed,
    Stopped,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct Download {
    #[specta(type = f64)]
    pub id: u64,
    pub state: DownloadState,
    pub name: Option<String>,
    #[specta(type = f64)]
    pub completed: u64,
    #[specta(type = f64)]
    pub total: u64,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Running,
    Finished,
    Failed,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
pub struct ModelResources {
    #[specta(type = f64)]
    pub process_memory: u64,
    #[specta(type = f64)]
    pub system_memory: u64,
    pub process_cpu: f32,
    pub devices: Vec<DeviceResources>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
pub struct DeviceResources {
    pub name: String,
    pub selected: bool,
    #[specta(type = Option<f64>)]
    pub memory_budget: Option<u64>,
    #[specta(type = Option<f64>)]
    pub memory_used: Option<u64>,
    pub utilization: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct AgentStatus {
    pub account: Option<Account>,
    pub models: Vec<CodexModel>,
    pub config: Config,
    pub running: Option<RunId>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct WindowState {
    pub maximized: bool,
    pub minimized: bool,
    pub fullscreen: bool,
    pub focused: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct BinaryPayload {
    pub attachment: String,
}
