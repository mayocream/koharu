mod runtime;
mod text_blocks;

use tracing::instrument;

use super::{
    AppResources,
    store::{self, ChangedField},
};

pub(crate) use runtime::VisionRuntime;

#[instrument(level = "info", skip_all)]
pub(crate) async fn detect(state: AppResources, document_index: usize) -> anyhow::Result<()> {
    let mut snapshot = store::read_doc(&state.state, document_index).await?;
    state.vision.detect(&mut snapshot).await?;
    store::update_doc(
        &state.state,
        document_index,
        snapshot,
        &[ChangedField::TextBlocks, ChangedField::Segment],
    )
    .await
}

#[instrument(level = "info", skip_all)]
pub(crate) async fn ocr(state: AppResources, document_index: usize) -> anyhow::Result<()> {
    let mut snapshot = store::read_doc(&state.state, document_index).await?;
    state.vision.ocr(&mut snapshot).await?;
    store::update_doc(
        &state.state,
        document_index,
        snapshot,
        &[ChangedField::TextBlocks],
    )
    .await
}

#[instrument(level = "info", skip_all)]
pub(crate) async fn inpaint(state: AppResources, document_index: usize) -> anyhow::Result<()> {
    let mut snapshot = store::read_doc(&state.state, document_index).await?;
    state.vision.inpaint(&mut snapshot).await?;
    store::update_doc(
        &state.state,
        document_index,
        snapshot,
        &[ChangedField::Inpainted],
    )
    .await
}
