use std::collections::{HashMap, HashSet};

use koharu_renderer::{Composition, Layer, LayerKind};
use koharu_scene::{Geometry, Point};
use vello::kurbo::{Affine, Point as KurboPoint};

use crate::{
    ElementFrame, ElementId, ElementPreview, Error, Frame, PageId, Result, TransformCommit,
};

/// Validated state for a transform preview. Only the selected layers are
/// materialized; all persistent visual data remains in the Composition.
pub(crate) struct ActiveTransform {
    page: PageId,
    originals: Vec<ElementPreview>,
    previews: Vec<ElementPreview>,
    index: HashMap<ElementId, usize>,
    supplied: HashMap<ElementId, Frame>,
    last_frame: Option<u64>,
    finished: bool,
}

impl ActiveTransform {
    pub fn new(composition: &Composition, controls: &[ElementFrame]) -> Result<Self> {
        let mut seen = HashSet::new();
        let originals = controls
            .iter()
            .map(|control| {
                if !seen.insert(control.element) {
                    return Err(Error::Invalid(format!(
                        "transform selection repeats element {}",
                        control.element
                    )));
                }
                let layer = composition.layer(control.element).ok_or_else(|| {
                    Error::Invalid(format!(
                        "transform element {} is not in the active composition",
                        control.element
                    ))
                })?;
                let presentation = layer.presentation();
                if !presentation.visible || presentation.opacity <= 0.0 {
                    return Err(Error::Invalid(format!(
                        "transform element {} is not visible",
                        control.element
                    )));
                }
                Ok(ElementPreview {
                    element: control.element,
                    frame: checked_frame(control.frame)?,
                    geometry: layer.geometry().clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Self::from_originals(composition.page(), originals)
    }

    fn from_originals(page: PageId, originals: Vec<ElementPreview>) -> Result<Self> {
        if originals.is_empty() {
            return Err(Error::Invalid(
                "an element transform requires a selection".into(),
            ));
        }
        let index = originals
            .iter()
            .enumerate()
            .map(|(index, preview)| (preview.element, index))
            .collect();
        Ok(Self {
            page,
            previews: originals.clone(),
            index,
            supplied: HashMap::with_capacity(originals.len()),
            originals,
            last_frame: None,
            finished: false,
        })
    }

    /// Replaces the preview with one complete, monotonically numbered UI frame.
    /// Stale and byte-equivalent frames do not invalidate native content.
    pub fn update(&mut self, frame: u64, elements: &[ElementFrame]) -> Result<bool> {
        if self.finished {
            return Err(Error::Invalid(
                "a finished transform cannot accept preview updates".into(),
            ));
        }
        if self.last_frame.is_some_and(|previous| frame <= previous) {
            return Ok(false);
        }
        if elements.len() != self.originals.len() {
            return Err(Error::Invalid(format!(
                "transform frame contains {} elements; expected {}",
                elements.len(),
                self.originals.len()
            )));
        }

        self.supplied.clear();
        for element in elements {
            let frame = checked_frame(element.frame)?;
            if self.supplied.insert(element.element, frame).is_some() {
                return Err(Error::Invalid(format!(
                    "transform frame repeats element {}",
                    element.element
                )));
            }
        }
        for original in &self.originals {
            if !self.supplied.contains_key(&original.element) {
                return Err(Error::Invalid(format!(
                    "transform frame is missing element {}",
                    original.element
                )));
            }
        }

        let mut changed = false;
        for (original, current) in self.originals.iter().zip(&mut self.previews) {
            let frame = self.supplied[&original.element];
            if current.frame == frame {
                continue;
            }
            *current = ElementPreview {
                element: original.element,
                frame,
                geometry: transformed_geometry(original, frame),
            };
            changed = true;
        }
        self.last_frame = Some(frame);
        Ok(changed)
    }

    pub fn affine(&self, element: ElementId) -> Option<Affine> {
        let index = *self.index.get(&element)?;
        Some(frame_transform(
            self.originals[index].frame,
            self.previews[index].frame,
        ))
    }

    pub fn finish(&mut self) -> Option<TransformCommit> {
        self.finished = true;
        let elements = self
            .previews
            .iter()
            .zip(&self.originals)
            .filter_map(|(preview, original)| {
                (preview.geometry != original.geometry).then(|| preview.clone())
            })
            .collect::<Vec<_>>();
        (!elements.is_empty()).then_some(TransformCommit {
            page: self.page,
            elements,
        })
    }
}

pub(crate) fn element_frame(layer: &Layer) -> Option<Frame> {
    if let LayerKind::Text(text) = layer.kind() {
        let bounds = text.rendered_bounds;
        let frame = Frame {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
            angle_degrees: text.angle_degrees,
        };
        return frame.is_valid().then_some(frame);
    }
    geometry_frame(layer.geometry())
}

fn transformed_geometry(original: &ElementPreview, preview: Frame) -> Geometry {
    let transform = frame_transform(original.frame, preview);
    Geometry {
        origin: original.geometry.origin.clone(),
        points: original
            .geometry
            .points
            .iter()
            .map(|point| {
                let point = transform * KurboPoint::new(point.x, point.y);
                Point {
                    x: point.x,
                    y: point.y,
                }
            })
            .collect(),
    }
}

pub(crate) fn geometry_frame(geometry: &Geometry) -> Option<Frame> {
    let first = geometry.points.first()?;
    if !first.x.is_finite() || !first.y.is_finite() {
        return None;
    }
    if let Some(frame) = rectangle_frame(&geometry.points) {
        return Some(frame);
    }
    let (mut min_x, mut min_y) = (first.x, first.y);
    let (mut max_x, mut max_y) = (first.x, first.y);
    for point in &geometry.points[1..] {
        if !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    let frame = Frame::new(
        min_x as f32,
        min_y as f32,
        (max_x - min_x) as f32,
        (max_y - min_y) as f32,
    );
    frame.is_valid().then_some(frame)
}

fn rectangle_frame(points: &[Point]) -> Option<Frame> {
    let [top_left, top_right, bottom_right, bottom_left] = points else {
        return None;
    };
    let top = (top_right.x - top_left.x, top_right.y - top_left.y);
    let right = (bottom_right.x - top_right.x, bottom_right.y - top_right.y);
    let bottom = (
        bottom_left.x - bottom_right.x,
        bottom_left.y - bottom_right.y,
    );
    let left = (top_left.x - bottom_left.x, top_left.y - bottom_left.y);
    let width = top.0.hypot(top.1);
    let height = right.0.hypot(right.1);
    if width <= f64::EPSILON || height <= f64::EPSILON {
        return None;
    }
    let scale = width.max(height).max(1.0);
    let length_tolerance = scale * 1e-6;
    let dot_tolerance = width * height * 1e-6;
    let diagonal_tolerance = scale * 1e-6;
    if (bottom.0.hypot(bottom.1) - width).abs() > length_tolerance
        || (left.0.hypot(left.1) - height).abs() > length_tolerance
        || (top.0 * right.0 + top.1 * right.1).abs() > dot_tolerance
        || ((top_left.x + bottom_right.x) - (top_right.x + bottom_left.x)).abs()
            > diagonal_tolerance
        || ((top_left.y + bottom_right.y) - (top_right.y + bottom_left.y)).abs()
            > diagonal_tolerance
    {
        return None;
    }
    let center_x = (top_left.x + top_right.x + bottom_right.x + bottom_left.x) * 0.25;
    let center_y = (top_left.y + top_right.y + bottom_right.y + bottom_left.y) * 0.25;
    let frame = Frame {
        x: (center_x - width * 0.5) as f32,
        y: (center_y - height * 0.5) as f32,
        width: width as f32,
        height: height as f32,
        angle_degrees: top.1.atan2(top.0).to_degrees() as f32,
    };
    frame.is_valid().then_some(frame)
}

pub(crate) fn frame_transform(original: Frame, preview: Frame) -> Affine {
    let original_angle = f64::from(original.angle_degrees).to_radians();
    let preview_angle = f64::from(preview.angle_degrees).to_radians();
    let (original_sin, original_cos) = original_angle.sin_cos();
    let (preview_sin, preview_cos) = preview_angle.sin_cos();
    let scale_x = f64::from(preview.width / original.width);
    let scale_y = f64::from(preview.height / original.height);
    let a = preview_cos * scale_x * original_cos + preview_sin * scale_y * original_sin;
    let b = preview_sin * scale_x * original_cos - preview_cos * scale_y * original_sin;
    let c = preview_cos * scale_x * original_sin - preview_sin * scale_y * original_cos;
    let d = preview_sin * scale_x * original_sin + preview_cos * scale_y * original_cos;
    let original_center_x = f64::from(original.x + original.width * 0.5);
    let original_center_y = f64::from(original.y + original.height * 0.5);
    let preview_center_x = f64::from(preview.x + preview.width * 0.5);
    let preview_center_y = f64::from(preview.y + preview.height * 0.5);
    Affine::new([
        a,
        b,
        c,
        d,
        preview_center_x - a * original_center_x - c * original_center_y,
        preview_center_y - b * original_center_x - d * original_center_y,
    ])
}

fn checked_frame(frame: Frame) -> Result<Frame> {
    frame
        .is_valid()
        .then_some(frame)
        .ok_or_else(|| Error::Invalid("transform frame must be finite and non-empty".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(element: ElementId, frame: Frame) -> ElementPreview {
        ElementPreview {
            element,
            frame,
            geometry: Geometry {
                origin: koharu_scene::Origin::User,
                points: crate::frame_corners(frame)
                    .into_iter()
                    .map(|point| Point {
                        x: point.x,
                        y: point.y,
                    })
                    .collect(),
            },
        }
    }

    fn ids() -> (PageId, [ElementId; 2]) {
        let session = koharu_scene::Session::memory().unwrap();
        let mut result = None;
        session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(
                    koharu_scene::PageDraft::new("transform test", 200.0, 200.0),
                    koharu_scene::At::End,
                )?;
                result = Some((
                    page,
                    [
                        edit.add_entity(page, koharu_scene::At::End)?,
                        edit.add_entity(page, koharu_scene::At::End)?,
                    ],
                ));
                Ok(())
            })
            .unwrap();
        result.unwrap()
    }

    #[test]
    fn frame_transform_maps_centers_and_dimensions() {
        let original = Frame::new(10.0, 20.0, 30.0, 40.0);
        let preview = Frame {
            x: 50.0,
            y: 60.0,
            width: 60.0,
            height: 20.0,
            angle_degrees: 90.0,
        };
        let mapped = frame_transform(original, preview) * KurboPoint::new(25.0, 40.0);
        assert!((mapped.x - 80.0).abs() < 1e-5);
        assert!((mapped.y - 70.0).abs() < 1e-5);
    }

    #[test]
    fn update_requires_complete_frames_and_ignores_stale_samples() {
        let (page, ids) = ids();
        let originals = [
            Frame::new(10.0, 20.0, 40.0, 30.0),
            Frame::new(80.0, 90.0, 20.0, 10.0),
        ];
        let mut transform = ActiveTransform::from_originals(
            page,
            ids.into_iter()
                .zip(originals)
                .map(|(id, frame)| preview(id, frame))
                .collect(),
        )
        .unwrap();
        let moved = [
            ElementFrame {
                element: ids[0],
                frame: Frame::new(15.0, 25.0, 40.0, 30.0),
            },
            ElementFrame {
                element: ids[1],
                frame: Frame::new(85.0, 95.0, 20.0, 10.0),
            },
        ];

        assert!(transform.update(2, &moved).unwrap());
        assert!(!transform.update(1, &moved).unwrap());
        assert!(transform.update(3, &moved[..1]).is_err());
        let commit = transform.finish().unwrap();
        assert_eq!(commit.elements.len(), 2);
        assert_eq!(commit.elements[0].frame, moved[0].frame);
        assert!(transform.update(4, &moved).is_err());
    }

    #[test]
    fn geometry_bounds_preserve_rotated_rectangles() {
        let expected = Frame {
            angle_degrees: -23.0,
            ..Frame::new(12.0, 34.0, 80.0, 45.0)
        };
        let geometry = preview(ids().1[0], expected).geometry;
        let actual = geometry_frame(&geometry).unwrap();
        assert!((actual.x - expected.x).abs() < 1e-4);
        assert!((actual.y - expected.y).abs() < 1e-4);
        assert!((actual.width - expected.width).abs() < 1e-4);
        assert!((actual.height - expected.height).abs() < 1e-4);
        assert!((actual.angle_degrees - expected.angle_degrees).abs() < 1e-4);
    }
}
