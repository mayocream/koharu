use koharu_core::FontFaceInfo;
use koharu_core::commands::{IndexPayload, RenderPayload};
use tracing::instrument;

use crate::{
    AppResources,
    state_tx::{self, ChangedField, ProjectStage},
};

#[instrument(level = "info", skip_all)]
pub async fn detect(state: AppResources, payload: IndexPayload) -> anyhow::Result<()> {
    let mut snapshot = state_tx::read_doc(&state.state, payload.index).await?;
    if let Err(err) = state.ml.detect(&mut snapshot).await {
        let _ = state_tx::mark_stage_failure(
            &state.state,
            payload.index,
            ProjectStage::Detect,
            err.to_string(),
        )
        .await;
        return Err(err);
    }
    state_tx::update_doc(
        &state.state,
        payload.index,
        snapshot,
        &[ChangedField::TextBlocks, ChangedField::Segment],
    )
    .await?;
    state_tx::mark_stage_success(&state.state, payload.index, ProjectStage::Detect).await
}

#[instrument(level = "info", skip_all)]
pub async fn ocr(state: AppResources, payload: IndexPayload) -> anyhow::Result<()> {
    let mut snapshot = state_tx::read_doc(&state.state, payload.index).await?;
    if let Err(err) = state.ml.ocr(&mut snapshot).await {
        let _ = state_tx::mark_stage_failure(
            &state.state,
            payload.index,
            ProjectStage::Ocr,
            err.to_string(),
        )
        .await;
        return Err(err);
    }
    state_tx::update_doc(
        &state.state,
        payload.index,
        snapshot,
        &[ChangedField::TextBlocks],
    )
    .await?;
    state_tx::mark_stage_success(&state.state, payload.index, ProjectStage::Ocr).await
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint(state: AppResources, payload: IndexPayload) -> anyhow::Result<()> {
    let mut snapshot = state_tx::read_doc(&state.state, payload.index).await?;
    if let Err(err) = state.ml.inpaint(&mut snapshot).await {
        let _ = state_tx::mark_stage_failure(
            &state.state,
            payload.index,
            ProjectStage::Inpaint,
            err.to_string(),
        )
        .await;
        return Err(err);
    }
    state_tx::update_doc(
        &state.state,
        payload.index,
        snapshot,
        &[ChangedField::Inpainted],
    )
    .await?;
    state_tx::mark_stage_success(&state.state, payload.index, ProjectStage::Inpaint).await
}

#[instrument(level = "info", skip_all)]
pub async fn render(state: AppResources, payload: RenderPayload) -> anyhow::Result<()> {
    let mut updated = state_tx::read_doc(&state.state, payload.index).await?;

    if let Err(err) = state.renderer.render(
        &mut updated,
        payload.text_block_index,
        payload.shader_effect.unwrap_or_default(),
        payload.shader_stroke,
        payload.font_family.as_deref(),
    ) {
        let _ = state_tx::mark_stage_failure(
            &state.state,
            payload.index,
            ProjectStage::Render,
            err.to_string(),
        )
        .await;
        return Err(err);
    }

    state_tx::update_doc(
        &state.state,
        payload.index,
        updated,
        &[ChangedField::TextBlocks, ChangedField::Rendered],
    )
    .await?;
    state_tx::mark_stage_success(&state.state, payload.index, ProjectStage::Render).await
}

pub async fn list_font_families(state: AppResources) -> anyhow::Result<Vec<FontFaceInfo>> {
    state.renderer.available_fonts()
}
