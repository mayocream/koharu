use std::fmt;

use koharu_pipeline::{Force, RunTarget, Scope};
use koharu_scene::{
    Element, ElementId, Frame, Page, PageAssets, PageId, ProjectId, Revision, Size, TextLayout,
    TextStyle,
};
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
        command: UiCommand,
    },
    Interaction {
        interaction: CanvasInteraction,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WindowAction {
    Drag,
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
        page: PageId,
        name: String,
    },
    DeletePage {
        page: PageId,
    },
    DeletePages {
        pages: Vec<PageId>,
    },
    MovePage {
        page: PageId,
        #[specta(type = f64)]
        index: usize,
    },
    AddText {
        page: PageId,
        frame: Frame,
    },
    SetTranslation {
        page: PageId,
        element: ElementId,
        translation: Option<String>,
    },
    SetTextStyle {
        page: PageId,
        element: ElementId,
        style: TextStyle,
    },
    SetTextLayout {
        page: PageId,
        element: ElementId,
        layout: TextLayout,
    },
    SetTextStyles {
        page: PageId,
        elements: Vec<ElementTextStyle>,
    },
    SetTextLayouts {
        page: PageId,
        elements: Vec<ElementTextLayout>,
    },
    SetElementFrames {
        elements: Vec<ElementFrame>,
    },
    SetElementOpacity {
        page: PageId,
        elements: Vec<ElementId>,
        opacity: f32,
    },
    SetElementVisibility {
        page: PageId,
        elements: Vec<ElementId>,
        visible: bool,
    },
    DeleteElements {
        page: PageId,
        elements: Vec<ElementId>,
    },
    MoveElement {
        page: PageId,
        element: ElementId,
        #[specta(type = f64)]
        index: usize,
    },
    Undo,
    Redo,
    RunPipeline {
        scope: Scope,
        target: RunTarget,
        force: Force,
    },
    CancelJob {
        job: RequestId,
    },
    ExportPages {
        pages: Vec<PageId>,
        format: ExportFormat,
    },
    GetSettings,
    SetSettings {
        pipeline: koharu_pipeline::PipelineConfig,
        translation: TranslationSettings,
    },
    CollectGarbage,
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
pub struct ElementFrame {
    pub page: PageId,
    pub element: ElementId,
    pub frame: Frame,
}

#[derive(Clone, Debug, Deserialize, Type)]
pub struct ElementTextStyle {
    pub element: ElementId,
    pub style: TextStyle,
}

#[derive(Clone, Debug, Deserialize, Type)]
pub struct ElementTextLayout {
    pub element: ElementId,
    pub layout: TextLayout,
}

#[derive(Debug, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CanvasInteraction {
    ShowPage {
        page: PageId,
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
        selected: Vec<ElementId>,
        hovered: Option<ElementId>,
        previews: Vec<ElementPreview>,
        draft: Option<Frame>,
        guides: Vec<CanvasGuide>,
        show_text_bounds: bool,
        brush_cursor: Option<CanvasBrushCursor>,
    },
    HitTest {
        #[specta(type = f64)]
        id: u64,
        x: f64,
        y: f64,
    },
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

#[derive(Clone, Copy, Debug, Deserialize, Type)]
pub struct ElementPreview {
    pub element: ElementId,
    pub frame: Frame,
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
    ProjectChanged(ProjectDelta),
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
        settings: SettingsView,
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

    pub fn into_parts(self) -> (TranslationConfig, TranslationCredentials) {
        (
            TranslationConfig {
                model: self.model,
                target_language: self.target_language,
                instructions: self.instructions,
            },
            self.credentials.into(),
        )
    }
}

#[derive(Clone, Default, Deserialize, Serialize, Type)]
pub struct TranslationCredentialsView {
    pub openai: String,
    pub gemini: String,
    pub claude: String,
    pub deepseek: String,
    pub openai_compatible: String,
    pub openrouter: String,
    pub lm_studio: String,
    pub deepl: String,
    pub google_cloud_translation: String,
    pub caiyun: String,
}

impl fmt::Debug for TranslationCredentialsView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TranslationCredentialsView([REDACTED])")
    }
}

impl From<&TranslationCredentials> for TranslationCredentialsView {
    fn from(credentials: &TranslationCredentials) -> Self {
        Self {
            openai: exposed(&credentials.openai),
            gemini: exposed(&credentials.gemini),
            claude: exposed(&credentials.claude),
            deepseek: exposed(&credentials.deepseek),
            openai_compatible: exposed(&credentials.openai_compatible),
            openrouter: exposed(&credentials.openrouter),
            lm_studio: exposed(&credentials.lm_studio),
            deepl: exposed(&credentials.deepl),
            google_cloud_translation: exposed(&credentials.google_cloud_translation),
            caiyun: exposed(&credentials.caiyun),
        }
    }
}

impl From<TranslationCredentialsView> for TranslationCredentials {
    fn from(credentials: TranslationCredentialsView) -> Self {
        Self {
            openai: SecretString::from(credentials.openai),
            gemini: SecretString::from(credentials.gemini),
            claude: SecretString::from(credentials.claude),
            deepseek: SecretString::from(credentials.deepseek),
            openai_compatible: SecretString::from(credentials.openai_compatible),
            openrouter: SecretString::from(credentials.openrouter),
            lm_studio: SecretString::from(credentials.lm_studio),
            deepl: SecretString::from(credentials.deepl),
            google_cloud_translation: SecretString::from(credentials.google_cloud_translation),
            caiyun: SecretString::from(credentials.caiyun),
        }
    }
}

fn exposed(secret: &SecretString) -> String {
    secret.expose_secret().to_owned()
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct TargetLanguageView {
    pub tag: String,
    pub name: String,
}

#[derive(Debug, Serialize, Type)]
pub struct ProjectHeader {
    pub id: ProjectId,
    pub name: String,
    pub visible_page: Option<PageId>,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct PageSummary {
    pub id: PageId,
    pub name: String,
    pub size: Size,
    pub source: String,
    pub clean: Option<String>,
    #[specta(type = f64)]
    pub elements: usize,
}

#[derive(Debug, Serialize, Type)]
pub struct PageView {
    pub id: PageId,
    pub name: String,
    pub size: Size,
    pub source: String,
    pub assets: AssetView,
    pub elements: Vec<Element>,
}

#[derive(Debug, Serialize, Type)]
pub struct AssetView {
    pub clean: Option<String>,
    pub rendered: Option<String>,
    pub text_mask: Option<String>,
    pub coo_mask: Option<String>,
    pub bubble_mask: Option<String>,
    pub brush_mask: Option<String>,
}

#[derive(Debug, Serialize, Type)]
pub struct ProjectDelta {
    pub from: Revision,
    pub revision: Revision,
    pub name: String,
    pub page_order: Vec<PageId>,
    pub pages: Vec<PageSummary>,
    pub deleted_pages: Vec<PageId>,
    pub visible_page: Option<PageDelta>,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Debug, Serialize, Type)]
pub struct PageDelta {
    pub id: PageId,
    pub name: String,
    pub size: Size,
    pub source: String,
    pub assets: AssetView,
    pub element_order: Vec<ElementId>,
    pub elements: Vec<Element>,
    pub deleted_elements: Vec<ElementId>,
}

#[derive(Clone, Copy, Debug, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HitTarget {
    Element { element: ElementId },
    Handle { element: ElementId, handle: Handle },
}

#[derive(Clone, Copy, Debug, Serialize, Type)]
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
        phase: Option<koharu_pipeline::Phase>,
        model: Option<String>,
    },
    Finished {
        id: RequestId,
    },
    Failed {
        id: RequestId,
        error: String,
    },
    Cancelled {
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

impl PageSummary {
    #[must_use]
    pub fn from_page(page: &Page) -> Self {
        Self {
            id: page.id,
            name: page.name.clone(),
            size: page.size,
            source: page.source.to_string(),
            clean: page.assets.clean.map(|blob| blob.to_string()),
            elements: page.elements.len(),
        }
    }
}

impl PageView {
    #[must_use]
    pub fn from_page(page: &Page) -> Self {
        Self {
            id: page.id,
            name: page.name.clone(),
            size: page.size,
            source: page.source.to_string(),
            assets: AssetView::from(&page.assets),
            elements: page.elements.clone(),
        }
    }
}

impl From<&PageAssets> for AssetView {
    fn from(assets: &PageAssets) -> Self {
        Self {
            clean: assets.clean.map(|blob| blob.to_string()),
            rendered: assets.rendered.map(|blob| blob.to_string()),
            text_mask: assets.text_mask.map(|blob| blob.to_string()),
            coo_mask: assets.coo_mask.map(|blob| blob.to_string()),
            bubble_mask: assets.bubble_mask.map(|blob| blob.to_string()),
            brush_mask: assets.brush_mask.map(|blob| blob.to_string()),
        }
    }
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
                command: UiCommand::CreateProject,
                ..
            } if id == RequestId::from(Uuid::parse_str("018f3b28-7fd8-7d5a-a833-6cb8637e6c00").unwrap())
        ));
    }

    #[test]
    fn page_projection_does_not_embed_blob_bytes() {
        let event = UiEvent::ProjectClosed;
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({ "type": "project_closed" })
        );
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
    fn project_delta_is_flat_for_typescript() {
        let event = UiEvent::ProjectChanged(ProjectDelta {
            from: Revision::ZERO,
            revision: Revision::new(1),
            name: "Book".into(),
            page_order: Vec::new(),
            pages: Vec::new(),
            deleted_pages: Vec::new(),
            visible_page: None,
            can_undo: true,
            can_redo: false,
        });
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "project_changed");
        assert_eq!(value["revision"], 1);
        assert_eq!(value["name"], "Book");
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
        assert_eq!(value["name"], "model.bin");
        assert_eq!(value["completed"], 25);
        assert_eq!(value["total"], 100);
    }

    #[test]
    fn settings_projection_exposes_credentials_for_editing() {
        let config = TranslationConfig::default();
        let translation = TranslationSettings {
            model: config.model,
            target_language: config.target_language,
            instructions: config.instructions,
            credentials: TranslationCredentialsView {
                openai: "secret-value".to_owned(),
                ..TranslationCredentialsView::default()
            },
        };
        let value = serde_json::to_value(UiEvent::SettingsChanged {
            settings: SettingsView {
                pipeline: koharu_pipeline::PipelineConfig::default(),
                translation,
                local_translation_models: vec!["lfm2.5-1.2b-instruct".into()],
                target_languages: vec![TargetLanguageView {
                    tag: "en-US".into(),
                    name: "English".into(),
                }],
            },
        })
        .unwrap();
        assert_eq!(
            value["settings"]["translation"]["credentials"]["openai"],
            "secret-value"
        );
    }
}
