//! Bubble-aware layout bounds derived from explicit scene relations.

use koharu_scene::Geometry;

const MAX_CONTOUR_POINTS: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeometryFrame {
    pub bounds: LayoutBox,
    pub angle_degrees: f32,
}

pub(crate) fn contour(geometry: &Geometry, frame: GeometryFrame) -> Vec<(f32, f32)> {
    if geometry.points.len() > MAX_CONTOUR_POINTS {
        return Vec::new();
    }
    let bounds = frame.bounds;
    let center_x = bounds.x + bounds.width * 0.5;
    let center_y = bounds.y + bounds.height * 0.5;
    let (sin, cos) = (-frame.angle_degrees.to_radians()).sin_cos();
    geometry
        .points
        .iter()
        .map(|point| {
            let x = point.x as f32 - center_x;
            let y = point.y as f32 - center_y;
            (
                x * cos - y * sin + center_x - bounds.x,
                x * sin + y * cos + center_y - bounds.y,
            )
        })
        .collect()
}

pub(crate) fn geometry_bounds(geometry: &Geometry) -> Option<LayoutBox> {
    if geometry
        .points
        .iter()
        .any(|point| !point.x.is_finite() || !point.y.is_finite())
    {
        return None;
    }
    let first = geometry.points.first()?;
    let (mut min_x, mut min_y) = (first.x, first.y);
    let (mut max_x, mut max_y) = (first.x, first.y);
    for point in &geometry.points[1..] {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    let x = min_x as f32;
    let y = min_y as f32;
    let width = (max_x - min_x) as f32;
    let height = (max_y - min_y) as f32;
    if !x.is_finite()
        || !y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
    {
        return None;
    }
    Some(LayoutBox {
        x,
        y,
        width,
        height,
    })
}

pub(crate) fn geometry_frame(geometry: &Geometry) -> Option<GeometryFrame> {
    let [top_left, top_right, bottom_right, bottom_left] = geometry.points.as_slice() else {
        return geometry_bounds(geometry).map(|bounds| GeometryFrame {
            bounds,
            angle_degrees: 0.0,
        });
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
    if !width.is_finite() || !height.is_finite() || width <= f64::EPSILON || height <= f64::EPSILON
    {
        return None;
    }

    let scale = width.max(height).max(1.0);
    let length_tolerance = scale * 1e-6;
    let opposite_lengths_match = (bottom.0.hypot(bottom.1) - width).abs() <= length_tolerance
        && (left.0.hypot(left.1) - height).abs() <= length_tolerance;
    let perpendicular = (top.0 * right.0 + top.1 * right.1).abs() <= width * height * 1e-6;
    let diagonals_bisect = ((top_left.x + bottom_right.x) - (top_right.x + bottom_left.x)).abs()
        <= length_tolerance
        && ((top_left.y + bottom_right.y) - (top_right.y + bottom_left.y)).abs()
            <= length_tolerance;
    if !opposite_lengths_match || !perpendicular || !diagonals_bisect {
        return geometry_bounds(geometry).map(|bounds| GeometryFrame {
            bounds,
            angle_degrees: 0.0,
        });
    }

    let center_x = (top_left.x + top_right.x + bottom_right.x + bottom_left.x) * 0.25;
    let center_y = (top_left.y + top_right.y + bottom_right.y + bottom_left.y) * 0.25;
    Some(GeometryFrame {
        bounds: LayoutBox {
            x: (center_x - width * 0.5) as f32,
            y: (center_y - height * 0.5) as f32,
            width: width as f32,
            height: height as f32,
        },
        angle_degrees: top.1.atan2(top.0).to_degrees() as f32,
    })
}

#[cfg(test)]
mod tests {
    use koharu_scene::{Geometry, Origin, Point};

    use super::*;

    #[test]
    fn polygon_bounds_use_all_points() {
        let geometry = Geometry {
            origin: Origin::User,
            points: vec![
                Point { x: 20.0, y: 30.0 },
                Point { x: 80.0, y: 30.0 },
                Point { x: 70.0, y: 90.0 },
                Point { x: 20.0, y: 80.0 },
            ],
        };

        assert_eq!(
            geometry_bounds(&geometry),
            Some(LayoutBox {
                x: 20.0,
                y: 30.0,
                width: 60.0,
                height: 60.0,
            })
        );
    }

    #[test]
    fn rotated_rectangle_preserves_layout_dimensions_and_angle() {
        let (sin, cos) = 27.0_f64.to_radians().sin_cos();
        let geometry = Geometry {
            origin: Origin::User,
            points: [(-40.0, -15.0), (40.0, -15.0), (40.0, 15.0), (-40.0, 15.0)]
                .map(|(x, y)| Point {
                    x: 100.0 + x * cos - y * sin,
                    y: 80.0 + x * sin + y * cos,
                })
                .into(),
        };

        let frame = geometry_frame(&geometry).unwrap();
        assert!((frame.bounds.x - 60.0).abs() < 1e-4);
        assert!((frame.bounds.y - 65.0).abs() < 1e-4);
        assert!((frame.bounds.width - 80.0).abs() < 1e-4);
        assert!((frame.bounds.height - 30.0).abs() < 1e-4);
        assert!((frame.angle_degrees - 27.0).abs() < 1e-4);
    }
}
