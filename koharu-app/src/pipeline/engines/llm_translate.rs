//! LLM-driven translation. Collects `text` from every text node on the page,
//! sends them through the loaded LLM as tagged blocks, writes the parsed
//! translations back via `UpdateNode { TextDataPatch { translation } }`.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{
    GlossaryEntry, NodeDataPatch, NodeId, NodePatch, Op, PageId, Scene, TextData, TextDataPatch,
    render_glossary_section,
};
use koharu_llm::{Language, prompt::system_prompt_base};

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
        let system_prompt = build_system_prompt(
            ctx.options.system_prompt.as_deref(),
            &ctx.options.glossary,
            ctx.options.target_language.as_deref(),
            &sources,
        );
        let translations = ctx
            .llm
            .translate_texts(
                &sources,
                ctx.options.target_language.as_deref(),
                system_prompt.as_deref(),
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

/// Compose the effective system prompt: the base prompt (custom or default)
/// followed by the glossary terms relevant to this page.
///
/// Returns `None` when there is no custom prompt and no applicable glossary, so
/// the LLM layer falls back to its built-in default exactly as before. When a
/// glossary applies, the base prompt is materialized here and the glossary is
/// inserted before the block-tag rules that the LLM layer appends last.
fn build_system_prompt(
    custom: Option<&str>,
    glossary: &[GlossaryEntry],
    target_language: Option<&str>,
    sources: &[String],
) -> Option<String> {
    let custom = custom.filter(|prompt| !prompt.trim().is_empty());

    let glossary_section = if glossary.is_empty() {
        None
    } else {
        render_glossary_section(glossary, &sources.join("\n"))
    };

    match glossary_section {
        None => custom.map(str::to_string),
        Some(section) => {
            let target = target_language
                .and_then(Language::parse)
                .unwrap_or(Language::English);
            let base = custom
                .map(str::to_string)
                .unwrap_or_else(|| system_prompt_base(target));
            Some(format!("{base}\n\n{section}"))
        }
    }
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

    fn glossary_entry(source: &str, target: &str) -> GlossaryEntry {
        GlossaryEntry {
            source: source.to_string(),
            target: target.to_string(),
            note: None,
            enabled: None,
        }
    }

    #[test]
    fn no_custom_prompt_and_no_glossary_keeps_default_fallback() {
        let prompt = build_system_prompt(None, &[], Some("en-US"), &["hello".to_string()]);
        assert!(prompt.is_none());
    }

    #[test]
    fn custom_prompt_without_glossary_is_passed_through() {
        let prompt =
            build_system_prompt(Some("be terse"), &[], Some("en-US"), &["hello".to_string()]);
        assert_eq!(prompt.as_deref(), Some("be terse"));
    }

    #[test]
    fn glossary_is_appended_to_default_base_prompt() {
        let glossary = vec![glossary_entry("春日", "Kasuga")];
        let prompt =
            build_system_prompt(None, &glossary, Some("en-US"), &["春日先輩".to_string()]).unwrap();
        assert!(prompt.contains("professional manga translator"));
        assert!(prompt.contains("春日 => Kasuga"));
    }

    #[test]
    fn glossary_is_appended_to_custom_prompt() {
        let glossary = vec![glossary_entry("春日", "Kasuga")];
        let prompt =
            build_system_prompt(Some("be terse"), &glossary, None, &["春日".to_string()]).unwrap();
        assert!(prompt.starts_with("be terse"));
        assert!(prompt.contains("春日 => Kasuga"));
    }

    #[test]
    fn irrelevant_glossary_terms_are_not_injected() {
        let glossary = vec![glossary_entry("海", "sea")];
        let prompt = build_system_prompt(None, &glossary, Some("en-US"), &["春日".to_string()]);
        assert!(prompt.is_none());
    }
}
