//! Koharu renderer engine. Rasterises each text node's translation into an
//! RGBA sprite, composites them onto the inpainted plane, and writes back:
//!
//! - per-block `UpdateNode { TextDataPatch { sprite, sprite_transform,
//!   rendered_direction, style } }` (sprite blob stored as raw RGBA)
//! - one `upsert Image { role: Rendered }` for the final composite (webp)
//!
//! Requires an `Image { role: Inpainted }` node on the page.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use koharu_core::{
    ImageRole, MaskRole, NodeDataPatch, NodePatch, Op, TextDataPatch, TextStyle, Transform,
};

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{
    find_image_node, find_mask_node, image_dimensions, load_source_image, text_nodes,
    upsert_image_blob,
};
use crate::renderer::{PageRenderOptions, RenderBlockInput};

pub struct Model;

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        // Find the target surface: prefer inpainted, fall back to source.
        let base = match find_image_node(ctx.scene, ctx.page, ImageRole::Inpainted) {
            Some((_, blob)) => ctx.blobs.load_image(&blob)?,
            None => load_source_image(ctx.scene, ctx.page, ctx.blobs)?,
        };
        let (w, h) = image_dimensions(&base);

        // Brush layer (optional): overlay before text sprites.
        let brush = match find_mask_node(ctx.scene, ctx.page, MaskRole::BrushInpaint) {
            Some((_, blob)) => Some(ctx.blobs.load_image(&blob)?),
            None => None,
        };

        // Bubble-interior mask (optional): grows latin layout boxes so text
        // wraps inside the available bubble space.
        let bubble = match find_mask_node(ctx.scene, ctx.page, MaskRole::Bubble) {
            Some((_, blob)) => Some(ctx.blobs.load_image(&blob)?),
            None => None,
        };

        // Build renderer input from every text node with a non-empty translation.
        let nodes = text_nodes(ctx.scene, ctx.page);
        let inputs: Vec<RenderBlockInput> = nodes
            .iter()
            .filter_map(|(id, transform, t)| {
                let translation = t.translation.as_ref()?.trim();
                if translation.is_empty() {
                    return None;
                }
                Some(RenderBlockInput {
                    node_id: *id,
                    transform: **transform,
                    translation: translation.to_string(),
                    style: t.style.clone(),
                    font_prediction: t.font_prediction.clone(),
                    source_direction: t.source_direction,
                    rendered_direction: t.rendered_direction,
                    lock_layout_box: t.lock_layout_box,
                })
            })
            .collect();

        if inputs.is_empty() {
            return Err(anyhow!("no text translations to render"));
        }

        let page_opts = PageRenderOptions {
            shader_effect: Default::default(),
            shader_stroke: None,
            document_font: ctx.options.default_font.clone(),
        };

        // `render_page` is synchronous and CPU-bound. It runs inline on the
        // current tokio worker; for multi-page jobs the driver parallelises
        // across pages via separate `run()` calls.
        let output = ctx.renderer.render_page(
            &base,
            brush.as_ref(),
            bubble.as_ref(),
            w,
            h,
            &inputs,
            &page_opts,
        )?;

        // Upload sprites + compose ops.
        let mut ops = Vec::with_capacity(output.blocks.len() + 1);
        for block_out in output.blocks {
            let sprite_ref = ctx.blobs.put_raw(&block_out.sprite)?;
            ops.push(Op::UpdateNode {
                page: ctx.page,
                id: block_out.node_id,
                patch: NodePatch {
                    data: Some(NodeDataPatch::Text(TextDataPatch {
                        sprite: Some(Some(sprite_ref)),
                        sprite_transform: Some(
                            block_out.expanded_transform.map(normalize_transform),
                        ),
                        rendered_direction: Some(Some(block_out.rendered_direction)),
                        // Persist an empty style so downstream re-renders skip
                        // re-deriving font families; mirrors legacy behaviour.
                        style: Some(Some(preserve_or_empty_style(
                            inputs
                                .iter()
                                .find(|i| i.node_id == block_out.node_id)
                                .and_then(|i| i.style.clone()),
                        ))),
                        ..Default::default()
                    })),
                    transform: None,
                    visible: None,
                },
                prev: NodePatch::default(),
            });
        }

        // Final composite → Image { Rendered } upsert.
        let final_blob = ctx.blobs.put_webp(&output.final_render)?;
        ops.push(upsert_image_blob(
            ctx.scene,
            ctx.page,
            ImageRole::Rendered,
            final_blob,
            w,
            h,
        ));
        Ok(ops)
    }
}

inventory::submit! {
    EngineInfo {
        id: "koharu-renderer",
        name: "Koharu Renderer",
        needs: &[
            Artifact::Inpainted,
            Artifact::Translations,
            Artifact::FontPredictions,
        ],
        produces: &[Artifact::FinalRender, Artifact::RenderedSprites],
        load: |_runtime, _cpu| Box::pin(async move {
            Ok(Box::new(Model) as Box<dyn Engine>)
        }),
    }
}

fn normalize_transform(t: Transform) -> Transform {
    Transform {
        x: t.x.round(),
        y: t.y.round(),
        width: t.width.round(),
        height: t.height.round(),
        rotation_deg: t.rotation_deg,
    }
}

fn preserve_or_empty_style(existing: Option<TextStyle>) -> TextStyle {
    existing.unwrap_or_else(|| TextStyle {
        font_families: Vec::new(),
        font_size: None,
        color: [0, 0, 0, 255],
        effect: None,
        stroke: None,
        text_align: None,
    })
}
