use std::collections::{HashMap, HashSet};

use koharu_renderer::{Frame as RendererFrame, Layer, LayerKind};
use koharu_scene::{Geometry, Point, Revision};
use vello::kurbo::{Affine, Point as KurboPoint};

use crate::{
    ElementFrame, ElementId, ElementPreview, Error, Frame, PageId, Result, TransformCommit,
};

struct TransformSeed {
    element: ElementId,
    frame: Frame,
    geometry: Geometry,
}

/// Complete, validated transient transform state. No document semantics are copied.
pub(crate) struct ActiveTransform {
    page: PageId,
    originals: Vec<ElementPreview>,
    previews: Vec<ElementPreview>,
    indices: HashMap<ElementId, usize>,
    supplied: HashMap<ElementId, Frame>,
    last_frame: Option<u64>,
}

pub(crate) enum TransformState {
    Active(ActiveTransform),
    Finishing(ActiveTransform),
    Waiting {
        edit: ActiveTransform,
        revision: Revision,
    },
}

impl TransformState {
    pub(crate) fn edit(&self) -> &ActiveTransform {
        match self {
            Self::Active(edit) | Self::Finishing(edit) | Self::Waiting { edit, .. } => edit,
        }
    }

    pub(crate) fn clears_for_frame(&self, revision: Revision) -> bool {
        match self {
            Self::Active(_) => true,
            Self::Finishing(_) => false,
            Self::Waiting {
                revision: replacement,
                ..
            } => *replacement <= revision,
        }
    }
}

impl ActiveTransform {
    pub fn new(frame: &RendererFrame, controls: &[ElementFrame]) -> Result<Self> {
        let mut seen = HashSet::with_capacity(controls.len());
        let seeds = controls
            .iter()
            .map(|control| {
                if !seen.insert(control.element) {
                    return Err(Error::Invalid(format!(
                        "transform selection repeats element {}",
                        control.element
                    )));
                }
                let layer = frame.layer(control.element).ok_or_else(|| {
                    Error::Invalid(format!(
                        "transform element {} is not in the active renderer frame",
                        control.element
                    ))
                })?;
                let presentation = layer.presentation();
                if control.element == frame.page()
                    || !presentation.visible
                    || presentation.opacity <= 0.0
                    || !matches!(layer.kind(), LayerKind::Text(_))
                {
                    return Err(Error::Invalid(format!(
                        "transform element {} is not selectable and visible",
                        control.element
                    )));
                }
                Ok(TransformSeed {
                    element: control.element,
                    frame: checked_frame(control.frame)?,
                    geometry: layer.geometry().clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Self::from_seeds(frame.page(), seeds)
    }

    fn from_seeds(page: PageId, seeds: Vec<TransformSeed>) -> Result<Self> {
        if seeds.is_empty() {
            return Err(Error::Invalid(
                "an element transform requires a selection".into(),
            ));
        }
        let originals = seeds
            .into_iter()
            .map(|seed| ElementPreview {
                element: seed.element,
                frame: seed.frame,
                geometry: seed.geometry,
            })
            .collect::<Vec<_>>();
        let indices = originals
            .iter()
            .enumerate()
            .map(|(index, preview)| (preview.element, index))
            .collect();
        Ok(Self {
            page,
            previews: originals.clone(),
            indices,
            supplied: HashMap::with_capacity(originals.len()),
            originals,
            last_frame: None,
        })
    }

    /// Replaces the preview with one complete, monotonically numbered UI frame.
    pub fn update(&mut self, frame: u64, elements: &[ElementFrame]) -> Result<bool> {
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
        let index = *self.indices.get(&element)?;
        Some(frame_transform(
            self.originals[index].frame,
            self.previews[index].frame,
        ))
    }

    pub fn is_changed(&self) -> bool {
        self.originals
            .iter()
            .zip(&self.previews)
            .any(|(original, preview)| original.geometry != preview.geometry)
    }

    pub fn finish(&self) -> Option<TransformCommit> {
        let elements = self
            .previews
            .iter()
            .zip(&self.originals)
            .filter_map(|(preview, original)| {
                (preview.geometry != original.geometry).then_some(preview.clone())
            })
            .collect::<Vec<_>>();
        if elements.is_empty() {
            return None;
        }
        Some(TransformCommit {
            page: self.page,
            elements,
        })
    }
}

pub(crate) fn element_frame(layer: &Layer) -> Option<Frame> {
    let LayerKind::Text(text) = layer.kind() else {
        return None;
    };
    let bounds = text.rendered_bounds;
    if bounds.width > 0.0 && bounds.height > 0.0 {
        return Some(Frame {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
            angle_degrees: text.angle_degrees,
        });
    }
    geometry_frame(layer.geometry())
}

fn geometry_frame(geometry: &Geometry) -> Option<Frame> {
    let points = &geometry.points;
    if points.is_empty()
        || points
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite())
    {
        return None;
    }
    if points.len() == 4 {
        let top = (points[1].x - points[0].x, points[1].y - points[0].y);
        let right = (points[2].x - points[1].x, points[2].y - points[1].y);
        let width = top.0.hypot(top.1);
        let height = right.0.hypot(right.1);
        if width > f64::EPSILON && height > f64::EPSILON {
            let center_x = points.iter().map(|point| point.x).sum::<f64>() * 0.25;
            let center_y = points.iter().map(|point| point.y).sum::<f64>() * 0.25;
            return Some(Frame {
                x: (center_x - width * 0.5) as f32,
                y: (center_y - height * 0.5) as f32,
                width: width as f32,
                height: height as f32,
                angle_degrees: top.1.atan2(top.0).to_degrees() as f32,
            });
        }
    }
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for point in points {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    let width = max_x - min_x;
    let height = max_y - min_y;
    (width > f64::EPSILON && height > f64::EPSILON).then_some(Frame {
        x: min_x as f32,
        y: min_y as f32,
        width: width as f32,
        height: height as f32,
        angle_degrees: 0.0,
    })
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
    use crate::geometry::frame_corners;
    use koharu_scene::Origin;

    fn page_id() -> PageId {
        koharu_scene::EntityId::new()
    }

    fn transform(frames: &[Frame]) -> ActiveTransform {
        ActiveTransform::from_seeds(
            page_id(),
            frames
                .iter()
                .map(|frame| TransformSeed {
                    element: koharu_scene::EntityId::new(),
                    frame: *frame,
                    geometry: Geometry {
                        origin: Origin::User,
                        points: frame_corners(*frame)
                            .into_iter()
                            .map(|point| Point {
                                x: point.x,
                                y: point.y,
                            })
                            .collect(),
                    },
                })
                .collect(),
        )
        .unwrap()
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
        let originals = [
            Frame::new(10.0, 20.0, 40.0, 30.0),
            Frame::new(80.0, 90.0, 20.0, 10.0),
        ];
        let mut transform = transform(&originals);
        let ids = transform
            .originals
            .iter()
            .map(|value| value.element)
            .collect::<Vec<_>>();
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
        assert_eq!(transform.finish().unwrap().elements.len(), 2);
    }

    #[test]
    fn finished_preview_waits_for_its_replacement_revision() {
        let edit = transform(&[Frame::new(10.0, 20.0, 40.0, 30.0)]);
        let finishing = TransformState::Finishing(edit);
        assert!(!finishing.clears_for_frame(Revision::new(8)));

        let TransformState::Finishing(edit) = finishing else {
            unreachable!()
        };
        let waiting = TransformState::Waiting {
            edit,
            revision: Revision::new(9),
        };
        assert!(!waiting.clears_for_frame(Revision::new(8)));
        assert!(waiting.clears_for_frame(Revision::new(9)));
    }

    #[test]
    fn geometry_bounds_preserve_rotated_rectangles() {
        let expected = Frame {
            x: 10.0,
            y: 20.0,
            width: 40.0,
            height: 30.0,
            angle_degrees: 25.0,
        };
        let geometry = Geometry {
            origin: Origin::User,
            points: frame_corners(expected)
                .into_iter()
                .map(|point| Point {
                    x: point.x,
                    y: point.y,
                })
                .collect(),
        };
        let actual = geometry_frame(&geometry).unwrap();
        assert!((actual.x - expected.x).abs() < 1e-4);
        assert!((actual.y - expected.y).abs() < 1e-4);
        assert!((actual.width - expected.width).abs() < 1e-4);
        assert!((actual.height - expected.height).abs() < 1e-4);
        assert!((actual.angle_degrees - expected.angle_degrees).abs() < 1e-4);
    }
}
