use std::sync::{Arc, Mutex};

use anyhow::Result;
use icu::properties::{CodePointMapData, props::Script};
use image::{DynamicImage, imageops};
use koharu_renderer::{
    font::{FamilyName, Font, FontBook, Properties},
    layout::{LayoutRun, TextLayout, WritingMode},
    renderer::{RenderOptions, TextShaderEffect, WgpuRenderer},
};
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

use crate::{
    image::SerializableDynamicImage,
    state::{Document, TextBlock, TextStyle},
};

pub struct Renderer {
    fontbook: Arc<Mutex<FontBook>>,
    renderer: WgpuRenderer,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            fontbook: Arc::new(Mutex::new(FontBook::new())),
            renderer: WgpuRenderer::new()?,
        })
    }

    pub fn available_fonts(&self) -> Result<Vec<String>> {
        let mut fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock fontbook"))?;
        let mut families = fontbook.all_families();
        families.sort();
        Ok(families)
    }

    pub fn render(
        &self,
        document: &mut Document,
        text_block_index: Option<usize>,
        effect: TextShaderEffect,
    ) -> Result<()> {
        let mut text_blocks = match text_block_index {
            Some(index) => document
                .text_blocks
                .get_mut(index)
                .map(|tb| vec![tb])
                .ok_or_else(|| anyhow::anyhow!("Text block index out of bounds"))?,
            None => document.text_blocks.iter_mut().collect(),
        };

        text_blocks
            .par_iter_mut()
            .try_for_each(|text_block| self.render_text_block(text_block, effect))?;

        if let Some(inpainted) = &document.inpainted
            && text_block_index.is_none()
        {
            let mut rendered = inpainted.to_rgba8();

            if let Some(brush_layer) = &document.brush_layer {
                let brush = brush_layer.to_rgba8();
                imageops::overlay(&mut rendered, &brush, 0, 0);
            }

            for text_block in text_blocks {
                let Some(block) = text_block.rendered.as_ref() else {
                    continue;
                };
                imageops::overlay(
                    &mut rendered,
                    &block.0,
                    text_block.x as i64,
                    text_block.y as i64,
                );
            }
            document.rendered = Some(SerializableDynamicImage(DynamicImage::ImageRgba8(rendered)));
        }
        Ok(())
    }

    fn render_text_block(
        &self,
        text_block: &mut TextBlock,
        effect: TextShaderEffect,
    ) -> Result<()> {
        let Some(translation) = &text_block.translation else {
            return Ok(());
        };
        if translation.is_empty() {
            return Ok(());
        };

        let style = text_block.style.clone().unwrap_or_default();
        let font = self.select_font(&style)?;
        let block_effect = style.effect.unwrap_or(effect);
        let color = text_block
            .style
            .as_ref()
            .map(|style| style.color)
            .or_else(|| {
                text_block.font_prediction.as_ref().map(|pred| {
                    [
                        pred.text_color[0],
                        pred.text_color[1],
                        pred.text_color[2],
                        255,
                    ]
                })
            })
            .unwrap_or([0, 0, 0, 255]);
        let writing_mode = writing_mode(text_block);
        let mut layout = TextLayout::new(&font, None)
            .with_max_height(text_block.height)
            .with_max_width(text_block.width)
            .with_writing_mode(writing_mode)
            .run(translation)?;
        if writing_mode == WritingMode::Horizontal && is_latin_only(translation) {
            center_layout_horizontally(&mut layout, text_block.width);
        }

        let rendered = self.renderer.render(
            &layout,
            writing_mode,
            &font,
            &RenderOptions {
                font_size: layout.font_size,
                color,
                effect: block_effect,
                ..Default::default()
            },
        )?;

        text_block.rendered = Some(SerializableDynamicImage(DynamicImage::ImageRgba8(rendered)));
        Ok(())
    }

    fn select_font(&self, style: &TextStyle) -> Result<Font> {
        let mut fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock fontbook"))?;
        let font = fontbook.query(
            style
                .font_families
                .iter()
                .map(|family| FamilyName::Title(family.to_string()))
                .collect::<Vec<_>>()
                .as_slice(),
            &Properties::default(),
        )?;
        Ok(font)
    }
}

fn writing_mode(text_block: &TextBlock) -> WritingMode {
    let text = match &text_block.translation {
        Some(t) => t,
        None => return WritingMode::Horizontal,
    };

    if !is_cjk(text) || text_block.width >= text_block.height {
        WritingMode::Horizontal
    } else {
        WritingMode::VerticalRl
    }
}

fn is_cjk(text: &str) -> bool {
    let script_map = CodePointMapData::<Script>::new();
    text.chars().any(|c| {
        matches!(
            script_map.get(c),
            Script::Han | Script::Hiragana | Script::Katakana | Script::Hangul | Script::Bopomofo
        )
    })
}

fn is_latin_only(text: &str) -> bool {
    let script_map = CodePointMapData::<Script>::new();
    text.chars().all(|c| {
        matches!(
            script_map.get(c),
            Script::Latin | Script::Common | Script::Inherited
        )
    })
}

fn center_layout_horizontally(layout: &mut LayoutRun, container_width: f32) {
    if !container_width.is_finite() || container_width <= 0.0 {
        return;
    }

    let target_width = layout.width.max(container_width);
    for line in &mut layout.lines {
        if line.advance <= 0.0 {
            continue;
        }
        let offset = ((container_width - line.advance) * 0.5).max(0.0);
        if offset > 0.0 {
            line.baseline.0 += offset;
        }
    }
    layout.width = target_width;
}
