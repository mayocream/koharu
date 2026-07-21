use std::collections::HashSet;

use koharu_scene::{ElementId, Frame, Page, PageId};

use crate::{ElementPreview, Error, Handle, HitTarget, PagePoint, Result, TransformCommit};

/// Pure-Rust state for one pointer-driven move, resize, or rotation.
///
/// The committed page is never mutated. Updates replace `previews`, rendering
/// reads those frames, and `finish` returns the minimal changed frame set for
/// the application to commit atomically.
pub(crate) struct ActiveTransform {
    page: PageId,
    target: HitTarget,
    start: PagePoint,
    originals: Vec<ElementPreview>,
    previews: Vec<ElementPreview>,
    previous_rotation: Option<f64>,
    rotation_delta: f64,
}

impl ActiveTransform {
    pub fn new(
        page: &Page,
        selected: &[ElementId],
        target: HitTarget,
        start: PagePoint,
    ) -> Result<Self> {
        if !start.x.is_finite() || !start.y.is_finite() {
            return Err(Error::Invalid(
                "transform point must contain finite coordinates".into(),
            ));
        }
        let target_element = target.element();
        if !selected.contains(&target_element) {
            return Err(Error::Invalid(
                "transform target must be part of the selection".into(),
            ));
        }

        let mut seen = HashSet::new();
        let ids: Vec<_> = match target {
            HitTarget::Element(_) => selected
                .iter()
                .copied()
                .filter(|element| seen.insert(*element))
                .collect(),
            HitTarget::Handle { element, .. } => vec![element],
        };
        if ids.is_empty() {
            return Err(Error::Invalid(
                "an element transform requires a selection".into(),
            ));
        }

        let originals = ids
            .into_iter()
            .map(|element| {
                let value = page.element(element).ok_or_else(|| {
                    Error::Invalid(format!(
                        "transform element {element} is not on the active page"
                    ))
                })?;
                if !value.visible || value.opacity <= 0.0 {
                    return Err(Error::Invalid(format!(
                        "transform element {element} is not visible"
                    )));
                }
                Ok(ElementPreview {
                    element,
                    frame: checked_frame(value.frame)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let previous_rotation = matches!(
            target,
            HitTarget::Handle {
                handle: Handle::Rotate,
                ..
            }
        )
        .then(|| pointer_angle(originals[0].frame, start));

        Ok(Self {
            page: page.id,
            target,
            start,
            previews: originals.clone(),
            originals,
            previous_rotation,
            rotation_delta: 0.0,
        })
    }

    pub fn update(&mut self, point: PagePoint) -> Result<()> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(Error::Invalid(
                "transform point must contain finite coordinates".into(),
            ));
        }
        let dx = point.x - self.start.x;
        let dy = point.y - self.start.y;
        let mut previews = self.originals.clone();
        match self.target {
            HitTarget::Element(_) => {
                for preview in &mut previews {
                    preview.frame = move_frame(preview.frame, dx, dy)?;
                }
            }
            HitTarget::Handle {
                handle: Handle::Rotate,
                ..
            } => {
                let current = pointer_angle(self.originals[0].frame, point);
                if let Some(previous) = self.previous_rotation {
                    self.rotation_delta += normalize_radians(current - previous);
                }
                self.previous_rotation = Some(current);
                previews[0].frame = rotate_frame(self.originals[0].frame, self.rotation_delta)?;
            }
            HitTarget::Handle { handle, .. } => {
                previews[0].frame = resize_frame(self.originals[0].frame, handle, dx, dy)?;
            }
        }
        self.previews = previews;
        Ok(())
    }

    pub fn preview(&self, element: ElementId) -> Option<Frame> {
        self.previews
            .iter()
            .find(|preview| preview.element == element)
            .map(|preview| preview.frame)
    }

    pub fn finish(self) -> Option<TransformCommit> {
        let elements: Vec<_> = self
            .previews
            .into_iter()
            .zip(self.originals)
            .filter_map(|(preview, original)| (preview.frame != original.frame).then_some(preview))
            .collect();
        (!elements.is_empty()).then_some(TransformCommit {
            page: self.page,
            elements,
        })
    }
}

impl HitTarget {
    const fn element(self) -> ElementId {
        match self {
            Self::Element(element) | Self::Handle { element, .. } => element,
        }
    }
}

fn move_frame(frame: Frame, dx: f64, dy: f64) -> Result<Frame> {
    checked_frame(Frame {
        x: (f64::from(frame.x) + dx) as f32,
        y: (f64::from(frame.y) + dy) as f32,
        ..frame
    })
}

fn resize_frame(frame: Frame, handle: Handle, dx: f64, dy: f64) -> Result<Frame> {
    let angle = f64::from(frame.angle_degrees).to_radians();
    let (sin, cos) = angle.sin_cos();
    let local_dx = dx * cos + dy * sin;
    let local_dy = -dx * sin + dy * cos;
    let mut left = -f64::from(frame.width) * 0.5;
    let mut right = f64::from(frame.width) * 0.5;
    let mut top = -f64::from(frame.height) * 0.5;
    let mut bottom = f64::from(frame.height) * 0.5;

    match handle {
        Handle::NorthWest | Handle::West | Handle::SouthWest => left += local_dx,
        Handle::North | Handle::South | Handle::Rotate => {}
        Handle::NorthEast | Handle::East | Handle::SouthEast => right += local_dx,
    }
    match handle {
        Handle::NorthWest | Handle::North | Handle::NorthEast => top += local_dy,
        Handle::East | Handle::West | Handle::Rotate => {}
        Handle::SouthEast | Handle::South | Handle::SouthWest => bottom += local_dy,
    }

    if right - left < 1.0 {
        if matches!(handle, Handle::NorthWest | Handle::West | Handle::SouthWest) {
            left = right - 1.0;
        } else {
            right = left + 1.0;
        }
    }
    if bottom - top < 1.0 {
        if matches!(
            handle,
            Handle::NorthWest | Handle::North | Handle::NorthEast
        ) {
            top = bottom - 1.0;
        } else {
            bottom = top + 1.0;
        }
    }

    let local_center_x = (left + right) * 0.5;
    let local_center_y = (top + bottom) * 0.5;
    let center_offset_x = local_center_x * cos - local_center_y * sin;
    let center_offset_y = local_center_x * sin + local_center_y * cos;
    let width = right - left;
    let height = bottom - top;
    let center_x = f64::from(frame.x) + f64::from(frame.width) * 0.5 + center_offset_x;
    let center_y = f64::from(frame.y) + f64::from(frame.height) * 0.5 + center_offset_y;
    checked_frame(Frame {
        x: (center_x - width * 0.5) as f32,
        y: (center_y - height * 0.5) as f32,
        width: width as f32,
        height: height as f32,
        ..frame
    })
}

fn rotate_frame(frame: Frame, delta: f64) -> Result<Frame> {
    checked_frame(Frame {
        angle_degrees: (f64::from(frame.angle_degrees) + delta.to_degrees()) as f32,
        ..frame
    })
}

fn pointer_angle(frame: Frame, point: PagePoint) -> f64 {
    let center_x = f64::from(frame.x) + f64::from(frame.width) * 0.5;
    let center_y = f64::from(frame.y) + f64::from(frame.height) * 0.5;
    (point.y - center_y).atan2(point.x - center_x)
}

fn normalize_radians(angle: f64) -> f64 {
    (angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

fn checked_frame(frame: Frame) -> Result<Frame> {
    if frame.x.is_finite()
        && frame.y.is_finite()
        && frame.width.is_finite()
        && frame.width > 0.0
        && frame.height.is_finite()
        && frame.height > 0.0
        && frame.angle_degrees.is_finite()
    {
        Ok(frame)
    } else {
        Err(Error::Invalid(
            "element transform produced an invalid frame".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_scene::Session;

    fn page_with_frame(frame: Frame) -> (Page, ElementId) {
        let mut session = Session::memory().unwrap();
        let mut commands = session.commands();
        let page = commands.add_page("page", rgba_png(200, 200)).unwrap();
        let element = commands
            .add_image(page, frame, "node", rgba_png(10, 10))
            .unwrap();
        session.apply(commands).unwrap();
        (session.page(page).unwrap().clone(), element)
    }

    fn rgba_png(width: u32, height: u32) -> Vec<u8> {
        use std::io::Cursor;

        use image::{DynamicImage, ImageFormat, RgbaImage};

        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([255, 255, 255, 255]),
        ))
        .write_to(&mut output, ImageFormat::Png)
        .unwrap();
        output.into_inner()
    }

    #[test]
    fn moving_translates_every_selected_element() {
        let (mut page, first) = page_with_frame(Frame::new(10.0, 20.0, 40.0, 30.0));
        let second = ElementId::new();
        let mut copy = page.elements[0].clone();
        copy.id = second;
        copy.frame = Frame::new(80.0, 90.0, 20.0, 10.0);
        page.elements.push(copy);
        let mut transform = ActiveTransform::new(
            &page,
            &[first, second],
            HitTarget::Element(first),
            PagePoint::new(5.0, 7.0),
        )
        .unwrap();

        transform.update(PagePoint::new(17.0, 3.0)).unwrap();
        let commit = transform.finish().unwrap();

        assert_eq!(commit.elements[0].frame.x, 22.0);
        assert_eq!(commit.elements[0].frame.y, 16.0);
        assert_eq!(commit.elements[1].frame.x, 92.0);
        assert_eq!(commit.elements[1].frame.y, 86.0);
    }

    #[test]
    fn rotated_resize_preserves_the_opposite_edge() {
        let frame = Frame {
            angle_degrees: 90.0,
            ..Frame::new(0.0, 0.0, 100.0, 50.0)
        };
        let resized = resize_frame(frame, Handle::East, 0.0, 20.0).unwrap();

        assert!((resized.width - 120.0).abs() < 1e-5);
        assert!((resized.height - 50.0).abs() < 1e-5);
        assert!((resized.x + 10.0).abs() < 1e-5);
        assert!((resized.y - 10.0).abs() < 1e-5);
    }

    #[test]
    fn every_resize_handle_preserves_its_opposite_anchor() {
        fn midpoint(a: PagePoint, b: PagePoint) -> PagePoint {
            PagePoint::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
        }

        fn opposite_anchor(frame: Frame, handle: Handle) -> PagePoint {
            let [north_west, north_east, south_east, south_west] = crate::frame_corners(frame);
            match handle {
                Handle::NorthWest => south_east,
                Handle::North => midpoint(south_east, south_west),
                Handle::NorthEast => south_west,
                Handle::East => midpoint(south_west, north_west),
                Handle::SouthEast => north_west,
                Handle::South => midpoint(north_west, north_east),
                Handle::SouthWest => north_east,
                Handle::West => midpoint(north_east, south_east),
                Handle::Rotate => unreachable!(),
            }
        }

        let frame = Frame {
            angle_degrees: 37.0,
            ..Frame::new(20.0, 30.0, 120.0, 70.0)
        };
        for handle in [
            Handle::NorthWest,
            Handle::North,
            Handle::NorthEast,
            Handle::East,
            Handle::SouthEast,
            Handle::South,
            Handle::SouthWest,
            Handle::West,
        ] {
            let before = opposite_anchor(frame, handle);
            let resized = resize_frame(frame, handle, 19.0, -13.0).unwrap();
            let after = opposite_anchor(resized, handle);
            assert!((before.x - after.x).abs() < 1e-4, "{handle:?}");
            assert!((before.y - after.y).abs() < 1e-4, "{handle:?}");
        }
    }

    #[test]
    fn resizing_only_changes_the_handle_owner() {
        let (mut page, first) = page_with_frame(Frame::new(10.0, 20.0, 40.0, 30.0));
        let second = ElementId::new();
        let mut copy = page.elements[0].clone();
        copy.id = second;
        copy.frame = Frame::new(80.0, 90.0, 20.0, 10.0);
        page.elements.push(copy);
        let mut transform = ActiveTransform::new(
            &page,
            &[first, second],
            HitTarget::Handle {
                element: first,
                handle: Handle::East,
            },
            PagePoint::new(50.0, 35.0),
        )
        .unwrap();

        transform.update(PagePoint::new(60.0, 35.0)).unwrap();
        let commit = transform.finish().unwrap();

        assert_eq!(commit.elements.len(), 1);
        assert_eq!(commit.elements[0].element, first);
        assert_eq!(commit.elements[0].frame.width, 50.0);
    }

    #[test]
    fn rotation_uses_pointer_angle_around_the_frame_center() {
        let (page, element) = page_with_frame(Frame::new(0.0, 0.0, 100.0, 50.0));
        let mut transform = ActiveTransform::new(
            &page,
            &[element],
            HitTarget::Handle {
                element,
                handle: Handle::Rotate,
            },
            PagePoint::new(50.0, -25.0),
        )
        .unwrap();

        transform.update(PagePoint::new(100.0, 25.0)).unwrap();
        let frame = transform.finish().unwrap().elements[0].frame;

        assert!((frame.angle_degrees - 90.0).abs() < 1e-5);
        assert_eq!(
            (frame.x, frame.y, frame.width, frame.height),
            (0.0, 0.0, 100.0, 50.0)
        );
    }

    #[test]
    fn cancelling_by_dropping_does_not_produce_a_commit() {
        let (page, element) = page_with_frame(Frame::new(0.0, 0.0, 100.0, 50.0));
        let transform = ActiveTransform::new(
            &page,
            &[element],
            HitTarget::Element(element),
            PagePoint::new(0.0, 0.0),
        )
        .unwrap();

        assert!(transform.finish().is_none());
    }
}
