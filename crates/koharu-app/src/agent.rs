use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
};

use anyhow::{Context as _, Result, anyhow, bail};
use async_trait::async_trait;
use koharu_agent::{Agent, Codex, Control, Host, Invocation, RunId, Tool, ToolCall};
use koharu_pipeline::{Committer, Operation, RunStatus, Scope, Stage, StageOutput, StopToken};
use koharu_protocol::{
    AgentStatus, AppEvent, CanvasFrame, GeometryUpdate, Point, Typography, TypographyUpdate,
};
use koharu_scene::{Commit, EntityId, Snapshot};
use parking_lot::Mutex;
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{Mutex as AsyncMutex, Notify};

use crate::{
    PageRenderer, ProcessingRuntime, ViewDisposition, event_hub::EventHub,
    presentation_coordinator::PresentationCoordinator, project::Project,
};

#[derive(Clone)]
pub(crate) struct KoharuHost {
    project: Arc<AsyncMutex<Option<Project>>>,
    renderer: Arc<dyn PageRenderer>,
    presentation_coordinator: PresentationCoordinator,
    processing: Arc<dyn ProcessingRuntime>,
    stops: Arc<Mutex<HashMap<koharu_protocol::JobId, StopToken>>>,
}

impl KoharuHost {
    pub(crate) fn new(
        project: Arc<AsyncMutex<Option<Project>>>,
        renderer: Arc<dyn PageRenderer>,
        presentation_coordinator: PresentationCoordinator,
        processing: Arc<dyn ProcessingRuntime>,
        stops: Arc<Mutex<HashMap<koharu_protocol::JobId, StopToken>>>,
    ) -> Self {
        Self {
            project,
            renderer,
            presentation_coordinator,
            processing,
            stops,
        }
    }

    async fn project_context(&self) -> Result<serde_json::Value> {
        let (project, pages) = {
            let current = self.project.lock().await;
            let project = current.as_ref().context("no project is open")?;
            let snapshot = project.snapshot();
            let pages = Project::pages(&snapshot)?
                .into_iter()
                .map(|page| Project::page(&snapshot, page.id))
                .collect::<Result<Vec<_>>>()?;
            (project.info(), pages)
        };
        let fonts = self
            .renderer
            .available_fonts()
            .await?
            .into_iter()
            .map(|font| font.name)
            .collect::<Vec<_>>();
        Ok(json!({
            "project": project,
            "pages": pages,
            "available_fonts": fonts,
        }))
    }

    async fn mutate<T>(
        &self,
        mutation: impl for<'project> FnOnce(
            &'project mut Project,
        ) -> Pin<
            Box<dyn Future<Output = Result<(Commit, T)>> + Send + 'project>,
        >,
    ) -> Result<Invocation>
    where
        T: serde::Serialize + Send,
    {
        let (commit, value, project) = {
            let mut current = self.project.lock().await;
            let project = current.as_mut().context("no project is open")?;
            let (commit, value) = mutation(project).await?;
            project.record_commit(&commit);
            project.reconcile_page();
            (commit, value, project.info())
        };
        let revision = commit.revision;
        self.presentation_coordinator
            .synchronize(ViewDisposition::Preserve, true)
            .await?;
        Invocation::changed(json!({
            "revision": revision,
            "project": project,
            "result": value,
        }))
    }

    async fn run_pipeline(&self, arguments: RunPipeline, control: &Control) -> Result<Invocation> {
        let scope = arguments.scope()?;
        let operation = arguments.operation.into();
        let snapshot = self
            .project
            .lock()
            .await
            .as_ref()
            .context("no project is open")?
            .snapshot();
        let job = koharu_protocol::JobId::new();
        let stop = StopToken::default();
        {
            let mut stops = self.stops.lock();
            if !stops.is_empty() {
                bail!("another pipeline process is already running");
            }
            stops.insert(job, stop.clone());
        }
        let watcher = tokio::spawn({
            let control = control.clone();
            let stop = stop.clone();
            async move {
                control.cancelled().await;
                stop.stop();
            }
        });
        let mut committer = AgentCommitter { host: self.clone() };
        let request = koharu_pipeline::Request {
            operation,
            scope,
            stop,
            progress: None,
            inpainting_mask: None,
        };
        let result = self
            .processing
            .execute(snapshot, request, &mut committer)
            .await;
        watcher.abort();
        self.stops.lock().remove(&job);
        let report = result?;
        if report.status == RunStatus::Stopped {
            bail!("pipeline processing was cancelled");
        }
        Invocation::changed(json!({
            "base_revision": report.base,
            "final_revision": report.final_revision,
            "completed": report.completed,
            "total": report.total,
            "elapsed_ms": report.elapsed.as_millis(),
        }))
    }
}

#[async_trait]
impl Host for KoharuHost {
    async fn context(&self) -> Result<serde_json::Value> {
        self.project_context().await
    }

    fn tools(&self) -> Vec<Tool> {
        static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
        TOOLS
            .get_or_init(|| {
                vec![
                    definition::<InspectProject>(
                        "inspect_project",
                        "Read the latest complete semantic project state after edits.",
                    ),
                    definition::<ViewPage>("view_page", "Render and inspect one page image."),
                    definition::<RenamePage>("rename_page", "Rename a project page."),
                    definition::<MovePage>(
                        "move_page",
                        "Move a page to a zero-based project index.",
                    ),
                    definition::<DeletePages>("delete_pages", "Delete pages and their contents."),
                    definition::<AddTextBox>("add_text_box", "Add a paragraph text box to a page."),
                    definition::<SetText>("set_source_text", "Replace an element's source text."),
                    definition::<SetTranslation>(
                        "set_translation",
                        "Set an element's translation, or pass null to remove it.",
                    ),
                    definition::<SetTypography>(
                        "set_typography",
                        "Replace an element's typography settings.",
                    ),
                    definition::<SetGeometry>(
                        "set_geometry",
                        "Replace an element's page-space polygon geometry.",
                    ),
                    definition::<SetVisibility>(
                        "set_visibility",
                        "Change visibility and/or opacity for elements.",
                    ),
                    definition::<DeleteElements>(
                        "delete_elements",
                        "Delete elements and descendants.",
                    ),
                    definition::<MoveElement>(
                        "move_element",
                        "Move an element under a page or element at a zero-based index.",
                    ),
                    definition::<RunPipeline>(
                        "run_pipeline",
                        "Run Koharu's configured processing pipeline.",
                    ),
                ]
            })
            .clone()
    }

    async fn invoke(&self, call: ToolCall, control: &Control) -> Result<Invocation> {
        match call.name.as_str() {
            "inspect_project" => {
                let _: InspectProject = arguments(&call)?;
                Invocation::read(self.project_context().await?)
            }
            "view_page" => {
                let arguments: ViewPage = arguments(&call)?;
                let page = entity(&arguments.page)?;
                let (label, snapshot) = {
                    let current = self.project.lock().await;
                    let project = current.as_ref().context("no project is open")?;
                    let snapshot = project.snapshot();
                    let label = snapshot.page(page)?.page()?.label;
                    (label, snapshot)
                };
                let bytes = rendered_preview(&*self.renderer, &snapshot, page).await?;
                use base64::Engine as _;
                Ok(
                    Invocation::read(json!({ "page": page, "label": label }))?.with_image(
                        format!("Rendered page {label} ({page})"),
                        format!(
                            "data:image/webp;base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(bytes)
                        ),
                    ),
                )
            }
            "rename_page" => {
                let arguments: RenamePage = arguments(&call)?;
                let page = entity(&arguments.page)?;
                self.mutate(|project| {
                    Box::pin(async move {
                        Ok((
                            project.rename_page(page, arguments.label).await?,
                            json!({ "page": page }),
                        ))
                    })
                })
                .await
            }
            "move_page" => {
                let arguments: MovePage = arguments(&call)?;
                let page = entity(&arguments.page)?;
                self.mutate(|project| {
                    Box::pin(async move {
                        Ok((
                            project.move_page(page, arguments.index).await?,
                            json!({ "page": page }),
                        ))
                    })
                })
                .await
            }
            "delete_pages" => {
                let arguments: DeletePages = arguments(&call)?;
                let pages = entities(&arguments.pages)?;
                self.mutate(|project| {
                    Box::pin(async move {
                        Ok((
                            project.delete_pages(pages.clone()).await?,
                            json!({ "pages": pages }),
                        ))
                    })
                })
                .await
            }
            "add_text_box" => {
                let arguments: AddTextBox = arguments(&call)?;
                let page = entity(&arguments.page)?;
                let frame = CanvasFrame {
                    x: arguments.x,
                    y: arguments.y,
                    width: arguments.width,
                    height: arguments.height,
                    angle_degrees: arguments.angle_degrees,
                };
                self.mutate(|project| {
                    Box::pin(async move {
                        let (commit, element) = project.add_text_box(page, frame).await?;
                        Ok((commit, json!({ "page": page, "element": element })))
                    })
                })
                .await
            }
            "set_source_text" => {
                let arguments: SetText = arguments(&call)?;
                let element = entity(&arguments.element)?;
                self.mutate(|project| {
                    Box::pin(async move {
                        Ok((
                            project.set_source_text(element, arguments.text).await?,
                            json!({ "element": element }),
                        ))
                    })
                })
                .await
            }
            "set_translation" => {
                let arguments: SetTranslation = arguments(&call)?;
                let element = entity(&arguments.element)?;
                self.mutate(|project| {
                    Box::pin(async move {
                        Ok((
                            project.set_translation(element, arguments.text).await?,
                            json!({ "element": element }),
                        ))
                    })
                })
                .await
            }
            "set_typography" => {
                let arguments: SetTypography = arguments(&call)?;
                let element = entity(&arguments.element)?;
                let typography = Typography {
                    preferred_font: arguments.preferred_font,
                    font_weight: arguments.font_weight,
                    font_style: arguments.font_style.map(Into::into),
                    size: arguments.size,
                    auto_fit: arguments.auto_fit,
                    color: arguments.color,
                    stroke_color: arguments.stroke_color,
                    stroke_width: arguments.stroke_width,
                    alignment: arguments.alignment.map(Into::into),
                    writing_mode: arguments.writing_mode.map(Into::into),
                };
                self.mutate(|project| {
                    Box::pin(async move {
                        Ok((
                            project
                                .set_typography(vec![TypographyUpdate {
                                    layer: element,
                                    typography,
                                }])
                                .await?,
                            json!({ "element": element }),
                        ))
                    })
                })
                .await
            }
            "set_geometry" => {
                let arguments: SetGeometry = arguments(&call)?;
                let element = entity(&arguments.element)?;
                let points = arguments
                    .points
                    .into_iter()
                    .map(|point| Point {
                        x: point.x,
                        y: point.y,
                    })
                    .collect();
                self.mutate(|project| {
                    Box::pin(async move {
                        Ok((
                            project
                                .set_geometry(vec![GeometryUpdate {
                                    layer: element,
                                    points: Some(points),
                                }])
                                .await?,
                            json!({ "element": element }),
                        ))
                    })
                })
                .await
            }
            "set_visibility" => {
                let arguments: SetVisibility = arguments(&call)?;
                let elements = entities(&arguments.elements)?;
                self.mutate(|project| {
                    Box::pin(async move {
                        Ok((
                            project
                                .set_visibility(
                                    elements.clone(),
                                    arguments.visible,
                                    arguments.opacity,
                                )
                                .await?,
                            json!({ "elements": elements }),
                        ))
                    })
                })
                .await
            }
            "delete_elements" => {
                let arguments: DeleteElements = arguments(&call)?;
                let elements = entities(&arguments.elements)?;
                self.mutate(|project| {
                    Box::pin(async move {
                        Ok((
                            project.delete_layers(elements.clone()).await?,
                            json!({ "elements": elements }),
                        ))
                    })
                })
                .await
            }
            "move_element" => {
                let arguments: MoveElement = arguments(&call)?;
                let element = entity(&arguments.element)?;
                let parent = entity(&arguments.parent)?;
                self.mutate(|project| {
                    Box::pin(async move {
                        Ok((
                            project.move_layer(element, parent, arguments.index).await?,
                            json!({ "element": element, "parent": parent }),
                        ))
                    })
                })
                .await
            }
            "run_pipeline" => self.run_pipeline(arguments(&call)?, control).await,
            name => bail!("unknown Koharu tool {name}"),
        }
    }
}

struct AgentCommitter {
    host: KoharuHost,
}

#[async_trait]
impl Committer for AgentCommitter {
    async fn commit(&mut self, output: StageOutput) -> Result<Snapshot> {
        let commit = {
            let mut current = self.host.project.lock().await;
            let project = current.as_mut().context("no project is open")?;
            let commit = project.session.commit(output.patch).await?;
            project.record_commit(&commit);
            commit
        };
        self.host
            .presentation_coordinator
            .synchronize(ViewDisposition::Preserve, true)
            .await?;
        Ok(commit.snapshot)
    }
}

async fn rendered_preview(
    renderer: &dyn PageRenderer,
    snapshot: &Snapshot,
    page: EntityId,
) -> Result<Vec<u8>> {
    let frame = renderer.render(snapshot, page).await?;
    let image = renderer.rasterize(&frame).await?;
    tokio::task::spawn_blocking(move || {
        use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
        let longest = image.width().max(image.height());
        let (width, height) = if longest > 1024 {
            (
                (u64::from(image.width()) * 1024 / u64::from(longest)).max(1) as u32,
                (u64::from(image.height()) * 1024 / u64::from(longest)).max(1) as u32,
            )
        } else {
            (image.width(), image.height())
        };
        let resized = if (width, height) == image.dimensions() {
            image
        } else {
            let mut resized = image::RgbaImage::new(width, height);
            Resizer::new().resize(
                &image,
                &mut resized,
                &ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
                    .use_alpha(true),
            )?;
            resized
        };
        Ok::<_, anyhow::Error>(
            webp::Encoder::from_rgba(resized.as_raw(), resized.width(), resized.height())
                .encode(85.0)
                .to_vec(),
        )
    })
    .await
    .context("preview encode worker stopped unexpectedly")?
}

fn definition<T: JsonSchema>(name: &str, description: &str) -> Tool {
    Tool::new(
        name,
        description,
        serde_json::to_value(schema_for!(T)).expect("tool argument schema must serialize"),
    )
}

fn arguments<T: for<'de> Deserialize<'de>>(call: &ToolCall) -> Result<T> {
    serde_json::from_str(&call.arguments)
        .with_context(|| format!("invalid arguments for {}", call.name))
}

fn entity(value: &str) -> Result<EntityId> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .with_context(|| format!("invalid entity ID {value}"))
}

fn entities(values: &[String]) -> Result<Vec<EntityId>> {
    values.iter().map(|value| entity(value)).collect()
}

#[derive(Deserialize, JsonSchema)]
struct InspectProject {}
#[derive(Deserialize, JsonSchema)]
struct ViewPage {
    page: String,
}
#[derive(Deserialize, JsonSchema)]
struct RenamePage {
    page: String,
    label: String,
}
#[derive(Deserialize, JsonSchema)]
struct MovePage {
    page: String,
    index: usize,
}
#[derive(Deserialize, JsonSchema)]
struct DeletePages {
    pages: Vec<String>,
}
#[derive(Deserialize, JsonSchema)]
struct AddTextBox {
    page: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    #[serde(default)]
    angle_degrees: f32,
}
#[derive(Deserialize, JsonSchema)]
struct SetText {
    element: String,
    text: String,
}
#[derive(Deserialize, JsonSchema)]
struct SetTranslation {
    element: String,
    text: Option<String>,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AgentTextAlignment {
    Start,
    Center,
    End,
    Justify,
}
impl From<AgentTextAlignment> for koharu_scene::TextAlignment {
    fn from(value: AgentTextAlignment) -> Self {
        match value {
            AgentTextAlignment::Start => Self::Start,
            AgentTextAlignment::Center => Self::Center,
            AgentTextAlignment::End => Self::End,
            AgentTextAlignment::Justify => Self::Justify,
        }
    }
}
#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AgentWritingMode {
    Horizontal,
    Vertical,
}
impl From<AgentWritingMode> for koharu_scene::WritingMode {
    fn from(value: AgentWritingMode) -> Self {
        match value {
            AgentWritingMode::Horizontal => Self::Horizontal,
            AgentWritingMode::Vertical => Self::Vertical,
        }
    }
}
#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AgentFontStyle {
    Normal,
    Italic,
    Oblique,
}
impl From<AgentFontStyle> for koharu_scene::FontStyle {
    fn from(value: AgentFontStyle) -> Self {
        match value {
            AgentFontStyle::Normal => Self::Normal,
            AgentFontStyle::Italic => Self::Italic,
            AgentFontStyle::Oblique => Self::Oblique,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct SetTypography {
    element: String,
    preferred_font: Option<String>,
    font_weight: Option<u16>,
    font_style: Option<AgentFontStyle>,
    size: Option<f32>,
    auto_fit: bool,
    color: Option<[u8; 4]>,
    stroke_color: Option<[u8; 4]>,
    stroke_width: Option<f32>,
    alignment: Option<AgentTextAlignment>,
    writing_mode: Option<AgentWritingMode>,
}
#[derive(Deserialize, JsonSchema)]
struct SetGeometry {
    element: String,
    points: Vec<AgentPoint>,
}
#[derive(Deserialize, JsonSchema)]
struct AgentPoint {
    x: f64,
    y: f64,
}
#[derive(Deserialize, JsonSchema)]
struct SetVisibility {
    elements: Vec<String>,
    visible: Option<bool>,
    opacity: Option<f32>,
}
#[derive(Deserialize, JsonSchema)]
struct DeleteElements {
    elements: Vec<String>,
}
#[derive(Deserialize, JsonSchema)]
struct MoveElement {
    element: String,
    parent: String,
    index: usize,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AgentPipelineOperation {
    Full,
    Detection,
    Ocr,
    Translation,
    Inpainting,
}
impl From<AgentPipelineOperation> for Operation {
    fn from(value: AgentPipelineOperation) -> Self {
        match value {
            AgentPipelineOperation::Full => Self::Full,
            AgentPipelineOperation::Detection => Self::Only {
                stage: Stage::Detection,
            },
            AgentPipelineOperation::Ocr => Self::Only { stage: Stage::Ocr },
            AgentPipelineOperation::Translation => Self::Only {
                stage: Stage::Translation,
            },
            AgentPipelineOperation::Inpainting => Self::Only {
                stage: Stage::Inpainting,
            },
        }
    }
}
#[derive(Deserialize, JsonSchema)]
struct RunPipeline {
    operation: AgentPipelineOperation,
    #[serde(default)]
    pages: Vec<String>,
    #[serde(default)]
    elements: Vec<String>,
}
impl RunPipeline {
    fn scope(&self) -> Result<Scope> {
        match (self.pages.is_empty(), self.elements.is_empty()) {
            (true, true) => Ok(Scope::Project),
            (false, true) => Ok(Scope::Pages(entities(&self.pages)?)),
            (true, false) => Ok(Scope::Entities(entities(&self.elements)?)),
            (false, false) => bail!("pipeline scope cannot contain both pages and elements"),
        }
    }
}

pub(crate) struct AgentState {
    agent: Arc<Agent<KoharuHost>>,
    runs: Mutex<HashMap<RunId, Control>>,
    login: Mutex<Option<Control>>,
    idle: Notify,
    events: EventHub,
}

impl AgentState {
    pub(crate) fn new(host: KoharuHost, events: EventHub) -> Result<Self> {
        Ok(Self {
            agent: Arc::new(Agent::new(Codex::new()?, host)?),
            runs: Mutex::new(HashMap::new()),
            login: Mutex::new(None),
            idle: Notify::new(),
            events,
        })
    }

    pub(crate) async fn status(&self) -> Result<AgentStatus> {
        let account = self.agent.codex().account()?;
        let models = if account.is_some() {
            self.agent.models().await?
        } else {
            Vec::new()
        };
        Ok(AgentStatus {
            account,
            models,
            config: self.agent.config()?,
            running: self.runs.lock().keys().next().copied(),
        })
    }

    pub(crate) async fn login(&self) -> Result<AgentStatus> {
        let control = Control::default();
        {
            let mut login = self.login.lock();
            if login.is_some() {
                return Err(anyhow!("Codex sign-in is already running"));
            }
            *login = Some(control.clone());
        }
        let events = self.events.clone();
        let result = self
            .agent
            .codex()
            .login_device(&control, move |event| {
                events.publish(AppEvent::AgentLogin { event });
            })
            .await;
        self.login.lock().take();
        self.idle.notify_waiters();
        result?;
        self.status().await
    }

    pub(crate) async fn logout(&self) -> Result<AgentStatus> {
        self.reset().await;
        self.agent.codex().logout()?;
        self.status().await
    }

    pub(crate) fn save_config(&self, config: koharu_agent::Config) -> Result<koharu_agent::Config> {
        self.agent.save_config(config)
    }

    pub(crate) fn run(self: &Arc<Self>, prompt: String) -> Result<RunId> {
        let prompt = prompt.trim().to_owned();
        if prompt.is_empty() {
            return Err(anyhow!("message cannot be empty"));
        }
        if self.agent.codex().account()?.is_none() {
            return Err(anyhow!("Codex is not signed in"));
        }
        let run = RunId::new();
        let control = Control::default();
        {
            let mut runs = self.runs.lock();
            if !runs.is_empty() {
                return Err(anyhow!("another agent request is already running"));
            }
            runs.insert(run, control.clone());
        }
        let state = Arc::clone(self);
        tokio::spawn(async move {
            let events = state.events.clone();
            let result = state
                .agent
                .run(run, prompt, control, move |event| {
                    events.publish(AppEvent::AgentRun { event });
                })
                .await;
            if let Err(error) = result {
                tracing::error!(%run, error = ?error, "agent request failed");
            }
            state.runs.lock().remove(&run);
            state.idle.notify_waiters();
        });
        Ok(run)
    }

    pub(crate) fn cancel(&self, run: RunId) -> Result<()> {
        self.runs
            .lock()
            .get(&run)
            .with_context(|| format!("agent run {run} is not active"))?
            .cancel();
        Ok(())
    }

    pub(crate) async fn reset(&self) {
        if let Some(login) = self.login.lock().as_ref() {
            login.cancel();
        }
        for control in self.runs.lock().values() {
            control.cancel();
        }
        loop {
            let idle = self.idle.notified();
            if self.login.lock().is_none() && self.runs.lock().is_empty() {
                break;
            }
            idle.await;
        }
        self.agent.clear().await;
    }

    pub(crate) fn cancel_all(&self) {
        if let Some(login) = self.login.lock().take() {
            login.cancel();
        }
        for control in self.runs.lock().drain().map(|(_, control)| control) {
            control.cancel();
        }
    }
}
