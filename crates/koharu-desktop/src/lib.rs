//! Native CEF and WGPU composition for Koharu's desktop window.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use koharu_canvas::{Camera, Canvas, ViewState};
use koharu_scene::{Commit, EntityId, Snapshot};
use parking_lot::{Mutex, MutexGuard};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Cef, CefOffscreenSurface, Manager as _, WebviewWindow};
use tokio::sync::{Mutex as AsyncMutex, OnceCell};

mod gpu;

pub use gpu::PhysicalRect;

use self::gpu::Presenter;

const MAIN_WINDOW: &str = "main";
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub angle_degrees: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type)]
pub struct TransformFrame {
    pub element: EntityId,
    pub frame: Frame,
}

impl From<TransformFrame> for koharu_canvas::ElementFrame {
    fn from(element: TransformFrame) -> Self {
        Self {
            element: element.element,
            frame: koharu_canvas::Frame {
                x: element.frame.x,
                y: element.frame.y,
                width: element.frame.width,
                height: element.frame.height,
                angle_degrees: element.frame.angle_degrees,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct CanvasState {
    pub zoom: f64,
    pub translation: [f64; 2],
    pub fitted: bool,
    pub element_frames: Vec<TransformFrame>,
}

pub struct Desktop {
    app: AppHandle<Cef>,
    renderer: koharu_renderer::Renderer,
    presenter: OnceCell<Mutex<Presenter>>,
    preparation: AsyncMutex<()>,
    frame_requested: AtomicBool,
}

impl Desktop {
    pub fn new(app: AppHandle<Cef>) -> Result<Self> {
        Ok(Self {
            app,
            renderer: koharu_renderer::Renderer::new()?,
            presenter: OnceCell::new(),
            preparation: AsyncMutex::new(()),
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

pub fn offscreen_surface(app: &AppHandle<Cef>) -> CefOffscreenSurface {
    let wake_app = app.clone();
    CefOffscreenSurface::new(move || {
        if let Some(desktop) = wake_app.try_state::<Desktop>()
            && let Err(error) = desktop.request_frame()
        {
            tracing::error!(%error, "failed to schedule a CEF frame");
        }
    })
}

pub async fn attach(window: WebviewWindow<Cef>, offscreen: CefOffscreenSurface) -> Result<()> {
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
        .get_or_try_init(|| async {
            Presenter::new(window.as_ref().window().clone(), wake, offscreen)
                .await
                .map(Mutex::new)
        })
        .await?;
    Ok(())
}

pub struct DesktopGuard<'a> {
    desktop: &'a Desktop,
    presenter: MutexGuard<'a, Presenter>,
    redraw: bool,
}

impl Desktop {
    pub fn lock(&self) -> DesktopGuard<'_> {
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

impl Desktop {
    #[must_use]
    pub fn renderer(&self) -> koharu_renderer::Renderer {
        self.renderer.clone()
    }

    #[tracing::instrument(skip_all)]
    pub async fn synchronize(
        &self,
        snapshot: &Snapshot,
        page: Option<EntityId>,
        commit: &Commit,
    ) -> Result<bool> {
        let _preparation = self.preparation.lock().await;
        let (current_page, revision, previous) = {
            let desktop = self.lock();
            (
                desktop.canvas_ref().page_id(),
                desktop.canvas_ref().revision(),
                desktop.canvas_ref().frame().cloned(),
            )
        };
        if current_page != page {
            self.show_page_locked(snapshot, page).await?;
            return Ok(true);
        }

        if revision == Some(snapshot.revision()) {
            return Ok(false);
        }

        let Some(page) = page else {
            self.lock().canvas().clear();
            self.renderer.discard_retained_nodes();
            return Ok(false);
        };
        let frame = if commit.revision == snapshot.revision()
            && revision == Some(commit.changes.from)
            && let Some(previous) = previous.as_ref()
        {
            self.renderer
                .update(previous, snapshot, &commit.changes)
                .await?
        } else {
            self.renderer.render(snapshot, page).await?
        };
        self.lock().canvas().set_frame(frame)?;
        Ok(false)
    }

    #[tracing::instrument(skip_all)]
    pub async fn show_page(&self, snapshot: &Snapshot, page: Option<EntityId>) -> Result<()> {
        let _preparation = self.preparation.lock().await;
        self.show_page_locked(snapshot, page).await
    }

    pub async fn clear(&self) {
        let _preparation = self.preparation.lock().await;
        self.lock().canvas().clear();
        self.renderer.discard_retained_nodes();
    }

    async fn show_page_locked(&self, snapshot: &Snapshot, page: Option<EntityId>) -> Result<()> {
        let Some(page) = page else {
            self.lock().canvas().clear();
            self.renderer.discard_retained_nodes();
            return Ok(());
        };
        let frame = self.renderer.render(snapshot, page).await?;
        let size = frame.size();
        let mut desktop = self.lock();
        desktop.canvas().set_frame(frame)?;
        {
            let mut view = desktop.view().clone();
            view.camera = koharu_canvas::Camera::contain(
                desktop.viewport().size(),
                koharu_canvas::PhysicalSize::new(size.0, size.1),
            );
            desktop.set_view(view);
        }
        Ok(())
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
