//! Bubble-aware layout bounds derived from explicit scene relations.

use std::collections::BTreeSet;

use koharu_scene::{
    EntityId, Geometry, Region, RegionKind, RelationId, RelationKind, SceneSnapshot,
};

use crate::Result;

const MAX_CONTOUR_POINTS: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct BubbleLayout {
    pub bounds: LayoutBox,
    pub contour: Vec<(f32, f32)>,
    pub relation: RelationId,
    pub region: EntityId,
}

pub(crate) fn resolve(
    snapshot: &SceneSnapshot,
    text: EntityId,
    page_entities: &BTreeSet<EntityId>,
    relation_kind: &RelationKind,
    region_kind: &RegionKind,
) -> Result<Option<BubbleLayout>> {
    for relation in snapshot.relations_from(text, Some(relation_kind)) {
        let value = relation.value();
        if !page_entities.contains(&value.target) {
            continue;
        }
        let Some(region) = snapshot.component::<Region>(value.target, "default")? else {
            continue;
        };
        if region.kind != *region_kind {
            continue;
        }
        let Some(geometry) = snapshot.component::<Geometry>(value.target, "default")? else {
            continue;
        };
        let Some(bounds) = geometry_bounds(&geometry) else {
            continue;
        };
        let contour = if geometry.points.len() <= MAX_CONTOUR_POINTS {
            geometry
                .points
                .iter()
                .map(|point| (point.x as f32 - bounds.x, point.y as f32 - bounds.y))
                .collect()
        } else {
            Vec::new()
        };
        return Ok(Some(BubbleLayout {
            bounds,
            contour,
            relation: relation.id(),
            region: value.target,
        }));
    }
    Ok(None)
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
}
