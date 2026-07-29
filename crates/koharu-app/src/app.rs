use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{Project, classify_error, failure};
use anyhow::{Result, anyhow};
use koharu_canvas::{
    Brush, Camera, DisplayState, ElementFrame, MaskOverlay as CanvasMaskOverlay,
    MaskPlane as CanvasMaskPlane, PageView as NativePageView, PhysicalPoint, StrokeMode,
};
use koharu_config::Config;
use koharu_desktop::{
    Application, DesktopContext, Frontend, MaskEncodingResult, Options as DesktopOptions,
};
use koharu_pipeline::{PipelineConfig, StopToken};
use koharu_scene::{
    Asset, AssetMetadata, BlobId, EntityId, LanguageTag, Revision, SceneChangeSet, SceneComponent,
    SceneSession,
};
use koharu_translator::TranslationConfig;
use rust_embed::Embed;
use serde_json::Value;

use crate::{
    jobs::{Background, ExportRequest, NativeEvent, PipelineRequest},
    protocol::{
        AppCommand, AppError, AppErrorCode, AppEvent, BridgeMessage, CanvasInteraction,
        CanvasPageView, DownloadStatus, FontFaceStyleView, FontFaceView, FontSourceView, JobKind,
        JobStatus, MaskPlane, RequestId, SettingsView, SystemResourcesView, TargetLanguageView,
        TranslationSettings,
    },
    resources::Resources,
};

const EVENT_NAME: &str = "app";

#[derive(Embed)]
#[folder = "$CARGO_WORKSPACE_DIR/ui/out/"]
#[allow_missing = true]
struct Assets;

pub fn run(initial_path: Option<PathBuf>) -> Result<()> {
    let pipeline = koharu_config::load::<PipelineConfig>("pipeline")?;
    let translation = TranslationConfig::load()?;
    let background = Background::new(pipeline.clone(), translation.clone())?;
    let resources = Resources::new();
    let render_resources = koharu_renderer::RenderResources::new();
    let fonts = render_resources
        .fonts()
        .available_fonts()
        .into_iter()
        .map(font_face_view)
        .collect();
    koharu_desktop::run(
        DesktopOptions {
            decorations: false,
            frontend: frontend(),
            protocols: vec![resources.protocol()],
            ..DesktopOptions::default()
        },
        App::new(
            background,
            pipeline,
            translation,
            resources,
            fonts,
            initial_path,
        ),
    )
}

fn frontend() -> Frontend {
    if cfg!(debug_assertions) {
        Frontend::Url("http://localhost:3000".into())
    } else {
        Frontend::embedded(|path| Assets::get(path).map(|asset| asset.data))
    }
}

fn font_face_view(value: koharu_renderer::FontFaceInfo) -> FontFaceView {
    FontFaceView {
        family_name: value.family_name,
        post_script_name: value.post_script_name,
        weight: value.weight,
        stretch: value.stretch,
        style: match value.style {
            koharu_renderer::FontFaceStyle::Normal => FontFaceStyleView::Normal,
            koharu_renderer::FontFaceStyle::Italic => FontFaceStyleView::Italic,
            koharu_renderer::FontFaceStyle::Oblique => FontFaceStyleView::Oblique,
        },
        source: match value.source {
            koharu_renderer::FontSource::System => FontSourceView::System,
            koharu_renderer::FontSource::Registered => FontSourceView::Registered,
        },
    }
}

pub struct App {
    project: Option<Project>,
    background: Background,
    pipeline: Config<PipelineConfig>,
    translation: Config<TranslationConfig>,
    resources: Resources,
    fonts: Vec<FontFaceView>,
    jobs: HashMap<RequestId, RunningJob>,
    downloads: HashMap<u64, DownloadStatus>,
    system_resources: SystemResourcesView,
    pending_masks: HashSet<(EntityId, CanvasMaskPlane, u64)>,
    initial_path: Option<PathBuf>,
    initialization_error: Option<String>,
    initialized: bool,
    frontend_ready: bool,
    auto_fit: bool,
}

struct RunningJob {
    stop: StopToken,
    status: JobStatus,
}

enum CommandOutcome {
    Accepted(Revision),
    Cancelled(Revision),
}

impl App {
    fn new(
        background: Background,
        pipeline: Config<PipelineConfig>,
        translation: Config<TranslationConfig>,
        resources: Resources,
        fonts: Vec<FontFaceView>,
        initial_path: Option<PathBuf>,
    ) -> Self {
        Self {
            project: None,
            background,
            pipeline,
            translation,
            resources,
            fonts,
            jobs: HashMap::new(),
            downloads: HashMap::new(),
            system_resources: SystemResourcesView::default(),
            pending_masks: HashSet::new(),
            initial_path,
            initialization_error: None,
            initialized: false,
            frontend_ready: false,
            auto_fit: true,
        }
    }

    fn open(&mut self, path: PathBuf, desktop: &mut DesktopContext<'_, NativeEvent>) -> Result<()> {
        let project = Project::open(path)?;
        self.install_project(project, desktop)
    }

    fn create(
        &mut self,
        path: PathBuf,
        desktop: &mut DesktopContext<'_, NativeEvent>,
    ) -> Result<()> {
        let project = Project::create(path)?;
        self.install_project(project, desktop)
    }

    fn install_project(
        &mut self,
        project: Project,
        desktop: &mut DesktopContext<'_, NativeEvent>,
    ) -> Result<()> {
        for job in self.jobs.values() {
            job.stop.stop();
        }
        self.jobs.clear();
        self.pending_masks.clear();
        self.resources
            .install(&project.snapshot(), project.path())?;
        self.project = Some(project);
        self.auto_fit = true;
        self.show_visible_page(desktop)?;
        Ok(())
    }

    fn close(&mut self, desktop: &mut DesktopContext<'_, NativeEvent>) -> Result<()> {
        for job in self.jobs.values() {
            job.stop.stop();
        }
        self.jobs.clear();
        self.pending_masks.clear();
        self.project = None;
        self.resources.clear();
        self.auto_fit = true;
        desktop.clear_page();
        desktop.emit(EVENT_NAME, AppEvent::ProjectClosed)
    }

    fn handle_message(
        &mut self,
        desktop: &mut DesktopContext<'_, NativeEvent>,
        message: BridgeMessage,
    ) -> Result<()> {
        match message {
            BridgeMessage::Interaction { interaction } => {
                if let Err(error) = self.interact(desktop, interaction) {
                    let code = classify_error(&error);
                    self.problem(desktop, code, error)?;
                }
                Ok(())
            }
            BridgeMessage::Command { id, base, command } => {
                if let Err(error) = self.refresh(desktop) {
                    self.reject(desktop, id, error)?;
                    return Ok(());
                }
                match self.command(desktop, id, base, *command) {
                    Ok(CommandOutcome::Accepted(revision)) => {
                        desktop.emit(EVENT_NAME, AppEvent::Accepted { id, revision })
                    }
                    Ok(CommandOutcome::Cancelled(revision)) => {
                        desktop.emit(EVENT_NAME, AppEvent::CommandCancelled { id, revision })
                    }
                    Err(error) => self.reject(desktop, id, error),
                }
            }
            BridgeMessage::Ready { .. }
            | BridgeMessage::Viewport { .. }
            | BridgeMessage::Window { .. } => self.problem(
                desktop,
                AppErrorCode::InvalidInput,
                anyhow!("desktop shell message reached the application protocol"),
            ),
        }
    }

    fn command(
        &mut self,
        desktop: &mut DesktopContext<'_, NativeEvent>,
        id: RequestId,
        base: Revision,
        command: AppCommand,
    ) -> Result<CommandOutcome> {
        if !self.initialized && !matches!(&command, AppCommand::Synchronize) {
            return Err(app_failure(
                AppErrorCode::Busy,
                "native runtimes are still initializing",
            ));
        }
        if !self.pending_masks.is_empty()
            && !matches!(
                &command,
                AppCommand::Synchronize
                    | AppCommand::StopJob { .. }
                    | AppCommand::GetSettings
                    | AppCommand::SetSettings { .. }
            )
        {
            return Err(app_failure(
                AppErrorCode::Busy,
                "mask changes are still being committed",
            ));
        }
        match command {
            AppCommand::Synchronize => {
                if self.initialized {
                    self.emit_state(desktop)?;
                }
                return Ok(CommandOutcome::Accepted(self.current_revision()));
            }
            AppCommand::CreateProject => {
                self.ensure_masks_committed()?;
                let Some(mut path) = project_dialog().set_file_name("Untitled.khr").save_file()
                else {
                    return Ok(CommandOutcome::Cancelled(self.current_revision()));
                };
                if path.extension().is_none() {
                    path.set_extension("khr");
                }
                self.create(path, desktop)?;
                self.emit_project(desktop)?;
                return Ok(CommandOutcome::Accepted(self.current_revision()));
            }
            AppCommand::OpenProject => {
                self.ensure_masks_committed()?;
                let Some(path) = project_dialog().pick_file() else {
                    return Ok(CommandOutcome::Cancelled(self.current_revision()));
                };
                self.open(path, desktop)?;
                self.emit_project(desktop)?;
                return Ok(CommandOutcome::Accepted(self.current_revision()));
            }
            AppCommand::CloseProject => {
                self.ensure_masks_committed()?;
                self.close(desktop)?;
                return Ok(CommandOutcome::Accepted(Revision::ZERO));
            }
            AppCommand::ImportPages => {
                self.require_base(base)?;
                let Some(files) = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                    .pick_files()
                    .filter(|files| !files.is_empty())
                else {
                    return Ok(CommandOutcome::Cancelled(base));
                };
                self.ensure_free_job(id)?;
                self.ensure_job_kind_available(JobKind::Import)?;
                let path = self.project_path()?.to_owned();
                let stop = self.background.import(id, path, files, desktop.handle())?;
                let status = JobStatus::Running {
                    id,
                    kind: JobKind::Import,
                    completed: 0,
                    total: 0,
                    page: None,
                    phase: None,
                    model: None,
                };
                self.jobs.insert(
                    id,
                    RunningJob {
                        stop,
                        status: status.clone(),
                    },
                );
                desktop.emit(EVENT_NAME, AppEvent::JobChanged(status))?;
                return Ok(CommandOutcome::Accepted(base));
            }
            AppCommand::Undo => {
                return self.undo(desktop, base).map(CommandOutcome::Accepted);
            }
            AppCommand::Redo => {
                return self.redo(desktop, base).map(CommandOutcome::Accepted);
            }
            AppCommand::RunPipeline { scope, operation } => {
                self.require_base(base)?;
                self.ensure_free_job(id)?;
                self.ensure_job_kind_available(JobKind::Pipeline)?;
                let stop = self.background.run_pipeline(
                    PipelineRequest {
                        id,
                        path: self.project_path()?.to_owned(),
                        scope,
                        operation,
                    },
                    desktop.handle(),
                )?;
                let status = JobStatus::Running {
                    id,
                    kind: JobKind::Pipeline,
                    completed: 0,
                    total: 0,
                    page: None,
                    phase: None,
                    model: None,
                };
                self.jobs.insert(
                    id,
                    RunningJob {
                        stop,
                        status: status.clone(),
                    },
                );
                desktop.emit(EVENT_NAME, AppEvent::JobChanged(status))?;
                return Ok(CommandOutcome::Accepted(base));
            }
            AppCommand::StopJob { job } => {
                self.require_base(base)?;
                self.jobs
                    .get(&job)
                    .ok_or_else(|| {
                        app_failure(AppErrorCode::NotFound, format!("job {job} is not running"))
                    })?
                    .stop
                    .stop();
                return Ok(CommandOutcome::Accepted(base));
            }
            AppCommand::ExportPages { pages, format } => {
                self.require_base(base)?;
                self.ensure_free_job(id)?;
                self.ensure_job_kind_available(JobKind::Export)?;
                let Some(directory) = rfd::FileDialog::new().pick_folder() else {
                    return Ok(CommandOutcome::Cancelled(base));
                };
                let pages = if pages.is_empty() {
                    self.session()?
                        .snapshot()
                        .pages()
                        .map(|page| page.id())
                        .collect()
                } else {
                    pages
                };
                if pages.is_empty() {
                    return Err(app_failure(
                        AppErrorCode::InvalidInput,
                        "there are no pages to export",
                    ));
                }
                let stop = self.background.export(
                    ExportRequest {
                        id,
                        snapshot: self.session()?.snapshot(),
                        directory,
                        pages,
                        format,
                        locale: Some(self.target_locale()?),
                    },
                    desktop.handle(),
                )?;
                let status = JobStatus::Running {
                    id,
                    kind: JobKind::Export,
                    completed: 0,
                    total: 0,
                    page: None,
                    phase: None,
                    model: None,
                };
                self.jobs.insert(
                    id,
                    RunningJob {
                        stop,
                        status: status.clone(),
                    },
                );
                desktop.emit(EVENT_NAME, AppEvent::JobChanged(status))?;
                return Ok(CommandOutcome::Accepted(base));
            }
            AppCommand::CollectGarbage => {
                self.require_base(base)?;
                if !self.jobs.is_empty() {
                    return Err(app_failure(
                        AppErrorCode::Busy,
                        "garbage collection is unavailable while a job is running",
                    ));
                }
                let report = self.session_mut()?.gc()?;
                desktop.emit(
                    EVENT_NAME,
                    AppEvent::GarbageCollected {
                        blobs: report.blobs,
                        bytes: report.bytes,
                    },
                )?;
                return Ok(CommandOutcome::Accepted(base));
            }
            AppCommand::GetSettings => {
                self.emit_settings(desktop)?;
                return Ok(CommandOutcome::Accepted(self.current_revision()));
            }
            AppCommand::SetSettings {
                pipeline,
                translation,
            } => {
                let (translation, credentials) = (*translation).into_parts()?;
                koharu_pipeline::Pipeline::new(pipeline.clone(), translation.clone())?;
                let target_language_changed =
                    self.translation.read()?.target_language != translation.target_language;
                let locale = LanguageTag::new(translation.target_language.clone())?;
                {
                    let mut current = self.pipeline.write()?;
                    *current = pipeline.clone();
                    current.save()?;
                }
                {
                    credentials.save()?;
                    let mut current = self.translation.write()?;
                    *current = translation.clone();
                    current.save()?;
                }
                self.background.reconfigure(pipeline, translation)?;
                desktop.canvas().set_locale(Some(locale.clone()));
                self.emit_settings(desktop)?;
                if target_language_changed
                    && let Some(page) = self.project.as_ref().and_then(Project::visible_page)
                {
                    desktop.emit(
                        EVENT_NAME,
                        AppEvent::PageLoaded {
                            revision: self.current_revision(),
                            page: self.project()?.page_view(page, Some(&locale))?,
                        },
                    )?;
                }
                return Ok(CommandOutcome::Accepted(self.current_revision()));
            }
            AppCommand::FinishTransform => {
                if let Err(error) = self.require_base(base) {
                    desktop.canvas().cancel_transform();
                    return Err(error);
                }
                let Some(commit) = desktop.canvas().finish_transform()? else {
                    return Ok(CommandOutcome::Accepted(base));
                };
                let changes = self.project_mut()?.set_geometries(
                    commit
                        .elements
                        .into_iter()
                        .map(|element| (element.element, element.geometry)),
                )?;
                self.present_changes(desktop, &changes)?;
                return Ok(CommandOutcome::Accepted(changes.to));
            }
            _ => {}
        }

        self.require_base(base)?;
        if matches!(&command, AppCommand::DeletePages { pages } if pages.iter().any(|page| self.pending_masks.iter().any(|(pending, _, _)| pending == page)))
        {
            return Err(app_failure(
                AppErrorCode::Busy,
                "a page mask is still being committed",
            ));
        }
        let changes = self.project_mut()?.apply(command)?;
        self.present_changes(desktop, &changes)?;
        Ok(CommandOutcome::Accepted(changes.to))
    }

    fn undo(
        &mut self,
        desktop: &mut DesktopContext<'_, NativeEvent>,
        base: Revision,
    ) -> Result<Revision> {
        let changes = self.project_mut()?.undo(base)?;
        self.present_changes(desktop, &changes)?;
        Ok(changes.to)
    }

    fn redo(
        &mut self,
        desktop: &mut DesktopContext<'_, NativeEvent>,
        base: Revision,
    ) -> Result<Revision> {
        let changes = self.project_mut()?.redo(base)?;
        self.present_changes(desktop, &changes)?;
        Ok(changes.to)
    }

    fn interact(
        &mut self,
        desktop: &mut DesktopContext<'_, NativeEvent>,
        interaction: CanvasInteraction,
    ) -> Result<()> {
        match interaction {
            CanvasInteraction::ShowPage { page } => {
                self.ensure_masks_committed()?;
                self.project_mut()?.show_page(page)?;
                desktop.show_page(&self.session()?.snapshot(), page)?;
                desktop.canvas().set_locale(Some(self.target_locale()?));
                self.auto_fit = true;
                self.fit_window(desktop)?;
                let locale = self.target_locale()?;
                desktop.emit(
                    EVENT_NAME,
                    AppEvent::PageLoaded {
                        revision: self.current_revision(),
                        page: self.project()?.page_view(page, Some(&locale))?,
                    },
                )?;
            }
            CanvasInteraction::SetCamera { zoom, translation } => {
                let mut view = desktop.view().clone();
                view.camera = Camera::new(zoom, translation)?;
                self.auto_fit = false;
                desktop.set_view(view);
                self.emit_view(desktop)?;
            }
            CanvasInteraction::SetZoom { zoom } => {
                if !zoom.is_finite() || !(0.02..=16.0).contains(&zoom) {
                    return Err(app_failure(
                        AppErrorCode::InvalidInput,
                        "camera zoom must be between 2% and 1600%",
                    ));
                }
                let viewport = desktop.viewport().size();
                let mut view = desktop.view().clone();
                view.camera.zoom_around(
                    PhysicalPoint::new(
                        f64::from(viewport.width) * 0.5,
                        f64::from(viewport.height) * 0.5,
                    ),
                    zoom,
                )?;
                self.auto_fit = false;
                desktop.set_view(view);
                self.emit_view(desktop)?;
            }
            CanvasInteraction::FitWindow => self.fit_window(desktop)?,
            CanvasInteraction::SetDisplay { display } => {
                if [display.text_mask, display.brush_mask]
                    .into_iter()
                    .flatten()
                    .any(|overlay| {
                        !overlay.opacity.is_finite() || !(0.0..=1.0).contains(&overlay.opacity)
                    })
                {
                    return Err(app_failure(
                        AppErrorCode::InvalidInput,
                        "mask overlay opacity must be between zero and one",
                    ));
                }
                let mut view = desktop.view().clone();
                view.display = DisplayState {
                    page: match display.page {
                        CanvasPageView::Source => NativePageView::EditableSource,
                        CanvasPageView::Clean => NativePageView::EditableClean,
                        CanvasPageView::Rendered => NativePageView::Rendered,
                    },
                    show_text: display.show_text,
                    text_mask: display
                        .text_mask
                        .map(|overlay| CanvasMaskOverlay::new(overlay.tint, overlay.opacity)),
                    brush_mask: display
                        .brush_mask
                        .map(|overlay| CanvasMaskOverlay::new(overlay.tint, overlay.opacity)),
                    transition: view.display.transition,
                };
                desktop.set_view(view);
            }
            CanvasInteraction::BeginTransform { elements } => {
                desktop.canvas().begin_transform(&elements)?
            }
            CanvasInteraction::UpdateTransform { frame, elements } => {
                let elements = elements
                    .into_iter()
                    .map(|element| ElementFrame {
                        element: element.element,
                        frame: canvas_frame(element.frame),
                    })
                    .collect::<Vec<_>>();
                desktop.canvas().update_transform(frame, &elements)?;
            }
            CanvasInteraction::CancelTransform => desktop.canvas().cancel_transform(),
            CanvasInteraction::BeginMaskStroke {
                plane,
                diameter,
                erase,
                x,
                y,
            } => desktop.canvas().begin_mask_stroke(
                mask_plane(plane),
                Brush {
                    diameter,
                    mode: if erase {
                        StrokeMode::Erase
                    } else {
                        StrokeMode::Paint
                    },
                },
                PhysicalPoint::new(x, y),
            )?,
            CanvasInteraction::ExtendMaskStroke { x, y } => desktop
                .canvas()
                .extend_mask_stroke(PhysicalPoint::new(x, y))?,
            CanvasInteraction::FinishMaskStroke => {
                if let Some(commit) = desktop.canvas().finish_mask_stroke()? {
                    self.pending_masks
                        .insert((commit.page, commit.plane, commit.generation));
                    desktop.submit_mask(commit);
                }
            }
            CanvasInteraction::CancelMaskStroke => desktop.canvas().cancel_mask_stroke(),
        }
        Ok(())
    }

    fn refresh(&mut self, desktop: &mut DesktopContext<'_, NativeEvent>) -> Result<()> {
        let Some(project) = self.project.as_mut() else {
            return Ok(());
        };
        let changes = project.refresh()?;
        if changes.to != changes.from {
            self.present_changes(desktop, &changes)?;
        }
        Ok(())
    }

    fn present_changes(
        &mut self,
        desktop: &mut DesktopContext<'_, NativeEvent>,
        changes: &SceneChangeSet,
    ) -> Result<()> {
        let previous_page = self.project()?.visible_page();
        if !changes.entities.is_empty()
            || changes
                .components
                .iter()
                .any(|change| change.kind == Asset::KIND)
        {
            let project = self.project()?;
            self.resources
                .install(&project.snapshot(), project.path())?;
        }
        self.project_mut()?.reconcile_visible_page();
        let visible_page = self.project()?.visible_page();
        if visible_page != previous_page {
            self.show_visible_page(desktop)?;
            self.auto_fit = true;
            self.fit_window(desktop)?;
        } else {
            desktop.sync(&self.session()?.snapshot(), changes)?;
        }
        let locale = self.target_locale()?;
        let delta = self.project_mut()?.delta(changes, Some(&locale))?;
        desktop.emit(EVENT_NAME, AppEvent::ProjectChanged(Box::new(delta)))?;
        if visible_page != previous_page
            && let Some(page) = visible_page
        {
            let locale = self.target_locale()?;
            desktop.emit(
                EVENT_NAME,
                AppEvent::PageLoaded {
                    revision: self.current_revision(),
                    page: self.project()?.page_view(page, Some(&locale))?,
                },
            )?;
        }
        Ok(())
    }

    fn show_visible_page(&self, desktop: &mut DesktopContext<'_, NativeEvent>) -> Result<()> {
        if let Some(project) = &self.project
            && let Some(page) = project.visible_page()
        {
            desktop.show_page(&project.snapshot(), page)?;
            desktop.canvas().set_locale(Some(self.target_locale()?));
        } else {
            desktop.clear_page();
        }
        Ok(())
    }

    fn fit_window(&mut self, desktop: &mut DesktopContext<'_, NativeEvent>) -> Result<()> {
        let Some(page) = self.project.as_ref().and_then(Project::visible_page) else {
            return Ok(());
        };
        let page = self.session()?.snapshot().page(page)?.page()?;
        let size =
            koharu_canvas::PhysicalSize::new(page.width.ceil() as u32, page.height.ceil() as u32);
        let mut view = desktop.view().clone();
        view.camera = Camera::contain(desktop.viewport().size(), size);
        self.auto_fit = true;
        desktop.set_view(view);
        self.emit_view(desktop)?;
        Ok(())
    }

    fn emit_view(&self, desktop: &DesktopContext<'_, NativeEvent>) -> Result<()> {
        let camera = desktop.view().camera;
        desktop.emit(
            EVENT_NAME,
            AppEvent::ViewChanged {
                zoom: camera.zoom(),
                translation: camera.translation(),
                auto_fit: self.auto_fit,
            },
        )
    }

    fn emit_project(&self, desktop: &DesktopContext<'_, NativeEvent>) -> Result<()> {
        let Some(project) = &self.project else {
            return desktop.emit(EVENT_NAME, AppEvent::ProjectClosed);
        };
        let session = project.session();
        let snapshot = session.snapshot();
        desktop.emit(
            EVENT_NAME,
            AppEvent::ProjectOpened {
                revision: snapshot.revision(),
                project: project.header(),
                pages: project.page_summaries()?,
            },
        )?;
        if let Some(page) = project.visible_page() {
            let locale = self.target_locale()?;
            desktop.emit(
                EVENT_NAME,
                AppEvent::PageLoaded {
                    revision: snapshot.revision(),
                    page: project.page_view(page, Some(&locale))?,
                },
            )?;
        }
        Ok(())
    }

    fn emit_settings(&self, desktop: &DesktopContext<'_, NativeEvent>) -> Result<()> {
        desktop.emit(
            EVENT_NAME,
            AppEvent::SettingsChanged {
                settings: Box::new(SettingsView {
                    pipeline: self.pipeline.read()?.clone(),
                    translation: TranslationSettings::from_config(&*self.translation.read()?)?,
                    local_translation_models: koharu_translator::local_models()
                        .into_iter()
                        .map(|model| model.id.to_owned())
                        .collect(),
                    target_languages: koharu_translator::Language::ALL
                        .iter()
                        .map(|language| TargetLanguageView {
                            tag: language.tag().to_owned(),
                            name: language.to_string(),
                        })
                        .collect(),
                    fonts: self.fonts.clone(),
                }),
            },
        )
    }

    fn emit_state(&self, desktop: &DesktopContext<'_, NativeEvent>) -> Result<()> {
        self.emit_project(desktop)?;
        self.emit_settings(desktop)?;
        self.emit_view(desktop)?;
        desktop.emit(
            EVENT_NAME,
            AppEvent::ResourcesChanged {
                resources: self.system_resources.clone(),
            },
        )?;
        for job in self.jobs.values() {
            desktop.emit(EVENT_NAME, AppEvent::JobChanged(job.status.clone()))?;
        }
        for download in self.downloads.values() {
            desktop.emit(EVENT_NAME, AppEvent::DownloadChanged(download.clone()))?;
        }
        Ok(())
    }

    fn reject(
        &self,
        desktop: &DesktopContext<'_, NativeEvent>,
        id: RequestId,
        error: anyhow::Error,
    ) -> Result<()> {
        let code = classify_error(&error);
        desktop.emit(
            EVENT_NAME,
            AppEvent::Rejected {
                id,
                error: AppError {
                    code,
                    message: error.to_string(),
                    current_revision: self.revision(),
                },
            },
        )
    }

    fn problem(
        &self,
        desktop: &DesktopContext<'_, NativeEvent>,
        code: AppErrorCode,
        error: impl std::fmt::Display,
    ) -> Result<()> {
        desktop.emit(
            EVENT_NAME,
            AppEvent::Problem {
                error: AppError {
                    code,
                    message: error.to_string(),
                    current_revision: self.revision(),
                },
            },
        )
    }

    fn require_base(&self, base: Revision) -> Result<()> {
        self.project()?.require_base(base)
    }

    fn ensure_free_job(&self, id: RequestId) -> Result<()> {
        if self.jobs.contains_key(&id) {
            return Err(app_failure(
                AppErrorCode::Busy,
                format!("job {id} is already running"),
            ));
        }
        Ok(())
    }

    fn ensure_job_kind_available(&self, kind: JobKind) -> Result<()> {
        if kind == JobKind::Export {
            return Ok(());
        }
        if let Some(running) = self.jobs.values().find_map(|job| match &job.status {
            JobStatus::Running { kind, .. } if *kind != JobKind::Export => Some(*kind),
            _ => None,
        }) {
            return Err(app_failure(
                AppErrorCode::Busy,
                format!("cannot start {kind:?} while {running:?} is running"),
            ));
        }
        Ok(())
    }

    fn ensure_masks_committed(&self) -> Result<()> {
        if !self.pending_masks.is_empty() {
            return Err(app_failure(
                AppErrorCode::Busy,
                "mask changes are still being committed",
            ));
        }
        Ok(())
    }

    fn project(&self) -> Result<&Project> {
        self.project
            .as_ref()
            .ok_or_else(|| app_failure(AppErrorCode::NoProject, "no project is open"))
    }

    fn project_mut(&mut self) -> Result<&mut Project> {
        self.project
            .as_mut()
            .ok_or_else(|| app_failure(AppErrorCode::NoProject, "no project is open"))
    }

    fn session(&self) -> Result<&SceneSession> {
        Ok(self.project()?.session())
    }

    fn session_mut(&mut self) -> Result<&mut SceneSession> {
        Ok(self.project_mut()?.session_mut())
    }

    fn target_locale(&self) -> Result<LanguageTag> {
        LanguageTag::new(self.translation.read()?.target_language.clone()).map_err(Into::into)
    }

    fn project_path(&self) -> Result<&Path> {
        Ok(self.project()?.path())
    }

    fn revision(&self) -> Option<Revision> {
        self.project.as_ref().map(Project::revision)
    }

    fn current_revision(&self) -> Revision {
        self.revision().unwrap_or(Revision::ZERO)
    }
}

fn app_failure(code: AppErrorCode, message: impl std::fmt::Display) -> anyhow::Error {
    failure(code, message)
}

impl Application for App {
    type Event = NativeEvent;

    fn started(&mut self, desktop: &mut DesktopContext<'_, Self::Event>) -> Result<()> {
        self.background.subscribe_downloads(desktop.handle());
        self.background.subscribe_resources(desktop.handle());
        start_runtime_initialization(desktop.handle());
        Ok(())
    }

    fn ready(
        &mut self,
        desktop: &mut DesktopContext<'_, Self::Event>,
        _dpr: f64,
        _width: f64,
        _height: f64,
    ) -> Result<()> {
        self.frontend_ready = true;
        if !self.initialized {
            if let Some(message) = &self.initialization_error {
                desktop.emit(
                    EVENT_NAME,
                    AppEvent::Problem {
                        error: AppError {
                            code: AppErrorCode::Internal,
                            message: message.clone(),
                            current_revision: self.revision(),
                        },
                    },
                )?;
            }
            return Ok(());
        }
        if self.auto_fit {
            self.fit_window(desktop)?;
        }
        self.emit_state(desktop)?;
        Ok(())
    }

    fn message(
        &mut self,
        desktop: &mut DesktopContext<'_, Self::Event>,
        message: Value,
    ) -> Result<()> {
        match serde_json::from_value(message) {
            Ok(message) => self.handle_message(desktop, message),
            Err(error) => self.problem(desktop, AppErrorCode::InvalidInput, error),
        }
    }

    fn event(
        &mut self,
        desktop: &mut DesktopContext<'_, Self::Event>,
        event: Self::Event,
    ) -> Result<()> {
        match event {
            NativeEvent::RuntimeInitialized => {
                if self.initialized {
                    return Ok(());
                }
                self.initialized = true;
                self.initialization_error = None;
                if let Some(path) = self.initial_path.take()
                    && let Err(error) = self.open(path, desktop)
                {
                    self.problem(desktop, AppErrorCode::IoFailed, error)?;
                }
                if self.frontend_ready {
                    self.emit_state(desktop)?;
                }
                Ok(())
            }
            NativeEvent::RuntimeInitializationFailed {
                error,
                retry_after_ms,
            } => {
                let message = format!(
                    "Native runtime initialization failed: {error}. Retrying in {:.1}s.",
                    retry_after_ms as f64 / 1000.0
                );
                self.initialization_error = Some(message.clone());
                if self.frontend_ready {
                    desktop.emit(
                        EVENT_NAME,
                        AppEvent::Problem {
                            error: AppError {
                                code: AppErrorCode::Internal,
                                message,
                                current_revision: self.revision(),
                            },
                        },
                    )?;
                }
                Ok(())
            }
            NativeEvent::Download(event) => {
                let status = match event {
                    koharu_runtime::download::Event::Started { id, name } => {
                        DownloadStatus::Running {
                            id,
                            name,
                            completed: 0,
                            total: 0,
                        }
                    }
                    koharu_runtime::download::Event::Progress {
                        id,
                        name,
                        completed,
                        total,
                    } => DownloadStatus::Running {
                        id,
                        name,
                        completed,
                        total,
                    },
                    koharu_runtime::download::Event::Finished { id } => {
                        self.downloads.remove(&id);
                        return desktop.emit(
                            EVENT_NAME,
                            AppEvent::DownloadChanged(DownloadStatus::Finished { id }),
                        );
                    }
                    koharu_runtime::download::Event::Failed { id, name, error } => {
                        self.downloads.remove(&id);
                        return desktop.emit(
                            EVENT_NAME,
                            AppEvent::DownloadChanged(DownloadStatus::Failed { id, name, error }),
                        );
                    }
                };
                self.downloads.insert(
                    match &status {
                        DownloadStatus::Running { id, .. } => *id,
                        DownloadStatus::Finished { .. } | DownloadStatus::Failed { .. } => {
                            unreachable!("terminal download states return above")
                        }
                    },
                    status.clone(),
                );
                desktop.emit(EVENT_NAME, AppEvent::DownloadChanged(status))
            }
            NativeEvent::Resources(snapshot) => {
                self.system_resources = snapshot.into();
                if self.frontend_ready {
                    desktop.emit(
                        EVENT_NAME,
                        AppEvent::ResourcesChanged {
                            resources: self.system_resources.clone(),
                        },
                    )?;
                }
                Ok(())
            }
            NativeEvent::ProjectAdvanced { job } => {
                if self.jobs.contains_key(&job) {
                    self.refresh(desktop)?;
                }
                Ok(())
            }
            NativeEvent::PipelineProgress {
                job,
                completed,
                total,
                page,
                stage,
                model,
            } => {
                let Some(running) = self.jobs.get_mut(&job) else {
                    return Ok(());
                };
                let status = JobStatus::Running {
                    id: job,
                    kind: JobKind::Pipeline,
                    completed,
                    total,
                    page,
                    phase: stage,
                    model,
                };
                running.status = status.clone();
                desktop.emit(EVENT_NAME, AppEvent::JobChanged(status))
            }
            NativeEvent::ImportProgress {
                job,
                completed,
                total,
            } => {
                let Some(running) = self.jobs.get_mut(&job) else {
                    return Ok(());
                };
                let status = JobStatus::Running {
                    id: job,
                    kind: JobKind::Import,
                    completed,
                    total,
                    page: None,
                    phase: None,
                    model: None,
                };
                running.status = status.clone();
                desktop.emit(EVENT_NAME, AppEvent::JobChanged(status))
            }
            NativeEvent::ExportProgress {
                job,
                completed,
                total,
            } => {
                let Some(running) = self.jobs.get_mut(&job) else {
                    return Ok(());
                };
                let status = JobStatus::Running {
                    id: job,
                    kind: JobKind::Export,
                    completed,
                    total,
                    page: None,
                    phase: None,
                    model: None,
                };
                running.status = status.clone();
                desktop.emit(EVENT_NAME, AppEvent::JobChanged(status))
            }
            NativeEvent::Finished {
                job,
                revisions,
                pages: _,
                stopped,
                error,
            } => {
                if self.jobs.remove(&job).is_none() {
                    return Ok(());
                }
                self.refresh(desktop)?;
                if !revisions.is_empty() {
                    self.project_mut()?.record_revisions(revisions);
                    let delta = self.project()?.history_delta();
                    desktop.emit(EVENT_NAME, AppEvent::ProjectChanged(Box::new(delta)))?;
                }
                let status = if stopped {
                    JobStatus::Stopped { id: job }
                } else if let Some(error) = error {
                    JobStatus::Failed { id: job, error }
                } else {
                    JobStatus::Finished { id: job }
                };
                desktop.emit(EVENT_NAME, AppEvent::JobChanged(status))
            }
        }
    }

    fn viewport_changed(&mut self, desktop: &mut DesktopContext<'_, Self::Event>) -> Result<()> {
        if self.auto_fit {
            self.fit_window(desktop)?;
        }
        Ok(())
    }

    fn mask_encoded(
        &mut self,
        desktop: &mut DesktopContext<'_, Self::Event>,
        result: MaskEncodingResult,
    ) -> Result<()> {
        match result {
            MaskEncodingResult::Ready(mask) => {
                self.pending_masks
                    .remove(&(mask.page, mask.plane, mask.generation));
                let role = mask.plane.slot();
                let blob = BlobId::for_bytes(&mask.bytes);
                let changes = self.project_mut()?.set_asset(
                    mask.page,
                    role,
                    mask.bytes,
                    "image/png",
                    AssetMetadata {
                        width: Some(mask.size.width),
                        height: Some(mask.size.height),
                        attributes: Default::default(),
                    },
                )?;
                desktop.canvas().acknowledge_mask_commit(
                    mask.page,
                    mask.plane,
                    mask.generation,
                    blob,
                )?;
                self.present_changes(desktop, &changes)
            }
            MaskEncodingResult::Failed(error) => {
                self.pending_masks
                    .remove(&(error.page, error.plane, error.generation));
                self.problem(desktop, AppErrorCode::IoFailed, error)
            }
        }
    }

    fn close_requested(&mut self, _desktop: &mut DesktopContext<'_, Self::Event>) -> Result<bool> {
        for job in self.jobs.values() {
            job.stop.stop();
        }
        Ok(self.pending_masks.is_empty())
    }
}

fn start_runtime_initialization(handle: koharu_desktop::DesktopHandle<NativeEvent>) {
    tokio::spawn(async move {
        let mut attempt = 1_u64;
        let mut delay = std::time::Duration::from_secs(1);
        loop {
            match koharu_ml::init().await {
                Ok(()) => {
                    let _ = handle.send_event(NativeEvent::RuntimeInitialized);
                    return;
                }
                Err(error) => {
                    let jitter = std::time::Duration::from_millis(attempt.wrapping_mul(137) % 251);
                    let wait = delay + jitter;
                    tracing::error!(
                        attempt,
                        retry_after_ms = wait.as_millis(),
                        %error,
                        "runtime initialization failed; retrying automatically"
                    );
                    if handle
                        .send_event(NativeEvent::RuntimeInitializationFailed {
                            error: error.to_string(),
                            retry_after_ms: u64::try_from(wait.as_millis()).unwrap_or(u64::MAX),
                        })
                        .is_err()
                    {
                        return;
                    }
                    tokio::time::sleep(wait).await;
                    attempt = attempt.saturating_add(1);
                    delay = delay
                        .saturating_mul(2)
                        .min(std::time::Duration::from_secs(30));
                }
            }
        }
    });
}

fn project_dialog() -> rfd::FileDialog {
    let dialog = rfd::FileDialog::new().add_filter("Koharu project", &["khr"]);
    let Some(directory) = project_directory() else {
        return dialog;
    };
    dialog.set_directory(directory)
}

fn project_directory() -> Option<PathBuf> {
    let directory = dirs::document_dir()?.join("koharu");
    if let Err(error) = fs::create_dir_all(&directory) {
        tracing::warn!(%error, path = %directory.display(), "could not create the project directory");
        return None;
    }
    Some(directory)
}

const fn canvas_frame(frame: crate::protocol::Frame) -> koharu_canvas::Frame {
    koharu_canvas::Frame {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
        angle_degrees: frame.angle_degrees,
    }
}

const fn mask_plane(plane: MaskPlane) -> CanvasMaskPlane {
    match plane {
        MaskPlane::Text => CanvasMaskPlane::Text,
        MaskPlane::Brush => CanvasMaskPlane::Brush,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_matches_the_build_profile() {
        let frontend = frontend();

        if cfg!(debug_assertions) {
            assert!(matches!(
                frontend,
                Frontend::Url(url) if url == "http://localhost:3000"
            ));
        } else {
            assert!(matches!(frontend, Frontend::Embedded(_)));
        }
    }
}
