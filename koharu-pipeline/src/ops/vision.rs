use koharu_types::FontFaceInfo;
use koharu_types::commands::{DetectPayload, IndexPayload, RenderPayload};
use tracing::instrument;

use crate::{
    AppResources,
    state_tx::{self, ChangedField},
};

#[instrument(level = "info", skip_all)]
pub async fn detect(state: AppResources, payload: IndexPayload) -> anyhow::Result<()> {
    let mut snapshot = state_tx::read_doc(&state.state, payload.index).await?;
    state.ml.detect(&mut snapshot).await?;
    state_tx::update_doc(
        &state.state,
        payload.index,
        snapshot,
        &[ChangedField::TextBlocks, ChangedField::Segment],
    )
    .await
}

#[instrument(level = "info", skip_all)]
pub async fn detect_with_options(
    state: AppResources,
    payload: DetectPayload,
) -> anyhow::Result<()> {
    let mut snapshot = state_tx::read_doc(&state.state, payload.index).await?;

    let options = koharu_ml::DetectOptions {
        sensitive: payload.sensitive,
    };

    // If a region is specified, crop the image, detect on the crop, then map blocks back
    if let Some(region) = payload.region {
        let (img_w, img_h) = (snapshot.width, snapshot.height);
        let x0 = (region.x).min(img_w.saturating_sub(1));
        let y0 = (region.y).min(img_h.saturating_sub(1));
        let rw = region.width.min(img_w - x0);
        let rh = region.height.min(img_h - y0);
        if rw == 0 || rh == 0 {
            return Ok(());
        }

        let crop = snapshot.image.crop_imm(x0, y0, rw, rh);
        let mut sub_doc = koharu_types::Document {
            image: crop.into(),
            width: rw,
            height: rh,
            ..Default::default()
        };
        state.ml.detect_with_options(&mut sub_doc, options).await?;

        // Offset detected blocks back to full-image coordinates and merge
        let new_blocks: Vec<_> = sub_doc
            .text_blocks
            .into_iter()
            .map(|mut b| {
                b.x += x0 as f32;
                b.y += y0 as f32;
                b
            })
            .collect();

        // Append to existing blocks (don't replace — user may have prior detection)
        snapshot.text_blocks.extend(new_blocks);

        // Refresh segmentation on full image with all blocks
        let probability_map = state
            .ml
            .segmenter()
            .inference_segmentation(&snapshot.image)?;
        let mask = koharu_ml::comic_text_detector::refine_segmentation_mask(
            &snapshot.image,
            &probability_map,
            &snapshot.text_blocks,
        );
        snapshot.segment = Some(image::DynamicImage::ImageLuma8(mask).into());
    } else {
        state.ml.detect_with_options(&mut snapshot, options).await?;
    }

    state_tx::update_doc(
        &state.state,
        payload.index,
        snapshot,
        &[ChangedField::TextBlocks, ChangedField::Segment],
    )
    .await
}

#[instrument(level = "info", skip_all)]
pub async fn ocr(state: AppResources, payload: IndexPayload) -> anyhow::Result<()> {
    let mut snapshot = state_tx::read_doc(&state.state, payload.index).await?;
    state.ml.ocr(&mut snapshot).await?;
    state_tx::update_doc(
        &state.state,
        payload.index,
        snapshot,
        &[ChangedField::TextBlocks],
    )
    .await
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint(state: AppResources, payload: IndexPayload) -> anyhow::Result<()> {
    let mut snapshot = state_tx::read_doc(&state.state, payload.index).await?;
    state.ml.inpaint(&mut snapshot).await?;
    state_tx::update_doc(
        &state.state,
        payload.index,
        snapshot,
        &[ChangedField::Inpainted],
    )
    .await
}

#[instrument(level = "info", skip_all)]
pub async fn render(state: AppResources, payload: RenderPayload) -> anyhow::Result<()> {
    let mut updated = state_tx::read_doc(&state.state, payload.index).await?;

    state.renderer.render(
        &mut updated,
        payload.text_block_index,
        payload.shader_effect.unwrap_or_default(),
        payload.shader_stroke,
        payload.font_family.as_deref(),
    )?;

    state_tx::update_doc(
        &state.state,
        payload.index,
        updated,
        &[ChangedField::TextBlocks, ChangedField::Rendered],
    )
    .await
}

pub async fn list_font_families(state: AppResources) -> anyhow::Result<Vec<FontFaceInfo>> {
    state.renderer.available_fonts()
}
