use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context as _;
use koharu_canvas::{MaskOverlay, MaskTarget, PagePoint, PhysicalPoint};
use koharu_scene::{EntityId, Revision};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Cef, Manager as _, State, ipc::Channel};

use super::{
    ChannelExt as _, Error, processing,
    processing::{JobChannel, JobId, Processing},
    project::CurrentProject,
};
use crate::desktop::Desktop;

const INPAINT_MASK: MaskTarget = MaskTarget::Scratch(0);
const INPAINT_OVERLAY: MaskOverlay = MaskOverlay::new([168, 85, 247, 210], 0.55);

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

impl From<Point> for PagePoint {
    fn from(value: Point) -> Self {
        Self::new(value.x, value.y)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
pub struct PaintBrush {
    pub diameter: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, Serialize, Type)]
pub struct LayerCommit {
    pub revision: Revision,
    pub layer: EntityId,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type)]
pub struct TransformFrame {
    pub element: EntityId,
    pub frame: Frame,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct CanvasState {
    pub zoom: f64,
    pub translation: [f64; 2],
    pub fitted: bool,
    pub element_frames: Vec<TransformFrame>,
}

pub(crate) struct CanvasView {
    pub(crate) fitted: AtomicBool,
}

#[derive(Default)]
pub(crate) struct CanvasChannel {
    pub(crate) channel: Mutex<Option<Channel<CanvasState>>>,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_zoom(
    desktop: State<'_, Desktop>,
    zoom: f32,
    canvas_view: State<'_, CanvasView>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    if !zoom.is_finite() || !(0.02..=16.0).contains(&zoom) {
        return Err(anyhow::anyhow!("camera zoom must be between 2% and 1600%").into());
    }
    let canvas = {
        let mut desktop = desktop.lock();
        let mut camera = desktop.view().camera;
        let center = PhysicalPoint::new(
            f64::from(desktop.viewport().size().width) * 0.5,
            f64::from(desktop.viewport().size().height) * 0.5,
        );
        camera.zoom_around(center, f64::from(zoom))?;
        desktop.set_camera(camera);
        canvas_view.fitted.store(false, Ordering::Release);
        desktop.canvas_state(false)
    };
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_canvas_view(
    desktop: State<'_, Desktop>,
    zoom: f64,
    translation: [f64; 2],
    canvas_view: State<'_, CanvasView>,
) -> Result<(), Error> {
    if !(0.02..=16.0).contains(&zoom) {
        return Err(anyhow::anyhow!("camera zoom must be between 2% and 1600%").into());
    }
    desktop
        .lock()
        .set_camera(koharu_canvas::Camera::new(zoom, translation)?);
    canvas_view.fitted.store(false, Ordering::Release);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn fit_canvas(
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_view: State<'_, CanvasView>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let size = {
        let project = project.project.lock().await;
        let project = project.as_ref().context("no project is open")?;
        let page = project
            .active_page()
            .context("the project has no active page")?;
        let snapshot = project.snapshot();
        let page = snapshot.page(page)?.page()?;
        koharu_canvas::PhysicalSize::new(page.width.ceil() as u32, page.height.ceil() as u32)
    };
    let canvas = {
        let mut desktop = desktop.lock();
        desktop.set_camera(koharu_canvas::Camera::contain(
            desktop.viewport().size(),
            size,
        ));
        canvas_view.fitted.store(true, Ordering::Release);
        desktop.canvas_state(true)
    };
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn add_point_text(
    point: Point,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
) -> Result<LayerCommit, Error> {
    let (commit, page, layer) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let page = project
            .active_page()
            .context("the project has no active page")?;
        let (commit, layer) = project.add_point_text(page, point).await?;
        project.record_commit(&commit);
        (commit, project.active_page(), layer)
    };
    desktop.synchronize(&commit.snapshot, page, &commit).await?;
    Ok(LayerCommit {
        revision: commit.revision,
        layer,
    })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn add_text_box(
    frame: Frame,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
) -> Result<LayerCommit, Error> {
    let (commit, page, layer) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let page = project
            .active_page()
            .context("the project has no active page")?;
        let (commit, layer) = project.add_text_box(page, frame).await?;
        project.record_commit(&commit);
        (commit, project.active_page(), layer)
    };
    desktop.synchronize(&commit.snapshot, page, &commit).await?;
    Ok(LayerCommit {
        revision: commit.revision,
        layer,
    })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn begin_paint(
    layer: Option<EntityId>,
    point: Point,
    brush: PaintBrush,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop.lock().canvas().begin_raster_stroke(
        layer,
        koharu_canvas::Brush {
            diameter: brush.diameter,
            color: brush.color,
            mode: koharu_canvas::StrokeMode::Paint,
        },
        point.into(),
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn extend_paint(
    points: Vec<Point>,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop
        .lock()
        .canvas()
        .extend_raster_stroke(&points.into_iter().map(PagePoint::from).collect::<Vec<_>>())?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn finish_paint(
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
) -> Result<LayerCommit, Error> {
    let stroke = desktop.lock().canvas().finish_raster_stroke()?;
    let result: anyhow::Result<_> = async {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let (commit, element) = project
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
            .await?;
        project.record_commit(&commit);
        Ok((commit, project.active_page(), element))
    }
    .await;
    let (commit, page, element) = match result {
        Ok(result) => result,
        Err(error) => {
            desktop.lock().canvas().cancel_raster_stroke();
            return Err(error.into());
        }
    };
    desktop
        .lock()
        .canvas()
        .acknowledge_raster_commit(stroke.page, commit.revision)?;
    desktop.synchronize(&commit.snapshot, page, &commit).await?;
    Ok(LayerCommit {
        revision: commit.revision,
        layer: element,
    })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cancel_paint(desktop: State<'_, Desktop>) -> Result<(), Error> {
    desktop.lock().canvas().cancel_raster_stroke();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn begin_erase(
    layer: EntityId,
    point: Point,
    diameter: f32,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop.lock().canvas().begin_raster_stroke(
        Some(layer),
        koharu_canvas::Brush {
            diameter,
            color: [0, 0, 0, 0],
            mode: koharu_canvas::StrokeMode::Erase,
        },
        point.into(),
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn extend_erase(
    points: Vec<Point>,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop
        .lock()
        .canvas()
        .extend_raster_stroke(&points.into_iter().map(PagePoint::from).collect::<Vec<_>>())?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn finish_erase(
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
) -> Result<LayerCommit, Error> {
    let stroke = desktop.lock().canvas().finish_raster_stroke()?;
    let result: anyhow::Result<_> = async {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let (commit, element) = project
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
            .await?;
        project.record_commit(&commit);
        Ok((commit, project.active_page(), element))
    }
    .await;
    let (commit, page, element) = match result {
        Ok(result) => result,
        Err(error) => {
            desktop.lock().canvas().cancel_raster_stroke();
            return Err(error.into());
        }
    };
    desktop
        .lock()
        .canvas()
        .acknowledge_raster_commit(stroke.page, commit.revision)?;
    desktop.synchronize(&commit.snapshot, page, &commit).await?;
    Ok(LayerCommit {
        revision: commit.revision,
        layer: element,
    })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cancel_erase(desktop: State<'_, Desktop>) -> Result<(), Error> {
    desktop.lock().canvas().cancel_raster_stroke();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn begin_transform(
    elements: Vec<TransformFrame>,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop.lock().canvas().begin_transform(
        &elements
            .into_iter()
            .map(koharu_canvas::ElementFrame::from)
            .collect::<Vec<_>>(),
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn update_transform(
    frame: u32,
    elements: Vec<TransformFrame>,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop.lock().canvas().update_transform(
        u64::from(frame),
        &elements
            .into_iter()
            .map(koharu_canvas::ElementFrame::from)
            .collect::<Vec<_>>(),
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn preview_opacity(
    element: EntityId,
    opacity: Option<f32>,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop.lock().canvas().preview_opacity(element, opacity)?;
    Ok(())
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

#[tauri::command]
#[specta::specta]
pub(crate) async fn finish_transform(
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_view: State<'_, CanvasView>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<Option<Revision>, Error> {
    let Some(transform) = desktop.lock().canvas().finish_transform()? else {
        return Ok(None);
    };
    let project_result: Result<_, Error> = async {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project
            .set_geometries(
                transform
                    .elements
                    .into_iter()
                    .map(|element| (element.element, element.geometry)),
            )
            .await?;
        project.record_commit(&commit);
        Ok((commit, project.active_page()))
    }
    .await;
    let (commit, page) = match project_result {
        Ok(result) => result,
        Err(error) => {
            desktop.lock().canvas().cancel_transform();
            return Err(error);
        }
    };
    desktop
        .lock()
        .canvas()
        .acknowledge_transform_commit(transform.page, commit.revision)?;
    desktop.synchronize(&commit.snapshot, page, &commit).await?;
    let canvas = {
        let mut desktop = desktop.lock();
        desktop.canvas_state(canvas_view.fitted.load(Ordering::Acquire))
    };
    canvas_channel.channel.publish(canvas);
    Ok(Some(commit.revision))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cancel_transform(desktop: State<'_, Desktop>) -> Result<(), Error> {
    desktop.lock().canvas().cancel_transform();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn begin_inpaint(
    point: Point,
    diameter: f32,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop.lock().canvas().begin_mask_stroke(
        INPAINT_MASK,
        INPAINT_OVERLAY,
        koharu_canvas::Brush {
            diameter,
            color: [0, 0, 0, 255],
            mode: koharu_canvas::StrokeMode::Paint,
        },
        point.into(),
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn extend_inpaint(
    points: Vec<Point>,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop.lock().canvas().extend_mask_stroke(
        INPAINT_MASK,
        &points.into_iter().map(PagePoint::from).collect::<Vec<_>>(),
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn finish_inpaint(
    handle: AppHandle<Cef>,
    desktop: State<'_, Desktop>,
) -> Result<Option<JobId>, Error> {
    let Some(mask) = desktop.lock().canvas().finish_mask_stroke(INPAINT_MASK)? else {
        return Ok(None);
    };
    let page = mask.page;
    *handle.state::<Processing>().inpainting_mask.lock() = Some(koharu_pipeline::InpaintingMask {
        page,
        png: Arc::from(mask.encode_png()?),
    });
    desktop.lock().canvas().clear_mask(INPAINT_MASK);
    Ok(Some(
        processing::process(
            handle.clone(),
            koharu_pipeline::Scope::Region {
                page,
                bounds: koharu_pipeline::Bounds {
                    x: f64::from(mask.dirty.x),
                    y: f64::from(mask.dirty.y),
                    width: f64::from(mask.dirty.width),
                    height: f64::from(mask.dirty.height),
                },
            },
            koharu_pipeline::Operation::Only {
                stage: koharu_pipeline::Stage::Inpainting,
            },
            handle.state::<CurrentProject>(),
            handle.state::<Processing>(),
            handle.state::<JobChannel>(),
        )
        .await?,
    ))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cancel_inpaint(desktop: State<'_, Desktop>) -> Result<(), Error> {
    desktop.lock().canvas().cancel_mask_stroke(INPAINT_MASK)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn sample_color(
    point: Point,
    desktop: State<'_, Desktop>,
) -> Result<[u8; 4], Error> {
    let (complete, sample) = tokio::sync::oneshot::channel();
    desktop
        .lock()
        .canvas()
        .sample_color(PhysicalPoint::new(point.x, point.y), move |result| {
            let _ = complete.send(result);
        })?;
    Ok(sample.await.context("color sample was cancelled")??)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn set_viewport(
    desktop: State<'_, Desktop>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    dpr: f64,
    background: [u8; 3],
    project: State<'_, CurrentProject>,
    canvas_view: State<'_, CanvasView>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let viewport = crate::desktop::PhysicalRect::from_logical(x, y, width, height, dpr)
        .map_err(|error| anyhow::anyhow!(error))?;
    let size = if canvas_view.fitted.load(Ordering::Acquire) {
        let project = project.project.lock().await;
        project.as_ref().and_then(|project| {
            let page = project.active_page()?;
            let snapshot = project.snapshot();
            let page = snapshot.page(page).ok()?.page().ok()?;
            Some(koharu_canvas::PhysicalSize::new(
                page.width.ceil() as u32,
                page.height.ceil() as u32,
            ))
        })
    } else {
        None
    };
    let canvas = {
        let mut desktop = desktop.lock();
        desktop.set_viewport(viewport, background);
        if let Some(size) = size {
            let mut view = desktop.view().clone();
            view.camera = koharu_canvas::Camera::contain(desktop.viewport().size(), size);
            desktop.set_view(view);
        }
        desktop.canvas_state(canvas_view.fitted.load(Ordering::Acquire))
    };
    canvas_channel.channel.publish(canvas);
    Ok(())
}
