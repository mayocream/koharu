use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, anyhow, bail};
use koharu_canvas::{
    Brush, BrushCursor, Camera, DisplayState, ElementPreview as CanvasElementPreview,
    Guide as CanvasGuide, Handle as CanvasHandle, HitTarget as CanvasHitTarget,
    MaskOverlay as CanvasMaskOverlay, MaskPlane as CanvasMaskPlane, OverlayState,
    PageView as NativePageView, PhysicalPoint, StrokeMode,
};
use koharu_config::Config;
use koharu_desktop::{
    Application, DesktopContext, Frontend, MaskEncodingResult, Options as DesktopOptions,
};
use koharu_pipeline::{CancellationToken, PipelineConfig};
use koharu_scene::{
    ChangeSet, Command, Commands, ElementChange, PageAsset, PageId, Revision, Session,
};
use koharu_translator::TranslationConfig;
use rust_embed::Embed;
use serde_json::Value;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    jobs::{Background, ExportRequest, NativeEvent, PipelineRequest},
    protocol::{
        AppCommand, AppError, AppErrorCode, AppEvent, BridgeMessage, CanvasInteraction,
        CanvasPageView, DownloadStatus, FontFaceView, Handle, HitTarget, JobKind, JobStatus,
        MaskPlane, PageDelta, PageSummary, PageView, ProjectDelta, ProjectHeader, RequestId,
        SettingsView, TargetLanguageView, TranslationSettings,
    },
    resources::Resources,
};

const EVENT_NAME: &str = "app";

#[derive(Embed)]
#[folder = "$CARGO_WORKSPACE_DIR/ui/out/"]
#[allow_missing = true]
struct Assets;

pub fn run(initial_path: Option<PathBuf>) -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .with(crate::sentry::tracing_layer())
        .with(crate::tracing::TimingLayer::new())
        .init();

    let pipeline = koharu_config::load::<PipelineConfig>("pipeline")?;
    let translation = TranslationConfig::load()?;
    let background = Background::new(pipeline.clone(), translation.clone());
    let resources = Resources::new();
    let font_renderer = koharu_renderer::SceneRenderer::new()?;
    let fonts = font_renderer
        .available_fonts()?
        .into_iter()
        .map(FontFaceView::from)
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
            font_renderer,
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

pub struct App {
    session: Option<Session>,
    path: Option<PathBuf>,
    visible_page: Option<PageId>,
    background: Background,
    pipeline: Config<PipelineConfig>,
    translation: Config<TranslationConfig>,
    resources: Resources,
    font_renderer: koharu_renderer::SceneRenderer,
    fonts: Vec<FontFaceView>,
    jobs: HashMap<RequestId, RunningJob>,
    downloads: HashMap<u64, DownloadStatus>,
    pending_masks: HashSet<(PageId, CanvasMaskPlane, u64)>,
    undo: Vec<Vec<Revision>>,
    redo: Vec<Vec<Revision>>,
    initial_path: Option<PathBuf>,
    auto_fit: bool,
}

struct RunningJob {
    cancellation: CancellationToken,
    status: JobStatus,
}

enum CommandOutcome {
    Accepted(Revision),
    Cancelled(Revision),
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
struct AppFailure {
    code: AppErrorCode,
    message: String,
}

impl App {
    fn new(
        background: Background,
        pipeline: Config<PipelineConfig>,
        translation: Config<TranslationConfig>,
        resources: Resources,
        font_renderer: koharu_renderer::SceneRenderer,
        fonts: Vec<FontFaceView>,
        initial_path: Option<PathBuf>,
    ) -> Self {
        Self {
            session: None,
            path: None,
            visible_page: None,
            background,
            pipeline,
            translation,
            resources,
            font_renderer,
            fonts,
            jobs: HashMap::new(),
            downloads: HashMap::new(),
            pending_masks: HashSet::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            initial_path,
            auto_fit: true,
        }
    }

    fn open(&mut self, path: PathBuf, desktop: &mut DesktopContext<'_, NativeEvent>) -> Result<()> {
        let session =
            Session::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        self.install_session(session, path, desktop)
    }

    fn create(
        &mut self,
        path: PathBuf,
        desktop: &mut DesktopContext<'_, NativeEvent>,
    ) -> Result<()> {
        let session = Session::create(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        self.install_session(session, path, desktop)
    }

    fn install_session(
        &mut self,
        session: Session,
        path: PathBuf,
        desktop: &mut DesktopContext<'_, NativeEvent>,
    ) -> Result<()> {
        for job in self.jobs.values() {
            job.cancellation.cancel();
        }
        self.jobs.clear();
        self.pending_masks.clear();
        self.resources.install(&session, &path);
        self.visible_page = session.project().pages.first().map(|page| page.id);
        self.session = Some(session);
        self.path = Some(path);
        self.undo.clear();
        self.redo.clear();
        self.auto_fit = true;
        self.show_visible_page(desktop)?;
        Ok(())
    }

    fn close(&mut self, desktop: &mut DesktopContext<'_, NativeEvent>) -> Result<()> {
        for job in self.jobs.values() {
            job.cancellation.cancel();
        }
        self.jobs.clear();
        self.pending_masks.clear();
        self.session = None;
        self.resources.clear();
        self.path = None;
        self.visible_page = None;
        self.undo.clear();
        self.redo.clear();
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
                match self.command(desktop, id, base, command) {
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
        match command {
            AppCommand::Synchronize => {
                self.emit_state(desktop)?;
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
                return Ok(CommandOutcome::Accepted(Revision::ZERO));
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
                let cancellation = self.background.import(id, path, files, desktop.handle())?;
                let status = JobStatus::Running {
                    id,
                    kind: JobKind::Import,
                    completed: 0,
                    total: 0,
                    phase: None,
                    model: None,
                };
                self.jobs.insert(
                    id,
                    RunningJob {
                        cancellation,
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
            AppCommand::RunPipeline {
                scope,
                target,
                force,
            } => {
                self.require_base(base)?;
                self.ensure_free_job(id)?;
                self.ensure_job_kind_available(JobKind::Pipeline)?;
                let cancellation = self.background.run_pipeline(
                    PipelineRequest {
                        id,
                        path: self.project_path()?.to_owned(),
                        scope,
                        target,
                        force,
                    },
                    desktop.handle(),
                )?;
                let status = JobStatus::Running {
                    id,
                    kind: JobKind::Pipeline,
                    completed: 0,
                    total: 0,
                    phase: None,
                    model: None,
                };
                self.jobs.insert(
                    id,
                    RunningJob {
                        cancellation,
                        status: status.clone(),
                    },
                );
                desktop.emit(EVENT_NAME, AppEvent::JobChanged(status))?;
                return Ok(CommandOutcome::Accepted(base));
            }
            AppCommand::CancelJob { job } => {
                self.require_base(base)?;
                self.jobs
                    .get(&job)
                    .ok_or_else(|| {
                        app_failure(AppErrorCode::NotFound, format!("job {job} is not running"))
                    })?
                    .cancellation
                    .cancel();
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
                        .project()
                        .pages
                        .iter()
                        .map(|page| page.id)
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
                let cancellation = self.background.export(
                    ExportRequest {
                        id,
                        path: self.project_path()?.to_owned(),
                        directory,
                        pages,
                        format,
                    },
                    desktop.handle(),
                )?;
                let status = JobStatus::Running {
                    id,
                    kind: JobKind::Export,
                    completed: 0,
                    total: 0,
                    phase: None,
                    model: None,
                };
                self.jobs.insert(
                    id,
                    RunningJob {
                        cancellation,
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
                let (translation, credentials) = translation.into_parts();
                {
                    let mut current = self.pipeline.write()?;
                    *current = pipeline;
                    current.save()?;
                }
                {
                    credentials.save()?;
                    let mut current = self.translation.write()?;
                    *current = translation;
                    current.save()?;
                }
                self.emit_settings(desktop)?;
                return Ok(CommandOutcome::Accepted(self.current_revision()));
            }
            AppCommand::CacheFont {
                family,
                weight,
                italic,
            } => {
                let renderer = self.font_renderer.clone();
                let handle = desktop.handle();
                tokio::spawn(async move {
                    let error = renderer
                        .fetch_google_font(&family, weight, italic)
                        .await
                        .err()
                        .map(|error| error.to_string());
                    let _ = handle.send_event(NativeEvent::FontCached {
                        family,
                        weight,
                        italic,
                        error,
                    });
                });
                return Ok(CommandOutcome::Accepted(self.current_revision()));
            }
            _ => {}
        }

        self.require_base(base)?;
        if matches!(&command, AppCommand::DeletePage { page } if self.pending_masks.iter().any(|(pending, _, _)| pending == page))
            || matches!(&command, AppCommand::DeletePages { pages } if pages.iter().any(|page| self.pending_masks.iter().any(|(pending, _, _)| pending == page)))
        {
            return Err(app_failure(
                AppErrorCode::Busy,
                "a page mask is still being committed",
            ));
        }
        let commands = build_commands(self.session()?, command)?;
        let changes = self.session_mut()?.apply(commands)?;
        if changes.to != changes.from {
            self.undo.push(vec![changes.to]);
            self.redo.clear();
        }
        self.present_changes(desktop, &changes)?;
        Ok(CommandOutcome::Accepted(changes.to))
    }

    fn undo(
        &mut self,
        desktop: &mut DesktopContext<'_, NativeEvent>,
        base: Revision,
    ) -> Result<Revision> {
        self.require_base(base)?;
        let group = self.undo.pop().ok_or_else(|| anyhow!("nothing to undo"))?;
        let result = self.session_mut()?.revert(group.iter().copied());
        let changes = match result {
            Ok(changes) => changes,
            Err(error) => {
                self.undo.push(group);
                return Err(error.into());
            }
        };
        self.redo.push(vec![changes.to]);
        self.present_changes(desktop, &changes)?;
        Ok(changes.to)
    }

    fn redo(
        &mut self,
        desktop: &mut DesktopContext<'_, NativeEvent>,
        base: Revision,
    ) -> Result<Revision> {
        self.require_base(base)?;
        let group = self.redo.pop().ok_or_else(|| anyhow!("nothing to redo"))?;
        let result = self.session_mut()?.revert(group.iter().copied());
        let changes = match result {
            Ok(changes) => changes,
            Err(error) => {
                self.redo.push(group);
                return Err(error.into());
            }
        };
        self.undo.push(vec![changes.to]);
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
                self.session()?.page(page)?;
                self.visible_page = Some(page);
                desktop.show_page(self.session()?, page)?;
                self.auto_fit = true;
                self.fit_window(desktop)?;
                desktop.emit(
                    EVENT_NAME,
                    AppEvent::PageLoaded {
                        revision: self.session()?.revision(),
                        page: PageView::from_page(self.session()?.page(page)?),
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
            CanvasInteraction::SetOverlays {
                selected,
                hovered,
                previews,
                draft,
                guides,
                show_text_bounds,
                brush_cursor,
            } => desktop.set_overlays(OverlayState {
                selected,
                hovered,
                draft,
                guides: guides
                    .into_iter()
                    .map(|guide| match guide {
                        crate::protocol::CanvasGuide::Horizontal(position) => {
                            CanvasGuide::Horizontal(position)
                        }
                        crate::protocol::CanvasGuide::Vertical(position) => {
                            CanvasGuide::Vertical(position)
                        }
                    })
                    .collect(),
                show_text_bounds,
                brush_cursor: brush_cursor.map(|cursor| BrushCursor {
                    point: PhysicalPoint::new(cursor.x, cursor.y),
                    diameter: cursor.diameter,
                }),
                element_previews: previews
                    .into_iter()
                    .map(|preview| CanvasElementPreview {
                        element: preview.element,
                        frame: preview.frame,
                    })
                    .collect(),
                ..OverlayState::default()
            }),
            CanvasInteraction::HitTest { id, x, y } => {
                let target = desktop
                    .canvas()
                    .hit_test(PhysicalPoint::new(x, y))
                    .map(hit_target);
                desktop.emit(EVENT_NAME, AppEvent::HitTest { id, target })?;
            }
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
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let changes = session.refresh()?;
        if changes.to != changes.from {
            self.present_changes(desktop, &changes)?;
        }
        Ok(())
    }

    fn present_changes(
        &mut self,
        desktop: &mut DesktopContext<'_, NativeEvent>,
        changes: &ChangeSet,
    ) -> Result<()> {
        let previous_page = self.visible_page;
        if let (Some(session), Some(path)) = (&self.session, &self.path) {
            self.resources.install(session, path);
        }
        let visible_exists = self.visible_page.is_some_and(|page| {
            self.session()
                .is_ok_and(|session| session.page(page).is_ok())
        });
        if !visible_exists {
            self.visible_page = self.session()?.project().pages.first().map(|page| page.id);
            self.show_visible_page(desktop)?;
            self.auto_fit = true;
            self.fit_window(desktop)?;
        } else {
            desktop.sync(self.session()?, changes)?;
        }
        let delta = self.delta(changes)?;
        desktop.emit(EVENT_NAME, AppEvent::ProjectChanged(delta))?;
        if self.visible_page != previous_page
            && let Some(page) = self.visible_page
        {
            desktop.emit(
                EVENT_NAME,
                AppEvent::PageLoaded {
                    revision: self.session()?.revision(),
                    page: PageView::from_page(self.session()?.page(page)?),
                },
            )?;
        }
        Ok(())
    }

    fn show_visible_page(&self, desktop: &mut DesktopContext<'_, NativeEvent>) -> Result<()> {
        if let (Some(session), Some(page)) = (self.session.as_ref(), self.visible_page) {
            desktop.show_page(session, page)?;
        } else {
            desktop.clear_page();
        }
        Ok(())
    }

    fn fit_window(&mut self, desktop: &mut DesktopContext<'_, NativeEvent>) -> Result<()> {
        let Some(page) = self.visible_page else {
            return Ok(());
        };
        let size = self.session()?.page(page)?.size;
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
        let Some(session) = self.session.as_ref() else {
            return desktop.emit(EVENT_NAME, AppEvent::ProjectClosed);
        };
        desktop.emit(
            EVENT_NAME,
            AppEvent::ProjectOpened {
                revision: session.revision(),
                project: self.project_header(session),
                pages: session
                    .project()
                    .pages
                    .iter()
                    .map(PageSummary::from_page)
                    .collect(),
            },
        )?;
        if let Some(page) = self.visible_page {
            desktop.emit(
                EVENT_NAME,
                AppEvent::PageLoaded {
                    revision: session.revision(),
                    page: PageView::from_page(session.page(page)?),
                },
            )?;
        }
        Ok(())
    }

    fn emit_settings(&self, desktop: &DesktopContext<'_, NativeEvent>) -> Result<()> {
        desktop.emit(
            EVENT_NAME,
            AppEvent::SettingsChanged {
                settings: SettingsView {
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
                },
            },
        )
    }

    fn emit_state(&self, desktop: &DesktopContext<'_, NativeEvent>) -> Result<()> {
        self.emit_project(desktop)?;
        self.emit_settings(desktop)?;
        self.emit_view(desktop)?;
        for job in self.jobs.values() {
            desktop.emit(EVENT_NAME, AppEvent::JobChanged(job.status.clone()))?;
        }
        for download in self.downloads.values() {
            desktop.emit(EVENT_NAME, AppEvent::DownloadChanged(download.clone()))?;
        }
        Ok(())
    }

    fn project_header(&self, session: &Session) -> ProjectHeader {
        ProjectHeader {
            id: session.id(),
            name: self
                .path
                .as_deref()
                .map(project_name)
                .unwrap_or_else(|| "Untitled".into()),
            visible_page: self.visible_page,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        }
    }

    fn delta(&self, changes: &ChangeSet) -> Result<ProjectDelta> {
        let session = self.session()?;
        let project = session.project();
        let pages = changes
            .pages
            .iter()
            .filter_map(|id| project.page(*id))
            .map(PageSummary::from_page)
            .collect();
        let deleted_pages = changes
            .pages
            .iter()
            .copied()
            .filter(|id| project.page(*id).is_none())
            .collect();
        let visible_page = self
            .visible_page
            .filter(|visible| {
                changes.pages.contains(visible)
                    || changes.elements.iter().any(|element| {
                        project
                            .page(*visible)
                            .is_some_and(|page| page.element(*element).is_some())
                    })
            })
            .and_then(|visible| project.page(visible))
            .map(|page| PageDelta {
                id: page.id,
                name: page.name.clone(),
                size: page.size,
                source: page.source.to_string(),
                assets: (&page.assets).into(),
                element_order: page.elements.iter().map(|element| element.id).collect(),
                elements: changes
                    .elements
                    .iter()
                    .filter_map(|id| page.element(*id).cloned())
                    .collect(),
                deleted_elements: changes
                    .elements
                    .iter()
                    .copied()
                    .filter(|id| page.element(*id).is_none())
                    .collect(),
            });
        Ok(ProjectDelta {
            from: changes.from,
            revision: changes.to,
            name: project_name(self.project_path()?),
            page_order: project.pages.iter().map(|page| page.id).collect(),
            pages,
            deleted_pages,
            visible_page,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        })
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
        let current = self
            .revision()
            .ok_or_else(|| app_failure(AppErrorCode::NoProject, "no project is open"))?;
        if base != current {
            return Err(app_failure(
                AppErrorCode::StaleRevision,
                format!("stale scene revision {base}; current revision is {current}"),
            ));
        }
        Ok(())
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
        if let Some(running) = self.jobs.values().find_map(|job| match &job.status {
            JobStatus::Running { kind, .. } => Some(*kind),
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

    fn session(&self) -> Result<&Session> {
        self.session
            .as_ref()
            .ok_or_else(|| app_failure(AppErrorCode::NoProject, "no project is open"))
    }

    fn session_mut(&mut self) -> Result<&mut Session> {
        self.session
            .as_mut()
            .ok_or_else(|| app_failure(AppErrorCode::NoProject, "no project is open"))
    }

    fn project_path(&self) -> Result<&Path> {
        self.path
            .as_deref()
            .ok_or_else(|| app_failure(AppErrorCode::NoProject, "no project is open"))
    }

    fn revision(&self) -> Option<Revision> {
        self.session.as_ref().map(Session::revision)
    }

    fn current_revision(&self) -> Revision {
        self.revision().unwrap_or(Revision::ZERO)
    }
}

fn app_failure(code: AppErrorCode, message: impl std::fmt::Display) -> anyhow::Error {
    AppFailure {
        code,
        message: message.to_string(),
    }
    .into()
}

fn classify_error(error: &anyhow::Error) -> AppErrorCode {
    if let Some(error) = error.downcast_ref::<AppFailure>() {
        return error.code;
    }
    if let Some(error) = error.downcast_ref::<koharu_scene::Error>() {
        return match error {
            koharu_scene::Error::Io(_) | koharu_scene::Error::Sql(_) => AppErrorCode::IoFailed,
            koharu_scene::Error::PageNotFound(_)
            | koharu_scene::Error::ElementNotFound(_)
            | koharu_scene::Error::HistoryNotFound(_) => AppErrorCode::NotFound,
            koharu_scene::Error::RevisionConflict { .. } => AppErrorCode::StaleRevision,
            koharu_scene::Error::Invalid(_)
            | koharu_scene::Error::ElementKind(_)
            | koharu_scene::Error::CommandConflict
            | koharu_scene::Error::HistoryConflict(_) => AppErrorCode::InvalidInput,
            _ => AppErrorCode::IoFailed,
        };
    }
    if error.downcast_ref::<std::io::Error>().is_some() {
        AppErrorCode::IoFailed
    } else {
        AppErrorCode::Internal
    }
}

impl Application for App {
    type Event = NativeEvent;

    fn started(&mut self, desktop: &mut DesktopContext<'_, Self::Event>) -> Result<()> {
        self.background.subscribe_downloads(desktop.handle());
        if let Some(path) = self.initial_path.take()
            && let Err(error) = self.open(path, desktop)
        {
            self.problem(desktop, AppErrorCode::IoFailed, error)?;
        }
        Ok(())
    }

    fn ready(
        &mut self,
        desktop: &mut DesktopContext<'_, Self::Event>,
        _dpr: f64,
        _width: f64,
        _height: f64,
    ) -> Result<()> {
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
            NativeEvent::ProjectAdvanced { job } => {
                if self.jobs.contains_key(&job) {
                    self.refresh(desktop)?;
                }
                Ok(())
            }
            NativeEvent::FontCached {
                family,
                weight,
                italic,
                error,
            } => {
                if let Some(error) = error {
                    return self.problem(desktop, AppErrorCode::IoFailed, anyhow!(error));
                }
                for face in &mut self.fonts {
                    if face.family_name == family
                        && face.weight == weight
                        && matches!(face.style, crate::protocol::FontFaceStyleView::Italic)
                            == italic
                    {
                        face.cached = true;
                    }
                }
                desktop.canvas().invalidate_fonts();
                self.emit_settings(desktop)
            }
            NativeEvent::PipelineProgress { job, progress } => {
                let Some(running) = self.jobs.get_mut(&job) else {
                    return Ok(());
                };
                let status = JobStatus::Running {
                    id: job,
                    kind: JobKind::Pipeline,
                    completed: progress.completed,
                    total: progress.total,
                    phase: Some(progress.phase),
                    model: Some(progress.model),
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
                cancelled,
                error,
            } => {
                if self.jobs.remove(&job).is_none() {
                    return Ok(());
                }
                self.refresh(desktop)?;
                if !revisions.is_empty() {
                    self.undo.push(revisions);
                    self.redo.clear();
                    let revision = self.session()?.revision();
                    self.present_changes(
                        desktop,
                        &ChangeSet {
                            from: revision,
                            to: revision,
                            ..ChangeSet::default()
                        },
                    )?;
                }
                let status = if cancelled {
                    JobStatus::Cancelled { id: job }
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
                let asset = match mask.plane {
                    CanvasMaskPlane::Text => PageAsset::TextMask,
                    CanvasMaskPlane::Brush => PageAsset::BrushMask,
                };
                let mut commands = self.session()?.commands();
                let blob = commands
                    .set_asset(mask.page, asset, Some(mask.bytes))?
                    .expect("a supplied mask creates a blob");
                let changes = self.session_mut()?.apply(commands)?;
                desktop.canvas().acknowledge_mask_commit(
                    mask.page,
                    mask.plane,
                    mask.generation,
                    blob,
                )?;
                if changes.to != changes.from {
                    self.undo.push(vec![changes.to]);
                    self.redo.clear();
                }
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
            job.cancellation.cancel();
        }
        Ok(self.pending_masks.is_empty())
    }
}

fn build_commands(session: &Session, command: AppCommand) -> Result<Commands> {
    let mut commands = session.commands();
    match command {
        AppCommand::RenamePage { page, name } => commands.push(Command::RenamePage { page, name }),
        AppCommand::DeletePage { page } => commands.push(Command::DeletePage(page)),
        AppCommand::DeletePages { pages } => {
            for page in pages {
                commands.push(Command::DeletePage(page));
            }
        }
        AppCommand::MovePage { page, index } => commands.push(Command::MovePage { page, index }),
        AppCommand::AddText { page, frame } => {
            commands.add_text(page, frame);
        }
        AppCommand::SetTranslation {
            page,
            element,
            translation,
        } => commands.push(Command::EditElement {
            page,
            element,
            edit: ElementChange::Translation(translation),
        }),
        AppCommand::SetTextStyle {
            page,
            element,
            style,
        } => commands.push(Command::EditElement {
            page,
            element,
            edit: ElementChange::Style(style),
        }),
        AppCommand::SetTextLayout {
            page,
            element,
            layout,
        } => commands.push(Command::EditElement {
            page,
            element,
            edit: ElementChange::Layout(layout),
        }),
        AppCommand::SetTextStyles { page, elements } => {
            for value in elements {
                commands.push(Command::EditElement {
                    page,
                    element: value.element,
                    edit: ElementChange::Style(value.style),
                });
            }
        }
        AppCommand::SetTextLayouts { page, elements } => {
            for value in elements {
                commands.push(Command::EditElement {
                    page,
                    element: value.element,
                    edit: ElementChange::Layout(value.layout),
                });
            }
        }
        AppCommand::SetElementFrames { elements } => {
            for value in elements {
                commands.push(Command::EditElement {
                    page: value.page,
                    element: value.element,
                    edit: ElementChange::Frame(value.frame),
                });
            }
        }
        AppCommand::SetElementOpacity {
            page,
            elements,
            opacity,
        } => {
            for element in elements {
                commands.push(Command::EditElement {
                    page,
                    element,
                    edit: ElementChange::Opacity(opacity),
                });
            }
        }
        AppCommand::SetElementVisibility {
            page,
            elements,
            visible,
        } => {
            for element in elements {
                commands.push(Command::EditElement {
                    page,
                    element,
                    edit: ElementChange::Visible(visible),
                });
            }
        }
        AppCommand::DeleteElements { page, elements } => {
            for element in elements {
                commands.push(Command::DeleteElement { page, element });
            }
        }
        AppCommand::MoveElement {
            page,
            element,
            index,
        } => commands.push(Command::MoveElement {
            page,
            element,
            index,
        }),
        _ => bail!("command is not a scene edit"),
    }
    Ok(commands)
}

fn project_name(path: &Path) -> String {
    path.file_stem()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
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

const fn mask_plane(plane: MaskPlane) -> CanvasMaskPlane {
    match plane {
        MaskPlane::Text => CanvasMaskPlane::Text,
        MaskPlane::Brush => CanvasMaskPlane::Brush,
    }
}

fn hit_target(target: CanvasHitTarget) -> HitTarget {
    match target {
        CanvasHitTarget::Element(element) => HitTarget::Element { element },
        CanvasHitTarget::Handle { element, handle } => HitTarget::Handle {
            element,
            handle: match handle {
                CanvasHandle::NorthWest => Handle::NorthWest,
                CanvasHandle::North => Handle::North,
                CanvasHandle::NorthEast => Handle::NorthEast,
                CanvasHandle::East => Handle::East,
                CanvasHandle::SouthEast => Handle::SouthEast,
                CanvasHandle::South => Handle::South,
                CanvasHandle::SouthWest => Handle::SouthWest,
                CanvasHandle::West => Handle::West,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_scene::{ElementKind, Frame, TextStyle};

    #[test]
    fn maps_one_ui_edit_to_one_scene_batch() {
        let mut session = Session::memory().unwrap();
        let mut commands = session.commands();
        let page = commands.add_page("page.png", png()).unwrap();
        session.apply(commands).unwrap();

        let commands = build_commands(
            &session,
            AppCommand::AddText {
                page,
                frame: Frame::new(1.0, 2.0, 30.0, 40.0),
            },
        )
        .unwrap();
        let changes = session.apply(commands).unwrap();

        assert_eq!(changes.elements.len(), 1);
        assert_eq!(session.page(page).unwrap().elements.len(), 1);
    }

    #[test]
    fn project_names_follow_the_file_name() {
        assert_eq!(project_name(Path::new("Volume 1.khr")), "Volume 1");
        assert_eq!(project_name(Path::new("Untitled")), "Untitled");
    }

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

    #[test]
    fn bulk_styles_share_one_scene_revision() {
        let mut session = Session::memory().unwrap();
        let mut commands = session.commands();
        let page = commands.add_page("page.png", png()).unwrap();
        session.apply(commands).unwrap();
        let mut commands = session.commands();
        let first = commands.add_text(page, Frame::new(0.0, 0.0, 20.0, 20.0));
        let second = commands.add_text(page, Frame::new(30.0, 0.0, 20.0, 20.0));
        session.apply(commands).unwrap();
        let before = session.revision();

        let mut first_style = TextStyle::default();
        first_style.font_size = 24.0;
        let mut second_style = TextStyle::default();
        second_style.font_size = 30.0;
        let commands = build_commands(
            &session,
            AppCommand::SetTextStyles {
                page,
                elements: vec![
                    crate::protocol::ElementTextStyle {
                        element: first,
                        style: first_style,
                    },
                    crate::protocol::ElementTextStyle {
                        element: second,
                        style: second_style,
                    },
                ],
            },
        )
        .unwrap();
        assert_eq!(commands.as_slice().len(), 2);
        let changes = session.apply(commands).unwrap();
        assert_eq!(changes.from, before);
        assert_eq!(changes.to.get(), before.get() + 1);
        let sizes = session
            .page(page)
            .unwrap()
            .elements
            .iter()
            .map(|element| match &element.kind {
                ElementKind::Text(text) => text.style.font_size,
                ElementKind::Image(_) | ElementKind::Region(_) => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(sizes, [24.0, 30.0]);
    }

    #[test]
    fn delete_pages_is_one_batch_and_app_failures_keep_stable_codes() {
        let mut session = Session::memory().unwrap();
        let mut commands = session.commands();
        let first = commands.add_page("one.png", png()).unwrap();
        let second = commands.add_page("two.png", png()).unwrap();
        session.apply(commands).unwrap();
        let before = session.revision();
        let commands = build_commands(
            &session,
            AppCommand::DeletePages {
                pages: vec![first, second],
            },
        )
        .unwrap();
        assert_eq!(commands.as_slice().len(), 2);
        let changes = session.apply(commands).unwrap();
        assert_eq!(changes.to.get(), before.get() + 1);
        assert!(session.project().pages.is_empty());

        let error = app_failure(AppErrorCode::StaleRevision, "stale");
        assert_eq!(classify_error(&error), AppErrorCode::StaleRevision);
    }

    fn png() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248, 207,
            192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
            96, 130,
        ]
    }
}
