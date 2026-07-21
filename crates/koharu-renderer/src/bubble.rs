//! Resolves the layout area for text associated with a scene bubble region.
//!
//! Bubble ownership is explicit in `koharu-scene`: a text block references a
//! region element by ID. Rendering follows that relationship instead of
//! rediscovering it from a page-wide segmentation image.

use koharu_scene::{Element, Page, RegionKind};

use crate::layout::WritingMode;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

const SAFE_PADDING_FRAC_HORIZONTAL: f32 = 0.12;
const SAFE_PADDING_FRAC_VERTICAL: f32 = 0.20;

/// Returns the safe layout rectangle of the bubble explicitly related to
/// `text`. Scene validation guarantees that a stored relationship targets a
/// bubble region, but the checks remain defensive for partially built pages.
pub(crate) fn layout_box(
    page: &Page,
    text: &Element,
    writing_mode: WritingMode,
) -> Option<LayoutBox> {
    let bubble = page.element(text.text()?.bubble?)?;
    let region = bubble.region()?;
    if region.kind != RegionKind::Bubble {
        return None;
    }

    let bounds = polygon_bounds(&region.polygon).unwrap_or(LayoutBox {
        x: bubble.frame.x,
        y: bubble.frame.y,
        width: bubble.frame.width,
        height: bubble.frame.height,
    });
    Some(inset_safe_area(bounds, writing_mode))
}

fn polygon_bounds(polygon: &[[f32; 2]]) -> Option<LayoutBox> {
    let [first, rest @ ..] = polygon else {
        return None;
    };
    let [mut min_x, mut min_y] = *first;
    let (mut max_x, mut max_y) = (min_x, min_y);
    for &[x, y] in rest {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let width = max_x - min_x;
    let height = max_y - min_y;
    (width > 0.0 && height > 0.0).then_some(LayoutBox {
        x: min_x,
        y: min_y,
        width,
        height,
    })
}

fn inset_safe_area(bounds: LayoutBox, writing_mode: WritingMode) -> LayoutBox {
    let fraction = match writing_mode {
        WritingMode::Horizontal => SAFE_PADDING_FRAC_HORIZONTAL,
        WritingMode::VerticalRl | WritingMode::VerticalLr => SAFE_PADDING_FRAC_VERTICAL,
    };
    let inset_x = bounds.width * fraction;
    let inset_y = bounds.height * fraction;
    LayoutBox {
        x: bounds.x + inset_x,
        y: bounds.y + inset_y,
        width: (bounds.width - inset_x * 2.0).max(1.0),
        height: (bounds.height - inset_y * 2.0).max(1.0),
    }
}

#[cfg(test)]
mod tests {
    use koharu_scene::{BlobId, ElementId, Frame, PageAssets, PageId, Region, Size, TextBlock};

    use super::*;

    fn scene(polygon: Vec<[f32; 2]>) -> (Page, ElementId) {
        let bubble = ElementId::new();
        let text = ElementId::new();
        let page = Page {
            id: PageId::new(),
            name: "scene-layout".into(),
            size: Size::new(200, 150),
            source: BlobId::for_bytes(b"source"),
            assets: PageAssets::default(),
            elements: vec![
                Element::new_region(
                    bubble,
                    Frame::new(10.0, 20.0, 100.0, 50.0),
                    Region {
                        kind: RegionKind::Bubble,
                        polygon,
                        mask_id: None,
                        reading_order: None,
                        predictions: Vec::new(),
                    },
                ),
                Element::new_text(
                    text,
                    Frame::new(25.0, 30.0, 60.0, 30.0),
                    TextBlock {
                        bubble: Some(bubble),
                        ..TextBlock::default()
                    },
                ),
            ],
        };
        (page, text)
    }

    #[test]
    fn resolves_the_explicit_scene_relationship() {
        let (page, text) = scene(Vec::new());
        let bounds =
            layout_box(&page, page.element(text).unwrap(), WritingMode::Horizontal).unwrap();

        assert_eq!(
            bounds,
            LayoutBox {
                x: 22.0,
                y: 26.0,
                width: 76.0,
                height: 38.0,
            }
        );
    }

    #[test]
    fn region_polygon_is_more_authoritative_than_its_frame() {
        let (page, text) = scene(vec![[20.0, 30.0], [80.0, 30.0], [70.0, 90.0], [20.0, 80.0]]);
        let bounds =
            layout_box(&page, page.element(text).unwrap(), WritingMode::Horizontal).unwrap();

        assert!((bounds.x - 27.2).abs() < 1e-5);
        assert!((bounds.y - 37.2).abs() < 1e-5);
        assert!((bounds.width - 45.6).abs() < 1e-5);
        assert!((bounds.height - 45.6).abs() < 1e-5);
    }

    #[test]
    fn text_without_a_related_region_has_no_bubble_layout() {
        let (mut page, text) = scene(Vec::new());
        page.elements
            .iter_mut()
            .find(|element| element.id == text)
            .unwrap()
            .kind = koharu_scene::ElementKind::Text(TextBlock::default());

        assert_eq!(
            layout_box(&page, page.element(text).unwrap(), WritingMode::Horizontal),
            None
        );
    }
}
