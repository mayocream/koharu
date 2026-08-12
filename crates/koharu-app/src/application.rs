use std::{
    collections::HashMap,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Result};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use futures::{StreamExt as _, TryStreamExt as _};
use image::ImageFormat;
use koharu_protocol::*;
use koharu_scene::{AssetInput, AssetMetadata, AssetRole, At, EntityId, PageDraft};
use koharu_secrets::ExposeSecret as _;
use rayon::prelude::*;
use tokio::sync::{Mutex, RwLock, Semaphore};
use walkdir::WalkDir;

use crate::agent::{AgentState, KoharuHost};
use crate::{
    CanvasOperation, CanvasOutput, DialogFilter, FileDialogs, PageRenderer, Presentation,
    PresentationUpdate, ProcessingRuntime, ProjectLibrary, ViewDisposition, event_hub::EventHub,
    project::Project,
};

const THUMBNAIL_WORKERS: usize = 2;

#[derive(Clone, Debug)]
pub enum Lifecycle {
    Starting,
    Ready(Box<StartupState>),
    Failed(AppError),
    Stopped,
}

#[derive(Clone, Debug)]
pub struct BinaryAttachmentPayload {
    pub id: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct DispatchOutput {
    pub result: CommandResult,
    pub attachments: Vec<BinaryAttachmentPayload>,
}

impl DispatchOutput {
    fn result(result: CommandResult) -> Self {
        Self {
            result,
            attachments: Vec::new(),
        }
    }

    fn binary(bytes: Vec<u8>) -> Self {
        static NEXT_ATTACHMENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT_ATTACHMENT
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_string();
        Self {
            result: CommandResult::Binary(BinaryPayload {
                attachment: id.clone(),
            }),
            attachments: vec![BinaryAttachmentPayload { id, bytes }],
        }
    }
}

pub struct Application {
    library: ProjectLibrary,
    project: Arc<Mutex<Option<Project>>>,
    renderer: Arc<dyn PageRenderer>,
    presentation: Arc<dyn Presentation>,
    dialogs: Arc<dyn FileDialogs>,
    processing: Arc<dyn ProcessingRuntime>,
    lifecycle: RwLock<Lifecycle>,
    jobs: Arc<parking_lot::Mutex<HashMap<JobId, Job>>>,
    stops: Arc<parking_lot::Mutex<HashMap<JobId, koharu_pipeline::StopToken>>>,
    agent: Arc<AgentState>,
    events: EventHub,
    thumbnail_workers: Semaphore,
}

impl Application {
    pub fn in_documents(
        presentation: Arc<dyn Presentation>,
        dialogs: Arc<dyn FileDialogs>,
    ) -> Result<Self> {
        Self::new(ProjectLibrary::in_documents()?, presentation, dialogs)
    }

    pub fn at(
        root: impl Into<PathBuf>,
        presentation: Arc<dyn Presentation>,
        dialogs: Arc<dyn FileDialogs>,
    ) -> Result<Self> {
        Self::new(ProjectLibrary::new(root)?, presentation, dialogs)
    }

    pub fn new(
        library: ProjectLibrary,
        presentation: Arc<dyn Presentation>,
        dialogs: Arc<dyn FileDialogs>,
    ) -> Result<Self> {
        Self::with_components(
            library,
            presentation,
            Arc::new(koharu_renderer::Renderer::new()?),
            dialogs,
            Arc::new(crate::KoharuProcessingRuntime::new()),
        )
    }

    pub fn with_components(
        library: ProjectLibrary,
        presentation: Arc<dyn Presentation>,
        renderer: Arc<dyn PageRenderer>,
        dialogs: Arc<dyn FileDialogs>,
        processing: Arc<dyn ProcessingRuntime>,
    ) -> Result<Self> {
        let project = Arc::new(Mutex::new(None));
        let events = EventHub::default();
        let stops = Arc::new(parking_lot::Mutex::new(HashMap::new()));
        let agent = Arc::new(AgentState::new(
            KoharuHost::new(
                Arc::clone(&project),
                Arc::clone(&renderer),
                Arc::clone(&presentation),
                Arc::clone(&processing),
                Arc::clone(&stops),
                events.clone(),
            ),
            events.clone(),
        )?);
        Ok(Self {
            library,
            project,
            renderer,
            presentation,
            dialogs,
            processing,
            lifecycle: RwLock::new(Lifecycle::Starting),
            jobs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            stops,
            events,
            agent,
            thumbnail_workers: Semaphore::new(THUMBNAIL_WORKERS),
        })
    }

    #[must_use]
    pub fn events(&self) -> EventHub {
        self.events.clone()
    }

    pub async fn lifecycle(&self) -> Lifecycle {
        self.lifecycle.read().await.clone()
    }

    pub async fn initialize(&self) -> std::result::Result<StartupState, AppError> {
        {
            let lifecycle = self.lifecycle.read().await;
            match &*lifecycle {
                Lifecycle::Ready(startup) => return Ok((**startup).clone()),
                Lifecycle::Failed(error) => return Err(error.clone()),
                Lifecycle::Stopped => {
                    return Err(AppError::new(
                        AppErrorCode::Unavailable,
                        "application has stopped",
                    ));
                }
                Lifecycle::Starting => {}
            }
        }

        let result = self.prepare_startup().await;
        match result {
            Ok(startup) => {
                *self.lifecycle.write().await = Lifecycle::Ready(Box::new(startup.clone()));
                self.events.publish(AppEvent::StartupReady {
                    startup: Box::new(startup.clone()),
                });
                Ok(startup)
            }
            Err(error) => {
                *self.lifecycle.write().await = Lifecycle::Failed(error.clone());
                self.events.publish(AppEvent::StartupFailed {
                    error: error.clone(),
                });
                Err(error)
            }
        }
    }

    async fn prepare_startup(&self) -> std::result::Result<StartupState, AppError> {
        // The ML runtime may begin package downloads during initialization, so
        // attach the application's sole download observer first.
        let events = self.events.clone();
        let mut downloads = koharu_runtime::downloads::subscribe();
        tokio::spawn(async move {
            loop {
                match downloads.recv().await {
                    Ok(event) => {
                        events.publish(AppEvent::Download {
                            download: protocol_download(event),
                        });
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "download event stream fell behind");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        if let Some(mut resources) = self
            .processing
            .initialize()
            .await
            .map_err(AppError::internal)?
        {
            let events = self.events.clone();
            tokio::spawn(async move {
                while resources.changed().await.is_ok() {
                    events.publish(AppEvent::Resources {
                        resources: protocol_resources(resources.borrow_and_update().clone()),
                    });
                }
            });
        }
        let canvas = {
            let project = self.project.lock().await;
            match project.as_ref() {
                Some(project) => {
                    self.present(
                        &project.snapshot(),
                        project.active_page(),
                        ViewDisposition::Fit,
                    )
                    .await?
                }
                None => self.clear_presentation().await?,
            }
        };
        let preferences = load_preferences().map_err(AppError::internal)?;
        Ok(StartupState {
            preferences,
            jobs: self.jobs.lock().values().cloned().collect(),
            canvas,
        })
    }

    pub async fn dispatch(
        &self,
        command: Command,
    ) -> std::result::Result<DispatchOutput, AppError> {
        match command {
            Command::GetThumbnail { page } => {
                self.get_thumbnail(page).await.map(DispatchOutput::binary)
            }
            Command::GetFontPreview { family_name } => self
                .renderer
                .font_preview(&family_name)
                .await
                .map(DispatchOutput::binary)
                .map_err(AppError::internal),
            command => self
                .dispatch_result(command)
                .await
                .map(DispatchOutput::result),
        }
    }

    async fn dispatch_result(
        &self,
        command: Command,
    ) -> std::result::Result<CommandResult, AppError> {
        match command {
            Command::GetStartup {} => Ok(CommandResult::Startup(self.startup().await?)),
            Command::GetProject {} => Ok(CommandResult::Project(self.project().await)),
            Command::GetPages {} => {
                let project = self.project.lock().await;
                let project = project.as_ref().ok_or_else(no_project)?;
                Ok(CommandResult::Pages(
                    Project::pages(&project.snapshot()).map_err(AppError::internal)?,
                ))
            }
            Command::GetPage {} => {
                let project = self.project.lock().await;
                let page = project
                    .as_ref()
                    .and_then(|project| {
                        project.active_page().map(|page| (project.snapshot(), page))
                    })
                    .map(|(snapshot, page)| Project::page(&snapshot, page))
                    .transpose()
                    .map_err(AppError::internal)?;
                Ok(CommandResult::Page(page))
            }
            Command::ListProjects {} => self
                .library
                .list()
                .map(CommandResult::Projects)
                .map_err(AppError::internal),
            Command::CreateProject { name } => {
                let project = self
                    .library
                    .create(&name)
                    .await
                    .map_err(AppError::internal)?;
                self.replace_project(project).await?;
                Ok(CommandResult::Unit(()))
            }
            Command::OpenProject { name } => {
                let project = self.library.open(&name).await.map_err(AppError::internal)?;
                self.replace_project(project).await?;
                Ok(CommandResult::Unit(()))
            }
            Command::DeleteProject { name } => {
                if self
                    .project
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|project| project.info().name == name)
                {
                    self.close_project().await?;
                }
                let library = self.library.clone();
                tokio::task::spawn_blocking(move || library.delete(&name))
                    .await
                    .context("project deletion worker stopped unexpectedly")
                    .map_err(AppError::internal)?
                    .map_err(AppError::internal)?;
                Ok(CommandResult::Unit(()))
            }
            Command::CloseProject {} => {
                self.close_project().await?;
                Ok(CommandResult::Unit(()))
            }
            Command::ImportPages { source } => {
                self.import_pages(source).await?;
                Ok(CommandResult::Unit(()))
            }
            Command::SelectPage { page } => {
                self.select_page(page).await?;
                Ok(CommandResult::Unit(()))
            }
            Command::RenamePage { page, label } => {
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .rename_page(page, label)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page())
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::DeletePages { pages } => {
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .delete_pages(pages)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    project.reconcile_page();
                    (commit, project.active_page())
                };
                self.publish_commit(&commit, active, ViewDisposition::Fit)
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::MovePage { page, index } => {
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .move_page(page, index as usize)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page())
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::SetSourceText { layer, text } => {
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .set_source_text(layer, text)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page())
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::SetTranslation { layer, text } => {
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .set_translation(layer, text)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page())
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::SetTypography { updates } => {
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .set_typography(updates)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page())
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::SetGeometry { updates } => {
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .set_geometry(updates)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page())
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::SetVisibility {
                layers,
                visible,
                opacity,
            } => {
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .set_visibility(layers, visible, opacity)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page())
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::DeleteLayers { layers } => {
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .delete_layers(layers)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page())
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::MoveLayer {
                layer,
                parent,
                index,
            } => {
                let (commit, active, view) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .move_layer(layer, parent, index as usize)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    let active = project.active_page().ok_or_else(|| {
                        AppError::new(AppErrorCode::NoProject, "project has no active page")
                    })?;
                    let view =
                        Project::page(&commit.snapshot, active).map_err(AppError::internal)?;
                    (commit, active, view)
                };
                self.publish_commit(&commit, Some(active), ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::PageValue(view))
            }
            operation @ (Command::Undo {} | Command::Redo {}) => {
                let redo = matches!(operation, Command::Redo {});
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = if redo {
                        project.redo().await
                    } else {
                        project.undo().await
                    }
                    .map_err(AppError::internal)?;
                    project.reconcile_page();
                    (commit, project.active_page())
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::Process { scope, operation } => self
                .start_process(scope, operation, None)
                .await
                .map(CommandResult::Job),
            Command::StopJob { job } => {
                self.stops
                    .lock()
                    .get(&job)
                    .ok_or_else(|| {
                        AppError::new(AppErrorCode::NotFound, format!("job {job} is not running"))
                    })?
                    .stop();
                Ok(CommandResult::Unit(()))
            }
            Command::ExportPages { pages, format } => {
                self.export_pages(pages, format).await?;
                Ok(CommandResult::Unit(()))
            }
            Command::GetFonts {} => self
                .renderer
                .available_fonts()
                .await
                .map(|families| {
                    CommandResult::Fonts(families.into_iter().map(protocol_font_family).collect())
                })
                .map_err(AppError::internal),
            Command::SavePreferences {
                pipeline,
                providers,
                typesetting,
            } => {
                let mut pipeline = *pipeline;
                remember_pipeline_profiles(&mut pipeline);
                save_preferences(pipeline, *providers, *typesetting)
                    .map(CommandResult::Preferences)
                    .map_err(AppError::internal)
            }
            Command::GetPreferences {} => load_preferences()
                .map(CommandResult::Preferences)
                .map_err(AppError::internal),
            Command::GetTranslationModels {} => koharu_translator::Translator::models()
                .await
                .map(CommandResult::Models)
                .map_err(AppError::internal),
            Command::SetZoom { zoom } => {
                self.canvas_unit(CanvasOperation::SetZoom(zoom)).await?;
                Ok(CommandResult::Unit(()))
            }
            Command::SetCanvasView { zoom, translation } => {
                self.canvas_unit(CanvasOperation::SetView { zoom, translation })
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::FitCanvas {} => {
                let size = self.active_page_size().await?;
                self.canvas_unit(CanvasOperation::Fit { page_size: size })
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::AddPointText { point } => {
                let (commit, active, layer) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let page = project.active_page().ok_or_else(no_project)?;
                    let (commit, layer) = project
                        .add_point_text(page, point)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page(), layer)
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::LayerCommit(LayerCommit {
                    revision: commit.revision,
                    layer,
                }))
            }
            Command::AddTextBox { frame } => {
                let (commit, active, layer) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let page = project.active_page().ok_or_else(no_project)?;
                    let (commit, layer) = project
                        .add_text_box(page, frame)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page(), layer)
                };
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::LayerCommit(LayerCommit {
                    revision: commit.revision,
                    layer,
                }))
            }
            Command::BeginPaint {
                layer,
                point,
                brush,
            } => {
                self.canvas_unit(CanvasOperation::BeginPaint {
                    layer,
                    point,
                    brush,
                })
                .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::BeginErase {
                layer,
                point,
                diameter,
            } => {
                self.canvas_unit(CanvasOperation::BeginErase {
                    layer,
                    point,
                    diameter,
                })
                .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::ExtendPaint { points } | Command::ExtendErase { points } => {
                self.canvas_unit(CanvasOperation::ExtendRaster(points))
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::FinishPaint {} | Command::FinishErase {} => {
                let stroke = match self
                    .presentation
                    .canvas(CanvasOperation::FinishRaster)
                    .await
                    .map_err(AppError::internal)?
                {
                    CanvasOutput::Raster(stroke) => stroke,
                    _ => return Err(invalid_canvas("finish raster")),
                };
                let (commit, active, layer) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let (commit, layer) = project
                        .apply_raster_stroke(
                            stroke.page,
                            stroke.layer,
                            stroke.mode,
                            stroke.color,
                            stroke.diameter,
                            stroke
                                .points
                                .into_iter()
                                .map(|point| koharu_scene::Point {
                                    x: point.x,
                                    y: point.y,
                                })
                                .collect(),
                        )
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page(), layer)
                };
                self.canvas_unit(CanvasOperation::AcknowledgeRaster {
                    page: stroke.page,
                    revision: commit.revision,
                })
                .await?;
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::LayerCommit(LayerCommit {
                    revision: commit.revision,
                    layer,
                }))
            }
            Command::CancelPaint {} | Command::CancelErase {} => {
                self.canvas_unit(CanvasOperation::CancelRaster).await?;
                Ok(CommandResult::Unit(()))
            }
            Command::BeginTransform { elements } => {
                self.canvas_unit(CanvasOperation::BeginTransform(elements))
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::UpdateTransform { frame, elements } => {
                self.canvas_unit(CanvasOperation::UpdateTransform {
                    frame: frame.into(),
                    elements,
                })
                .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::PreviewOpacity { element, opacity } => {
                self.canvas_unit(CanvasOperation::PreviewOpacity { element, opacity })
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::FinishTransform {} => {
                let Some(transform) = (match self
                    .presentation
                    .canvas(CanvasOperation::FinishTransform)
                    .await
                    .map_err(AppError::internal)?
                {
                    CanvasOutput::Transform(value) => value,
                    _ => return Err(invalid_canvas("finish transform")),
                }) else {
                    return Ok(CommandResult::Revision(None));
                };
                let (commit, active) = {
                    let mut project = self.project.lock().await;
                    let project = project.as_mut().ok_or_else(no_project)?;
                    let commit = project
                        .set_geometries(transform.elements)
                        .await
                        .map_err(AppError::internal)?;
                    project.record_commit(&commit);
                    (commit, project.active_page())
                };
                self.canvas_unit(CanvasOperation::AcknowledgeTransform {
                    page: transform.page,
                    revision: commit.revision,
                })
                .await?;
                self.publish_commit(&commit, active, ViewDisposition::Preserve)
                    .await?;
                Ok(CommandResult::Revision(Some(commit.revision)))
            }
            Command::CancelTransform {} => {
                self.canvas_unit(CanvasOperation::CancelTransform).await?;
                Ok(CommandResult::Unit(()))
            }
            Command::BeginInpaint { point, diameter } => {
                self.canvas_unit(CanvasOperation::BeginInpaint { point, diameter })
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::ExtendInpaint { points } => {
                self.canvas_unit(CanvasOperation::ExtendInpaint(points))
                    .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::FinishInpaint {} => {
                let commit = match self
                    .presentation
                    .canvas(CanvasOperation::FinishInpaint)
                    .await
                    .map_err(AppError::internal)?
                {
                    CanvasOutput::Inpaint(commit) => commit,
                    _ => return Err(invalid_canvas("finish inpaint")),
                };
                let Some(commit) = commit else {
                    return Ok(CommandResult::OptionalJob(None));
                };
                let page = commit.mask.page;
                let job = self
                    .start_process(
                        koharu_pipeline::Scope::Region {
                            page,
                            bounds: commit.bounds,
                        },
                        koharu_pipeline::Operation::Only {
                            stage: koharu_pipeline::Stage::Inpainting,
                        },
                        Some(commit.mask),
                    )
                    .await?;
                Ok(CommandResult::OptionalJob(Some(job)))
            }
            Command::CancelInpaint {} => {
                self.canvas_unit(CanvasOperation::CancelInpaint).await?;
                Ok(CommandResult::Unit(()))
            }
            Command::SampleColor { point } => match self
                .presentation
                .canvas(CanvasOperation::SampleColor(point))
                .await
                .map_err(AppError::internal)?
            {
                CanvasOutput::Color(color) => Ok(CommandResult::Color(color)),
                _ => Err(invalid_canvas("sample color")),
            },
            Command::SetViewport {
                x,
                y,
                width,
                height,
                dpr,
                background,
            } => {
                let fitted_page = self.active_page_size().await.ok();
                self.canvas_unit(CanvasOperation::SetViewport {
                    x,
                    y,
                    width,
                    height,
                    dpr,
                    background,
                    fitted_page,
                })
                .await?;
                Ok(CommandResult::Unit(()))
            }
            Command::GetAgentStatus {} => self
                .agent
                .status()
                .await
                .map(CommandResult::AgentStatus)
                .map_err(AppError::internal),
            Command::LoginAgent {} => self
                .agent
                .login()
                .await
                .map(CommandResult::AgentStatus)
                .map_err(AppError::internal),
            Command::LogoutAgent {} => self
                .agent
                .logout()
                .await
                .map(CommandResult::AgentStatus)
                .map_err(AppError::internal),
            Command::SaveAgentConfig { config } => self
                .agent
                .save_config(config)
                .map(CommandResult::AgentConfig)
                .map_err(AppError::internal),
            Command::RunAgent { prompt } => self
                .agent
                .run(prompt)
                .map(CommandResult::AgentRun)
                .map_err(AppError::internal),
            Command::CancelAgent { run } => {
                self.agent.cancel(run).map_err(AppError::internal)?;
                Ok(CommandResult::Unit(()))
            }
            Command::GetThumbnail { .. } | Command::GetFontPreview { .. } => {
                unreachable!("binary commands are handled before result dispatch")
            }
            Command::WindowMinimize {}
            | Command::WindowToggleMaximize {}
            | Command::WindowClose {}
            | Command::WindowBeginDrag {}
            | Command::OpenExternal { .. }
            | Command::GetVersion {} => Err(AppError::new(
                AppErrorCode::InvalidRequest,
                "command belongs to the desktop shell",
            )),
        }
    }

    async fn startup(&self) -> std::result::Result<StartupState, AppError> {
        match &*self.lifecycle.read().await {
            Lifecycle::Ready(startup) => Ok((**startup).clone()),
            Lifecycle::Failed(error) => Err(error.clone()),
            Lifecycle::Starting => Err(AppError::new(
                AppErrorCode::NotReady,
                "application startup is still running",
            )),
            Lifecycle::Stopped => Err(AppError::new(
                AppErrorCode::Unavailable,
                "application has stopped",
            )),
        }
    }

    async fn project(&self) -> Option<ProjectInfo> {
        self.project.lock().await.as_ref().map(Project::info)
    }

    async fn canvas_unit(&self, operation: CanvasOperation) -> std::result::Result<(), AppError> {
        match self
            .presentation
            .canvas(operation)
            .await
            .map_err(AppError::internal)?
        {
            CanvasOutput::Unit => Ok(()),
            CanvasOutput::State(state) => {
                self.events.publish(AppEvent::Canvas { state });
                Ok(())
            }
            _ => Err(invalid_canvas("unit canvas operation")),
        }
    }

    async fn import_pages(&self, source: PageImportSource) -> std::result::Result<(), AppError> {
        if !self.jobs.lock().is_empty() {
            return Err(AppError::new(
                AppErrorCode::Conflict,
                "pages cannot be imported while processing is running",
            ));
        }

        let filters = [DialogFilter {
            name: "Images".to_owned(),
            extensions: ["png", "jpg", "jpeg", "webp"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }];
        let files = match source {
            PageImportSource::Files => self.dialogs.pick_files(&filters).await,
            PageImportSource::Folder => self
                .dialogs
                .pick_folder()
                .await
                .map(|folder| folder.map(collect_image_files)),
        }
        .map_err(AppError::internal)?;
        let Some(mut files) = files else {
            return Ok(());
        };
        if files.is_empty() {
            return Err(AppError::new(
                AppErrorCode::InvalidRequest,
                "no supported images were found in the selection",
            ));
        }
        alphanumeric_sort::sort_slice_by_os_str_key(&mut files, |path| {
            path.file_name().unwrap_or_else(|| path.as_os_str())
        });
        let pages = tokio::task::spawn_blocking(move || load_page_sources(files))
            .await
            .context("page import worker stopped unexpectedly")
            .map_err(AppError::internal)?
            .map_err(AppError::internal)?;

        let (commit, page) = {
            let mut project = self.project.lock().await;
            let project = project.as_mut().ok_or_else(no_project)?;
            let source = AssetRole::new("source").map_err(AppError::internal)?;
            let patch = project
                .snapshot()
                .patch(|edit| {
                    for page in pages {
                        let entity = edit.add_page(
                            PageDraft::new(
                                page.name,
                                f64::from(page.width),
                                f64::from(page.height),
                            ),
                            At::End,
                        )?;
                        edit.set_asset(
                            entity,
                            &source,
                            AssetInput::new(
                                page.bytes,
                                page.format.to_mime_type(),
                                AssetMetadata {
                                    width: Some(page.width),
                                    height: Some(page.height),
                                    attributes: Default::default(),
                                },
                            ),
                        )?;
                    }
                    Ok(())
                })
                .map_err(AppError::internal)?;
            let commit = project
                .session
                .commit(patch)
                .await
                .map_err(AppError::internal)?;
            project.record_commit(&commit);
            project.reconcile_page();
            (commit, project.active_page())
        };
        self.publish_commit(&commit, page, ViewDisposition::Fit)
            .await
    }

    async fn export_pages(
        &self,
        pages: Vec<EntityId>,
        format: ExportFormat,
    ) -> std::result::Result<(), AppError> {
        let snapshot = {
            let project = self.project.lock().await;
            project.as_ref().ok_or_else(no_project)?.snapshot()
        };
        let Some(directory) = self
            .dialogs
            .pick_folder()
            .await
            .map_err(AppError::internal)?
        else {
            return Ok(());
        };
        let pages = if pages.is_empty() {
            snapshot.pages().map(|page| page.id()).collect::<Vec<_>>()
        } else {
            pages
        };
        if pages.is_empty() {
            return Err(AppError::new(
                AppErrorCode::InvalidRequest,
                "there are no pages to export",
            ));
        }

        let mut jobs = Vec::with_capacity(pages.len());
        for (index, page) in pages.into_iter().enumerate() {
            let label = snapshot
                .page(page)
                .and_then(|page| page.page())
                .map_err(AppError::internal)?
                .label;
            jobs.push((page, export_stem(index, &label)));
        }
        futures::stream::iter(jobs)
            .map(|(page, stem)| {
                let renderer = Arc::clone(&self.renderer);
                let snapshot = snapshot.clone();
                let directory = directory.clone();
                async move {
                    let frame = renderer.render(&snapshot, page).await?;
                    match format {
                        ExportFormat::Png => {
                            let image = renderer.rasterize(&frame).await?;
                            tokio::task::spawn_blocking(move || {
                                image.save(directory.join(format!("{stem}.png")))
                            })
                            .await
                            .context("PNG export worker stopped unexpectedly")??;
                        }
                        ExportFormat::Psd => {
                            let bytes = renderer.export_psd(&snapshot, &frame).await?;
                            tokio::fs::write(directory.join(format!("{stem}.psd")), bytes).await?;
                        }
                    }
                    Ok::<_, anyhow::Error>(())
                }
            })
            .buffer_unordered(4)
            .try_collect::<Vec<_>>()
            .await
            .map(|_| ())
            .map_err(AppError::internal)
    }

    async fn get_thumbnail(&self, page: EntityId) -> std::result::Result<Vec<u8>, AppError> {
        // Decoding manga pages is CPU- and memory-heavy. Bound this separately
        // from Tokio's shared blocking pool so a visible rail cannot starve CEF.
        let _worker = self
            .thumbnail_workers
            .acquire()
            .await
            .expect("the application never closes its thumbnail semaphore");
        let snapshot = {
            let project = self.project.lock().await;
            project.as_ref().ok_or_else(no_project)?.snapshot()
        };
        snapshot.page(page).map_err(AppError::internal)?;
        let role = AssetRole::new("source").map_err(AppError::internal)?;
        let blob = snapshot
            .asset(page, &role)
            .map_err(AppError::internal)?
            .ok_or_else(|| {
                AppError::new(
                    AppErrorCode::NotFound,
                    format!("page {page} has no source image"),
                )
            })?
            .blob;
        let bytes = snapshot.read_blob(blob).await.map_err(AppError::internal)?;
        tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            let image = image::load_from_memory(&bytes).context("failed to decode source image")?;
            if image.width() == 0 || image.height() == 0 {
                anyhow::bail!("source image is empty");
            }
            let width = image.width();
            let height = image.height();
            let longest = width.max(height);
            let target_width = (u64::from(width) * 128 / u64::from(longest)).max(1) as u32;
            let target_height = (u64::from(height) * 128 / u64::from(longest)).max(1) as u32;
            let source = image.to_rgba8();
            let mut thumbnail = image::RgbaImage::new(target_width, target_height);
            Resizer::new()
                .resize(
                    &source,
                    &mut thumbnail,
                    &ResizeOptions::new()
                        .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
                        .use_alpha(true),
                )
                .context("failed to resize thumbnail")?;
            Ok(
                webp::Encoder::from_rgba(thumbnail.as_raw(), thumbnail.width(), thumbnail.height())
                    .encode(80.0)
                    .to_vec(),
            )
        })
        .await
        .context("thumbnail worker stopped unexpectedly")
        .map_err(AppError::internal)?
        .map_err(AppError::internal)
    }

    async fn start_process(
        &self,
        scope: koharu_pipeline::Scope,
        operation: koharu_pipeline::Operation,
        inpainting_mask: Option<koharu_pipeline::InpaintingMask>,
    ) -> std::result::Result<JobId, AppError> {
        let snapshot = self
            .project
            .lock()
            .await
            .as_ref()
            .ok_or_else(no_project)?
            .snapshot();
        let id = JobId::new();
        let stop = koharu_pipeline::StopToken::default();
        {
            let mut stops = self.stops.lock();
            if !stops.is_empty() {
                return Err(AppError::new(
                    AppErrorCode::Conflict,
                    "another process is already running",
                ));
            }
            stops.insert(id, stop.clone());
        }
        let job = Job {
            id,
            state: JobState::Running,
            completed: 0,
            total: 0,
            page: None,
            stage: None,
            model: None,
            error: None,
        };
        self.jobs.lock().insert(id, job.clone());
        self.events.publish(AppEvent::Job { job });

        let jobs = Arc::clone(&self.jobs);
        let stops = Arc::clone(&self.stops);
        let events = self.events.clone();
        let processing = Arc::clone(&self.processing);
        let project = Arc::clone(&self.project);
        let renderer = Arc::clone(&self.renderer);
        let presentation = Arc::clone(&self.presentation);
        tokio::spawn(async move {
            let progress = Arc::new(parking_lot::Mutex::new((0_usize, 0_usize)));
            let progress_state = Arc::clone(&progress);
            let progress_jobs = Arc::clone(&jobs);
            let progress_events = events.clone();
            let mut request = koharu_pipeline::Request {
                operation,
                scope,
                stop: stop.clone(),
                progress: None,
                inpainting_mask,
            };
            request.progress = Some(Arc::new(move |event| {
                use koharu_pipeline::Progress;
                let update = match event {
                    Progress::Started { pages, stages } => {
                        let mut progress = progress_state.lock();
                        *progress = (0, pages.len().saturating_mul(stages.len()));
                        Some((0, progress.1, None, None, None))
                    }
                    Progress::Loading { page, stage, model } => {
                        let progress = progress_state.lock();
                        Some((progress.0, progress.1, Some(page), Some(stage), Some(model)))
                    }
                    Progress::Finished {
                        page, stage, model, ..
                    } => {
                        let mut progress = progress_state.lock();
                        progress.0 = progress.0.saturating_add(1).min(progress.1);
                        Some((progress.0, progress.1, Some(page), Some(stage), Some(model)))
                    }
                    Progress::Skipped { page, stage } => {
                        let mut progress = progress_state.lock();
                        progress.0 = progress.0.saturating_add(1).min(progress.1);
                        Some((progress.0, progress.1, Some(page), Some(stage), None))
                    }
                    Progress::Running { .. } => None,
                };
                if let Some((completed, total, page, stage, model)) = update
                    && let Some(job) = progress_jobs.lock().get_mut(&id)
                {
                    job.completed = completed;
                    job.total = total;
                    job.page = page;
                    job.stage = stage;
                    job.model = model;
                    progress_events.publish(AppEvent::Job { job: job.clone() });
                }
            }));

            let mut committer = ApplicationCommitter {
                project,
                renderer,
                presentation,
                events: events.clone(),
                revisions: Vec::new(),
            };
            let result = processing.execute(snapshot, request, &mut committer).await;
            stops.lock().remove(&id);
            if !committer.revisions.is_empty()
                && let Some(project) = committer.project.lock().await.as_mut()
            {
                project.record(std::mem::take(&mut committer.revisions));
                events.publish(AppEvent::Project {
                    project: Some(project.info()),
                });
            }
            if let Some(mut job) = jobs.lock().remove(&id) {
                match result {
                    Ok(report) => {
                        job.state = if report.status == koharu_pipeline::RunStatus::Stopped {
                            JobState::Stopped
                        } else {
                            JobState::Finished
                        };
                    }
                    Err(error) => {
                        tracing::error!(%id, error = ?error, "processing failed");
                        job.state = JobState::Failed;
                        job.error = Some(format!("{error:#}"));
                    }
                }
                events.publish(AppEvent::Job { job });
            }
        });
        Ok(id)
    }

    async fn active_page_size(&self) -> std::result::Result<(u32, u32), AppError> {
        let project = self.project.lock().await;
        let project = project.as_ref().ok_or_else(no_project)?;
        let page = project.active_page().ok_or_else(no_project)?;
        let snapshot = project.snapshot();
        let page = snapshot
            .page(page)
            .map_err(AppError::internal)?
            .page()
            .map_err(AppError::internal)?;
        Ok((page.width.ceil() as u32, page.height.ceil() as u32))
    }

    async fn publish_commit(
        &self,
        commit: &koharu_scene::Commit,
        page: Option<EntityId>,
        view: ViewDisposition,
    ) -> std::result::Result<(), AppError> {
        let canvas = self.present(&commit.snapshot, page, view).await?;
        let info = self.project().await;
        self.events.publish(AppEvent::Project { project: info });
        self.events.publish(AppEvent::Canvas { state: canvas });
        Ok(())
    }

    async fn replace_project(&self, project: Project) -> std::result::Result<(), AppError> {
        self.agent.reset().await;
        let canvas = self
            .present(
                &project.snapshot(),
                project.active_page(),
                ViewDisposition::Fit,
            )
            .await?;
        let info = project.info();
        self.cancel_jobs();
        *self.project.lock().await = Some(project);
        self.events.publish(AppEvent::Project {
            project: Some(info),
        });
        self.events.publish(AppEvent::Canvas { state: canvas });
        Ok(())
    }

    async fn close_project(&self) -> std::result::Result<(), AppError> {
        self.agent.reset().await;
        let canvas = self.clear_presentation().await?;
        self.cancel_jobs();
        self.project.lock().await.take();
        self.events.publish(AppEvent::Project { project: None });
        self.events.publish(AppEvent::Canvas { state: canvas });
        Ok(())
    }

    async fn select_page(&self, page: EntityId) -> std::result::Result<(), AppError> {
        let (snapshot, info) = {
            let project = self.project.lock().await;
            let project = project
                .as_ref()
                .ok_or_else(|| AppError::new(AppErrorCode::NoProject, "no project is open"))?;
            project.snapshot().page(page).map_err(AppError::internal)?;
            (project.snapshot(), project.info())
        };
        let canvas = self
            .present(&snapshot, Some(page), ViewDisposition::Fit)
            .await?;
        let info = {
            let mut project = self.project.lock().await;
            let project = project.as_mut().ok_or_else(|| {
                AppError::new(
                    AppErrorCode::NoProject,
                    "project closed while selecting a page",
                )
            })?;
            if project.info().revision != info.revision {
                return Err(AppError::new(
                    AppErrorCode::Conflict,
                    "project changed while selecting a page",
                ));
            }
            project.select_page(page).map_err(AppError::internal)?;
            project.info()
        };
        self.events.publish(AppEvent::Project {
            project: Some(info),
        });
        self.events.publish(AppEvent::Canvas { state: canvas });
        Ok(())
    }

    async fn present(
        &self,
        snapshot: &koharu_scene::Snapshot,
        page: Option<EntityId>,
        view: ViewDisposition,
    ) -> std::result::Result<CanvasState, AppError> {
        let Some(page) = page else {
            return self.clear_presentation().await;
        };
        let frame = self
            .renderer
            .render(snapshot, page)
            .await
            .map_err(AppError::internal)?;
        self.presentation
            .apply(PresentationUpdate::Frame { frame, view })
            .await
            .map_err(AppError::internal)
    }

    async fn clear_presentation(&self) -> std::result::Result<CanvasState, AppError> {
        let canvas = self
            .presentation
            .apply(PresentationUpdate::Clear)
            .await
            .map_err(AppError::internal)?;
        self.renderer.discard_retained_nodes();
        Ok(canvas)
    }

    pub async fn stop(&self) {
        self.agent.cancel_all();
        self.cancel_jobs();
        *self.lifecycle.write().await = Lifecycle::Stopped;
    }

    fn cancel_jobs(&self) {
        for stop in self.stops.lock().values() {
            stop.stop();
        }
        self.stops.lock().clear();
        self.jobs.lock().clear();
    }
}

struct ApplicationCommitter {
    project: Arc<Mutex<Option<Project>>>,
    renderer: Arc<dyn PageRenderer>,
    presentation: Arc<dyn Presentation>,
    events: EventHub,
    revisions: Vec<koharu_scene::Revision>,
}

#[async_trait::async_trait]
impl koharu_pipeline::Committer for ApplicationCommitter {
    async fn commit(
        &mut self,
        output: koharu_pipeline::StageOutput,
    ) -> Result<koharu_scene::Snapshot> {
        let (commit, page, project_info) = {
            let mut project = self.project.lock().await;
            let project = project.as_mut().context("no project is open")?;
            let commit = project.session.commit(output.patch).await?;
            if commit.changes.from != commit.changes.to {
                self.revisions.push(commit.revision);
            }
            (commit, project.active_page(), project.info())
        };
        let canvas = if let Some(page) = page {
            let frame = self.renderer.render(&commit.snapshot, page).await?;
            self.presentation
                .apply(PresentationUpdate::Frame {
                    frame,
                    view: ViewDisposition::Preserve,
                })
                .await?
        } else {
            self.presentation.apply(PresentationUpdate::Clear).await?
        };
        self.events.publish(AppEvent::Project {
            project: Some(project_info),
        });
        self.events.publish(AppEvent::Canvas { state: canvas });
        Ok(commit.snapshot)
    }
}

fn load_preferences() -> Result<Preferences> {
    let pipeline = koharu_pipeline::PipelineConfig::load()?.read()?.clone();
    let providers = koharu_translator::ProvidersConfig::load()?.read()?.clone();
    let typesetting = koharu_renderer::TypesettingConfig::load()?.read()?.clone();
    let languages = koharu_translator::Language::ALL
        .iter()
        .map(|language| LanguageChoice {
            tag: language.tag().to_owned(),
            name: language.to_string(),
        })
        .collect();
    Ok(Preferences {
        pipeline,
        providers: provider_preferences(&providers)?,
        typesetting,
        languages,
    })
}

fn provider_preferences(
    config: &koharu_translator::ProvidersConfig,
) -> Result<ProviderPreferences> {
    config
        .entries()
        .into_iter()
        .map(|config| {
            let provider = config.provider();
            let credential = if provider == koharu_translator::Provider::Local {
                None
            } else {
                let key: &'static str = provider.into();
                Some(CredentialInput {
                    configured: koharu_secrets::get(key)?
                        .is_some_and(|secret| !secret.expose_secret().trim().is_empty()),
                    value: None,
                    clear: false,
                })
            };
            Ok(ProviderPreference {
                name: provider.name().to_owned(),
                config,
                credential,
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(|entries| ProviderPreferences { entries })
}

fn save_preferences(
    pipeline: koharu_pipeline::PipelineConfig,
    providers: ProviderPreferences,
    typesetting: koharu_renderer::TypesettingConfig,
) -> Result<Preferences> {
    let mut provider_configs = Vec::with_capacity(providers.entries.len());
    let mut credentials = Vec::with_capacity(providers.entries.len().saturating_sub(1));
    for entry in providers.entries {
        let provider = entry.config.provider();
        match entry.credential {
            None if provider == koharu_translator::Provider::Local => {}
            Some(credential) if provider != koharu_translator::Provider::Local => {
                let key: &'static str = provider.into();
                credentials.push((key, credential));
            }
            None => anyhow::bail!("missing credential input for {provider}"),
            Some(_) => anyhow::bail!("local translation does not accept credentials"),
        }
        provider_configs.push(entry.config);
    }
    let providers = koharu_translator::ProvidersConfig::from_entries(provider_configs)?;
    for (key, credential) in credentials {
        if credential.clear {
            koharu_secrets::delete(key)?;
        } else if let Some(value) = credential.value {
            if value.trim().is_empty() {
                koharu_secrets::delete(key)?;
            } else {
                koharu_secrets::set(key, &value.into())?;
            }
        }
    }

    let pipeline_config = koharu_pipeline::PipelineConfig::load()?;
    let providers_config = koharu_translator::ProvidersConfig::load()?;
    let typesetting_config = koharu_renderer::TypesettingConfig::load()?;
    {
        let mut current = pipeline_config.write()?;
        *current = pipeline;
        current.save()?;
    }
    {
        let mut current = providers_config.write()?;
        *current = providers;
        current.save()?;
    }
    {
        let mut current = typesetting_config.write()?;
        *current = typesetting;
        current.save()?;
    }
    load_preferences()
}

fn remember_pipeline_profiles(config: &mut koharu_pipeline::PipelineConfig) {
    let koharu_pipeline::DetectionModel::KoharuLayoutRFDetrSeg2XL(settings) = &config.detection;
    config.processor.koharu_layout_rfdetr_seg_2xl = Some(settings.clone());
    if let koharu_pipeline::InpaintingModel::Flux2Klein(settings) = &config.inpainting {
        config.processor.flux2_klein = Some(settings.clone());
    }
    if let koharu_pipeline::InpaintingModel::RoremMixed(settings) = &config.inpainting {
        config.processor.rorem_mixed = Some(settings.clone());
    }
}

fn protocol_font_family(family: koharu_renderer::FontFamily) -> FontFamily {
    FontFamily {
        name: family.name,
        metadata: FontMetadata {
            primary_script: family.metadata.primary_script,
            scripts: family.metadata.scripts,
            languages: family.metadata.languages,
            category: family.metadata.category,
            classifications: family.metadata.classifications,
            use_cases: family.metadata.use_cases,
        },
        sources: family
            .sources
            .into_iter()
            .map(|source| match source {
                koharu_renderer::FontSource::System => FontSource::System,
                koharu_renderer::FontSource::Bundled => FontSource::Bundled,
            })
            .collect(),
        faces: family
            .faces
            .into_iter()
            .map(|face| FontFace {
                postscript_name: face.post_script_name,
                weight: face.weight,
                weight_range: face.weight_range.map(|range| FontRange {
                    minimum: range.minimum,
                    maximum: range.maximum,
                }),
                style: match face.style {
                    koharu_renderer::FontStyle::Normal => koharu_scene::FontStyle::Normal,
                    koharu_renderer::FontStyle::Italic => koharu_scene::FontStyle::Italic,
                    koharu_renderer::FontStyle::Oblique => koharu_scene::FontStyle::Oblique,
                },
            })
            .collect(),
    }
}

fn protocol_resources(value: koharu_pipeline::ResourceSnapshot) -> ModelResources {
    ModelResources {
        process_memory: value.process_memory_bytes,
        system_memory: value.system_memory_bytes,
        process_cpu: value.process_cpu_percent,
        devices: value
            .devices
            .into_iter()
            .map(|device| DeviceResources {
                name: device.name,
                selected: device.selected,
                memory_budget: device.memory_budget_bytes,
                memory_used: device.memory_used_bytes,
                utilization: device.utilization_percent,
            })
            .collect(),
    }
}

fn protocol_download(event: koharu_runtime::downloads::Event) -> Download {
    match event {
        koharu_runtime::downloads::Event::Started { id, name } => Download {
            id,
            state: DownloadState::Running,
            name: Some(name),
            completed: 0,
            total: 0,
            error: None,
        },
        koharu_runtime::downloads::Event::Progress {
            id,
            name,
            completed,
            total,
        } => Download {
            id,
            state: DownloadState::Running,
            name: Some(name),
            completed,
            total,
            error: None,
        },
        koharu_runtime::downloads::Event::Finished { id } => Download {
            id,
            state: DownloadState::Finished,
            name: None,
            completed: 0,
            total: 0,
            error: None,
        },
        koharu_runtime::downloads::Event::Failed { id, name, error } => Download {
            id,
            state: DownloadState::Failed,
            name: Some(name),
            completed: 0,
            total: 0,
            error: Some(error),
        },
    }
}

fn export_stem(index: usize, label: &str) -> String {
    let label = label
        .trim()
        .trim_end_matches(|character: char| character == '.' || character.is_whitespace());
    let label = label.rsplit_once('.').map_or(label, |(stem, _)| stem);
    let label = label
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    format!(
        "{:04}_{}",
        index + 1,
        if label.is_empty() { "page" } else { &label }
    )
}

fn no_project() -> AppError {
    AppError::new(AppErrorCode::NoProject, "no project is open")
}

fn invalid_canvas(operation: &str) -> AppError {
    AppError::new(
        AppErrorCode::Internal,
        format!("canvas returned an invalid result for {operation}"),
    )
}

struct PageSource {
    name: String,
    bytes: Arc<[u8]>,
    format: ImageFormat,
    width: u32,
    height: u32,
}

fn collect_image_files(folder: PathBuf) -> Vec<PathBuf> {
    WalkDir::new(folder)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) if entry.file_type().is_file() => Some(entry.into_path()),
            Ok(_) => None,
            Err(error) => {
                tracing::warn!(%error, "could not inspect an import directory entry");
                None
            }
        })
        .filter(|path| is_supported_image(path))
        .collect()
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp"
            )
        })
}

fn load_page_sources(files: Vec<PathBuf>) -> Result<Vec<PageSource>> {
    files
        .into_par_iter()
        .map(|file| {
            let bytes =
                fs::read(&file).with_context(|| format!("failed to read {}", file.display()))?;
            let format = image::guess_format(&bytes)
                .with_context(|| format!("failed to identify {}", file.display()))?;
            let (width, height) =
                image::ImageReader::with_format(Cursor::new(bytes.as_slice()), format)
                    .into_dimensions()
                    .with_context(|| format!("failed to read dimensions of {}", file.display()))?;
            Ok(PageSource {
                name: file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("page")
                    .to_owned(),
                bytes: Arc::from(bytes),
                format,
                width,
                height,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;
    use koharu_protocol::{RequestId, Response};
    use tempfile::TempDir;

    use super::*;

    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct FakePresentation {
        fail: AtomicBool,
    }

    #[async_trait]
    impl Presentation for FakePresentation {
        async fn apply(&self, _update: PresentationUpdate) -> Result<CanvasState> {
            if self.fail.load(Ordering::Relaxed) {
                anyhow::bail!("presenter failed");
            }
            Ok(CanvasState {
                zoom: 1.0,
                fitted: true,
                ..CanvasState::default()
            })
        }

        async fn canvas(&self, _operation: CanvasOperation) -> Result<CanvasOutput> {
            Ok(CanvasOutput::State(CanvasState::default()))
        }
    }

    struct FakeRenderer;

    #[async_trait]
    impl PageRenderer for FakeRenderer {
        async fn render(
            &self,
            _snapshot: &koharu_scene::Snapshot,
            _page: EntityId,
        ) -> Result<koharu_renderer::Frame> {
            anyhow::bail!("empty test projects do not render pages")
        }

        async fn rasterize(&self, _frame: &koharu_renderer::Frame) -> Result<image::RgbaImage> {
            anyhow::bail!("empty test projects do not rasterize pages")
        }

        async fn export_psd(
            &self,
            _snapshot: &koharu_scene::Snapshot,
            _frame: &koharu_renderer::Frame,
        ) -> Result<Vec<u8>> {
            anyhow::bail!("empty test projects do not export pages")
        }

        async fn available_fonts(&self) -> Result<Vec<koharu_renderer::FontFamily>> {
            Ok(Vec::new())
        }

        async fn font_preview(&self, _family_name: &str) -> Result<Vec<u8>> {
            Ok(vec![1, 2, 3])
        }

        fn discard_retained_nodes(&self) {}
    }

    struct FakeDialogs;

    #[async_trait]
    impl FileDialogs for FakeDialogs {
        async fn pick_files(&self, _filters: &[DialogFilter]) -> Result<Option<Vec<PathBuf>>> {
            Ok(None)
        }

        async fn pick_folder(&self) -> Result<Option<PathBuf>> {
            Ok(None)
        }

        async fn save_file(
            &self,
            _suggested_name: &str,
            _filters: &[DialogFilter],
        ) -> Result<Option<PathBuf>> {
            Ok(None)
        }
    }

    struct FakeProcessing;

    #[async_trait]
    impl ProcessingRuntime for FakeProcessing {
        async fn initialize(
            &self,
        ) -> Result<Option<tokio::sync::watch::Receiver<koharu_pipeline::ResourceSnapshot>>>
        {
            Ok(None)
        }

        async fn execute(
            &self,
            _snapshot: koharu_scene::Snapshot,
            _request: koharu_pipeline::Request,
            _committer: &mut dyn koharu_pipeline::Committer,
        ) -> Result<koharu_pipeline::Report> {
            anyhow::bail!("test processing is disabled")
        }
    }

    fn application(root: &TempDir, fail: bool) -> Application {
        Application::with_components(
            ProjectLibrary::new(root.path()).unwrap(),
            Arc::new(FakePresentation {
                fail: AtomicBool::new(fail),
            }),
            Arc::new(FakeRenderer),
            Arc::new(FakeDialogs),
            Arc::new(FakeProcessing),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn startup_publishes_ready_once() {
        let _test = TEST_LOCK.lock().await;
        let root = TempDir::new().unwrap();
        let app = application(&root, false);
        let mut events = app.events().subscribe();
        let startup = app.initialize().await.unwrap();
        assert!(startup.canvas.fitted);
        assert!(matches!(
            events.recv().await.unwrap().event,
            AppEvent::StartupReady { .. }
        ));
        assert!(matches!(app.lifecycle().await, Lifecycle::Ready(_)));
        app.initialize().await.unwrap();
        assert!(matches!(app.lifecycle().await, Lifecycle::Ready(_)));
    }

    #[tokio::test]
    async fn startup_failure_is_durable_and_published() {
        let _test = TEST_LOCK.lock().await;
        let root = TempDir::new().unwrap();
        let app = application(&root, true);
        let mut events = app.events().subscribe();
        assert_eq!(
            app.initialize().await.unwrap_err().message,
            "presenter failed"
        );
        assert!(matches!(
            events.recv().await.unwrap().event,
            AppEvent::StartupFailed { .. }
        ));
        assert!(matches!(app.lifecycle().await, Lifecycle::Failed(_)));
    }

    #[tokio::test]
    async fn project_commands_use_application_state_and_publish_ordered_events() {
        let _test = TEST_LOCK.lock().await;
        let root = TempDir::new().unwrap();
        let app = application(&root, false);
        let mut events = app.events().subscribe();
        app.initialize().await.unwrap();
        assert_eq!(events.recv().await.unwrap().sequence, 1);
        assert!(matches!(
            app.dispatch(Command::CreateProject {
                name: "Chapter 1".to_owned()
            })
            .await
            .unwrap(),
            DispatchOutput { result: CommandResult::Unit(()), attachments } if attachments.is_empty()
        ));
        assert!(matches!(
            app.dispatch(Command::GetProject {}).await.unwrap(),
            DispatchOutput { result: CommandResult::Project(Some(ProjectInfo { ref name, .. })), .. } if name == "Chapter 1"
        ));
        assert_eq!(events.recv().await.unwrap().sequence, 2);
        assert_eq!(events.recv().await.unwrap().sequence, 3);
        assert!(matches!(
            app.dispatch(Command::ListProjects {}).await.unwrap(),
            DispatchOutput { result: CommandResult::Projects(projects), .. } if projects.len() == 1
        ));

        let response = Response::success(RequestId::new(), CommandResult::Unit(()));
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn binary_marker_matches_owned_attachment_payload() {
        let _test = TEST_LOCK.lock().await;
        let root = TempDir::new().unwrap();
        let app = application(&root, false);
        let output = app
            .dispatch(Command::GetFontPreview {
                family_name: "Test".to_owned(),
            })
            .await
            .unwrap();
        let CommandResult::Binary(marker) = output.result else {
            panic!("expected a binary marker");
        };
        assert_eq!(output.attachments.len(), 1);
        assert_eq!(output.attachments[0].id, marker.attachment);
        assert!(
            marker
                .attachment
                .chars()
                .all(|character| character.is_ascii_digit())
        );
        assert_eq!(output.attachments[0].bytes, [1, 2, 3]);
    }
}
