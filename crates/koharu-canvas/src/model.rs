use std::collections::{HashMap, HashSet};

use koharu_scene::{
    AssetRole, BlobId, EntityId, Geometry, Group, RasterLayer, RasterLayerKind, Snapshot,
    TextLayout, Visibility,
};

use crate::{Error, Frame, PhysicalSize, Result};

const MAX_SURFACE_DIMENSION: u32 = 32_768;
const MAX_SURFACE_PIXELS: u64 = 268_435_456;

#[derive(Clone, Debug, Default)]
pub(crate) struct PageAssets {
    pub source: Option<BlobId>,
    pub rendered: Option<BlobId>,
    pub text_mask: Option<BlobId>,
}

impl PageAssets {
    pub(crate) const fn mask(&self, plane: crate::MaskPlane) -> Option<BlobId> {
        match plane {
            crate::MaskPlane::Text => self.text_mask,
            crate::MaskPlane::Inpaint => None,
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
    pub local_opacity: f32,
    pub groups: Vec<EntityId>,
    pub image: Option<BlobId>,
    pub has_text: bool,
    pub raster: Option<RasterLayerKind>,
}

impl CanvasElement {
    pub(crate) const fn selectable(&self) -> bool {
        self.raster.is_none() && (self.has_text || self.image.is_some())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CanvasPage {
    pub id: EntityId,
    pub size: PhysicalSize,
    pub assets: PageAssets,
    pub members: HashSet<EntityId>,
    pub elements: Vec<CanvasElement>,
    pub group_opacities: HashMap<EntityId, f32>,
}

impl CanvasPage {
    pub(crate) fn load(snapshot: &Snapshot, id: EntityId) -> Result<Self> {
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
            source: asset_blob(snapshot, id, &AssetRole::new("source")?)?,
            rendered: asset_blob(snapshot, id, &AssetRole::new("rendered")?)?,
            text_mask: asset_blob(snapshot, id, &AssetRole::new("text-mask")?)?,
        };
        if assets.source.is_none() {
            return Err(Error::Invalid(format!("page {id} has no source asset")));
        }

        let members = snapshot
            .subtree(id)?
            .skip(1)
            .map(|entity| entity.id())
            .collect();
        let mut collector = ElementCollector {
            snapshot,
            size,
            group_opacities: HashMap::new(),
            elements: Vec::new(),
        };
        collector.collect(id, true, 1.0, &mut Vec::new())?;

        Ok(Self {
            id,
            size,
            assets,
            members,
            elements: collector.elements,
            group_opacities: collector.group_opacities,
        })
    }

    pub(crate) fn element(&self, id: EntityId) -> Option<&CanvasElement> {
        self.elements.iter().find(|element| element.id == id)
    }

    pub(crate) fn contains(&self, id: EntityId) -> bool {
        id == self.id || self.members.contains(&id)
    }

    pub(crate) fn sync_presentation(&mut self, snapshot: &Snapshot) -> Result<()> {
        let groups = self
            .group_opacities
            .keys()
            .map(|id| Ok((*id, visibility(snapshot, *id)?)))
            .collect::<Result<HashMap<_, _>>>()?;
        for (id, opacity) in &mut self.group_opacities {
            *opacity = groups[id].opacity;
        }
        for element in &mut self.elements {
            let local = visibility(snapshot, element.id)?;
            element.local_opacity = local.opacity;
            element.visible = local.visible
                && element
                    .groups
                    .iter()
                    .all(|group| groups.get(group).is_none_or(|value| value.visible));
            element.opacity = local.opacity
                * element
                    .groups
                    .iter()
                    .map(|group| groups.get(group).map_or(1.0, |value| value.opacity))
                    .product::<f32>();
        }
        Ok(())
    }
}

struct ElementCollector<'a> {
    snapshot: &'a Snapshot,
    size: PhysicalSize,
    group_opacities: HashMap<EntityId, f32>,
    elements: Vec<CanvasElement>,
}

impl ElementCollector<'_> {
    fn collect(
        &mut self,
        parent: EntityId,
        inherited_visible: bool,
        inherited_opacity: f32,
        groups: &mut Vec<EntityId>,
    ) -> Result<()> {
        for entity in self.snapshot.children(parent)? {
            let visibility = visibility(self.snapshot, entity)?;
            let visible = inherited_visible && visibility.visible;
            let opacity = inherited_opacity * visibility.opacity;
            if self.snapshot.component::<Group>(entity)?.is_some() {
                self.group_opacities.insert(entity, visibility.opacity);
                groups.push(entity);
                self.collect(entity, visible, opacity, groups)?;
                groups.pop();
                continue;
            }
            if let Some(raster) = self.snapshot.component::<RasterLayer>(entity)? {
                self.elements.push(CanvasElement {
                    id: entity,
                    frame: Frame::new(0.0, 0.0, self.size.width as f32, self.size.height as f32),
                    geometry: Geometry::rectangle(
                        0.0,
                        0.0,
                        f64::from(self.size.width),
                        f64::from(self.size.height),
                    ),
                    visible,
                    opacity,
                    local_opacity: visibility.opacity,
                    groups: groups.clone(),
                    image: asset_blob(self.snapshot, entity, &AssetRole::new("source")?)?,
                    has_text: false,
                    raster: Some(raster.kind),
                });
                continue;
            }
            let has_text = self.snapshot.component::<TextLayout>(entity)?.is_some();
            let geometry = if has_text {
                text_geometry(self.snapshot, entity)?
            } else {
                self.snapshot.component::<Geometry>(entity)?
            };
            let Some(geometry) = geometry else {
                continue;
            };
            let Some(frame) = geometry_frame(&geometry) else {
                continue;
            };
            self.elements.push(CanvasElement {
                id: entity,
                geometry,
                frame,
                visible,
                opacity,
                local_opacity: visibility.opacity,
                groups: groups.clone(),
                image: asset_blob(self.snapshot, entity, &AssetRole::new("source")?)?,
                has_text,
                raster: None,
            });
        }
        Ok(())
    }
}

fn visibility(snapshot: &Snapshot, entity: EntityId) -> Result<Visibility> {
    Ok(snapshot
        .component::<Visibility>(entity)?
        .unwrap_or(Visibility {
            origin: koharu_scene::Origin::User,
            visible: true,
            opacity: 1.0,
        }))
}

fn text_geometry(snapshot: &Snapshot, layer: EntityId) -> Result<Option<Geometry>> {
    Ok(snapshot.text_layer(layer)?.frame()?)
}

fn asset_blob(snapshot: &Snapshot, entity: EntityId, role: &AssetRole) -> Result<Option<BlobId>> {
    Ok(snapshot.asset(entity, role)?.map(|asset| asset.blob))
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
    use koharu_scene::{
        At, BubbleRegion, FitsTo, Geometry, Origin, PageDraft, Point, Session, TextLayout,
        TextLayoutKind, Visibility,
    };

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
    fn text_layers_resolve_automatic_frames_from_regions() {
        let mut session = Session::memory().unwrap();
        let mut entities = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(PageDraft::new("page", 100.0, 100.0), At::End)?;
                let bubble = edit.add_analysis_region::<BubbleRegion>(
                    page,
                    At::End,
                    &Geometry::rectangle(10.0, 20.0, 30.0, 40.0),
                    Some("bubble".into()),
                )?;
                let content = edit.add_text_content(page, At::End)?;
                let text = edit.add_text_layer(
                    page,
                    At::End,
                    content,
                    &TextLayout {
                        origin: Origin::User,
                        kind: TextLayoutKind::Paragraph,
                    },
                )?;
                edit.relate::<FitsTo>(text, bubble)?;
                entities = Some((text, bubble));
                Ok(())
            })
            .unwrap();
        let snapshot = session.commit(patch).unwrap().snapshot;
        let (text, bubble) = entities.unwrap();

        assert_eq!(
            text_geometry(&snapshot, text).unwrap(),
            Some(Geometry::rectangle(10.0, 20.0, 30.0, 40.0))
        );

        let frame = Frame::new(0.0, 0.0, 10.0, 10.0);
        let analysis_region = CanvasElement {
            id: bubble,
            geometry: Geometry::rectangle(0.0, 0.0, 10.0, 10.0),
            frame,
            visible: true,
            opacity: 1.0,
            local_opacity: 1.0,
            groups: Vec::new(),
            image: None,
            raster: None,
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

    #[test]
    fn presentation_sync_updates_inherited_group_state_without_reloading_the_page() {
        let mut session = Session::memory().unwrap();
        let mut ids = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(PageDraft::new("page", 100.0, 100.0), At::End)?;
                let content = edit.add_text_content(page, At::End)?;
                let text = edit.add_text_layer(
                    page,
                    At::End,
                    content,
                    &TextLayout {
                        origin: Origin::User,
                        kind: TextLayoutKind::Paragraph,
                    },
                )?;
                ids = Some((page, text));
                Ok(())
            })
            .unwrap();
        let snapshot = session.commit(patch).unwrap().snapshot;
        let (page, text) = ids.unwrap();
        let group = snapshot
            .page(page)
            .unwrap()
            .text_group()
            .unwrap()
            .unwrap()
            .id();
        let mut canvas_page = CanvasPage {
            id: page,
            size: PhysicalSize::new(100, 100),
            assets: PageAssets::default(),
            members: HashSet::from([group, text]),
            elements: vec![CanvasElement {
                id: text,
                geometry: Geometry::rectangle(0.0, 0.0, 10.0, 10.0),
                frame: Frame::new(0.0, 0.0, 10.0, 10.0),
                visible: true,
                opacity: 1.0,
                local_opacity: 1.0,
                groups: vec![group],
                image: None,
                has_text: true,
                raster: None,
            }],
            group_opacities: HashMap::from([(group, 1.0)]),
        };
        let patch = snapshot
            .patch(|edit| {
                edit.set(
                    group,
                    &Visibility {
                        origin: Origin::User,
                        visible: false,
                        opacity: 0.25,
                    },
                )?;
                edit.set(
                    text,
                    &Visibility {
                        origin: Origin::User,
                        visible: true,
                        opacity: 0.5,
                    },
                )
            })
            .unwrap();
        let snapshot = session.commit(patch).unwrap().snapshot;

        canvas_page.sync_presentation(&snapshot).unwrap();

        let element = &canvas_page.elements[0];
        assert!(!element.visible);
        assert_eq!(element.local_opacity, 0.5);
        assert_eq!(element.opacity, 0.125);
        assert_eq!(canvas_page.group_opacities[&group], 0.25);
    }
}
