//! LLM-driven translation. Collects `text` from every text node on the page,
//! sends them through the loaded LLM as tagged blocks, writes the parsed
//! translations back via `UpdateNode { TextDataPatch { translation } }`.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{NodeDataPatch, NodeId, NodePatch, Op, PageId, Scene, TextData, TextDataPatch};

use crate::llm::TranslationContextEntry;
use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo, TranslationContextConfig};
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
        let translations = if ctx.options.translation_context.enabled
            && ctx.llm.translation_context_supported().await
        {
            let contexts = targets
                .iter()
                .map(|(node_id, _)| {
                    collect_translation_context(
                        ctx.scene,
                        ctx.page,
                        *node_id,
                        &ctx.options.translation_context,
                    )
                })
                .collect::<Vec<_>>();
            ctx.llm
                .translate_texts_with_contexts(
                    &sources,
                    &contexts,
                    ctx.options.target_language.as_deref(),
                    ctx.options.system_prompt.as_deref(),
                )
                .await?
        } else {
            ctx.llm
                .translate_texts(
                    &sources,
                    ctx.options.target_language.as_deref(),
                    ctx.options.system_prompt.as_deref(),
                )
                .await?
        };

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

fn collect_translation_context(
    scene: &Scene,
    page: PageId,
    target: NodeId,
    config: &TranslationContextConfig,
) -> Vec<TranslationContextEntry> {
    if !config.enabled || config.previous_blocks == 0 || config.max_context_chars == 0 {
        return Vec::new();
    }

    // `Scene.pages` is an IndexMap, so insertion order is the project page
    // order. `text_nodes` keeps page node insertion/stacking order, which is
    // the closest reliable reading-order signal after detector sorting.
    let page_ids = scene.pages.keys().copied().collect::<Vec<_>>();
    let Some(page_index) = page_ids.iter().position(|id| *id == page) else {
        return Vec::new();
    };
    let current_nodes = text_nodes(scene, page);
    let Some(target_block_index) = current_nodes
        .iter()
        .position(|(node_id, _, _)| *node_id == target)
    else {
        return Vec::new();
    };

    let mut nearest_first = Vec::new();
    for block_index in (0..target_block_index).rev() {
        if let Some(entry) = context_entry_from_text_node(
            page_index,
            block_index,
            current_nodes[block_index].2,
            config.include_previous_translations,
        ) {
            nearest_first.push(entry);
        }
        if nearest_first.len() >= config.previous_blocks {
            break;
        }
    }

    if nearest_first.len() < config.previous_blocks && config.previous_pages > 0 {
        let first_page = page_index.saturating_sub(config.previous_pages);
        for previous_page_index in (first_page..page_index).rev() {
            let previous_nodes = text_nodes(scene, page_ids[previous_page_index]);
            for block_index in (0..previous_nodes.len()).rev() {
                if let Some(entry) = context_entry_from_text_node(
                    previous_page_index,
                    block_index,
                    previous_nodes[block_index].2,
                    config.include_previous_translations,
                ) {
                    nearest_first.push(entry);
                }
                if nearest_first.len() >= config.previous_blocks {
                    break;
                }
            }
            if nearest_first.len() >= config.previous_blocks {
                break;
            }
        }
    }

    trim_context_window(nearest_first, config.max_context_chars)
}

fn context_entry_from_text_node(
    page_index: usize,
    block_index: usize,
    text_data: &TextData,
    include_translation: bool,
) -> Option<TranslationContextEntry> {
    let source_text = text_data.text.as_deref()?.trim();
    if source_text.is_empty() {
        return None;
    }

    Some(TranslationContextEntry {
        page_index,
        block_index,
        source_text: source_text.to_string(),
        translated_text: include_translation
            .then(|| text_data.translation.as_deref().map(str::trim))
            .flatten()
            .filter(|translation| !translation.is_empty())
            .map(ToOwned::to_owned),
    })
}

fn trim_context_window(
    nearest_first: Vec<TranslationContextEntry>,
    max_context_chars: usize,
) -> Vec<TranslationContextEntry> {
    let mut selected = Vec::new();
    let mut used_chars = 0usize;
    for entry in nearest_first {
        let entry_chars = context_entry_chars(&entry);
        if !selected.is_empty() && used_chars + entry_chars > max_context_chars {
            break;
        }
        used_chars += entry_chars;
        selected.push(entry);
    }

    // Collection is nearest-to-farthest so budget trimming preserves the
    // closest context. The prompt is easier to read in chronological order.
    selected.reverse();
    selected
}

fn context_entry_chars(entry: &TranslationContextEntry) -> usize {
    entry.source_text.chars().count()
        + entry
            .translated_text
            .as_deref()
            .map(|text| text.chars().count())
            .unwrap_or(0)
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

    fn page_id_from(value: u128) -> PageId {
        PageId(Uuid::from_u128(value))
    }

    fn page_id() -> PageId {
        page_id_from(1)
    }

    fn text_node(id: NodeId, text: Option<&str>) -> Node {
        text_node_with_translation(id, text, None)
    }

    fn text_node_with_translation(
        id: NodeId,
        text: Option<&str>,
        translation: Option<&str>,
    ) -> Node {
        Node {
            id,
            transform: Transform::default(),
            visible: true,
            kind: NodeKind::Text(TextData {
                text: text.map(str::to_string),
                translation: translation.map(str::to_string),
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

    fn scene_with_pages(pages: Vec<(PageId, Vec<Node>)>) -> Scene {
        let mut scene = Scene::default();
        for (page_id, nodes) in pages {
            let mut page = Page::new("page", 100, 100);
            page.id = page_id;
            page.nodes = nodes.into_iter().map(|node| (node.id, node)).collect();
            scene.pages.insert(page_id, page);
        }
        scene
    }

    fn context_config() -> TranslationContextConfig {
        TranslationContextConfig {
            enabled: true,
            previous_blocks: 6,
            previous_pages: 1,
            include_previous_translations: true,
            max_context_chars: 4000,
        }
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

    #[test]
    fn disabled_context_collects_nothing() {
        let target = node_id(2);
        let scene = scene_with_texts(vec![
            text_node(node_id(1), Some("previous")),
            text_node(target, Some("current")),
        ]);

        let context = collect_translation_context(
            &scene,
            page_id(),
            target,
            &TranslationContextConfig::default(),
        );

        assert!(context.is_empty());
    }

    #[test]
    fn previous_blocks_limits_context_count() {
        let target = node_id(4);
        let scene = scene_with_texts(vec![
            text_node(node_id(1), Some("a")),
            text_node(node_id(2), Some("b")),
            text_node(node_id(3), Some("c")),
            text_node(target, Some("d")),
        ]);
        let config = TranslationContextConfig {
            previous_blocks: 2,
            ..context_config()
        };

        let context = collect_translation_context(&scene, page_id(), target, &config);

        assert_eq!(
            context
                .iter()
                .map(|entry| entry.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "c"]
        );
    }

    #[test]
    fn previous_pages_collects_from_nearest_prior_page() {
        let page1 = page_id_from(1);
        let page2 = page_id_from(2);
        let target = node_id(5);
        let scene = scene_with_pages(vec![
            (
                page1,
                vec![
                    text_node(node_id(1), Some("p1-a")),
                    text_node(node_id(2), Some("p1-b")),
                    text_node(node_id(3), Some("p1-c")),
                ],
            ),
            (page2, vec![text_node(target, Some("p2-a"))]),
        ]);
        let config = TranslationContextConfig {
            previous_blocks: 2,
            previous_pages: 1,
            ..context_config()
        };

        let context = collect_translation_context(&scene, page2, target, &config);

        assert_eq!(
            context
                .iter()
                .map(|entry| (
                    entry.page_index,
                    entry.block_index,
                    entry.source_text.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![(0, 1, "p1-b"), (0, 2, "p1-c")]
        );
    }

    #[test]
    fn can_exclude_previous_translations() {
        let target = node_id(2);
        let scene = scene_with_texts(vec![
            text_node_with_translation(node_id(1), Some("raw"), Some("translated")),
            text_node(target, Some("current")),
        ]);
        let config = TranslationContextConfig {
            include_previous_translations: false,
            ..context_config()
        };

        let context = collect_translation_context(&scene, page_id(), target, &config);

        assert_eq!(context[0].source_text, "raw");
        assert_eq!(context[0].translated_text, None);
    }

    #[test]
    fn can_include_previous_translations() {
        let target = node_id(2);
        let scene = scene_with_texts(vec![
            text_node_with_translation(node_id(1), Some("raw"), Some("translated")),
            text_node(target, Some("current")),
        ]);

        let context = collect_translation_context(&scene, page_id(), target, &context_config());

        assert_eq!(context[0].source_text, "raw");
        assert_eq!(context[0].translated_text.as_deref(), Some("translated"));
    }

    #[test]
    fn max_context_chars_keeps_nearest_context() {
        let target = node_id(3);
        let scene = scene_with_texts(vec![
            text_node(node_id(1), Some("farfar")),
            text_node(node_id(2), Some("near")),
            text_node(target, Some("current")),
        ]);
        let config = TranslationContextConfig {
            previous_blocks: 2,
            max_context_chars: 4,
            ..context_config()
        };

        let context = collect_translation_context(&scene, page_id(), target, &config);

        assert_eq!(
            context
                .iter()
                .map(|entry| entry.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["near"]
        );

        let tiny_config = TranslationContextConfig {
            previous_blocks: 2,
            max_context_chars: 1,
            ..context_config()
        };
        let tiny_context = collect_translation_context(&scene, page_id(), target, &tiny_config);

        assert_eq!(
            tiny_context
                .iter()
                .map(|entry| entry.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["near"]
        );
    }

    #[test]
    fn current_block_is_not_context() {
        let target = node_id(2);
        let scene = scene_with_texts(vec![
            text_node(node_id(1), Some("previous")),
            text_node(target, Some("current")),
        ]);

        let context = collect_translation_context(&scene, page_id(), target, &context_config());

        assert_eq!(context.len(), 1);
        assert!(context.iter().all(|entry| entry.source_text != "current"));
    }

    #[test]
    fn missing_history_context_is_empty() {
        let target = node_id(1);
        let scene = scene_with_texts(vec![text_node(target, Some("current"))]);

        let context = collect_translation_context(&scene, page_id(), target, &context_config());

        assert!(context.is_empty());
    }
}
