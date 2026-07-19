use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use image::DynamicImage;
use koharu_ml::font_detector::{FontDetector, FontPrediction};
use koharu_scene::{Command, ElementChange, ElementId, PageId, TextRole, TextStyle};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct FontDetectorConfig {
    #[specta(type = f64)]
    pub top_k: usize,
}

impl Default for FontDetectorConfig {
    fn default() -> Self {
        Self { top_k: 3 }
    }
}

pub(super) struct FontDetectorProcessor {
    model: Arc<Mutex<FontDetector>>,
    top_k: usize,
}

impl FontDetectorProcessor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        config: &FontDetectorConfig,
    ) -> Result<Self> {
        Ok(Self {
            model: Arc::new(Mutex::new(FontDetector::load(device).await?)),
            top_k: config.top_k,
        })
    }
}

#[async_trait]
impl Processor for FontDetectorProcessor {
    fn name(&self) -> &'static str {
        "FontDetector"
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[Artifact::SourceImage, Artifact::TextRegion]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[Artifact::Typography]
    }

    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let mut inputs = Vec::new();
        for page in context.pages() {
            let source = context.source(page.id)?;
            for (element, text) in page.texts() {
                if text.role == TextRole::Onomatopoeia {
                    continue;
                }
                if !context.includes_element(page.id, element.id, element.frame) {
                    continue;
                }
                let x = (element.frame.x.floor().max(0.0) as u32).min(source.width());
                let y = (element.frame.y.floor().max(0.0) as u32).min(source.height());
                let right = ((element.frame.x + element.frame.width).ceil().max(0.0) as u32)
                    .min(source.width());
                let bottom = ((element.frame.y + element.frame.height).ceil().max(0.0) as u32)
                    .min(source.height());
                if right <= x || bottom <= y {
                    bail!("text frame does not overlap its source image");
                }
                inputs.push(TextInput {
                    page: page.id,
                    element: element.id,
                    image: source.crop_imm(x, y, right - x, bottom - y),
                });
            }
        }
        if inputs.is_empty() {
            return Ok(context.commands());
        }
        let images = inputs
            .iter()
            .map(|input| input.image.clone())
            .collect::<Vec<_>>();
        let top_k = self.top_k;
        let model = self.model.clone();
        let predictions = tokio::task::spawn_blocking(move || {
            model
                .lock()
                .map_err(|_| anyhow!("font detector model lock is poisoned"))?
                .inference(&images, top_k)
        })
        .await??;

        let mut commands = context.commands();
        for (input, prediction) in inputs.into_iter().zip(predictions) {
            let text = context
                .page(input.page)
                .expect("captured page")
                .text(input.element)
                .expect("captured text");
            let style = apply_prediction(text.style.clone(), prediction);
            commands.push(Command::EditElement {
                page: input.page,
                element: input.element,
                edit: ElementChange::Style(style),
            });
        }
        Ok(commands)
    }
}

struct TextInput {
    page: PageId,
    element: ElementId,
    image: DynamicImage,
}

fn apply_prediction(mut style: TextStyle, prediction: FontPrediction) -> TextStyle {
    style.color = [
        prediction.text_color[0],
        prediction.text_color[1],
        prediction.text_color[2],
        255,
    ];
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prediction_only_changes_the_text_color() {
        let original = TextStyle {
            font_families: vec!["Noto Sans".into()],
            font_size: 27.0,
            line_height: 1.45,
            letter_spacing: 2.0,
            word_spacing: 3.0,
            angle_degrees: -8.0,
            ..TextStyle::default()
        };
        let prediction = FontPrediction {
            text_color: [12, 34, 56],
            font_size_px: 18.0,
            line_height: 0.8,
            angle_deg: 16.0,
            stroke_color: [90, 80, 70],
            stroke_width_px: 4.0,
            ..FontPrediction::default()
        };
        let mut expected = original.clone();
        expected.color = [12, 34, 56, 255];

        assert_eq!(apply_prediction(original, prediction), expected);
    }
}
