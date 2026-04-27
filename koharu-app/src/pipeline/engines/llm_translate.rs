//! LLM-driven translation. Collects `text` from every text node on the page,
//! sends them through the loaded LLM as tagged blocks, writes the parsed
//! translations back via `UpdateNode { TextDataPatch { translation } }`.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{NodeDataPatch, NodeId, NodePatch, Op, TextDataPatch};

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::text_nodes;
use crate::terminology::{protect_text, restore_text, system_prompt_with_terminology};

pub struct Model;

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        // Collect (node_id, source_text) for every text node with a non-empty `text`.
        let mut targets: Vec<(NodeId, String)> = Vec::new();
        for (id, _, text_data) in text_nodes(ctx.scene, ctx.page) {
            let Some(source) = text_data.text.as_ref() else {
                continue;
            };
            if source.trim().is_empty() {
                continue;
            }
            targets.push((id, source.clone()));
        }
        if targets.is_empty() {
            return Ok(Vec::new());
        }

        let protected = targets
            .iter()
            .map(|(_, source)| protect_text(source, &ctx.options.terminology))
            .collect::<Vec<_>>();
        let sources: Vec<String> = protected.iter().map(|item| item.text.clone()).collect();
        let system_prompt = system_prompt_with_terminology(
            ctx.options.system_prompt.as_deref(),
            ctx.options.target_language.as_deref(),
            &ctx.options.terminology,
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
        for (((node_id, _), protected), translation) in
            targets.into_iter().zip(protected).zip(translations)
        {
            let translation = restore_text(&translation, &protected.replacements);
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
