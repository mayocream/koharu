use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use image::DynamicImage;
use koharu_ml::baberu_ocr::BaberuOcr;
use koharu_scene::{
    Command, ElementChange, ElementId, PageId, SourceText, TextDirection, TextRole,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct BaberuOcrConfig {}

pub(super) struct BaberuOcrProcessor {
    model: Arc<Mutex<BaberuOcr>>,
}

impl BaberuOcrProcessor {
    pub(super) async fn load(device: koharu_ml::Device, _config: &BaberuOcrConfig) -> Result<Self> {
        Ok(Self {
            model: Arc::new(Mutex::new(BaberuOcr::load(device).await?)),
        })
    }
}

#[async_trait]
impl Processor for BaberuOcrProcessor {
    fn name(&self) -> &'static str {
        "BaberuOcr"
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[Artifact::SourceImage, Artifact::TextRegion]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[Artifact::SourceText]
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
                    source: text.source.clone(),
                });
            }
        }
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("Baberu OCR model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let text = model.inference(&input.image)?;
                    Ok((input, text))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await??;

        let mut commands = context.commands();
        for (input, text) in outputs {
            let mut source = input.source.unwrap_or(SourceText {
                text: String::new(),
                language: None,
                direction: TextDirection::Auto,
                confidence: None,
                lines: Vec::new(),
            });
            source.text = text;
            commands.push(Command::EditElement {
                page: input.page,
                element: input.element,
                edit: ElementChange::Source(Some(source)),
            });
            commands.push(Command::EditElement {
                page: input.page,
                element: input.element,
                edit: ElementChange::Translation(None),
            });
        }
        Ok(commands)
    }
}

struct TextInput {
    page: PageId,
    element: ElementId,
    image: DynamicImage,
    source: Option<SourceText>,
}
