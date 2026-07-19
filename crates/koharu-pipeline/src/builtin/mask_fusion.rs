use std::{io::Cursor, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use imageproc::{drawing::draw_polygon_mut, point::Point};
use koharu_scene::{PageAsset, TextRole};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct MaskFusionConfig {
    /// Extra page pixels around a COO region used when the detector has no polygon.
    pub coo_padding: u32,
}

impl Default for MaskFusionConfig {
    fn default() -> Self {
        Self { coo_padding: 4 }
    }
}

pub(super) struct MaskFusionProcessor {
    config: MaskFusionConfig,
}

impl MaskFusionProcessor {
    pub(super) fn new(config: &MaskFusionConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

#[async_trait]
impl Processor for MaskFusionProcessor {
    fn name(&self) -> &'static str {
        "MaskFusion"
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[
            Artifact::TextMaskCandidate,
            Artifact::LayoutTextMask,
            Artifact::CooRegion,
        ]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[Artifact::TextMask, Artifact::CooMask]
    }

    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let mut commands = context.commands();
        for page in context.pages() {
            let mut foreground = GrayImage::new(page.size.width, page.size.height);
            for asset in [PageAsset::TextMaskCandidate, PageAsset::LayoutTextMask] {
                if let Some(mask) = context.asset(page.id, asset)? {
                    for (target, source) in
                        foreground.as_mut().iter_mut().zip(mask.to_luma8().as_raw())
                    {
                        *target = (*target).max(*source);
                    }
                }
            }

            let regions = coo_regions(page, self.config.coo_padding);
            let mut coo_region_mask = GrayImage::new(page.size.width, page.size.height);
            for region in regions {
                draw_polygon_mut(&mut coo_region_mask, &region, Luma([u8::MAX]));
            }

            let mut text = GrayImage::new(page.size.width, page.size.height);
            let mut coo = GrayImage::new(page.size.width, page.size.height);
            for (((text, coo), foreground), region) in text
                .as_mut()
                .iter_mut()
                .zip(coo.as_mut())
                .zip(foreground.as_raw())
                .zip(coo_region_mask.as_raw())
            {
                if *region == 0 {
                    *text = *foreground;
                } else {
                    *coo = *foreground;
                }
            }

            commands.set_asset(page.id, PageAsset::TextMask, Some(encode(text)?))?;
            commands.set_asset(page.id, PageAsset::CooMask, Some(encode(coo)?))?;
        }
        Ok(commands)
    }
}

fn coo_regions(page: &koharu_scene::Page, padding: u32) -> Vec<Vec<Point<i32>>> {
    page.texts()
        .filter(|(_, text)| text.role == TextRole::Onomatopoeia)
        .filter_map(|(element, text)| {
            let points = if text.polygon.len() >= 3 {
                text.polygon
                    .iter()
                    .map(|point| Point::new(point[0].round() as i32, point[1].round() as i32))
                    .collect()
            } else {
                let padding = padding as f32;
                let left = (element.frame.x - padding).floor().max(0.0) as i32;
                let top = (element.frame.y - padding).floor().max(0.0) as i32;
                let right = (element.frame.x + element.frame.width + padding)
                    .ceil()
                    .min(page.size.width as f32) as i32;
                let bottom = (element.frame.y + element.frame.height + padding)
                    .ceil()
                    .min(page.size.height as f32) as i32;
                vec![
                    Point::new(left, top),
                    Point::new(right, top),
                    Point::new(right, bottom),
                    Point::new(left, bottom),
                ]
            };
            (points.len() >= 3).then_some(points)
        })
        .collect()
}

fn encode(mask: GrayImage) -> Result<Arc<[u8]>> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(mask).write_to(&mut bytes, ImageFormat::Png)?;
    Ok(Arc::from(bytes.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_scene::{Element, ElementId, Frame, TextBlock};

    #[test]
    fn frame_fallback_expands_coo_regions() {
        let mut page = koharu_scene::Page {
            id: koharu_scene::PageId::new(),
            name: String::new(),
            size: koharu_scene::Size::new(20, 20),
            source: koharu_scene::BlobId::for_bytes(b"source"),
            assets: Default::default(),
            elements: Vec::new(),
        };
        let mut text = TextBlock::default();
        text.role = TextRole::Onomatopoeia;
        page.elements.push(Element::new_text(
            ElementId::new(),
            Frame::new(5.0, 6.0, 4.0, 3.0),
            text,
        ));

        let regions = coo_regions(&page, 2);

        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0][0], Point::new(3, 4));
        assert_eq!(regions[0][2], Point::new(11, 11));
    }
}
