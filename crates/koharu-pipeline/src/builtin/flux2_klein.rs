use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use koharu_ml::flux2_klein::{Flux2KleinInpaint, Flux2KleinInpaintOptions};
use koharu_scene::{PageAsset, PageId};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct Flux2KleinConfig {}

const PROMPT: &str = "Remove the text and reconstruct the background.";

pub(super) struct Flux2KleinProcessor {
    model: Arc<Mutex<Flux2KleinInpaint>>,
}

impl Flux2KleinProcessor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        _config: &Flux2KleinConfig,
    ) -> Result<Self> {
        Ok(Self {
            model: Arc::new(Mutex::new(Flux2KleinInpaint::load(device).await?)),
        })
    }
}

#[async_trait]
impl Processor for Flux2KleinProcessor {
    fn name(&self) -> &'static str {
        "FLUX.2 Klein"
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
                    width: page.size.width,
                    height: page.size.height,
                    image,
                    mask: DynamicImage::ImageLuma8(mask),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("FLUX.2 Klein model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let image = model.inference(
                        PROMPT,
                        &input.image,
                        None,
                        &input.mask,
                        &Flux2KleinInpaintOptions::default(),
                    )?;
                    let image = if image.width() == input.width && image.height() == input.height {
                        image
                    } else {
                        image.resize_exact(
                            input.width,
                            input.height,
                            image::imageops::FilterType::Lanczos3,
                        )
                    };
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
    width: u32,
    height: u32,
    image: Arc<DynamicImage>,
    mask: DynamicImage,
}
