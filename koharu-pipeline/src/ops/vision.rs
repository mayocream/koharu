use koharu_types::FontFaceInfo;
use koharu_types::commands::{IndexPayload, RenderPayload};
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
