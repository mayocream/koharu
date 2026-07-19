use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use koharu_ml::lama::{InpaintRequest, LaMa};
use koharu_scene::{PageAsset, PageId};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct LaMaConfig {}

pub(super) struct LaMaProcessor {
    model: Arc<Mutex<LaMa>>,
}

impl LaMaProcessor {
    pub(super) async fn load(device: koharu_ml::Device, _config: &LaMaConfig) -> Result<Self> {
        Ok(Self {
            model: Arc::new(Mutex::new(LaMa::load(device).await?)),
        })
    }
}

#[async_trait]
impl Processor for LaMaProcessor {
    fn name(&self) -> &'static str {
        "LaMa"
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[
            Artifact::SourceImage,
            Artifact::TextMask,
            Artifact::CooMask,
            Artifact::BrushMask,
        ]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[Artifact::CleanImage]
    }

    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let inputs = context
            .pages()
            .iter()
            .map(|page| {
                let image = if context.region(page.id).is_some() {
                    context
                        .asset(page.id, PageAsset::Clean)?
                        .unwrap_or(context.source(page.id)?)
                } else {
                    context.source(page.id)?
                };
                let mut mask = GrayImage::new(page.size.width, page.size.height);
                for asset in [
                    PageAsset::TextMask,
                    PageAsset::CooMask,
                    PageAsset::BrushMask,
                ] {
                    if let Some(value) = context.asset(page.id, asset)? {
                        for (target, source) in
                            mask.as_mut().iter_mut().zip(value.to_luma8().as_raw())
                        {
                            *target = (*target).max(*source);
                        }
                    }
                }
                if let Some(region) = context.region(page.id) {
                    for (x, y, pixel) in mask.enumerate_pixels_mut() {
                        if x as f32 + 1.0 <= region.x
                            || y as f32 + 1.0 <= region.y
                            || x as f32 >= region.x + region.width
                            || y as f32 >= region.y + region.height
                        {
                            *pixel = Luma([0]);
                        }
                    }
                }
                Ok(InpaintInput {
                    page: page.id,
                    image,
                    mask,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("LaMa model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let image = DynamicImage::ImageRgb8(model.inference(
                        &input.image,
                        &input.mask,
                        &InpaintRequest::default(),
                    )?);
                    Ok((input.page, image))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await??;
        let mut commands = context.commands();
        for (page, image) in outputs {
            let mut bytes = Cursor::new(Vec::new());
            image.write_to(&mut bytes, ImageFormat::Png)?;
            let bytes: Arc<[u8]> = Arc::from(bytes.into_inner());
            commands.set_asset(page, PageAsset::Clean, Some(bytes))?;
        }
        Ok(commands)
    }
}

struct InpaintInput {
    page: PageId,
    image: Arc<DynamicImage>,
    mask: GrayImage,
}
