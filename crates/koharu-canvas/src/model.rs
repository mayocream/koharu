use std::collections::HashSet;

use koharu_scene::{
    Asset, BlobId, EntityId, Geometry, Region, SceneSnapshot, SourceText, Visibility,
};

use crate::{Error, Frame, PhysicalSize, Result};

const MAX_SURFACE_DIMENSION: u32 = 32_768;
const MAX_SURFACE_PIXELS: u64 = 268_435_456;

#[derive(Clone, Debug, Default)]
pub(crate) struct PageAssets {
    pub source: Option<BlobId>,
    pub clean: Option<BlobId>,
    pub rendered: Option<BlobId>,
    pub text_mask: Option<BlobId>,
    pub brush_mask: Option<BlobId>,
}

impl PageAssets {
    pub(crate) const fn mask(&self, plane: crate::MaskPlane) -> Option<BlobId> {
        match plane {
            crate::MaskPlane::Text => self.text_mask,
            crate::MaskPlane::Brush => self.brush_mask,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CanvasElement {
    pub id: EntityId,
    pub geometry: Geometry,
    pub frame: Frame,
    pub visible: bool,
    pub opacity: f32,
    pub image: Option<BlobId>,
    pub has_text: bool,
}

impl CanvasElement {
    pub(crate) const fn selectable(&self) -> bool {
        self.has_text || self.image.is_some()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CanvasPage {
    pub id: EntityId,
    pub size: PhysicalSize,
    pub assets: PageAssets,
    pub members: HashSet<EntityId>,
    pub elements: Vec<CanvasElement>,
}

impl CanvasPage {
    pub(crate) fn load(snapshot: &SceneSnapshot, id: EntityId) -> Result<Self> {
        let page = snapshot.page(id)?.page()?;
        let size = PhysicalSize::new(page.width.ceil() as u32, page.height.ceil() as u32);
        if size.is_empty()
            || size.width > MAX_SURFACE_DIMENSION
            || size.height > MAX_SURFACE_DIMENSION
            || u64::from(size.width) * u64::from(size.height) > MAX_SURFACE_PIXELS
        {
            return Err(Error::Invalid(format!(
                "page {id} surface {}x{} exceeds canvas limits",
                size.width, size.height
            )));
        }

        let assets = PageAssets {
            source: asset_blob(snapshot, id, "source")?,
            clean: asset_blob(snapshot, id, "clean")?,
            rendered: asset_blob(snapshot, id, "rendered")?,
            text_mask: asset_blob(snapshot, id, "text-mask")?,
            brush_mask: asset_blob(snapshot, id, "brush-mask")?,
        };
        if assets.source.is_none() {
            return Err(Error::Invalid(format!("page {id} has no source asset")));
        }

        let mut members = HashSet::new();
        let mut elements = Vec::new();
        for entity in snapshot.subtree(id)?.skip(1) {
            let entity = entity.id();
            members.insert(entity);
            let Some(geometry) = snapshot.component::<Geometry>(entity, "default")? else {
                continue;
            };
            let Some(frame) = geometry_frame(&geometry) else {
                continue;
            };
            let visibility = snapshot
                .component::<Visibility>(entity, "default")?
                .unwrap_or(Visibility {
                    origin: koharu_scene::Origin::User,
                    visible: true,
                    opacity: 1.0,
                });
            elements.push(CanvasElement {
                id: entity,
                geometry,
                frame,
                visible: visibility.visible,
                opacity: visibility.opacity,
                image: asset_blob(snapshot, entity, "source")?,
                has_text: is_text_block(snapshot, entity)?,
            });
        }

        Ok(Self {
            id,
            size,
            assets,
            members,
            elements,
        })
    }

    pub(crate) fn element(&self, id: EntityId) -> Option<&CanvasElement> {
        self.elements.iter().find(|element| element.id == id)
    }

    pub(crate) fn contains(&self, id: EntityId) -> bool {
        id == self.id || self.members.contains(&id)
    }
}

fn is_text_block(snapshot: &SceneSnapshot, entity: EntityId) -> Result<bool> {
    if snapshot
        .component::<SourceText>(entity, "default")?
        .is_some()
    {
        return Ok(true);
    }
    Ok(snapshot
        .component::<Region>(entity, "default")?
        .is_some_and(|region| region.kind.as_str() == "dev.koharu.region.text"))
}

fn asset_blob(snapshot: &SceneSnapshot, entity: EntityId, slot: &str) -> Result<Option<BlobId>> {
    Ok(snapshot
        .component::<Asset>(entity, slot)?
        .map(|asset| asset.blob))
}

fn geometry_frame(geometry: &Geometry) -> Option<Frame> {
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

fn rectangle_frame(points: &[koharu_scene::Point]) -> Option<Frame> {
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

    // Preserve rotation only when the polygon is actually a rectangle. Other
    // polygons retain their axis-aligned editing bounds.
    let scale = width.max(height).max(1.0);
    let length_tolerance = scale * 1e-6;
    let dot_tolerance = width * height * 1e-6;
    let diagonal_tolerance = scale * 1e-6;
    let opposite_lengths_match = (bottom.0.hypot(bottom.1) - width).abs() <= length_tolerance
        && (left.0.hypot(left.1) - height).abs() <= length_tolerance;
    let perpendicular = (top.0 * right.0 + top.1 * right.1).abs() <= dot_tolerance;
    let diagonals_bisect = ((top_left.x + bottom_right.x) - (top_right.x + bottom_left.x)).abs()
        <= diagonal_tolerance
        && ((top_left.y + bottom_right.y) - (top_right.y + bottom_left.y)).abs()
            <= diagonal_tolerance;
    if !opposite_lengths_match || !perpendicular || !diagonals_bisect {
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

#[cfg(test)]
mod tests {
    use koharu_scene::{At, Geometry, Origin, PageDraft, Point, Region, RegionKind, SceneSession};

    use super::*;

    #[test]
    fn rotated_rectangle_preserves_its_editor_frame() {
        let expected = Frame {
            angle_degrees: -23.0,
            ..Frame::new(12.0, 34.0, 80.0, 45.0)
        };
        let center_x = f64::from(expected.x + expected.width * 0.5);
        let center_y = f64::from(expected.y + expected.height * 0.5);
        let angle = f64::from(expected.angle_degrees).to_radians();
        let (sin, cos) = angle.sin_cos();
        let geometry = Geometry {
            origin: Origin::User,
            points: [(-40.0, -22.5), (40.0, -22.5), (40.0, 22.5), (-40.0, 22.5)]
                .map(|(x, y)| Point {
                    x: center_x + x * cos - y * sin,
                    y: center_y + x * sin + y * cos,
                })
                .into(),
        };

        let actual = geometry_frame(&geometry).unwrap();
        assert!((actual.x - expected.x).abs() < 1e-4);
        assert!((actual.y - expected.y).abs() < 1e-4);
        assert!((actual.width - expected.width).abs() < 1e-4);
        assert!((actual.height - expected.height).abs() < 1e-4);
        assert!((actual.angle_degrees - expected.angle_degrees).abs() < 1e-4);
    }

    #[test]
    fn arbitrary_polygon_uses_axis_aligned_bounds() {
        let geometry = Geometry {
            origin: Origin::User,
            points: vec![
                Point { x: 2.0, y: 3.0 },
                Point { x: 12.0, y: 4.0 },
                Point { x: 9.0, y: 11.0 },
            ],
        };

        assert_eq!(
            geometry_frame(&geometry),
            Some(Frame::new(2.0, 3.0, 10.0, 8.0))
        );
    }

    #[test]
    fn detected_text_regions_are_canvas_text_blocks_before_ocr() {
        let mut session = SceneSession::memory().unwrap();
        let mut entities = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(PageDraft::new("page", 100.0, 100.0), At::End)?;
                let text = edit.add_entity(page, At::End)?;
                edit.set(
                    text,
                    "default",
                    &Region {
                        origin: Origin::User,
                        kind: RegionKind::new("dev.koharu.region.text")?,
                        label: Some("text".into()),
                    },
                )?;
                let bubble = edit.add_entity(page, At::End)?;
                edit.set(
                    bubble,
                    "default",
                    &Region {
                        origin: Origin::User,
                        kind: RegionKind::new("dev.koharu.region.bubble")?,
                        label: Some("bubble".into()),
                    },
                )?;
                entities = Some((text, bubble));
                Ok(())
            })
            .unwrap();
        let snapshot = session.commit(patch).unwrap().snapshot;
        let (text, bubble) = entities.unwrap();

        assert!(is_text_block(&snapshot, text).unwrap());
        assert!(!is_text_block(&snapshot, bubble).unwrap());

        let frame = Frame::new(0.0, 0.0, 10.0, 10.0);
        let analysis_region = CanvasElement {
            id: bubble,
            geometry: Geometry::rectangle(0.0, 0.0, 10.0, 10.0),
            frame,
            visible: true,
            opacity: 1.0,
            image: None,
            has_text: false,
        };
        let text_block = CanvasElement {
            id: text,
            has_text: true,
            ..analysis_region.clone()
        };
        assert!(!analysis_region.selectable());
        assert!(text_block.selectable());
    }
}
