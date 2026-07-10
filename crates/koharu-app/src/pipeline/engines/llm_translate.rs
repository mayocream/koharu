//! LLM-driven translation. Collects `text` from every text node on the page,
//! sends them through the loaded LLM as tagged blocks, writes the parsed
//! translations back via `UpdateNode { TextDataPatch { translation } }`.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{NodeDataPatch, NodeId, NodePatch, Op, PageId, Scene, TextData, TextDataPatch};

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::text_nodes;

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

        let mut ops = Vec::with_capacity(targets.len());
        for ((node_id, _), translation) in targets.into_iter().zip(translations) {
            ops.push(Op::UpdateNode {
                page: ctx.page,
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
        Ok(ops)
    }
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
