use std::collections::{HashMap, HashSet};

use koharu_scene::{Geometry, Point};
use vello::kurbo::{Affine, Point as KurboPoint};

use crate::{
    CanvasPage, ElementFrame, ElementId, ElementPreview, Error, Frame, PageId, Result,
    TransformCommit,
};

/// Validated Rust-side state for transform previews authored by React.
///
/// The committed page is never mutated. Every UI animation frame replaces the
/// complete preview set, rendering reads those frames, and `finish` returns the
/// minimal changed geometry set for one atomic scene commit.
pub(crate) struct ActiveTransform {
    page: PageId,
    originals: Vec<ElementPreview>,
    previews: Vec<ElementPreview>,
    supplied: HashMap<ElementId, Frame>,
    last_frame: Option<u64>,
}

impl ActiveTransform {
    pub fn new(page: &CanvasPage, controls: &[ElementFrame]) -> Result<Self> {
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
                let value = page.element(control.element).ok_or_else(|| {
                    Error::Invalid(format!(
                        "transform element {} is not on the active page",
                        control.element
                    ))
                })?;
                if !value.selectable() || !value.visible || value.opacity <= 0.0 {
                    return Err(Error::Invalid(format!(
                        "transform element {} is not selectable and visible",
                        control.element
                    )));
                }
                let frame = checked_frame(control.frame)?;
                Ok(ElementPreview {
                    element: control.element,
                    frame,
                    geometry: value.geometry.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if originals.is_empty() {
            return Err(Error::Invalid(
                "an element transform requires a selection".into(),
            ));
        }
        Ok(Self {
            page: page.id,
            previews: originals.clone(),
            supplied: HashMap::with_capacity(originals.len()),
            originals,
            last_frame: None,
        })
    }

    /// Replaces the preview with one complete, monotonically numbered UI frame.
    /// Returns `false` for a stale or byte-equivalent frame so callers can avoid
    /// unnecessary Vello scene composition.
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
            let next = ElementPreview {
                element: original.element,
                frame,
                geometry: transformed_geometry(original, frame),
            };
            changed = true;
            *current = next;
        }
        self.last_frame = Some(frame);
        Ok(changed)
    }

    pub fn affine(&self, element: ElementId) -> Option<Affine> {
        self.originals
            .iter()
            .zip(&self.previews)
            .find(|(original, _)| original.element == element)
            .map(|(original, preview)| frame_transform(original.frame, preview.frame))
    }

    pub fn finish(self) -> Option<TransformCommit> {
        let elements = self
            .previews
            .into_iter()
            .zip(self.originals)
            .filter_map(|(preview, original)| {
                (preview.geometry != original.geometry).then_some(preview)
            })
            .collect::<Vec<_>>();
        (!elements.is_empty()).then_some(TransformCommit {
            page: self.page,
            elements,
        })
    }
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
    use crate::{CanvasElement, PhysicalSize, model::PageAssets};

    fn page_with_frames(frames: &[Frame]) -> (CanvasPage, Vec<ElementId>) {
        let session = koharu_scene::Session::memory().unwrap();
        let mut ids = None;
        session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(
                    koharu_scene::PageDraft::new("transform test", 200.0, 200.0),
                    koharu_scene::At::End,
                )?;
                let elements = frames
                    .iter()
                    .map(|_| edit.add_entity(page, koharu_scene::At::End))
                    .collect::<koharu_scene::Result<Vec<_>>>()?;
                ids = Some((page, elements));
                Ok(())
            })
            .unwrap();
        let (page, ids) = ids.unwrap();
        let elements = ids
            .iter()
            .copied()
            .zip(frames.iter().copied())
            .map(|(id, frame)| CanvasElement {
                id,
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
                frame,
                visible: true,
                opacity: 1.0,
                local_opacity: 1.0,
                groups: Vec::new(),
                image: None,
                raster: None,
                has_text: true,
            })
            .collect();
        (
            CanvasPage {
                id: page,
                size: PhysicalSize::new(200, 200),
                assets: PageAssets::default(),
                members: ids.iter().copied().collect(),
                elements,
                group_opacities: Default::default(),
            },
            ids,
        )
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
    fn invalid_preview_frames_are_rejected() {
        assert!(checked_frame(Frame::new(0.0, 0.0, 0.0, 10.0)).is_err());
        assert!(checked_frame(Frame::new(f32::NAN, 0.0, 10.0, 10.0)).is_err());
    }

    #[test]
    fn update_requires_complete_frames_and_ignores_stale_samples() {
        let originals = [
            Frame::new(10.0, 20.0, 40.0, 30.0),
            Frame::new(80.0, 90.0, 20.0, 10.0),
        ];
        let (page, ids) = page_with_frames(&originals);
        let controls = ids
            .iter()
            .copied()
            .zip(originals)
            .map(|(element, frame)| ElementFrame { element, frame })
            .collect::<Vec<_>>();
        let mut transform = ActiveTransform::new(&page, &controls).unwrap();
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
        assert!(
            !transform
                .update(
                    1,
                    &[
                        ElementFrame {
                            element: ids[0],
                            frame: originals[0],
                        },
                        ElementFrame {
                            element: ids[1],
                            frame: originals[1],
                        },
                    ],
                )
                .unwrap()
        );
        let mapped = transform.affine(ids[0]).unwrap()
            * KurboPoint::new(
                originals[0].x as f64 + originals[0].width as f64 * 0.5,
                originals[0].y as f64 + originals[0].height as f64 * 0.5,
            );
        assert!((mapped.x - 35.0).abs() < 1e-5);
        assert!((mapped.y - 40.0).abs() < 1e-5);
        assert!(transform.update(3, &moved[..1]).is_err());

        let commit = transform.finish().unwrap();
        assert_eq!(commit.elements.len(), 2);
        assert_eq!(commit.elements[0].frame, moved[0].frame);
        assert_eq!(commit.elements[1].frame, moved[1].frame);
    }

    #[test]
    fn rendered_text_control_translates_the_resolved_text_geometry() {
        let source = Frame::new(10.0, 20.0, 80.0, 50.0);
        let (page, ids) = page_with_frames(&[source]);
        let control = Frame::new(30.0, 35.0, 40.0, 20.0);
        let mut transform = ActiveTransform::new(
            &page,
            &[ElementFrame {
                element: ids[0],
                frame: control,
            }],
        )
        .unwrap();
        transform
            .update(
                1,
                &[ElementFrame {
                    element: ids[0],
                    frame: Frame {
                        x: control.x + 12.0,
                        y: control.y - 7.0,
                        ..control
                    },
                }],
            )
            .unwrap();

        let commit = transform.finish().unwrap();
        let expected = crate::frame_corners(Frame::new(22.0, 13.0, 80.0, 50.0));
        for (actual, expected) in commit.elements[0].geometry.points.iter().zip(expected) {
            assert!((actual.x - expected.x).abs() < 1e-5);
            assert!((actual.y - expected.y).abs() < 1e-5);
        }
    }
}
