use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use image::DynamicImage;
use koharu_ml::paddle_ocr_vl::{PaddleOCRVL, PaddleOCRVLTask};
use koharu_scene::{Command, ElementChange, ElementId, PageId, SourceText, TextDirection};

use crate::{Context, PaddleOcrVl1_6Config, Processor, Stage};

pub(super) struct PaddleOcrVl1_6Processor {
    model: Arc<Mutex<PaddleOCRVL>>,
}

impl PaddleOcrVl1_6Processor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        _config: &PaddleOcrVl1_6Config,
    ) -> Result<Self> {
        Ok(Self {
            model: Arc::new(Mutex::new(PaddleOCRVL::load(device).await?)),
        })
    }
}

#[async_trait]
impl Processor for PaddleOcrVl1_6Processor {
    fn name(&self) -> &'static str {
        "PaddleOCR-VL 1.6"
    }

    fn stage(&self) -> Stage {
        Stage::Ocr
    }

    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let mut inputs = Vec::new();
        for page in context.pages() {
            let source = context.source(page.id)?;
            for (element, text) in page.texts() {
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
                .map_err(|_| anyhow!("PaddleOCR-VL model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let result = model.inference(&input.image, PaddleOCRVLTask::Ocr)?;
                    Ok((input, result.text))
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
