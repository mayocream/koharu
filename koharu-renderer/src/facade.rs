use std::sync::{Arc, Mutex};

use anyhow::Result;
use image::{DynamicImage, GrayImage, imageops};
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

use koharu_types::{
    Document, SerializableDynamicImage, TextAlign, TextBlock, TextShaderEffect, TextStrokeStyle,
    TextStyle,
};

use crate::{
    font::{FamilyName, Font, FontBook, Properties},
    layout::{LayoutRun, TextLayout, WritingMode},
    renderer::{RenderOptions, RenderStrokeOptions, TinySkiaRenderer},
    text::{
        latin::{
            LayoutBox, expand_latin_layout_box_relaxed, expand_latin_layout_box_strict,
            is_expanded_layout_box, latin_layout_underfilled, latin_width_overflow_factor,
            layout_box_area, layout_box_from_block, pick_better_latin_candidate,
        },
        script::{
            font_families_for_text, is_latin_only, normalize_translation_for_layout,
            writing_mode_for_block,
        },
    },
};

pub struct Renderer {
    fontbook: Arc<Mutex<FontBook>>,
    renderer: TinySkiaRenderer,
    symbol_fallbacks: Vec<Font>,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        let mut fontbook = FontBook::new();
        let symbol_fallbacks = load_symbol_fallbacks(&mut fontbook);
        Ok(Self {
            fontbook: Arc::new(Mutex::new(fontbook)),
            renderer: TinySkiaRenderer::new()?,
            symbol_fallbacks,
        })
    }

    pub fn available_fonts(&self) -> Result<Vec<String>> {
        let mut fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock fontbook"))?;
        let mut families = fontbook.all_families();
        families.sort();
        Ok(families)
    }

    pub fn render(
        &self,
        document: &mut Document,
        text_block_index: Option<usize>,
        effect: TextShaderEffect,
        stroke: Option<TextStrokeStyle>,
        font_family: Option<&str>,
    ) -> Result<()> {
        let bubble_map = if let Some(inpainted) = &document.inpainted {
            inpainted.to_luma8()
        } else {
            document.image.to_luma8()
        };

        let mut text_blocks = match text_block_index {
            Some(index) => document
                .text_blocks
                .get_mut(index)
                .map(|tb| vec![tb])
                .ok_or_else(|| anyhow::anyhow!("Text block index out of bounds"))?,
            None => document.text_blocks.iter_mut().collect(),
        };

        text_blocks.par_iter_mut().for_each(|text_block| {
            let _ = self.render_text_block(
                text_block,
                effect,
                stroke.clone(),
                font_family,
                Some(&bubble_map),
            );
        });

        if let Some(inpainted) = &document.inpainted
            && text_block_index.is_none()
        {
            let mut rendered = inpainted.to_rgba8();

            if let Some(brush_layer) = &document.brush_layer {
                let brush = brush_layer.to_rgba8();
                imageops::overlay(&mut rendered, &brush, 0, 0);
            }

            for text_block in text_blocks {
                let Some(block) = text_block.rendered.as_ref() else {
                    continue;
                };
                imageops::overlay(
                    &mut rendered,
                    &block.0,
                    text_block.x as i64,
                    text_block.y as i64,
                );
            }
            document.rendered = Some(SerializableDynamicImage(DynamicImage::ImageRgba8(rendered)));
        }
        Ok(())
    }

    fn render_text_block(
        &self,
        text_block: &mut TextBlock,
        effect: TextShaderEffect,
        global_stroke: Option<TextStrokeStyle>,
        font_family: Option<&str>,
        bubble_map: Option<&GrayImage>,
    ) -> Result<()> {
        let Some(translation) = text_block.translation.as_ref().cloned() else {
            return Ok(());
        };
        if translation.is_empty() {
            return Ok(());
        };
        let normalized_translation = normalize_translation_for_layout(&translation);
        let (seed_x, seed_y, seed_width, seed_height) = text_block.seed_layout_box();
        let layout_source_block = TextBlock {
            x: seed_x,
            y: seed_y,
            width: seed_width,
            height: seed_height,
            translation: Some(translation.clone()),
            ..Default::default()
        };

        let mut style = text_block.style.clone().unwrap_or_else(|| TextStyle {
            font_families: font_families_for_text(&normalized_translation),
            font_size: None,
            color: [0, 0, 0, 255],
            effect: None,
            stroke: None,
            text_align: None,
        });

        apply_global_font_family(&mut style.font_families, font_family);
        apply_default_font_families(&mut style.font_families, &normalized_translation);
        let font = self.select_font(&style)?;
        let block_effect = style.effect.unwrap_or(effect);
        let color = text_block
            .style
            .as_ref()
            .map(|style| style.color)
            .or_else(|| {
                text_block.font_prediction.as_ref().map(|pred| {
                    [
                        pred.text_color[0],
                        pred.text_color[1],
                        pred.text_color[2],
                        255,
                    ]
                })
            })
            .unwrap_or([0, 0, 0, 255]);
        let writing_mode = writing_mode_for_block(&layout_source_block);
        let english_layout =
            english_layout_behavior(text_block, &normalized_translation, writing_mode);
        let english_horizontal_layout = english_layout != EnglishLayoutBehavior::Disabled;
        let auto_expand_english_layout = english_layout == EnglishLayoutBehavior::AutoExpand;
        let text_align = style.text_align.unwrap_or({
            if english_horizontal_layout {
                TextAlign::Center
            } else {
                TextAlign::Left
            }
        });
        let original_layout_box = layout_box_from_block(&layout_source_block);
        let mut layout_box = if auto_expand_english_layout {
            bubble_map
                .map(|map| expand_latin_layout_box_strict(&layout_source_block, map))
                .unwrap_or(original_layout_box)
        } else {
            original_layout_box
        };

        let build_layout = |box_for_layout: LayoutBox, allow_expanded_overflow: bool| {
            let expanded_box = is_expanded_layout_box(box_for_layout, original_layout_box);
            let overflow = if english_horizontal_layout {
                if expanded_box {
                    latin_width_overflow_factor(true, allow_expanded_overflow)
                } else {
                    latin_width_overflow_factor(false, allow_expanded_overflow)
                }
            } else {
                1.0
            };
            let max_width = if box_for_layout.width.is_finite() && box_for_layout.width > 0.0 {
                box_for_layout.width * overflow
            } else {
                box_for_layout.width
            };

            TextLayout::new(&font, None)
                .with_fallback_fonts(&self.symbol_fallbacks)
                .with_max_height(box_for_layout.height)
                .with_max_width(max_width)
                .with_writing_mode(writing_mode)
                .run(&normalized_translation)
        };

        let mut layout = build_layout(layout_box, false)?;
        if auto_expand_english_layout {
            let underfilled = latin_layout_underfilled(&layout, layout_box.height);
            if underfilled {
                let relaxed_box = bubble_map
                    .map(|map| expand_latin_layout_box_relaxed(&layout_source_block, map))
                    .unwrap_or(layout_box);
                let relaxed_candidate =
                    if layout_box_area(relaxed_box) > layout_box_area(layout_box) * 1.06 {
                        build_layout(relaxed_box, true)
                            .ok()
                            .map(|layout| (layout, relaxed_box))
                    } else {
                        None
                    };

                let overflow_candidate = build_layout(layout_box, true)
                    .ok()
                    .map(|layout| (layout, layout_box));
                if let Some((candidate_layout, candidate_box)) =
                    pick_better_latin_candidate(&layout, relaxed_candidate, overflow_candidate)
                {
                    layout = candidate_layout;
                    layout_box = candidate_box;
                }
            }

            center_layout_vertically(&mut layout, layout_box.height);
        }
        align_layout_horizontally(&mut layout, writing_mode, layout_box.width, text_align);

        let resolved_stroke = resolve_stroke_style(
            text_block,
            style.stroke.as_ref(),
            global_stroke.as_ref(),
            layout.font_size,
        );
        let rendered = self.renderer.render(
            &layout,
            writing_mode,
            &RenderOptions {
                font_size: layout.font_size,
                color,
                effect: block_effect,
                stroke: resolved_stroke,
                ..Default::default()
            },
        )?;

        text_block.x = layout_box.x;
        text_block.y = layout_box.y;
        text_block.width = layout_box.width;
        text_block.height = layout_box.height;
        text_block.rendered = Some(SerializableDynamicImage(DynamicImage::ImageRgba8(rendered)));
        Ok(())
    }

    fn select_font(&self, style: &TextStyle) -> Result<Font> {
        let mut fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock fontbook"))?;
        let font = fontbook.query(
            style
                .font_families
                .iter()
                .map(|family| FamilyName::Title(family.to_string()))
                .collect::<Vec<_>>()
                .as_slice(),
            &Properties::default(),
        )?;
        Ok(font)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnglishLayoutBehavior {
    Disabled,
    AutoExpand,
    LockedToManualSize,
}

fn english_layout_behavior(
    text_block: &TextBlock,
    normalized_translation: &str,
    writing_mode: WritingMode,
) -> EnglishLayoutBehavior {
    let is_english_horizontal =
        writing_mode == WritingMode::Horizontal && is_latin_only(normalized_translation);
    if !is_english_horizontal {
        return EnglishLayoutBehavior::Disabled;
    }

    if text_block.lock_layout_box {
        EnglishLayoutBehavior::LockedToManualSize
    } else {
        EnglishLayoutBehavior::AutoExpand
    }
}

fn default_stroke_width(font_size: f32) -> f32 {
    (font_size * 0.10).clamp(1.2, 8.0)
}

fn apply_global_font_family(font_families: &mut Vec<String>, font_family: Option<&str>) {
    if font_families.is_empty()
        && let Some(font_family) = font_family
    {
        font_families.push(font_family.to_string());
    }
}

fn apply_default_font_families(font_families: &mut Vec<String>, text: &str) {
    if font_families.is_empty() {
        *font_families = font_families_for_text(text);
    }
}

fn resolve_stroke_style(
    block: &TextBlock,
    block_stroke: Option<&TextStrokeStyle>,
    global_stroke: Option<&TextStrokeStyle>,
    font_size: f32,
) -> Option<RenderStrokeOptions> {
    if let Some(stroke) = block_stroke {
        if !stroke.enabled {
            return None;
        }
        return Some(RenderStrokeOptions {
            color: stroke.color,
            width_px: stroke
                .width_px
                .unwrap_or_else(|| default_stroke_width(font_size)),
        });
    }

    if let Some(stroke) = global_stroke {
        if !stroke.enabled {
            return None;
        }
        return Some(RenderStrokeOptions {
            color: stroke.color,
            width_px: stroke
                .width_px
                .unwrap_or_else(|| default_stroke_width(font_size)),
        });
    }

    if let Some(pred) = &block.font_prediction
        && pred.stroke_width_px > 0.0
    {
        return Some(RenderStrokeOptions {
            color: [
                pred.stroke_color[0],
                pred.stroke_color[1],
                pred.stroke_color[2],
                255,
            ],
            width_px: pred.stroke_width_px,
        });
    }

    Some(RenderStrokeOptions {
        color: [255, 255, 255, 255],
        width_px: default_stroke_width(font_size),
    })
}

fn align_layout_horizontally(
    layout: &mut LayoutRun<'_>,
    writing_mode: WritingMode,
    container_width: f32,
    text_align: TextAlign,
) {
    if !container_width.is_finite() || container_width <= 0.0 {
        return;
    }

    let target_width = layout.width.max(container_width);
    if writing_mode.is_vertical() {
        let remaining = (container_width - layout.width).max(0.0);
        let offset = match text_align {
            TextAlign::Left => 0.0,
            TextAlign::Center => remaining * 0.5,
            TextAlign::Right => remaining,
        };
        if offset > 0.0 {
            for line in &mut layout.lines {
                line.baseline.0 += offset;
            }
        }
        layout.width = target_width;
        return;
    }

    for line in &mut layout.lines {
        if line.advance <= 0.0 {
            continue;
        }
        let remaining = (container_width - line.advance).max(0.0);
        let offset = match text_align {
            TextAlign::Left => 0.0,
            TextAlign::Center => remaining * 0.5,
            TextAlign::Right => remaining,
        };
        if offset > 0.0 {
            line.baseline.0 += offset;
        }
    }
    layout.width = target_width;
}

fn center_layout_vertically(layout: &mut LayoutRun<'_>, container_height: f32) {
    if !container_height.is_finite() || container_height <= 0.0 || layout.lines.is_empty() {
        return;
    }
    let offset = ((container_height - layout.height) * 0.5).max(0.0);
    if offset <= 0.0 {
        return;
    }

    for line in &mut layout.lines {
        line.baseline.1 += offset;
    }
    layout.height = layout.height.max(container_height);
}

fn load_symbol_fallbacks(fontbook: &mut FontBook) -> Vec<Font> {
    let props = Properties::default();
    let candidates = [
        "Segoe UI Symbol",
        "Segoe UI Emoji",
        "Noto Sans Symbols",
        "Noto Sans Symbols2",
        "Noto Color Emoji",
        "Apple Color Emoji",
        "Apple Symbols",
        "Symbola",
        "Arial Unicode MS",
    ];
    let mut fonts = Vec::new();
    for name in candidates {
        if let Ok(font) = fontbook.query(&[FamilyName::Title(name.to_string())], &props) {
            fonts.push(font);
        }
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::{
        EnglishLayoutBehavior, align_layout_horizontally, apply_default_font_families,
        apply_global_font_family, center_layout_vertically, english_layout_behavior,
    };
    use crate::layout::{LayoutLine, LayoutRun, WritingMode};
    use koharu_types::{TextAlign, TextBlock};

    #[test]
    fn horizontal_alignment_offsets_each_line() {
        let mut layout = LayoutRun {
            lines: vec![
                LayoutLine {
                    advance: 40.0,
                    baseline: (0.0, 10.0),
                    ..Default::default()
                },
                LayoutLine {
                    advance: 80.0,
                    baseline: (0.0, 30.0),
                    ..Default::default()
                },
            ],
            width: 80.0,
            height: 40.0,
            font_size: 16.0,
        };

        align_layout_horizontally(
            &mut layout,
            WritingMode::Horizontal,
            100.0,
            TextAlign::Center,
        );

        assert_eq!(layout.lines[0].baseline.0, 30.0);
        assert_eq!(layout.lines[1].baseline.0, 10.0);
        assert_eq!(layout.width, 100.0);
    }

    #[test]
    fn right_alignment_uses_full_remaining_width() {
        let mut layout = LayoutRun {
            lines: vec![LayoutLine {
                advance: 40.0,
                baseline: (0.0, 10.0),
                ..Default::default()
            }],
            width: 40.0,
            height: 20.0,
            font_size: 16.0,
        };

        align_layout_horizontally(
            &mut layout,
            WritingMode::Horizontal,
            100.0,
            TextAlign::Right,
        );

        assert_eq!(layout.lines[0].baseline.0, 60.0);
    }

    #[test]
    fn vertical_alignment_offsets_all_columns_as_a_group() {
        let mut layout = LayoutRun {
            lines: vec![
                LayoutLine {
                    baseline: (10.0, 12.0),
                    ..Default::default()
                },
                LayoutLine {
                    baseline: (30.0, 12.0),
                    ..Default::default()
                },
            ],
            width: 40.0,
            height: 80.0,
            font_size: 16.0,
        };

        align_layout_horizontally(
            &mut layout,
            WritingMode::VerticalRl,
            100.0,
            TextAlign::Center,
        );

        assert_eq!(layout.lines[0].baseline.0, 40.0);
        assert_eq!(layout.lines[1].baseline.0, 60.0);
        assert_eq!(layout.width, 100.0);
    }

    #[test]
    fn vertical_centering_preserves_existing_behavior() {
        let mut layout = LayoutRun {
            lines: vec![LayoutLine {
                advance: 40.0,
                baseline: (0.0, 12.0),
                ..Default::default()
            }],
            width: 40.0,
            height: 20.0,
            font_size: 16.0,
        };

        center_layout_vertically(&mut layout, 60.0);

        assert_eq!(layout.lines[0].baseline.1, 32.0);
        assert_eq!(layout.height, 60.0);
    }

    #[test]
    fn explicit_block_font_should_not_be_overridden_by_global_font() {
        let mut font_families = vec!["Block Font".to_string()];
        apply_global_font_family(&mut font_families, Some("Global Font"));

        assert_eq!(font_families, vec!["Block Font".to_string()]);
    }

    #[test]
    fn global_font_should_fill_empty_block_font_list() {
        let mut font_families = Vec::new();
        apply_global_font_family(&mut font_families, Some("Global Font"));
        assert_eq!(font_families, vec!["Global Font".to_string()]);
    }

    #[test]
    fn default_font_families_should_fill_empty_list() {
        let mut font_families = Vec::new();
        apply_default_font_families(&mut font_families, "hello");
        assert!(!font_families.is_empty());
    }

    #[test]
    fn english_layout_auto_expands_by_default() {
        let block = TextBlock::default();
        let behavior = english_layout_behavior(&block, "HELLO WORLD", WritingMode::Horizontal);
        assert_eq!(behavior, EnglishLayoutBehavior::AutoExpand);
    }

    #[test]
    fn english_layout_stops_auto_expand_after_manual_resize() {
        let block = TextBlock {
            lock_layout_box: true,
            ..Default::default()
        };
        let behavior = english_layout_behavior(&block, "HELLO WORLD", WritingMode::Horizontal);
        assert_eq!(behavior, EnglishLayoutBehavior::LockedToManualSize);
    }

    #[test]
    fn non_english_layout_never_uses_english_expansion_logic() {
        let block = TextBlock::default();
        let behavior = english_layout_behavior(&block, "こんにちは", WritingMode::Horizontal);
        assert_eq!(behavior, EnglishLayoutBehavior::Disabled);
    }
}
