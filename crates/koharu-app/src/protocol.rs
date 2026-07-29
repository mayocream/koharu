//! Stable JSON boundary between the native application and the web UI.
//!
//! These are application DTOs, not serialized scene components. The protocol
//! exposes ergonomic projections and accepts user intent while keeping scene
//! provenance, component slots, and renderer caches on the Rust side.

use std::fmt;

use koharu_pipeline::{Operation, Scope};
use koharu_scene::{EntityId, ProjectId, Revision, TextAlignment, WritingMode};
use koharu_secrets::{ExposeSecret, SecretString};
use koharu_translator::{Providers, TranslationConfig, TranslationCredentials};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

pub use self::{
    UiCommand as AppCommand, UiError as AppError, UiErrorCode as AppErrorCode, UiEvent as AppEvent,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(transparent)]
pub struct RequestId(Uuid);

impl From<Uuid> for RequestId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeMessage {
    Ready {
        dpr: f64,
        width: f64,
        height: f64,
    },
    Viewport {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        dpr: f64,
        background: [u8; 3],
    },
    Window {
        action: WindowAction,
    },
    Command {
        id: RequestId,
        base: Revision,
        command: Box<UiCommand>,
    },
    Interaction {
        interaction: CanvasInteraction,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WindowAction {
    Drag,
    ResizeEast,
    ResizeNorth,
    ResizeNorthEast,
    ResizeNorthWest,
    ResizeSouth,
    ResizeSouthEast,
    ResizeSouthWest,
    ResizeWest,
    Minimize,
    ToggleMaximize,
    Close,
}

#[derive(Debug, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiCommand {
    Synchronize,
    CreateProject,
    OpenProject,
    CloseProject,
    ImportPages,
    RenamePage {
        page: EntityId,
        label: String,
    },
    DeletePages {
        pages: Vec<EntityId>,
    },
    MovePage {
        page: EntityId,
        #[specta(type = f64)]
        index: usize,
    },
    AddText {
        page: EntityId,
        frame: Frame,
    },
    SetSourceText {
        entity: EntityId,
        text: String,
    },
    SetTranslation {
        entity: EntityId,
        locale: String,
        text: Option<String>,
    },
    SetTypography {
        entities: Vec<EntityTypography>,
    },
    SetGeometry {
        entities: Vec<EntityGeometry>,
    },
    SetVisibility {
        entities: Vec<EntityId>,
        visible: Option<bool>,
        opacity: Option<f32>,
    },
    DeleteEntities {
        entities: Vec<EntityId>,
    },
    MoveEntity {
        entity: EntityId,
        parent: EntityId,
        #[specta(type = f64)]
        index: usize,
    },
    FinishTransform,
    Undo,
    Redo,
    RunPipeline {
        scope: Scope,
        operation: Operation,
    },
    StopJob {
        job: RequestId,
    },
    ExportPages {
        pages: Vec<EntityId>,
        format: ExportFormat,
    },
    GetSettings,
    SetSettings {
        pipeline: koharu_pipeline::PipelineConfig,
        translation: Box<TranslationSettings>,
    },
    CollectGarbage,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
pub struct Frame {
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

#[derive(Clone, Debug, Deserialize, Type)]
pub struct EntityGeometry {
    pub entity: EntityId,
    pub points: Vec<Point>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct TypographyIntent {
    pub preferred_font: Option<String>,
    pub size: Option<f32>,
    pub alignment: Option<TextAlignment>,
    pub writing_mode: Option<WritingMode>,
}

#[derive(Clone, Debug, Deserialize, Type)]
pub struct EntityTypography {
    pub entity: EntityId,
    pub typography: TypographyIntent,
}

#[derive(Debug, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CanvasInteraction {
    ShowPage {
        page: EntityId,
    },
    SetCamera {
        zoom: f64,
        translation: [f64; 2],
    },
    SetZoom {
        zoom: f64,
    },
    FitWindow,
    SetDisplay {
        display: CanvasDisplay,
    },
    SetOverlays {
        selected: Vec<EntityId>,
        hovered: Option<EntityId>,
        draft: Option<Frame>,
        guides: Vec<CanvasGuide>,
        brush_cursor: Option<CanvasBrushCursor>,
    },
    HitTest {
        #[specta(type = f64)]
        id: u64,
        x: f64,
        y: f64,
    },
    BeginTransform {
        elements: Vec<EntityId>,
        target: HitTarget,
        x: f64,
        y: f64,
    },
    UpdateTransform {
        x: f64,
        y: f64,
    },
    CancelTransform,
    BeginMaskStroke {
        plane: MaskPlane,
        diameter: f32,
        erase: bool,
        x: f64,
        y: f64,
    },
    ExtendMaskStroke {
        x: f64,
        y: f64,
    },
    FinishMaskStroke,
    CancelMaskStroke,
}

#[derive(Clone, Debug, Deserialize, Type)]
pub struct CanvasDisplay {
    pub page: CanvasPageView,
    pub show_text: bool,
    pub text_mask: Option<CanvasMaskOverlay>,
    pub brush_mask: Option<CanvasMaskOverlay>,
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CanvasPageView {
    Source,
    Clean,
    Rendered,
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
pub struct CanvasMaskOverlay {
    pub tint: [u8; 4],
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(tag = "axis", content = "position", rename_all = "snake_case")]
pub enum CanvasGuide {
    Horizontal(f64),
    Vertical(f64),
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
pub struct CanvasBrushCursor {
    pub x: f64,
    pub y: f64,
    pub diameter: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MaskPlane {
    Text,
    Brush,
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Png,
    Psd,
}

#[derive(Debug, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    Accepted {
        id: RequestId,
        revision: Revision,
    },
    CommandCancelled {
        id: RequestId,
        revision: Revision,
    },
    Rejected {
        id: RequestId,
        error: UiError,
    },
    Problem {
        error: UiError,
    },
    ProjectOpened {
        revision: Revision,
        project: ProjectHeader,
        pages: Vec<PageSummary>,
    },
    PageLoaded {
        revision: Revision,
        page: PageView,
    },
    ProjectChanged(Box<ProjectDelta>),
    ProjectClosed,
    HitTest {
        #[specta(type = f64)]
        id: u64,
        target: Option<HitTarget>,
    },
    ViewChanged {
        zoom: f64,
        translation: [f64; 2],
        auto_fit: bool,
    },
    JobChanged(JobStatus),
    DownloadChanged(DownloadStatus),
    SettingsChanged {
        settings: Box<SettingsView>,
    },
    ResourcesChanged {
        resources: SystemResourcesView,
    },
    GarbageCollected {
        #[specta(type = f64)]
        blobs: usize,
        #[specta(type = f64)]
        bytes: u64,
    },
}

#[derive(Debug, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    App { payload: UiEvent },
}

#[derive(Debug, Serialize, Type)]
pub struct UiError {
    pub code: UiErrorCode,
    pub message: String,
    pub current_revision: Option<Revision>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UiErrorCode {
    StaleRevision,
    NoProject,
    NotFound,
    Busy,
    InvalidInput,
    IoFailed,
    Internal,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct SettingsView {
    pub pipeline: koharu_pipeline::PipelineConfig,
    pub translation: TranslationSettings,
    pub local_translation_models: Vec<String>,
    pub target_languages: Vec<TargetLanguageView>,
    pub fonts: Vec<FontFaceView>,
}

#[derive(Clone, Debug, Default, Serialize, Type)]
pub struct SystemResourcesView {
    #[specta(type = f64)]
    pub process_memory_bytes: u64,
    #[specta(type = f64)]
    pub system_memory_total_bytes: u64,
    #[specta(type = f64)]
    pub system_memory_used_bytes: u64,
    pub process_cpu_percent: f32,
    pub system_cpu_percent: f32,
    pub devices: Vec<DeviceResourcesView>,
}

#[derive(Clone, Debug, Default, Serialize, Type)]
pub struct DeviceResourcesView {
    pub name: String,
    pub selected: bool,
    #[specta(type = Option<f64>)]
    pub memory_budget_bytes: Option<u64>,
    #[specta(type = Option<f64>)]
    pub memory_used_bytes: Option<u64>,
    pub utilization_percent: Option<f32>,
}

impl From<koharu_pipeline::ResourceSnapshot> for SystemResourcesView {
    fn from(snapshot: koharu_pipeline::ResourceSnapshot) -> Self {
        Self {
            process_memory_bytes: snapshot.process_memory_bytes,
            system_memory_total_bytes: snapshot.system_memory_total_bytes,
            system_memory_used_bytes: snapshot.system_memory_used_bytes,
            process_cpu_percent: snapshot.process_cpu_percent,
            system_cpu_percent: snapshot.system_cpu_percent,
            devices: snapshot
                .devices
                .into_iter()
                .map(|device| DeviceResourcesView {
                    name: device.name,
                    selected: device.selected,
                    memory_budget_bytes: device.memory_budget_bytes,
                    memory_used_bytes: device.memory_used_bytes,
                    utilization_percent: device.utilization_percent,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct FontFaceView {
    pub family_name: String,
    pub post_script_name: String,
    pub weight: u16,
    pub stretch: u16,
    pub style: FontFaceStyleView,
    pub source: FontSourceView,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FontFaceStyleView {
    Normal,
    Italic,
    Oblique,
}

#[derive(Clone, Copy, Debug, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FontSourceView {
    System,
    Registered,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct TranslationSettings {
    pub model: Providers,
    pub target_language: String,
    pub instructions: Option<String>,
    pub credentials: TranslationCredentialsView,
}

impl TranslationSettings {
    pub fn from_config(config: &TranslationConfig) -> anyhow::Result<Self> {
        Ok(Self {
            model: config.model.clone(),
            target_language: config.target_language.clone(),
            instructions: config.instructions.clone(),
            credentials: TranslationCredentialsView::from(&TranslationCredentials::load()?),
        })
    }

    pub fn into_parts(self) -> anyhow::Result<(TranslationConfig, TranslationCredentials)> {
        let mut credentials = TranslationCredentials::load()?;
        self.credentials.apply(&mut credentials);
        Ok((
            TranslationConfig {
                model: self.model,
                target_language: self.target_language,
                instructions: self.instructions,
            },
            credentials,
        ))
    }
}

#[derive(Clone, Default, Deserialize, Serialize, Type)]
pub struct TranslationCredentialsView {
    pub openai: CredentialEdit,
    pub gemini: CredentialEdit,
    pub claude: CredentialEdit,
    pub deepseek: CredentialEdit,
    pub openai_compatible: CredentialEdit,
    pub openrouter: CredentialEdit,
    pub lm_studio: CredentialEdit,
    pub deepl: CredentialEdit,
    pub google_cloud_translation: CredentialEdit,
    pub caiyun: CredentialEdit,
}

impl fmt::Debug for TranslationCredentialsView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TranslationCredentialsView([REDACTED])")
    }
}

impl From<&TranslationCredentials> for TranslationCredentialsView {
    fn from(credentials: &TranslationCredentials) -> Self {
        Self {
            openai: CredentialEdit::from(&credentials.openai),
            gemini: CredentialEdit::from(&credentials.gemini),
            claude: CredentialEdit::from(&credentials.claude),
            deepseek: CredentialEdit::from(&credentials.deepseek),
            openai_compatible: CredentialEdit::from(&credentials.openai_compatible),
            openrouter: CredentialEdit::from(&credentials.openrouter),
            lm_studio: CredentialEdit::from(&credentials.lm_studio),
            deepl: CredentialEdit::from(&credentials.deepl),
            google_cloud_translation: CredentialEdit::from(&credentials.google_cloud_translation),
            caiyun: CredentialEdit::from(&credentials.caiyun),
        }
    }
}

impl TranslationCredentialsView {
    fn apply(self, credentials: &mut TranslationCredentials) {
        self.openai.apply(&mut credentials.openai);
        self.gemini.apply(&mut credentials.gemini);
        self.claude.apply(&mut credentials.claude);
        self.deepseek.apply(&mut credentials.deepseek);
        self.openai_compatible
            .apply(&mut credentials.openai_compatible);
        self.openrouter.apply(&mut credentials.openrouter);
        self.lm_studio.apply(&mut credentials.lm_studio);
        self.deepl.apply(&mut credentials.deepl);
        self.google_cloud_translation
            .apply(&mut credentials.google_cloud_translation);
        self.caiyun.apply(&mut credentials.caiyun);
    }
}

#[derive(Clone, Default, Deserialize, Serialize, Type)]
pub struct CredentialEdit {
    pub configured: bool,
    pub value: Option<String>,
    pub clear: bool,
}

impl fmt::Debug for CredentialEdit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialEdit")
            .field("configured", &self.configured)
            .field("value", &self.value.as_ref().map(|_| "[REDACTED]"))
            .field("clear", &self.clear)
            .finish()
    }
}

impl From<&SecretString> for CredentialEdit {
    fn from(secret: &SecretString) -> Self {
        Self {
            configured: !secret.expose_secret().trim().is_empty(),
            value: None,
            clear: false,
        }
    }
}

impl CredentialEdit {
    fn apply(self, current: &mut SecretString) {
        if self.clear {
            *current = SecretString::default();
        } else if let Some(value) = self.value {
            *current = SecretString::from(value);
        }
    }
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct TargetLanguageView {
    pub tag: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct ProjectHeader {
    pub id: ProjectId,
    pub name: String,
    pub visible_page: Option<EntityId>,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Type)]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct PageSummary {
    pub id: EntityId,
    pub label: String,
    pub size: PageSize,
    pub source: Option<String>,
    pub clean: Option<String>,
    #[specta(type = f64)]
    pub entities: usize,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct PageView {
    pub id: EntityId,
    pub label: String,
    pub size: PageSize,
    pub assets: AssetView,
    pub entities: Vec<EntityView>,
}

#[derive(Clone, Debug, Default, Serialize, Type)]
pub struct AssetView {
    pub source: Option<String>,
    pub clean: Option<String>,
    pub rendered: Option<String>,
    pub text_mask: Option<String>,
    pub coo_mask: Option<String>,
    pub bubble_mask: Option<String>,
    pub brush_mask: Option<String>,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct EntityView {
    pub id: EntityId,
    pub parent: Option<EntityId>,
    pub geometry: Option<GeometryView>,
    pub visibility: VisibilityView,
    pub image: Option<String>,
    pub source_text: Option<SourceTextView>,
    pub translation: Option<TranslationView>,
    pub typography: Option<TypographyIntent>,
    pub region: Option<RegionView>,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct GeometryView {
    pub points: Vec<Point>,
}

#[derive(Clone, Copy, Debug, Serialize, Type)]
pub struct VisibilityView {
    pub visible: bool,
    pub opacity: f32,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct SourceTextView {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct TranslationView {
    pub locale: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct RegionView {
    pub kind: String,
    pub label: Option<String>,
}

#[derive(Debug, Serialize, Type)]
pub struct ProjectDelta {
    pub from: Revision,
    pub revision: Revision,
    pub name: String,
    pub page_order: Vec<EntityId>,
    pub pages: Vec<PageSummary>,
    pub deleted_pages: Vec<EntityId>,
    pub visible_page: Option<PageView>,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HitTarget {
    Element { element: EntityId },
    Handle { element: EntityId, handle: Handle },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Handle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    Rotate,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobStatus {
    Running {
        id: RequestId,
        kind: JobKind,
        #[specta(type = f64)]
        completed: usize,
        #[specta(type = f64)]
        total: usize,
        page: Option<EntityId>,
        phase: Option<koharu_pipeline::Stage>,
        model: Option<String>,
    },
    Finished {
        id: RequestId,
    },
    Failed {
        id: RequestId,
        error: String,
    },
    Stopped {
        id: RequestId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Pipeline,
    Import,
    Export,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DownloadStatus {
    Running {
        #[specta(type = f64)]
        id: u64,
        name: String,
        #[specta(type = f64)]
        completed: u64,
        #[specta(type = f64)]
        total: u64,
    },
    Finished {
        #[specta(type = f64)]
        id: u64,
    },
    Failed {
        #[specta(type = f64)]
        id: u64,
        name: String,
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_protocol_is_explicitly_tagged() {
        let message: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type": "command",
            "id": "018f3b28-7fd8-7d5a-a833-6cb8637e6c00",
            "base": 0,
            "command": { "type": "create_project" }
        }))
        .unwrap();
        assert!(matches!(
            message,
            BridgeMessage::Command {
                id,
                command,
                ..
            } if id == RequestId::from(Uuid::parse_str("018f3b28-7fd8-7d5a-a833-6cb8637e6c00").unwrap())
                && matches!(*command, UiCommand::CreateProject)
        ));
    }

    #[test]
    fn translation_commands_require_an_explicit_locale() {
        let message: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type": "command",
            "id": "018f3b28-7fd8-7d5a-a833-6cb8637e6c00",
            "base": 1,
            "command": {
                "type": "set_translation",
                "entity": "018f3b28-7fd8-7d5a-a833-6cb8637e6c01",
                "locale": "ar-EG",
                "text": "مرحبا"
            }
        }))
        .unwrap();
        assert!(matches!(
            message,
            BridgeMessage::Command {
                command,
                ..
            } if matches!(*command, UiCommand::SetTranslation { ref locale, .. } if locale == "ar-EG")
        ));
    }

    #[test]
    fn source_text_corrections_are_explicit_commands() {
        let message: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type": "command",
            "id": "018f3b28-7fd8-7d5a-a833-6cb8637e6c00",
            "base": 1,
            "command": {
                "type": "set_source_text",
                "entity": "018f3b28-7fd8-7d5a-a833-6cb8637e6c01",
                "text": "corrected"
            }
        }))
        .unwrap();
        assert!(matches!(
            message,
            BridgeMessage::Command {
                command,
                ..
            } if matches!(*command, UiCommand::SetSourceText { ref text, .. } if text == "corrected")
        ));
    }

    #[test]
    fn interaction_has_a_nested_canvas_tag() {
        let message: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type": "interaction",
            "interaction": { "type": "fit_window" }
        }))
        .unwrap();
        assert!(matches!(
            message,
            BridgeMessage::Interaction {
                interaction: CanvasInteraction::FitWindow
            }
        ));
    }

    #[test]
    fn download_progress_is_flat_for_typescript() {
        let value = serde_json::to_value(UiEvent::DownloadChanged(DownloadStatus::Running {
            id: 7,
            name: "model.bin".into(),
            completed: 25,
            total: 100,
        }))
        .unwrap();
        assert_eq!(value["type"], "download_changed");
        assert_eq!(value["state"], "running");
        assert_eq!(value["completed"], 25);
    }

    #[test]
    fn settings_projection_never_exposes_stored_credentials() {
        let config = TranslationConfig::default();
        let translation = TranslationSettings {
            model: config.model,
            target_language: config.target_language,
            instructions: config.instructions,
            credentials: TranslationCredentialsView {
                openai: CredentialEdit {
                    configured: true,
                    value: None,
                    clear: false,
                },
                ..TranslationCredentialsView::default()
            },
        };
        let value = serde_json::to_value(UiEvent::SettingsChanged {
            settings: Box::new(SettingsView {
                pipeline: koharu_pipeline::PipelineConfig::default(),
                translation,
                local_translation_models: Vec::new(),
                target_languages: Vec::new(),
                fonts: Vec::new(),
            }),
        })
        .unwrap();
        let credential = &value["settings"]["translation"]["credentials"]["openai"];
        assert_eq!(credential["configured"], true);
        assert!(credential["value"].is_null());
    }

    #[test]
    fn credential_edits_distinguish_keep_set_and_clear() {
        let mut secret = SecretString::from("stored");
        CredentialEdit::default().apply(&mut secret);
        assert_eq!(secret.expose_secret(), "stored");
        CredentialEdit {
            configured: true,
            value: Some("replacement".into()),
            clear: false,
        }
        .apply(&mut secret);
        assert_eq!(secret.expose_secret(), "replacement");
        CredentialEdit {
            configured: true,
            value: Some("ignored".into()),
            clear: true,
        }
        .apply(&mut secret);
        assert!(secret.expose_secret().is_empty());
    }
}
