use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::DynamicImage;
use koharu_ml::pp_doclayout_v3::{PPDocLayoutV3, PPDocLayoutV3Region};
use koharu_scene::{Command, ElementChange, ElementKind, Frame, PageId, SourceText, TextDirection};

use crate::{Context, PPDocLayoutV3Config, Processor, Stage};

pub(super) struct PPDocLayoutV3Processor {
    model: Arc<Mutex<PPDocLayoutV3>>,
    confidence: f32,
}

impl PPDocLayoutV3Processor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        config: &PPDocLayoutV3Config,
    ) -> Result<Self> {
        ensure!(
            config.confidence.is_finite() && (0.0..=1.0).contains(&config.confidence),
            "PPDocLayoutV3 confidence must be between 0 and 1"
        );
        Ok(Self {
            model: Arc::new(Mutex::new(PPDocLayoutV3::load(device).await?)),
            confidence: config.confidence,
        })
    }
}

#[async_trait]
impl Processor for PPDocLayoutV3Processor {
    fn name(&self) -> &'static str {
        "PPDocLayoutV3"
    }

    fn stage(&self) -> Stage {
        Stage::Detection
    }

    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let inputs = context
            .pages()
            .iter()
            .map(|page| {
                let source = context.source(page.id)?;
                let area = if let Some(region) = context.region(page.id) {
                    let x = (region.x.floor().max(0.0) as u32).min(source.width());
                    let y = (region.y.floor().max(0.0) as u32).min(source.height());
                    let right =
                        ((region.x + region.width).ceil().max(0.0) as u32).min(source.width());
                    let bottom =
                        ((region.y + region.height).ceil().max(0.0) as u32).min(source.height());
                    if right <= x || bottom <= y {
                        bail!("pipeline region does not overlap page {}", page.id);
                    }
                    PixelArea {
                        x,
                        y,
                        width: right - x,
                        height: bottom - y,
                    }
                } else {
                    PixelArea {
                        x: 0,
                        y: 0,
                        width: source.width(),
                        height: source.height(),
                    }
                };
                let image = if area.x == 0
                    && area.y == 0
                    && area.width == source.width()
                    && area.height == source.height()
                {
                    source
                } else {
                    Arc::new(source.crop_imm(area.x, area.y, area.width, area.height))
                };
                Ok(PageInput {
                    page: page.id,
                    image,
                    area,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let confidence = self.confidence;
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("PPDocLayoutV3 model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let detections = model.inference(&input.image, confidence)?;
                    Ok((input, detections.regions))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await??;

        let mut commands = context.commands();
        for (input, regions) in outputs {
            let page = context.page(input.page).expect("captured page");
            for element in &page.elements {
                if matches!(element.kind, ElementKind::Text(_))
                    && context.includes_element(input.page, element.id, element.frame)
                {
                    commands.push(Command::DeleteElement {
                        page: input.page,
                        element: element.id,
                    });
                }
            }
            let mut texts = detected_texts(&regions, input.area).collect::<Vec<_>>();
            texts.sort_by(|left, right| {
                left.frame
                    .y
                    .total_cmp(&right.frame.y)
                    .then_with(|| right.frame.x.total_cmp(&left.frame.x))
            });
            for text in texts {
                let element = commands.add_text(input.page, text.frame);
                commands.push(Command::EditElement {
                    page: input.page,
                    element,
                    edit: ElementChange::Source(Some(text.source)),
                });
            }
        }
        Ok(commands)
    }
}

#[derive(Clone, Copy)]
struct PixelArea {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

struct PageInput {
    page: PageId,
    image: Arc<DynamicImage>,
    area: PixelArea,
}

struct DetectedText {
    frame: Frame,
    source: SourceText,
}

fn detected_texts(
    regions: &[PPDocLayoutV3Region],
    area: PixelArea,
) -> impl Iterator<Item = DetectedText> + '_ {
    regions.iter().filter_map(move |region| {
        let label = region.label.to_ascii_lowercase();
        if label != "content" && !label.contains("text") && !label.contains("title") {
            return None;
        }
        let x1 = region.bbox[0].min(region.bbox[2]).max(0.0);
        let y1 = region.bbox[1].min(region.bbox[3]).max(0.0);
        let width = (region.bbox[0].max(region.bbox[2]) - x1).max(0.0);
        let height = (region.bbox[1].max(region.bbox[3]) - y1).max(0.0);
        if width < 6.0 || height < 6.0 || width * height < 48.0 {
            return None;
        }
        Some(DetectedText {
            frame: Frame::new(x1 + area.x as f32, y1 + area.y as f32, width, height),
            source: SourceText {
                text: String::new(),
                language: None,
                direction: if height >= width * 1.15 {
                    TextDirection::Vertical
                } else {
                    TextDirection::Horizontal
                },
                confidence: Some(region.score.clamp(0.0, 1.0)),
                lines: Vec::new(),
            },
        })
    })
}
