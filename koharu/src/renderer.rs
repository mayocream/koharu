use std::sync::{Arc, Mutex};

use anyhow::Result;
use icu::properties::{CodePointMapData, props::Script};
use image::{DynamicImage, imageops};
use koharu_renderer::{
    font::{FamilyName, Font, FontBook, Properties},
    layout::{TextLayout, WritingMode},
    renderer::{RenderOptions, WgpuRenderer},
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

    pub fn render(&self, document: &mut Document, text_block_index: Option<usize>) -> Result<()> {
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
            .try_for_each(|text_block| self.render_text_block(text_block))?;

        if let Some(inpainted) = &document.inpainted
            && text_block_index.is_none()
        {
            let mut rendered = inpainted.to_rgba8();
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

    fn render_text_block(&self, text_block: &mut TextBlock) -> Result<()> {
        let Some(translation) = &text_block.translation else {
            return Ok(());
        };

        let font = self.select_font(&text_block.style.clone().unwrap_or_default())?;
        let writing_mode = writing_mode(text_block);
        let layout = TextLayout::new(&font, None)
            .with_max_height(text_block.height)
            .with_max_width(text_block.width)
            .with_writing_mode(writing_mode)
            .run(translation)?;

        let rendered = self.renderer.render(
            &layout,
            writing_mode,
            &font,
            &RenderOptions {
                font_size: layout.font_size,
                color: text_block
                    .font_prediction
                    .as_ref()
                    .map(|pred| {
                        [
                            pred.text_color[0],
                            pred.text_color[1],
                            pred.text_color[2],
                            255,
                        ]
                    })
                    .unwrap_or([0, 0, 0, 255]),
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
    text.chars().any(|c| {
        matches!(
            CodePointMapData::<Script>::new().get(c),
            Script::Han | Script::Hiragana | Script::Katakana | Script::Hangul | Script::Bopomofo
        )
    })
}
