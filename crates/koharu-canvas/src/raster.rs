use vello::{
    Scene,
    kurbo::{Affine, BezPath, Circle, Stroke},
    peniko::{Color, Fill},
};

use crate::{PagePoint, RasterStrokeCommit};

/// Retains accepted stroke geometry so pointer updates append only their new
/// segment instead of rebuilding the complete preview path.
pub(crate) struct RasterStrokeEdit {
    pub(crate) commit: RasterStrokeCommit,
    pub(crate) preview: Scene,
}

impl RasterStrokeEdit {
    pub(crate) fn new(commit: RasterStrokeCommit) -> Self {
        let mut preview = Scene::new();
        draw_dot(
            &mut preview,
            commit.points[0],
            commit.diameter,
            commit.color,
        );
        Self { commit, preview }
    }

    pub(crate) fn push_point(&mut self, point: PagePoint) {
        let previous = *self
            .commit
            .points
            .last()
            .expect("a raster edit always has its initial point");
        draw_segment(
            &mut self.preview,
            previous,
            point,
            self.commit.diameter,
            self.commit.color,
        );
        self.commit.points.push(point);
    }
}

pub(crate) enum RasterStrokeState {
    Active(RasterStrokeEdit),
    Finishing(RasterStrokeEdit),
    Waiting {
        edit: RasterStrokeEdit,
        revision: koharu_scene::Revision,
    },
}

impl RasterStrokeState {
    pub(crate) fn edit(&self) -> &RasterStrokeEdit {
        match self {
            Self::Active(edit) | Self::Finishing(edit) | Self::Waiting { edit, .. } => edit,
        }
    }
}

fn draw_dot(scene: &mut Scene, point: PagePoint, diameter: f32, color: [u8; 4]) {
    // Geometry stays opaque: paint alpha is applied when the preview is
    // composed, while black erase geometry is consumed as a luminance mask.
    let color = Color::from_rgba8(color[0], color[1], color[2], u8::MAX);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color,
        None,
        &Circle::new((point.x, point.y), f64::from(diameter) * 0.5),
    );
}

fn draw_segment(scene: &mut Scene, from: PagePoint, to: PagePoint, diameter: f32, color: [u8; 4]) {
    let color = Color::from_rgba8(color[0], color[1], color[2], u8::MAX);
    let mut path = BezPath::new();
    path.move_to((from.x, from.y));
    path.line_to((to.x, to.y));
    scene.stroke(
        &Stroke::new(f64::from(diameter)),
        Affine::IDENTITY,
        color,
        None,
        &path,
    );
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color,
        None,
        &Circle::new((to.x, to.y), f64::from(diameter) * 0.5),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StrokeMode;
    use koharu_scene::EntityId;

    fn commit(mode: StrokeMode) -> RasterStrokeCommit {
        RasterStrokeCommit {
            page: EntityId::new(),
            layer: None,
            mode,
            color: [20, 40, 60, 128],
            diameter: 12.0,
            points: vec![PagePoint::new(1.0, 2.0)],
        }
    }

    #[test]
    fn retained_preview_records_only_accepted_points() {
        let mut edit = RasterStrokeEdit::new(commit(StrokeMode::Paint));
        edit.push_point(PagePoint::new(8.0, 9.0));
        edit.push_point(PagePoint::new(12.0, 15.0));
        assert_eq!(edit.commit.points.len(), 3);
    }

    #[test]
    fn erase_retains_preview_geometry_for_luminance_mask() {
        let mut edit = RasterStrokeEdit::new(commit(StrokeMode::Erase));
        edit.push_point(PagePoint::new(8.0, 9.0));
        assert_eq!(edit.commit.points.len(), 2);
    }
}
