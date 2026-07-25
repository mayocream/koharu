//! LLM-driven translation. Collects `text` from every text node on the page,
//! sends them through the loaded LLM as tagged blocks, writes the parsed
//! translations back via `UpdateNode { TextDataPatch { translation } }`.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{NodeDataPatch, NodeId, NodePatch, Op, PageId, Scene, TextData, TextDataPatch};

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{ConcurrencyHint, Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::text_nodes;

/// Pages folded into one request when the translator supports it. Bounded
/// because the whole group shares a context window — and because a rejected
/// batch costs a full re-translation of every page in it.
const MAX_BATCH_PAGES: usize = 4;

/// Concurrent in-flight requests against a remote provider.
const REMOTE_WORKERS: usize = 4;

pub struct Model;

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let targets = collect_translation_targets(&ctx);
        if targets.is_empty() {
            return Ok(Vec::new());
        }

        let sources: Vec<String> = targets.iter().map(|(_, s)| s.clone()).collect();
        let translations = ctx
            .llm
            .translate_texts(
                &sources,
                ctx.options.target_language.as_deref(),
                ctx.options.system_prompt.as_deref(),
            )
            .await?;

        Ok(translation_ops(ctx.page, targets, translations))
    }

    /// Remote providers are network-bound and stateless, so requests overlap.
    /// A local llama.cpp context is `&mut` and single-use — fanning it out
    /// would only queue on the state lock.
    fn max_workers(&self, hint: &ConcurrencyHint) -> usize {
        if hint.translator_is_remote {
            REMOTE_WORKERS
        } else {
            1
        }
    }

    /// Batching here is one combined *request*, not a tensor batch. It only
    /// pays against a remote provider, where the win is collapsing N network
    /// round-trips into one. A custom system prompt disables it: that prompt
    /// documents the single-page `[N]` tags and can't be assumed to teach the
    /// page-qualified form the response is validated against.
    fn max_batch(&self, hint: &ConcurrencyHint) -> usize {
        if hint.translator_is_remote && !hint.custom_system_prompt {
            MAX_BATCH_PAGES.min(hint.max_batch_pages)
        } else {
            1
        }
    }

    async fn run_batch(&self, ctxs: Vec<EngineCtx<'_>>) -> Vec<Result<Vec<Op>>> {
        if ctxs.len() <= 1 {
            let mut out = Vec::with_capacity(ctxs.len());
            for ctx in ctxs {
                out.push(self.run(ctx).await);
            }
            return out;
        }

        let per_page: Vec<Vec<(NodeId, String)>> =
            ctxs.iter().map(collect_translation_targets).collect();
        let sources: Vec<Vec<String>> = per_page
            .iter()
            .map(|targets| targets.iter().map(|(_, s)| s.clone()).collect())
            .collect();

        // Options are uniform across a run, so page 0's are representative.
        let options = ctxs[0].options;
        let translated = ctxs[0]
            .llm
            .translate_pages(
                &sources,
                options.target_language.as_deref(),
                options.system_prompt.as_deref(),
            )
            .await;

        match translated {
            Ok(pages) => ctxs
                .iter()
                .zip(per_page)
                .zip(pages)
                .map(|((ctx, targets), translations)| {
                    Ok(translation_ops(ctx.page, targets, translations))
                })
                .collect(),
            // The whole request failed (network, no LLM loaded, …). Report it
            // against every page in the batch rather than silently dropping
            // pages the driver still expects an answer for.
            Err(err) => ctxs
                .iter()
                .map(|_| Err(anyhow::anyhow!("{err:#}")))
                .collect(),
        }
    }
}

fn translation_ops(
    page: PageId,
    targets: Vec<(NodeId, String)>,
    translations: Vec<String>,
) -> Vec<Op> {
    let mut ops = Vec::with_capacity(targets.len());
    for ((node_id, _), translation) in targets.into_iter().zip(translations) {
        ops.push(Op::UpdateNode {
            page,
            id: node_id,
            patch: NodePatch {
                data: Some(NodeDataPatch::Text(TextDataPatch {
                    translation: Some(Some(translation)),
                    ..Default::default()
                })),
                transform: None,
                visible: None,
            },
            prev: NodePatch::default(),
        });
    }
    ops
}

fn collect_translation_targets(ctx: &EngineCtx<'_>) -> Vec<(NodeId, String)> {
    collect_translation_targets_from(ctx.scene, ctx.page, ctx.options.text_node_ids.as_deref())
}

fn collect_translation_targets_from(
    scene: &Scene,
    page: PageId,
    allowed_ids: Option<&[NodeId]>,
) -> Vec<(NodeId, String)> {
    text_nodes(scene, page)
        .into_iter()
        .filter(|(id, _, text_data)| should_translate(*id, text_data, allowed_ids))
        .filter_map(|(id, _, text_data)| text_data.text.as_ref().map(|source| (id, source.clone())))
        .collect()
}

fn should_translate(id: NodeId, text_data: &TextData, allowed_ids: Option<&[NodeId]>) -> bool {
    if let Some(ids) = allowed_ids
        && !ids.contains(&id)
    {
        return false;
    }
    text_data
        .text
        .as_ref()
        .is_some_and(|source| !source.trim().is_empty())
}

inventory::submit! {
    EngineInfo {
        id: "llm",
        name: "LLM",
        needs: &[Artifact::OcrText],
        produces: &[Artifact::Translations],
        load: |_runtime, _cpu| Box::pin(async move {
            Ok(Box::new(Model) as Box<dyn Engine>)
        }),
    }
}

#[cfg(test)]
mod tests {
    use koharu_core::{Node, NodeKind, Page, PageId, Scene, TextData, Transform};
    use uuid::Uuid;

    use super::*;

    fn node_id(value: u128) -> NodeId {
        NodeId(Uuid::from_u128(value))
    }

    fn page_id() -> PageId {
        PageId(Uuid::from_u128(1))
    }

    fn text_node(id: NodeId, text: Option<&str>) -> Node {
        Node {
            id,
            transform: Transform::default(),
            visible: true,
            kind: NodeKind::Text(TextData {
                text: text.map(str::to_string),
                ..Default::default()
            }),
        }
    }

    fn scene_with_texts(nodes: Vec<Node>) -> Scene {
        let page_id = page_id();
        let mut page = Page::new("page", 100, 100);
        page.id = page_id;
        page.nodes = nodes.into_iter().map(|node| (node.id, node)).collect();
        let mut scene = Scene::default();
        scene.pages.insert(page_id, page);
        scene
    }

    #[test]
    fn should_translate_only_requested_nodes() {
        let first = node_id(11);
        let second = node_id(22);
        let scene = scene_with_texts(vec![
            text_node(first, Some("first")),
            text_node(second, Some("second")),
        ]);
        let options = crate::PipelineRunOptions {
            text_node_ids: Some(vec![second]),
            ..Default::default()
        };

        let targets =
            collect_translation_targets_from(&scene, page_id(), options.text_node_ids.as_deref());

        assert_eq!(targets, vec![(second, "second".to_string())]);
    }

    #[test]
    fn should_ignore_requested_nodes_without_ocr_text() {
        let blank = node_id(33);
        let scene = scene_with_texts(vec![
            text_node(blank, Some("   ")),
            text_node(node_id(44), Some("translated")),
        ]);
        let options = crate::PipelineRunOptions {
            text_node_ids: Some(vec![blank]),
            ..Default::default()
        };

        let targets =
            collect_translation_targets_from(&scene, page_id(), options.text_node_ids.as_deref());

        assert!(targets.is_empty());
    }
}
