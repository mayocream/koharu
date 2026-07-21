use koharu_scene::Frame;
use vello::kurbo::Affine;

use crate::{Error, Result};

/// Viewport dimensions after applying the host's device-pixel ratio.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PhysicalSize {
    pub width: u32,
    pub height: u32,
}

impl PhysicalSize {
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

pub type PixelSize = PhysicalSize;

/// Pointer or overlay position in physical viewport pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicalPoint {
    pub x: f64,
    pub y: f64,
}

impl PhysicalPoint {
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Position in the page's persistent coordinate system.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PagePoint {
    pub x: f64,
    pub y: f64,
}

impl PagePoint {
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self
            .x
            .saturating_add(self.width)
            .max(other.x.saturating_add(other.width));
        let y1 = self
            .y
            .saturating_add(self.height)
            .max(other.y.saturating_add(other.height));
        Self::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// Converts between page coordinates and physical viewport pixels.
///
/// The mapping is `screen = page * zoom + translation`. Keeping that one
/// affine authoritative prevents rendering, hit testing, and gestures from
/// disagreeing about device-pixel ratio or zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    zoom: f64,
    translation: [f64; 2],
}

impl Default for Camera {
    fn default() -> Self {
        Self::actual_size()
    }
}

impl Camera {
    pub fn new(zoom: f64, translation: [f64; 2]) -> Result<Self> {
        if !zoom.is_finite() || zoom <= 0.0 || !translation.into_iter().all(f64::is_finite) {
            return Err(Error::Invalid(
                "camera values must be finite and zoom positive".into(),
            ));
        }
        Ok(Self { zoom, translation })
    }

    #[must_use]
    pub const fn actual_size() -> Self {
        Self {
            zoom: 1.0,
            translation: [0.0, 0.0],
        }
    }

    #[must_use]
    pub fn contain(viewport: PhysicalSize, page: koharu_scene::Size) -> Self {
        if viewport.is_empty() || page.width == 0 || page.height == 0 {
            return Self::actual_size();
        }
        let zoom = (f64::from(viewport.width) / f64::from(page.width))
            .min(f64::from(viewport.height) / f64::from(page.height))
            .max(f64::EPSILON);
        let translation = [
            (f64::from(viewport.width) - f64::from(page.width) * zoom) * 0.5,
            (f64::from(viewport.height) - f64::from(page.height) * zoom) * 0.5,
        ];
        Self { zoom, translation }
    }

    #[must_use]
    pub const fn zoom(self) -> f64 {
        self.zoom
    }

    #[must_use]
    pub const fn translation(self) -> [f64; 2] {
        self.translation
    }

    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        if dx.is_finite() && dy.is_finite() {
            self.translation[0] += dx;
            self.translation[1] += dy;
        }
    }

    pub fn zoom_around(&mut self, point: PhysicalPoint, zoom: f64) -> Result<()> {
        if !zoom.is_finite() || zoom <= 0.0 {
            return Err(Error::Invalid(
                "camera zoom must be finite and positive".into(),
            ));
        }
        let page = self.screen_to_page(point);
        self.zoom = zoom;
        self.translation = [point.x - page.x * zoom, point.y - page.y * zoom];
        Ok(())
    }

    #[must_use]
    pub fn page_to_screen(self, point: PagePoint) -> PhysicalPoint {
        PhysicalPoint::new(
            point.x * self.zoom + self.translation[0],
            point.y * self.zoom + self.translation[1],
        )
    }

    #[must_use]
    pub fn screen_to_page(self, point: PhysicalPoint) -> PagePoint {
        PagePoint::new(
            (point.x - self.translation[0]) / self.zoom,
            (point.y - self.translation[1]) / self.zoom,
        )
    }

    pub(crate) fn affine(self) -> Affine {
        Affine::new([
            self.zoom,
            0.0,
            0.0,
            self.zoom,
            self.translation[0],
            self.translation[1],
        ])
    }
}

pub(crate) fn frame_corners(frame: Frame) -> [PagePoint; 4] {
    let center = PagePoint::new(
        f64::from(frame.x + frame.width * 0.5),
        f64::from(frame.y + frame.height * 0.5),
    );
    let angle = f64::from(frame.angle_degrees).to_radians();
    let (sin, cos) = angle.sin_cos();
    let half_width = f64::from(frame.width) * 0.5;
    let half_height = f64::from(frame.height) * 0.5;
    [
        (-half_width, -half_height),
        (half_width, -half_height),
        (half_width, half_height),
        (-half_width, half_height),
    ]
    .map(|(x, y)| PagePoint::new(center.x + x * cos - y * sin, center.y + x * sin + y * cos))
}

pub(crate) fn frame_contains(frame: Frame, point: PagePoint) -> bool {
    let center_x = f64::from(frame.x + frame.width * 0.5);
    let center_y = f64::from(frame.y + frame.height * 0.5);
    let angle = -f64::from(frame.angle_degrees).to_radians();
    let (sin, cos) = angle.sin_cos();
    let x = point.x - center_x;
    let y = point.y - center_y;
    let local_x = x * cos - y * sin;
    let local_y = x * sin + y * cos;
    local_x.abs() <= f64::from(frame.width) * 0.5 && local_y.abs() <= f64::from(frame.height) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_round_trips_points() {
        let camera = Camera::new(2.75, [31.0, -18.0]).unwrap();
        let page = PagePoint::new(143.25, 88.5);
        let round_trip = camera.screen_to_page(camera.page_to_screen(page));
        assert!((round_trip.x - page.x).abs() < 1e-9);
        assert!((round_trip.y - page.y).abs() < 1e-9);
    }

    #[test]
    fn zoom_around_preserves_the_anchor() {
        let anchor = PhysicalPoint::new(320.0, 180.0);
        let mut camera = Camera::new(1.25, [17.0, 9.0]).unwrap();
        let before = camera.screen_to_page(anchor);
        camera.zoom_around(anchor, 4.0).unwrap();
        let after = camera.screen_to_page(anchor);
        assert!((before.x - after.x).abs() < 1e-9);
        assert!((before.y - after.y).abs() < 1e-9);
    }

    #[test]
    fn rotated_frame_hit_test_uses_local_coordinates() {
        let frame = Frame {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 20.0,
            angle_degrees: 90.0,
        };
        assert!(frame_contains(frame, PagePoint::new(60.0, 70.0)));
        assert!(!frame_contains(frame, PagePoint::new(105.0, 30.0)));
    }

    #[test]
    fn dirty_rect_union_handles_an_empty_side() {
        let dirty = PixelRect::new(12, 8, 20, 10);
        assert_eq!(PixelRect::default().union(dirty), dirty);
        assert_eq!(
            dirty.union(PixelRect::new(5, 10, 10, 20)),
            PixelRect::new(5, 8, 27, 22)
        );
    }
}
