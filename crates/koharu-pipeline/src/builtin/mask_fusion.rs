use std::{io::Cursor, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use imageproc::{
    drawing::{draw_filled_rect_mut, draw_polygon_mut},
    point::Point,
    rect::Rect,
};
use koharu_scene::{Page, PageAsset, TextRole};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct MaskFusionConfig {
    /// Retained for compatibility with saved settings. Region masks are exact and this is ignored.
    pub coo_padding: u32,
}

impl Default for MaskFusionConfig {
    fn default() -> Self {
        Self { coo_padding: 0 }
    }
}

pub(super) struct MaskFusionProcessor;

impl MaskFusionProcessor {
    pub(super) fn new(_config: &MaskFusionConfig) -> Self {
        Self
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
            Artifact::TextRegion,
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

            let (text_regions, coo_regions) = region_masks(page);
            let (text, coo) = clip_to_regions(&foreground, &text_regions, &coo_regions);

            commands.set_asset(page.id, PageAsset::TextMask, Some(encode(text)?))?;
            commands.set_asset(page.id, PageAsset::CooMask, Some(encode(coo)?))?;
        }
        Ok(commands)
    }
}

fn region_masks(page: &Page) -> (GrayImage, GrayImage) {
    let mut text_regions = GrayImage::new(page.size.width, page.size.height);
    let mut coo_regions = GrayImage::new(page.size.width, page.size.height);
    for (element, text) in page.texts() {
        let mask = if text.role == TextRole::Onomatopoeia {
            &mut coo_regions
        } else {
            &mut text_regions
        };
        if text.polygon.len() >= 3 {
            let polygon = text
                .polygon
                .iter()
                .map(|point| Point::new(point[0].round() as i32, point[1].round() as i32))
                .collect::<Vec<_>>();
            draw_polygon_mut(mask, &polygon, Luma([u8::MAX]));
            continue;
        }

        let left = element.frame.x.floor().clamp(0.0, page.size.width as f32) as i32;
        let top = element.frame.y.floor().clamp(0.0, page.size.height as f32) as i32;
        let right = (element.frame.x + element.frame.width)
            .ceil()
            .clamp(0.0, page.size.width as f32) as i32;
        let bottom = (element.frame.y + element.frame.height)
            .ceil()
            .clamp(0.0, page.size.height as f32) as i32;
        if right > left && bottom > top {
            draw_filled_rect_mut(
                mask,
                Rect::at(left, top).of_size((right - left) as u32, (bottom - top) as u32),
                Luma([u8::MAX]),
            );
        }
    }
    (text_regions, coo_regions)
}

fn clip_to_regions(
    foreground: &GrayImage,
    text_regions: &GrayImage,
    coo_regions: &GrayImage,
) -> (GrayImage, GrayImage) {
    let mut text = GrayImage::new(foreground.width(), foreground.height());
    let mut coo = GrayImage::new(foreground.width(), foreground.height());
    for ((((text, coo), foreground), text_region), coo_region) in text
        .as_mut()
        .iter_mut()
        .zip(coo.as_mut())
        .zip(foreground.as_raw())
        .zip(text_regions.as_raw())
        .zip(coo_regions.as_raw())
    {
        if *coo_region != 0 {
            *coo = *foreground;
        } else if *text_region != 0 {
            *text = *foreground;
        }
    }
    (text, coo)
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

    use crate::{ProcessorConfig, plan::ConfiguredModel};

    #[test]
    fn processor_inputs_match_the_pipeline_contract() {
        let processor = MaskFusionProcessor::new(&MaskFusionConfig::default());
        let model =
            ConfiguredModel::Processor(ProcessorConfig::MaskFusion(MaskFusionConfig::default()));

        assert_eq!(processor.inputs(), model.inputs());
    }

    #[test]
    fn foreground_is_clipped_to_exact_text_and_coo_regions() {
        let mut page = koharu_scene::Page {
            id: koharu_scene::PageId::new(),
            name: String::new(),
            size: koharu_scene::Size::new(10, 5),
            source: koharu_scene::BlobId::for_bytes(b"source"),
            assets: Default::default(),
            elements: Vec::new(),
        };
        page.elements.push(Element::new_text(
            ElementId::new(),
            Frame::new(1.0, 1.0, 2.0, 2.0),
            TextBlock::default(),
        ));
        let coo = TextBlock {
            role: TextRole::Onomatopoeia,
            ..TextBlock::default()
        };
        page.elements.push(Element::new_text(
            ElementId::new(),
            Frame::new(6.0, 1.0, 2.0, 2.0),
            coo,
        ));

        let foreground = GrayImage::from_pixel(10, 5, Luma([u8::MAX]));
        let (text_regions, coo_regions) = region_masks(&page);
        let (text, coo) = clip_to_regions(&foreground, &text_regions, &coo_regions);

        assert_eq!(text.get_pixel(1, 1).0[0], u8::MAX);
        assert_eq!(coo.get_pixel(1, 1).0[0], 0);
        assert_eq!(text.get_pixel(6, 1).0[0], 0);
        assert_eq!(coo.get_pixel(6, 1).0[0], u8::MAX);
        assert_eq!(text.get_pixel(4, 1).0[0], 0);
        assert_eq!(coo.get_pixel(4, 1).0[0], 0);
    }
}
