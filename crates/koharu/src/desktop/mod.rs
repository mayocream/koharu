use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use koharu_canvas::{Camera, Canvas, ViewState};
use koharu_scene::{Commit, EntityId, Snapshot};
use parking_lot::{Mutex, MutexGuard};
use tauri::{AppHandle, Manager as _, WebviewWindow};
use tokio::sync::OnceCell;

mod gpu;

pub(crate) use gpu::PhysicalRect;

use self::gpu::Renderer;
use crate::commands::canvas::{CanvasState, Frame, TransformFrame};

const MAIN_WINDOW: &str = "main";
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

pub(crate) struct Desktop {
    app: AppHandle,
    renderer: OnceCell<Mutex<Renderer>>,
    frame_requested: AtomicBool,
}

impl Desktop {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self {
            app,
            renderer: OnceCell::new(),
            frame_requested: AtomicBool::new(false),
        }
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
                    let Some(renderer) = desktop.renderer.get() else {
                        return Ok(false);
                    };
                    renderer.lock().present(
                        Instant::now(),
                        koharu_canvas::PhysicalSize::new(size.width, size.height),
                    )
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
        .renderer
        .get_or_try_init(|| async { Renderer::new(window, wake).await.map(Mutex::new) })
        .await?;
    Ok(())
}

pub(crate) struct DesktopGuard<'a> {
    desktop: &'a Desktop,
    renderer: MutexGuard<'a, Renderer>,
    redraw: bool,
}

impl Desktop {
    pub(crate) fn lock(&self) -> DesktopGuard<'_> {
        DesktopGuard {
            desktop: self,
            renderer: self
                .renderer
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
    pub fn synchronize(
        &mut self,
        snapshot: &Snapshot,
        page: Option<EntityId>,
        commit: &Commit,
    ) -> Result<bool> {
        if self.canvas_ref().page_id() != page {
            self.show_page(snapshot, page)?;
            return Ok(true);
        }

        let revision = self.canvas_ref().revision();
        if revision == snapshot.revision() {
            return Ok(false);
        }

        if commit.revision == snapshot.revision() && commit.changes.from == revision {
            self.canvas().sync(&commit.snapshot, &commit.changes)?;
        } else {
            self.canvas().show_snapshot(snapshot, page)?;
        }
        Ok(false)
    }

    pub fn show_page(&mut self, snapshot: &Snapshot, page: Option<EntityId>) -> Result<()> {
        self.canvas().show_snapshot(snapshot, page)?;
        if let Some(page) = page {
            let page = snapshot.page(page)?.page()?;
            let mut view = self.view().clone();
            view.camera = koharu_canvas::Camera::contain(
                self.viewport().size(),
                koharu_canvas::PhysicalSize::new(
                    page.width.ceil() as u32,
                    page.height.ceil() as u32,
                ),
            );
            self.set_view(view);
        }
        Ok(())
    }

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
        self.renderer.viewport()
    }

    #[must_use]
    pub fn view(&self) -> &ViewState {
        self.renderer.view()
    }

    pub fn set_view(&mut self, view: ViewState) {
        self.renderer.set_view(view);
        self.request_redraw();
    }

    pub fn set_camera(&mut self, camera: Camera) {
        self.renderer.canvas().set_camera(camera);
        self.request_redraw();
    }

    pub fn set_viewport(&mut self, viewport: PhysicalRect, background: [u8; 3]) {
        self.renderer.set_viewport(viewport, background);
        self.request_redraw();
    }

    pub fn canvas(&mut self) -> &mut Canvas {
        self.request_redraw();
        self.renderer.canvas()
    }

    #[must_use]
    pub fn canvas_ref(&self) -> &Canvas {
        self.renderer.canvas_ref()
    }

    fn request_redraw(&mut self) {
        self.redraw = true;
    }
}
