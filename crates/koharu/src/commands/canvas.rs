use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context as _;
use koharu_canvas::{
    DisplayState, MaskOverlay as CanvasMask, MaskPlane, PagePoint, PageView as CanvasPageImage,
    PhysicalPoint,
};
use koharu_scene::{AssetMetadata, AssetRole, EntityId, Revision};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager as _, State, ipc::Channel};

use super::{
    ChannelExt as _, Error, processing,
    processing::{JobChannel, JobId, Processing},
    project::CurrentProject,
};
use crate::desktop::Desktop;

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

#[derive(Clone, Copy, Debug, Deserialize, Type)]
pub struct MaskBrush {
    pub diameter: f32,
    pub erase: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type)]
pub struct TransformFrame {
    pub element: EntityId,
    pub frame: Frame,
}

#[derive(Clone, Debug, Deserialize, Type)]
pub struct CanvasPresentation {
    pub image: PageImage,
    pub show_text: bool,
    pub text_mask: Option<MaskTint>,
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
pub struct MaskTint {
    pub color: [u8; 4],
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PageImage {
    Source,
    Rendered,
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
        let mut view = desktop.view().clone();
        let center = PhysicalPoint::new(
            f64::from(desktop.viewport().size().width) * 0.5,
            f64::from(desktop.viewport().size().height) * 0.5,
        );
        view.camera.zoom_around(center, f64::from(zoom))?;
        desktop.set_view(view);
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
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    if !(0.02..=16.0).contains(&zoom) {
        return Err(anyhow::anyhow!("camera zoom must be between 2% and 1600%").into());
    }
    let canvas = {
        let mut desktop = desktop.lock();
        let mut view = desktop.view().clone();
        view.camera = koharu_canvas::Camera::new(zoom, translation)?;
        desktop.set_view(view);
        canvas_view.fitted.store(false, Ordering::Release);
        desktop.canvas_state(false)
    };
    canvas_channel.channel.publish(canvas);
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
        let project = project.project.lock();
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
        let mut view = desktop.view().clone();
        view.camera = koharu_canvas::Camera::contain(desktop.viewport().size(), size);
        desktop.set_view(view);
        canvas_view.fitted.store(true, Ordering::Release);
        desktop.canvas_state(true)
    };
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_presentation(
    desktop: State<'_, Desktop>,
    presentation: CanvasPresentation,
) -> Result<(), Error> {
    let mut desktop = desktop.lock();
    let mut view = desktop.view().clone();
    view.display = DisplayState {
        page: match presentation.image {
            PageImage::Source => CanvasPageImage::Editable,
            PageImage::Rendered => CanvasPageImage::Rendered,
        },
        show_text: presentation.show_text,
        text_mask: presentation
            .text_mask
            .map(|mask| CanvasMask::new(mask.color, mask.opacity)),
        transition: view.display.transition,
    };
    desktop.set_view(view);
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
        let mut project = project.project.lock();
        let project = project.as_mut().context("no project is open")?;
        let page = project
            .active_page()
            .context("the project has no active page")?;
        let (commit, layer) = project.add_point_text(page, point)?;
        project.record_commit(&commit);
        (commit, project.active_page(), layer)
    };
    desktop
        .lock()
        .synchronize(&commit.snapshot, page, &commit)?;
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
        let mut project = project.project.lock();
        let project = project.as_mut().context("no project is open")?;
        let page = project
            .active_page()
            .context("the project has no active page")?;
        let (commit, layer) = project.add_text_box(page, frame)?;
        project.record_commit(&commit);
        (commit, project.active_page(), layer)
    };
    desktop
        .lock()
        .synchronize(&commit.snapshot, page, &commit)?;
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
    let (commit, page, element) = {
        let mut project = project.project.lock();
        let project = project.as_mut().context("no project is open")?;
        let (commit, element) = project.apply_raster_stroke(
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
        )?;
        project.record_commit(&commit);
        (commit, project.active_page(), element)
    };
    desktop
        .lock()
        .synchronize(&commit.snapshot, page, &commit)?;
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
    let (commit, page, element) = {
        let mut project = project.project.lock();
        let project = project.as_mut().context("no project is open")?;
        let (commit, element) = project.apply_raster_stroke(
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
        )?;
        project.record_commit(&commit);
        (commit, project.active_page(), element)
    };
    desktop
        .lock()
        .synchronize(&commit.snapshot, page, &commit)?;
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
    let (commit, page) = {
        let mut project = project.project.lock();
        let project = project.as_mut().context("no project is open")?;
        let commit = project.set_geometries(
            transform
                .elements
                .into_iter()
                .map(|element| (element.element, element.geometry)),
        )?;
        project.record_commit(&commit);
        (commit, project.active_page())
    };
    let canvas = {
        let mut desktop = desktop.lock();
        desktop.synchronize(&commit.snapshot, page, &commit)?;
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
pub(crate) async fn begin_text_mask(
    point: Point,
    brush: MaskBrush,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop.lock().canvas().begin_mask_stroke(
        MaskPlane::Text,
        koharu_canvas::Brush {
            diameter: brush.diameter,
            color: [0, 0, 0, 255],
            mode: if brush.erase {
                koharu_canvas::StrokeMode::Erase
            } else {
                koharu_canvas::StrokeMode::Paint
            },
        },
        point.into(),
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn extend_text_mask(
    points: Vec<Point>,
    desktop: State<'_, Desktop>,
) -> Result<(), Error> {
    desktop.lock().canvas().extend_mask_stroke(
        MaskPlane::Text,
        &points.into_iter().map(PagePoint::from).collect::<Vec<_>>(),
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn finish_text_mask(
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
) -> Result<Option<Revision>, Error> {
    let Some(mask) = desktop
        .lock()
        .canvas()
        .finish_mask_stroke(MaskPlane::Text)?
    else {
        return Ok(None);
    };
    let current = {
        let project = project.project.lock();
        project
            .as_ref()
            .context("no project is open")?
            .snapshot()
            .asset(mask.page, &AssetRole::new(mask.plane.asset_role())?)?
            .map(|asset| asset.blob)
    };
    if current != mask.base {
        return Err(anyhow::anyhow!("the text mask changed while it was being edited").into());
    }
    let size = mask.size();
    let encoded = mask.encode_png()?;
    let (commit, page, blob) = {
        let mut project = project.project.lock();
        let project = project.as_mut().context("no project is open")?;
        let commit = project.set_asset(
            mask.page,
            mask.plane.asset_role(),
            encoded,
            "image/png",
            AssetMetadata {
                width: Some(size.width),
                height: Some(size.height),
                attributes: Default::default(),
            },
        )?;
        project.record_commit(&commit);
        let blob = commit
            .snapshot
            .asset(mask.page, &AssetRole::new(mask.plane.asset_role())?)?
            .context("the committed text mask asset is missing")?
            .blob;
        (commit, project.active_page(), blob)
    };
    {
        let mut desktop = desktop.lock();
        desktop
            .canvas()
            .acknowledge_mask_commit(mask.page, mask.plane, mask.generation, blob)?;
        desktop.synchronize(&commit.snapshot, page, &commit)?;
    }
    Ok(Some(commit.revision))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cancel_text_mask(desktop: State<'_, Desktop>) -> Result<(), Error> {
    desktop
        .lock()
        .canvas()
        .cancel_mask_stroke(MaskPlane::Text)?;
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
        MaskPlane::Inpaint,
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
        MaskPlane::Inpaint,
        &points.into_iter().map(PagePoint::from).collect::<Vec<_>>(),
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn finish_inpaint(
    handle: AppHandle,
    desktop: State<'_, Desktop>,
) -> Result<Option<JobId>, Error> {
    let Some(mask) = desktop
        .lock()
        .canvas()
        .finish_mask_stroke(MaskPlane::Inpaint)?
    else {
        return Ok(None);
    };
    let page = mask.page;
    *handle.state::<Processing>().inpainting_mask.lock() = Some(koharu_pipeline::InpaintingMask {
        page,
        png: Arc::from(mask.encode_png()?),
    });
    desktop.lock().canvas().clear_inpaint_mask();
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
    desktop
        .lock()
        .canvas()
        .cancel_mask_stroke(MaskPlane::Inpaint)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn sample_color(
    point: Point,
    desktop: State<'_, Desktop>,
) -> Result<[u8; 4], Error> {
    Ok(desktop
        .lock()
        .canvas()
        .sample_color(PhysicalPoint::new(point.x, point.y))?)
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
        let project = project.project.lock();
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
