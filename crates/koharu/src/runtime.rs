use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow, bail};
use async_trait::async_trait;
use koharu_app::{
    Application, CanvasOperation, CanvasOutput, DialogFilter, FileDialogs, InpaintCommit,
    Presentation, PresentationUpdate, RasterStroke, TransformCommit, ViewDisposition,
};
use koharu_canvas::{
    Brush, Camera, ElementFrame, Frame, MaskOverlay, MaskTarget, PagePoint, PhysicalPoint,
    PhysicalSize, StrokeMode, ViewState,
};
use koharu_desktop::{Presenter, browser::BinaryAttachment};
use koharu_desktop::{
    browser::WebMessage,
    geometry::{LayoutError, LogicalRect, PhysicalRect},
    platform::{PlatformError, PlatformServices, WindowAction, WindowActionError, WindowState},
    runtime::{DesktopConfig, DesktopDelegate, DesktopHandle},
};
use koharu_protocol::{
    AppError, AppErrorCode, AppEvent, CanvasState, Command, CommandResult, Request, RequestId,
    Response, ServerEvent, ServerMessage, TransformFrame,
};
use parking_lot::Mutex;
use tokio::sync::{broadcast, oneshot};
use url::Url;

const INPAINT_MASK: MaskTarget = MaskTarget::Scratch(0);
const INPAINT_OVERLAY: MaskOverlay = MaskOverlay::new([168, 85, 247, 210], 0.55);

pub async fn run() -> Result<()> {
    let paths = DesktopPaths::resolve()?;
    let presentation = Arc::new(DesktopPresentation::default());
    let dialogs = Arc::new(DesktopFileDialogs);
    let application = Arc::new(
        Application::in_documents(presentation.clone(), dialogs)
            .context("failed to create the application")?,
    );
    let fatal = Arc::new(Mutex::new(None));
    let updater = Arc::new(
        koharu_updater::Updater::new(env!("CARGO_PKG_VERSION"))
            .context("failed to configure the application updater")?,
    );
    let delegate = ApplicationDelegate::new(
        tokio::runtime::Handle::current(),
        Arc::clone(&application),
        presentation,
        Arc::clone(&fatal),
        updater,
    );
    let config = DesktopConfig {
        title: "Koharu".into(),
        icon_png: include_bytes!("../icons/icon.png"),
        initial_width: 1024.0,
        initial_height: 768.0,
        cef_distribution: paths.cef,
        browser_cache_root: paths.cache,
        initial_url: paths.url.as_str().into(),
        browser_resources_root: paths.ui,
    };

    let result = koharu_desktop::runtime::run(config, delegate, DesktopPlatform);
    application.stop().await;
    result?;
    if let Some(error) = fatal.lock().take() {
        bail!(error);
    }
    Ok(())
}

struct DesktopPaths {
    cef: PathBuf,
    cache: PathBuf,
    url: Url,
    ui: Option<PathBuf>,
}

impl DesktopPaths {
    fn resolve() -> Result<Self> {
        let executable =
            std::env::current_exe().context("failed to locate the Koharu executable")?;
        let executable_dir = executable
            .parent()
            .context("the Koharu executable has no parent directory")?;
        let bundled = bundled_layout(executable_dir);
        let cef = bundled
            .as_ref()
            .map(|layout| layout.cef.clone())
            .unwrap_or_else(|| executable_dir.to_path_buf());
        let cache = dirs::cache_dir()
            .context("the operating system did not provide a cache directory")?
            .join("Koharu")
            .join("cef");
        std::fs::create_dir_all(&cache)
            .with_context(|| format!("failed to create browser cache {}", cache.display()))?;

        let (url, ui) = if cfg!(debug_assertions) {
            (
                Url::parse("http://localhost:3000").expect("development UI URL is valid"),
                None,
            )
        } else {
            let ui = bundled.map_or_else(source_ui, |layout| layout.ui);
            if !ui.join("index.html").is_file() {
                bail!(
                    "the exported frontend is missing {}; run `bun run --filter @koharu/app build`",
                    ui.join("index.html").display()
                );
            }
            (
                Url::parse("koharu://app/").expect("static application URL is valid"),
                Some(ui),
            )
        };
        Ok(Self {
            cef,
            cache,
            url,
            ui,
        })
    }
}

struct BundledLayout {
    cef: PathBuf,
    ui: PathBuf,
}

fn bundled_layout(executable_dir: &Path) -> Option<BundledLayout> {
    #[cfg(target_os = "macos")]
    {
        let contents = executable_dir.parent()?;
        let ui = contents.join("Resources/ui");
        let cef = contents.join("Frameworks");
        ui.join("index.html")
            .is_file()
            .then_some(BundledLayout { cef, ui })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let roots = [
            executable_dir.to_path_buf(),
            executable_dir.parent()?.join("lib"),
        ];
        roots.into_iter().find_map(|cef| {
            let ui = cef.join("resources/ui");
            ui.join("index.html")
                .is_file()
                .then_some(BundledLayout { cef, ui })
        })
    }
}

fn source_ui() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/koharu/out")
}

#[derive(Default)]
struct DesktopPresentation {
    handle: Mutex<Option<DesktopHandle>>,
    queue: Mutex<VecDeque<PresentationRequest>>,
    viewports: Mutex<HashMap<u64, PendingViewport>>,
    next_viewport: AtomicU64,
}

enum PresentationRequest {
    Apply(PresentationUpdate, oneshot::Sender<Result<CanvasState>>),
    Canvas(CanvasOperation, oneshot::Sender<Result<CanvasOutput>>),
}

struct PendingViewport {
    fitted_page: Option<(u32, u32)>,
    complete: oneshot::Sender<Result<CanvasOutput>>,
}

#[derive(Default)]
struct PresentationState {
    fitted: bool,
}

impl DesktopPresentation {
    fn attach(&self, handle: DesktopHandle) {
        *self.handle.lock() = Some(handle);
    }

    fn submit(&self, request: PresentationRequest) -> Result<()> {
        let handle = self
            .handle
            .lock()
            .clone()
            .context("desktop presentation is not attached")?;
        self.queue.lock().push_back(request);
        if let Err(error) = handle.wake() {
            self.fail_queued(anyhow!(error));
            return Err(error.into());
        }
        Ok(())
    }

    fn fail_queued(&self, error: anyhow::Error) {
        let message = error.to_string();
        for request in self.queue.lock().drain(..) {
            match request {
                PresentationRequest::Apply(_, complete) => {
                    let _ = complete.send(Err(anyhow!(message.clone())));
                }
                PresentationRequest::Canvas(_, complete) => {
                    let _ = complete.send(Err(anyhow!(message.clone())));
                }
            }
        }
    }

    fn drain(&self, presenter: &mut Presenter, state: &mut PresentationState) {
        for request in self.queue.lock().drain(..) {
            match request {
                PresentationRequest::Apply(update, complete) => {
                    let _ = complete.send(apply_presentation(presenter, state, update));
                }
                PresentationRequest::Canvas(CanvasOperation::SampleColor(point), complete) => {
                    let complete = Arc::new(Mutex::new(Some(complete)));
                    let callback = Arc::clone(&complete);
                    let result = presenter.canvas_mut().sample_color(
                        PhysicalPoint::new(point.x, point.y),
                        move |result| {
                            if let Some(complete) = callback.lock().take() {
                                let _ = complete.send(
                                    result.map(CanvasOutput::Color).map_err(anyhow::Error::from),
                                );
                            }
                        },
                    );
                    if let Err(error) = result
                        && let Some(complete) = complete.lock().take()
                    {
                        let _ = complete.send(Err(error.into()));
                    }
                }
                PresentationRequest::Canvas(operation, complete) => {
                    let _ = complete.send(apply_canvas(presenter, state, operation));
                }
            }
        }
    }

    fn request_viewport(
        &self,
        bounds: LogicalRect,
        scale_factor: f64,
        workspace_color: [u8; 3],
        fitted_page: Option<(u32, u32)>,
        complete: oneshot::Sender<Result<CanvasOutput>>,
    ) -> Result<()> {
        let handle = self
            .handle
            .lock()
            .clone()
            .context("desktop presentation is not attached")?;
        let id = self
            .next_viewport
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1)
            .max(1);
        self.viewports.lock().insert(
            id,
            PendingViewport {
                fitted_page,
                complete,
            },
        );
        if let Err(error) = handle.set_viewport(id, bounds, scale_factor, workspace_color) {
            if let Some(pending) = self.viewports.lock().remove(&id) {
                let _ = pending.complete.send(Err(anyhow!(error)));
            }
            return Err(error.into());
        }
        Ok(())
    }

    fn complete_viewport(
        &self,
        id: u64,
        result: std::result::Result<PhysicalRect, LayoutError>,
        presenter: &mut Presenter,
        state: &mut PresentationState,
    ) {
        let Some(pending) = self.viewports.lock().remove(&id) else {
            tracing::warn!(id, "received an unknown viewport completion");
            return;
        };
        let output = result.map_err(anyhow::Error::from).map(|_| {
            if state.fitted
                && let Some((width, height)) = pending.fitted_page
            {
                set_camera(
                    presenter,
                    Camera::contain(
                        presenter.viewport().size(),
                        PhysicalSize::new(width, height),
                    ),
                );
            }
            CanvasOutput::State(canvas_state(presenter, state.fitted))
        });
        let _ = pending.complete.send(output);
    }
}

#[async_trait]
impl Presentation for DesktopPresentation {
    async fn apply(&self, update: PresentationUpdate) -> Result<CanvasState> {
        let (complete, result) = oneshot::channel();
        self.submit(PresentationRequest::Apply(update, complete))?;
        result.await.context("desktop presentation stopped")?
    }

    async fn canvas(&self, operation: CanvasOperation) -> Result<CanvasOutput> {
        let (complete, result) = oneshot::channel();
        if let CanvasOperation::SetViewport {
            x,
            y,
            width,
            height,
            dpr,
            background,
            fitted_page,
        } = operation
        {
            self.request_viewport(
                LogicalRect {
                    x,
                    y,
                    width,
                    height,
                },
                dpr,
                background,
                fitted_page,
                complete,
            )?;
        } else {
            self.submit(PresentationRequest::Canvas(operation, complete))?;
        }
        result.await.context("desktop canvas stopped")?
    }
}

fn apply_presentation(
    presenter: &mut Presenter,
    state: &mut PresentationState,
    update: PresentationUpdate,
) -> Result<CanvasState> {
    match update {
        PresentationUpdate::Frame { frame, view } => {
            let size = frame.size();
            presenter.canvas_mut().set_frame(frame)?;
            if view == ViewDisposition::Fit {
                set_camera(
                    presenter,
                    Camera::contain(
                        presenter.viewport().size(),
                        PhysicalSize::new(size.0, size.1),
                    ),
                );
                state.fitted = true;
            }
        }
        PresentationUpdate::Clear => {
            presenter.canvas_mut().clear();
            state.fitted = true;
        }
    }
    Ok(canvas_state(presenter, state.fitted))
}

fn apply_canvas(
    presenter: &mut Presenter,
    state: &mut PresentationState,
    operation: CanvasOperation,
) -> Result<CanvasOutput> {
    let output = match operation {
        CanvasOperation::SetZoom(zoom) => {
            if !zoom.is_finite() || !(0.02..=16.0).contains(&zoom) {
                bail!("camera zoom must be between 2% and 1600%");
            }
            let mut camera = presenter.canvas().camera();
            let size = presenter.viewport().size();
            camera.zoom_around(
                PhysicalPoint::new(f64::from(size.width) * 0.5, f64::from(size.height) * 0.5),
                f64::from(zoom),
            )?;
            set_camera(presenter, camera);
            state.fitted = false;
            CanvasOutput::State(canvas_state(presenter, false))
        }
        CanvasOperation::SetView { zoom, translation } => {
            if !(0.02..=16.0).contains(&zoom) {
                bail!("camera zoom must be between 2% and 1600%");
            }
            set_camera(presenter, Camera::new(zoom, translation)?);
            state.fitted = false;
            CanvasOutput::State(canvas_state(presenter, false))
        }
        CanvasOperation::Fit { page_size } => {
            set_camera(
                presenter,
                Camera::contain(
                    presenter.viewport().size(),
                    PhysicalSize::new(page_size.0, page_size.1),
                ),
            );
            state.fitted = true;
            CanvasOutput::State(canvas_state(presenter, true))
        }
        CanvasOperation::BeginPaint {
            layer,
            point,
            brush,
        } => {
            presenter.canvas_mut().begin_raster_stroke(
                layer,
                Brush {
                    diameter: brush.diameter,
                    color: brush.color,
                    mode: StrokeMode::Paint,
                },
                PagePoint::new(point.x, point.y),
            )?;
            CanvasOutput::Unit
        }
        CanvasOperation::BeginErase {
            layer,
            point,
            diameter,
        } => {
            presenter.canvas_mut().begin_raster_stroke(
                Some(layer),
                Brush {
                    diameter,
                    color: [0, 0, 0, 0],
                    mode: StrokeMode::Erase,
                },
                PagePoint::new(point.x, point.y),
            )?;
            CanvasOutput::Unit
        }
        CanvasOperation::ExtendRaster(points) => {
            presenter.canvas_mut().extend_raster_stroke(
                &points
                    .into_iter()
                    .map(|point| PagePoint::new(point.x, point.y))
                    .collect::<Vec<_>>(),
            )?;
            CanvasOutput::Unit
        }
        CanvasOperation::FinishRaster => {
            let stroke = presenter.canvas_mut().finish_raster_stroke()?;
            CanvasOutput::Raster(RasterStroke {
                page: stroke.page,
                layer: stroke.layer,
                mode: stroke.mode,
                color: stroke.color,
                diameter: stroke.diameter,
                points: stroke
                    .points
                    .into_iter()
                    .map(|point| koharu_protocol::Point {
                        x: point.x,
                        y: point.y,
                    })
                    .collect(),
            })
        }
        CanvasOperation::CancelRaster => {
            presenter.canvas_mut().cancel_raster_stroke();
            CanvasOutput::Unit
        }
        CanvasOperation::BeginTransform(elements) => {
            presenter
                .canvas_mut()
                .begin_transform(&elements.into_iter().map(element_frame).collect::<Vec<_>>())?;
            CanvasOutput::Unit
        }
        CanvasOperation::UpdateTransform { frame, elements } => {
            presenter.canvas_mut().update_transform(
                frame,
                &elements.into_iter().map(element_frame).collect::<Vec<_>>(),
            )?;
            CanvasOutput::Unit
        }
        CanvasOperation::PreviewOpacity { element, opacity } => {
            presenter.canvas_mut().preview_opacity(element, opacity)?;
            CanvasOutput::Unit
        }
        CanvasOperation::FinishTransform => {
            let commit = presenter
                .canvas_mut()
                .finish_transform()?
                .map(|commit| TransformCommit {
                    page: commit.page,
                    elements: commit
                        .elements
                        .into_iter()
                        .map(|element| (element.element, element.geometry))
                        .collect(),
                });
            CanvasOutput::Transform(commit)
        }
        CanvasOperation::CancelTransform => {
            presenter.canvas_mut().cancel_transform();
            CanvasOutput::Unit
        }
        CanvasOperation::BeginInpaint { point, diameter } => {
            presenter.canvas_mut().begin_mask_stroke(
                INPAINT_MASK,
                INPAINT_OVERLAY,
                Brush {
                    diameter,
                    color: [0, 0, 0, 255],
                    mode: StrokeMode::Paint,
                },
                PagePoint::new(point.x, point.y),
            )?;
            CanvasOutput::Unit
        }
        CanvasOperation::ExtendInpaint(points) => {
            presenter.canvas_mut().extend_mask_stroke(
                INPAINT_MASK,
                &points
                    .into_iter()
                    .map(|point| PagePoint::new(point.x, point.y))
                    .collect::<Vec<_>>(),
            )?;
            CanvasOutput::Unit
        }
        CanvasOperation::FinishInpaint => {
            let commit = presenter.canvas_mut().finish_mask_stroke(INPAINT_MASK)?;
            let commit = commit
                .map(|mask| {
                    let bounds = koharu_pipeline::Bounds {
                        x: f64::from(mask.dirty.x),
                        y: f64::from(mask.dirty.y),
                        width: f64::from(mask.dirty.width),
                        height: f64::from(mask.dirty.height),
                    };
                    let page = mask.page;
                    let png = Arc::from(mask.encode_png()?);
                    presenter.canvas_mut().clear_mask(INPAINT_MASK);
                    Ok::<_, anyhow::Error>(InpaintCommit {
                        mask: koharu_pipeline::InpaintingMask { page, png },
                        bounds,
                    })
                })
                .transpose()?;
            CanvasOutput::Inpaint(commit)
        }
        CanvasOperation::CancelInpaint => {
            presenter.canvas_mut().cancel_mask_stroke(INPAINT_MASK)?;
            CanvasOutput::Unit
        }
        CanvasOperation::AcknowledgeRaster { page, revision } => {
            presenter
                .canvas_mut()
                .acknowledge_raster_commit(page, revision)?;
            CanvasOutput::Unit
        }
        CanvasOperation::AcknowledgeTransform { page, revision } => {
            presenter
                .canvas_mut()
                .acknowledge_transform_commit(page, revision)?;
            CanvasOutput::Unit
        }
        CanvasOperation::SampleColor(_) | CanvasOperation::SetViewport { .. } => {
            unreachable!("asynchronous canvas operations are routed before direct application")
        }
    };
    Ok(output)
}

fn set_camera(presenter: &mut Presenter, camera: Camera) {
    let view = ViewState {
        size: presenter.viewport().size(),
        camera,
    };
    presenter.set_canvas_view(view);
}

fn element_frame(element: TransformFrame) -> ElementFrame {
    ElementFrame {
        element: element.element,
        frame: Frame {
            x: element.frame.x,
            y: element.frame.y,
            width: element.frame.width,
            height: element.frame.height,
            angle_degrees: element.frame.angle_degrees,
        },
    }
}

fn canvas_state(presenter: &Presenter, fitted: bool) -> CanvasState {
    let camera = presenter.canvas().camera();
    CanvasState {
        zoom: camera.zoom(),
        translation: camera.translation(),
        fitted,
        element_frames: presenter
            .canvas()
            .element_frames()
            .into_iter()
            .map(|element| TransformFrame {
                element: element.element,
                frame: koharu_protocol::CanvasFrame {
                    x: element.frame.x,
                    y: element.frame.y,
                    width: element.frame.width,
                    height: element.frame.height,
                    angle_degrees: element.frame.angle_degrees,
                },
            })
            .collect(),
    }
}

struct ApplicationDelegate {
    runtime: tokio::runtime::Handle,
    application: Arc<Application>,
    presentation: Arc<DesktopPresentation>,
    presentation_state: PresentationState,
    pending: HashMap<u64, PendingShell>,
    next_pending: u64,
    initialized: bool,
    event_subscription: Option<broadcast::Receiver<ServerEvent>>,
    fatal: Arc<Mutex<Option<String>>>,
    updater: Arc<koharu_updater::Updater>,
    available_update: Arc<Mutex<Option<koharu_updater::Update>>>,
}

struct PendingShell {
    request: RequestId,
    kind: ShellCommand,
}

#[derive(Clone, Copy)]
enum ShellCommand {
    Minimize,
    ToggleMaximize,
    BeginDrag,
    Close,
    OpenExternal,
}

impl ApplicationDelegate {
    fn new(
        runtime: tokio::runtime::Handle,
        application: Arc<Application>,
        presentation: Arc<DesktopPresentation>,
        fatal: Arc<Mutex<Option<String>>>,
        updater: Arc<koharu_updater::Updater>,
    ) -> Self {
        let event_subscription = application.events().subscribe();
        Self {
            runtime,
            application,
            presentation,
            presentation_state: PresentationState { fitted: true },
            pending: HashMap::new(),
            next_pending: 1,
            initialized: false,
            event_subscription: Some(event_subscription),
            fatal,
            updater,
            available_update: Arc::new(Mutex::new(None)),
        }
    }

    fn begin_events(&mut self, handle: &DesktopHandle) {
        let Some(mut subscription) = self.event_subscription.take() else {
            return;
        };
        let handle = handle.clone();
        self.runtime.spawn(async move {
            loop {
                match subscription.recv().await {
                    Ok(event) => send_server(&handle, ServerMessage::from(event), Vec::new()),
                    Err(error) => {
                        tracing::error!(%error, "application event stream lost ordering");
                        let _ = handle.shutdown();
                        break;
                    }
                }
            }
        });
    }

    fn dispatch_application(&self, request: RequestId, command: Command, handle: DesktopHandle) {
        let application = Arc::clone(&self.application);
        self.runtime.spawn(async move {
            match application.dispatch(command).await {
                Ok(output) => {
                    let attachments = output
                        .attachments
                        .into_iter()
                        .map(|attachment| BinaryAttachment::new(attachment.id, attachment.bytes))
                        .collect::<std::result::Result<Vec<_>, _>>();
                    match attachments {
                        Ok(attachments) => send_server(
                            &handle,
                            Response::success(request, output.result).into(),
                            attachments,
                        ),
                        Err(error) => send_failure(&handle, request, AppError::internal(error)),
                    }
                }
                Err(error) => send_failure(&handle, request, error),
            }
        });
    }

    fn check_update(&self, request: RequestId, handle: DesktopHandle) {
        let updater = Arc::clone(&self.updater);
        let available_update = Arc::clone(&self.available_update);
        self.runtime.spawn(async move {
            let info = match updater.check().await {
                Ok(Some(update)) => {
                    let info = koharu_protocol::UpdateInfo {
                        version: update.version().to_string(),
                        body: update.body().map(str::to_owned),
                    };
                    *available_update.lock() = Some(update);
                    Some(info)
                }
                Ok(None) => {
                    *available_update.lock() = None;
                    None
                }
                Err(error) => {
                    *available_update.lock() = None;
                    tracing::warn!(%error, "could not discover a Koharu update");
                    None
                }
            };
            send_server(
                &handle,
                Response::success(request, CommandResult::OptionalUpdate(info)).into(),
                Vec::new(),
            );
        });
    }

    fn install_update(&self, request: RequestId, version: String, handle: DesktopHandle) {
        let update = self
            .available_update
            .lock()
            .as_ref()
            .filter(|update| update.version().to_string() == version)
            .cloned();
        let Some(update) = update else {
            send_failure(
                &handle,
                request,
                AppError::new(
                    AppErrorCode::NotFound,
                    "the requested update is no longer available; check again",
                ),
            );
            return;
        };
        let updater = Arc::clone(&self.updater);
        let events = self.application.events();
        self.runtime.spawn(async move {
            let progress_version = version.clone();
            let result = updater
                .download_and_install(&update, move |progress| {
                    events.publish(AppEvent::UpdateProgress {
                        progress: koharu_protocol::UpdateProgress {
                            version: progress_version.clone(),
                            downloaded: progress.downloaded,
                            total: progress.total,
                        },
                    });
                })
                .await;
            match result {
                Ok(()) => {
                    send_server(
                        &handle,
                        Response::success(request, CommandResult::Unit(())).into(),
                        Vec::new(),
                    );
                    if let Err(error) = handle.shutdown() {
                        tracing::warn!(%error, "could not close Koharu after installing an update");
                    }
                }
                Err(error) => send_failure(
                    &handle,
                    request,
                    AppError::new(AppErrorCode::Unavailable, error.to_string()),
                ),
            }
        });
    }

    fn pending(&mut self, request: RequestId, kind: ShellCommand) -> u64 {
        let id = self.next_pending;
        self.next_pending = self.next_pending.wrapping_add(1).max(1);
        self.pending.insert(id, PendingShell { request, kind });
        id
    }

    fn shell_error(&mut self, id: u64, handle: &DesktopHandle, error: impl std::fmt::Display) {
        if let Some(pending) = self.pending.remove(&id) {
            send_failure(handle, pending.request, AppError::internal(error));
        }
    }
}

impl DesktopDelegate for ApplicationDelegate {
    fn browser_ready(&mut self, handle: &DesktopHandle) {
        self.presentation.attach(handle.clone());
        self.begin_events(handle);
        if self.initialized {
            return;
        }
        self.initialized = true;
        let application = Arc::clone(&self.application);
        self.runtime.spawn(async move {
            let _ = application.initialize().await;
        });
    }

    fn browser_message(
        &mut self,
        message: WebMessage,
        _presenter: &mut Presenter,
        handle: &DesktopHandle,
    ) {
        let request: Request = match serde_json::from_str(&message.json) {
            Ok(request) => request,
            Err(error) => {
                tracing::warn!(%error, "ignored malformed browser request");
                return;
            }
        };
        let Request { id, command } = request;
        if !message.attachments.is_empty() {
            send_failure(
                handle,
                id,
                AppError::new(
                    AppErrorCode::InvalidRequest,
                    "client commands do not accept binary attachments",
                ),
            );
            return;
        }
        match command {
            Command::WindowMinimize {} => {
                let request = self.pending(id, ShellCommand::Minimize);
                if let Err(error) = handle.window(request, WindowAction::Minimize) {
                    self.shell_error(request, handle, error);
                }
            }
            Command::WindowToggleMaximize {} => {
                let request = self.pending(id, ShellCommand::ToggleMaximize);
                if let Err(error) = handle.window(request, WindowAction::ToggleMaximize) {
                    self.shell_error(request, handle, error);
                }
            }
            Command::WindowBeginDrag {} => {
                let request = self.pending(id, ShellCommand::BeginDrag);
                if let Err(error) = handle.window(request, WindowAction::BeginDrag) {
                    self.shell_error(request, handle, error);
                }
            }
            Command::WindowClose {} => {
                let request = self.pending(id, ShellCommand::Close);
                if let Err(error) = handle.window(request, WindowAction::Close) {
                    self.shell_error(request, handle, error);
                }
            }
            Command::OpenExternal { url } => match external_url(&url) {
                Ok(url) => {
                    let request = self.pending(id, ShellCommand::OpenExternal);
                    if let Err(error) = handle.open_external(request, url) {
                        self.shell_error(request, handle, error);
                    }
                }
                Err(error) => send_failure(handle, id, AppError::internal(error)),
            },
            Command::GetVersion {} => send_server(
                handle,
                Response::success(id, CommandResult::Version(env!("CARGO_PKG_VERSION").into()))
                    .into(),
                Vec::new(),
            ),
            Command::CheckUpdate {} => self.check_update(id, handle.clone()),
            Command::InstallUpdate { version } => {
                self.install_update(id, version, handle.clone());
            }
            command => self.dispatch_application(id, command, handle.clone()),
        }
    }

    fn wake(&mut self, presenter: &mut Presenter, _handle: &DesktopHandle) {
        self.presentation
            .drain(presenter, &mut self.presentation_state);
    }

    fn platform_response(
        &mut self,
        id: u64,
        response: std::result::Result<(), PlatformError>,
        handle: &DesktopHandle,
    ) {
        let Some(pending) = self.pending.remove(&id) else {
            tracing::warn!(id, "received an unknown platform completion");
            return;
        };
        match response {
            Ok(_) if matches!(pending.kind, ShellCommand::OpenExternal) => send_server(
                handle,
                Response::success(pending.request, CommandResult::Unit(())).into(),
                Vec::new(),
            ),
            Ok(_) => send_failure(
                handle,
                pending.request,
                AppError::new(AppErrorCode::Internal, "unexpected platform completion"),
            ),
            Err(error) => send_failure(handle, pending.request, AppError::internal(error)),
        }
    }

    fn window_action_response(
        &mut self,
        id: u64,
        _action: WindowAction,
        response: std::result::Result<WindowState, WindowActionError>,
        handle: &DesktopHandle,
    ) {
        let Some(pending) = self.pending.remove(&id) else {
            tracing::warn!(id, "received an unknown window completion");
            return;
        };
        match response {
            Ok(state) => {
                let result = match pending.kind {
                    ShellCommand::ToggleMaximize => {
                        CommandResult::WindowState(protocol_window_state(state))
                    }
                    ShellCommand::Minimize | ShellCommand::BeginDrag | ShellCommand::Close => {
                        CommandResult::Unit(())
                    }
                    ShellCommand::OpenExternal => {
                        send_failure(
                            handle,
                            pending.request,
                            AppError::new(AppErrorCode::Internal, "unexpected window completion"),
                        );
                        return;
                    }
                };
                send_server(
                    handle,
                    Response::success(pending.request, result).into(),
                    Vec::new(),
                );
            }
            Err(error) => send_failure(handle, pending.request, AppError::internal(error)),
        }
    }

    fn window_state_changed(&mut self, state: WindowState, _handle: &DesktopHandle) {
        self.application.events().publish(AppEvent::WindowState {
            state: protocol_window_state(state),
        });
    }

    fn viewport_applied(
        &mut self,
        id: u64,
        result: std::result::Result<PhysicalRect, LayoutError>,
        presenter: &mut Presenter,
        _handle: &DesktopHandle,
    ) {
        self.presentation
            .complete_viewport(id, result, presenter, &mut self.presentation_state);
    }

    fn fatal_error(&mut self, error: &str) {
        tracing::error!(error, "desktop runtime stopped");
        *self.fatal.lock() = Some(error.to_owned());
    }
}

fn protocol_window_state(state: WindowState) -> koharu_protocol::WindowState {
    koharu_protocol::WindowState {
        maximized: state.maximized,
        minimized: state.minimized,
        fullscreen: state.fullscreen,
        focused: state.focused,
    }
}

fn send_failure(handle: &DesktopHandle, id: RequestId, error: AppError) {
    send_server(handle, Response::failure(id, error).into(), Vec::new());
}

fn send_server(handle: &DesktopHandle, message: ServerMessage, attachments: Vec<BinaryAttachment>) {
    let result = serde_json::to_string(&message)
        .context("failed to serialize a server message")
        .and_then(|json| {
            WebMessage::with_attachments(json, attachments)
                .context("failed to prepare browser attachments")
        })
        .and_then(|message| {
            handle
                .send_web_message(message)
                .context("desktop browser is closed")
        });
    if let Err(error) = result {
        tracing::error!(?error, "failed to send a browser message");
    }
}

fn external_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("external URL is invalid")?;
    if !matches!(url.scheme(), "http" | "https" | "mailto") {
        bail!("external URL scheme is not allowed");
    }
    Ok(url)
}

struct DesktopPlatform;

impl PlatformServices for DesktopPlatform {
    fn open_external(&mut self, url: &Url) -> std::result::Result<(), PlatformError> {
        open::that(url.as_str()).map_err(|error| PlatformError::Failed(error.to_string()))
    }
}

struct DesktopFileDialogs;

#[async_trait]
impl FileDialogs for DesktopFileDialogs {
    async fn pick_files(&self, filters: &[DialogFilter]) -> Result<Option<Vec<PathBuf>>> {
        Ok(async_dialog(filters).pick_files().await.map(|files| {
            files
                .into_iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        }))
    }

    async fn pick_folder(&self) -> Result<Option<PathBuf>> {
        Ok(rfd::AsyncFileDialog::new()
            .pick_folder()
            .await
            .map(|folder| folder.path().to_path_buf()))
    }

    async fn save_file(
        &self,
        suggested_name: &str,
        filters: &[DialogFilter],
    ) -> Result<Option<PathBuf>> {
        Ok(async_dialog(filters)
            .set_file_name(suggested_name)
            .save_file()
            .await
            .map(|file| file.path().to_path_buf()))
    }
}

fn async_dialog(filters: &[DialogFilter]) -> rfd::AsyncFileDialog {
    filters
        .iter()
        .fold(rfd::AsyncFileDialog::new(), |dialog, filter| {
            dialog.add_filter(
                &filter.name,
                &filter
                    .extensions
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
        })
}
