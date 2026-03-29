mod runtime;

use koharu_core::FontFaceInfo;
use tracing::instrument;

use super::{
    AppResources,
    request::RenderJob,
    store::{self, ChangedField},
};

pub(crate) use runtime::RendererRuntime;

#[instrument(level = "info", skip_all)]
pub(crate) async fn render(state: AppResources, job: RenderJob) -> anyhow::Result<()> {
    let mut updated = store::read_doc(&state.state, job.document_index).await?;

    state.renderer.render(
        &mut updated,
        job.text_block_index,
        job.shader_effect.unwrap_or_default(),
        job.shader_stroke,
        job.font_family.as_deref(),
    )?;

    store::update_doc(
        &state.state,
        job.document_index,
        updated,
        &[ChangedField::TextBlocks, ChangedField::Rendered],
    )
    .await
}

pub(crate) async fn list_font_families(state: AppResources) -> anyhow::Result<Vec<FontFaceInfo>> {
    state.renderer.available_fonts()
}
