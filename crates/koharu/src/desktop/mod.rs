use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use koharu_canvas::{Camera, Canvas, ViewState};
use koharu_renderer::Renderer;
use koharu_scene::{Commit, EntityId, Snapshot};
use parking_lot::{Mutex, MutexGuard};
use tauri::{AppHandle, Manager as _, WebviewWindow};
use tokio::sync::OnceCell;

mod gpu;

pub(crate) use gpu::PhysicalRect;

use self::gpu::Presenter;
use crate::commands::canvas::{CanvasState, Frame, TransformFrame};

const MAIN_WINDOW: &str = "main";
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

pub(crate) struct Desktop {
    app: AppHandle,
    renderer: Renderer,
    presenter: OnceCell<Mutex<Presenter>>,
    composition_generation: AtomicU64,
    frame_requested: AtomicBool,
}

impl Desktop {
    pub(crate) fn new(app: AppHandle) -> Result<Self> {
        Ok(Self {
            app,
            renderer: Renderer::new().context("failed to initialize the page renderer")?,
            presenter: OnceCell::new(),
            composition_generation: AtomicU64::new(0),
            frame_requested: AtomicBool::new(false),
        })
    }

    fn request_frame(&self) -> Result<()> {
        if self.frame_requested.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let frame_app = self.app.clone();
        // A guard can also be dropped while the event loop is presenting. Calling
        // `run_on_main_thread` directly there could run the callback inline while
        // the renderer mutex is still held.
        tauri::async_runtime::spawn(async move {
            let callback_app = frame_app.clone();
            let next_frame_app = frame_app.clone();
            let error_app = frame_app.clone();
            if let Err(error) = frame_app.run_on_main_thread(move || {
                let Some(desktop) = callback_app.try_state::<Desktop>() else {
                    return;
                };
                desktop.frame_requested.store(false, Ordering::Release);
                let result = (|| {
                    let window = callback_app
                        .get_webview_window(MAIN_WINDOW)
                        .context("the main Tauri webview window is unavailable")?;
                    let size = window.inner_size()?;
                    let Some(presenter) = desktop.presenter.get() else {
                        return Ok(false);
                    };
                    presenter
                        .lock()
                        .present(koharu_canvas::PhysicalSize::new(size.width, size.height))
                })();
                match result {
                    Ok(true) => {
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(FRAME_INTERVAL).await;
                            if let Some(desktop) = next_frame_app.try_state::<Desktop>()
                                && let Err(error) = desktop.request_frame()
                            {
                                tracing::error!(%error, "failed to schedule the next canvas frame");
                            }
                        });
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::error!(error = ?error, "failed to present the canvas");
                    }
                }
            }) {
                if let Some(desktop) = error_app.try_state::<Desktop>() {
                    desktop.frame_requested.store(false, Ordering::Release);
                }
                tracing::error!(%error, "failed to schedule the canvas frame");
            }
        });
        Ok(())
    }
}

pub(crate) async fn attach(window: WebviewWindow) -> Result<()> {
    let app = window.app_handle().clone();
    let wake_app = app.clone();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        if let Some(desktop) = wake_app.try_state::<Desktop>()
            && let Err(error) = desktop.request_frame()
        {
            tracing::error!(%error, "failed to schedule a canvas frame");
        }
    });
    let desktop = app.state::<Desktop>();
    desktop
        .presenter
        .get_or_try_init(|| async { Presenter::new(window, wake).await.map(Mutex::new) })
        .await?;
    Ok(())
}

impl Desktop {
    #[must_use]
    pub(crate) const fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub(crate) async fn show_page(
        &self,
        snapshot: &Snapshot,
        page: Option<EntityId>,
    ) -> Result<()> {
        let generation = self
            .composition_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        let composition = match page {
            Some(page) => Some(self.renderer.compose(snapshot, page).await?),
            None => None,
        };
        if self.composition_generation.load(Ordering::Acquire) != generation {
            return Ok(());
        }

        let mut desktop = self.lock();
        match composition {
            Some(composition) => {
                let (width, height) = composition.size();
                desktop.canvas().set_composition(composition)?;
                let mut view = desktop.view().clone();
                view.camera = koharu_canvas::Camera::contain(
                    desktop.viewport().size(),
                    koharu_canvas::PhysicalSize::new(width, height),
                );
                desktop.set_view(view);
            }
            None => desktop.canvas().clear(),
        }
        Ok(())
    }

    pub(crate) async fn synchronize(
        &self,
        snapshot: &Snapshot,
        page: Option<EntityId>,
        commit: &Commit,
    ) -> Result<bool> {
        let (current_page, previous) = {
            let desktop = self.lock();
            (
                desktop.canvas_ref().page_id(),
                desktop.canvas_ref().composition().cloned(),
            )
        };
        if current_page != page {
            self.show_page(snapshot, page).await?;
            return Ok(true);
        }
        let Some(page) = page else {
            return Ok(false);
        };
        let Some(previous) = previous else {
            self.show_page(snapshot, Some(page)).await?;
            return Ok(true);
        };
        if previous.revision() == snapshot.revision() {
            return Ok(false);
        }

        let generation = self
            .composition_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        let next = if commit.revision == snapshot.revision()
            && commit.changes.from == previous.revision()
        {
            self.renderer
                .update(&previous, snapshot, &commit.changes)
                .await?
        } else {
            self.renderer.compose(snapshot, page).await?
        };
        if self.composition_generation.load(Ordering::Acquire) == generation {
            self.lock().canvas().set_composition(next)?;
        }
        Ok(false)
    }
}

pub(crate) struct DesktopGuard<'a> {
    desktop: &'a Desktop,
    presenter: MutexGuard<'a, Presenter>,
    redraw: bool,
}

impl Desktop {
    pub(crate) fn lock(&self) -> DesktopGuard<'_> {
        DesktopGuard {
            desktop: self,
            presenter: self
                .presenter
                .get()
                .expect("desktop startup completes before canvas IPC is accepted")
                .lock(),
            redraw: false,
        }
    }
}

impl Drop for DesktopGuard<'_> {
    fn drop(&mut self) {
        if self.redraw
            && let Err(error) = self.desktop.request_frame()
        {
            tracing::error!(%error, "failed to schedule a canvas frame");
        }
    }
}

impl DesktopGuard<'_> {
    #[must_use]
    pub fn canvas_state(&mut self, fitted: bool) -> CanvasState {
        let camera = self.view().camera;
        let element_frames = self
            .canvas()
            .element_frames()
            .into_iter()
            .map(|element| TransformFrame {
                element: element.element,
                frame: Frame {
                    x: element.frame.x,
                    y: element.frame.y,
                    width: element.frame.width,
                    height: element.frame.height,
                    angle_degrees: element.frame.angle_degrees,
                },
            })
            .collect();
        CanvasState {
            zoom: camera.zoom(),
            translation: camera.translation(),
            fitted,
            element_frames,
        }
    }

    #[must_use]
    pub fn viewport(&self) -> PhysicalRect {
        self.presenter.viewport()
    }

    #[must_use]
    pub fn view(&self) -> &ViewState {
        self.presenter.view()
    }

    pub fn set_view(&mut self, view: ViewState) {
        self.presenter.set_view(view);
        self.request_redraw();
    }

    pub fn set_camera(&mut self, camera: Camera) {
        self.presenter.canvas().set_camera(camera);
        self.request_redraw();
    }

    pub fn set_viewport(&mut self, viewport: PhysicalRect, background: [u8; 3]) {
        self.presenter.set_viewport(viewport, background);
        self.request_redraw();
    }

    pub fn canvas(&mut self) -> &mut Canvas {
        self.request_redraw();
        self.presenter.canvas()
    }

    #[must_use]
    pub fn canvas_ref(&self) -> &Canvas {
        self.presenter.canvas_ref()
    }

    fn request_redraw(&mut self) {
        self.redraw = true;
    }
}
