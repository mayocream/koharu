use std::sync::Arc;

use anyhow::Result;
use image::{DynamicImage, RgbaImage};
use koharu_core::image::SerializableDynamicImage;
use koharu_renderer::{
    Script,
    font::{FontBook, Language},
    layout::{LayoutRequest, Layouter, Orientation},
    render::{RenderRequest, Renderer},
};
use tokio::sync::Mutex;

use crate::state::{Document, TextBlock};

pub struct TextRenderer {
    fontbook: Arc<Mutex<FontBook>>,
    renderer: Arc<Mutex<Renderer>>,
    layouter: Arc<Mutex<Layouter>>,
}

impl TextRenderer {
    pub fn new() -> Self {
        Self {
            fontbook: Arc::new(Mutex::new(FontBook::new())),
            renderer: Arc::new(Mutex::new(Renderer::new())),
            layouter: Arc::new(Mutex::new(Layouter::new())),
        }
    }

    pub async fn available_fonts(&self) -> Vec<String> {
        let fontbook = self.fontbook.lock().await;
        fontbook
            .filter_by_language(&Language::Chinese_PeoplesRepublicOfChina)
            .iter()
            .map(|face| face.families.iter().map(|f| f.0.clone()))
            .flatten()
            .collect()
    }

    pub async fn render(&self, doc: &mut Document) -> Result<()> {
        doc.rendered = None;

        let Some(inpainted) = doc.inpainted.as_deref() else {
            return Ok(());
        };

        let mut rendered = inpainted.to_rgba8();

        for block in &doc.text_blocks {
            self.render_block(block, &mut rendered).await?;
        }

        doc.rendered = Some(SerializableDynamicImage::from(DynamicImage::ImageRgba8(
            rendered,
        )));

        Ok(())
    }

    async fn render_block(&self, block: &TextBlock, image: &mut RgbaImage) -> Result<()> {
        let mut fontbook = self.fontbook.lock().await;
        let style = block.style.clone().unwrap_or_default();
        let fonts = fontbook
            .filter_by_families(&style.font_families)
            .iter()
            .filter_map(|face| fontbook.font(face).ok())
            .collect::<Vec<_>>();

        let direction = if block.width >= block.height {
            Orientation::Horizontal
        } else {
            Orientation::Vertical
        };

        let mut layouter = self.layouter.lock().await;
        let glyphs = layouter.layout(&LayoutRequest {
            text: block.translation.as_deref().unwrap_or_default(),
            fonts: &fonts,
            font_size: style.font_size,
            line_height: style.line_height * style.font_size,
            script: Script::Han,
            max_primary_axis: if direction == Orientation::Horizontal {
                block.width
            } else {
                block.height
            },
            direction: direction,
        })?;

        let mut renderer = self.renderer.lock().await;
        renderer.render(&mut RenderRequest {
            layout: &glyphs,
            image,
            x: if direction == Orientation::Horizontal {
                block.x
            } else {
                block.x + block.width + style.font_size
            },
            y: block.y + style.font_size,
            font_size: style.font_size,
            color: style.color,
        })?;

        Ok(())
    }
}
