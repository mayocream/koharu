//! Text layout and Vello glyph recording.

use std::sync::Arc;

use anyhow::Result;
use koharu_scene::{
    FontStyle as SceneFontStyle, LanguageTag, TextAlignment, TextLayout as AuthoredTextLayout,
    TextLayoutKind, Typography, VerticalAlignment as SceneVerticalAlignment,
    WritingMode as SceneWritingMode,
};
use vello::{
    FontEmbolden, Glyph, Scene,
    kurbo::{Affine, Diagonal2, Join, Stroke},
    peniko::Fill,
};

use crate::{
    Error, HyphenationPolicy, LayoutRun, RenderBounds, RenderDiagnostic, Result as RenderResult,
    TextAlign, TextLayout, WritingMode,
    bubble::LayoutBox,
    fonts::{Fonts, font_key},
    rasterizer::rgba,
    script::is_cjk_text,
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextNodeDescriptor {
    pub(crate) entity: koharu_scene::EntityId,
    pub(crate) text: String,
    pub(crate) language: Option<LanguageTag>,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) balloon_contour: Option<Vec<(f32, f32)>>,
    pub(crate) layout: AuthoredTextLayout,
    pub(crate) typography: Typography,
}

pub(crate) struct RenderedTextNode {
    pub(crate) scene: Arc<Scene>,
    pub(crate) local_bounds: RenderBounds,
    pub(crate) metadata: RenderedTextNodeMetadata,
    pub(crate) diagnostics: Vec<RenderDiagnostic>,
}

pub(crate) struct RenderedTextNodeMetadata {
    pub(crate) rendered_bounds: RenderBounds,
    pub(crate) layout_bounds: RenderBounds,
    pub(crate) post_script_fonts: Vec<String>,
    pub(crate) font_size: f32,
    pub(crate) color: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeOptions {
    pub color: [u8; 4],
    pub width_px: f32,
}

/// Paint options used when recording one laid-out text run into a Vello scene.
#[derive(Debug, Clone)]
pub struct TextRenderOptions {
    pub color: [u8; 4],
    pub hint_glyphs: bool,
    pub padding: f32,
    pub baseline_shift: f32,
    pub stroke: Option<StrokeOptions>,
}

impl Default for TextRenderOptions {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 255],
            hint_glyphs: true,
            padding: 0.0,
            baseline_shift: 0.0,
            stroke: None,
        }
    }
}

/// Shapes text and records the resulting glyphs into vector scenes.
#[derive(Clone, Copy, Debug, Default)]
pub struct TextRenderer;

impl TextRenderer {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn layout<'a>(&self, builder: &TextLayout<'a>, text: &str) -> Result<LayoutRun<'a>> {
        builder.run(text)
    }

    pub fn render(
        &self,
        scene: &mut Scene,
        layout: &LayoutRun<'_>,
        writing_mode: WritingMode,
        options: &TextRenderOptions,
        transform: Affine,
    ) {
        if let Some(stroke) = options
            .stroke
            .filter(|stroke| stroke.width_px > 0.0 && stroke.color[3] > 0)
        {
            draw_layout(
                scene,
                layout,
                writing_mode,
                options,
                transform,
                DrawStyle::Stroke(stroke),
            );
        }
        draw_layout(
            scene,
            layout,
            writing_mode,
            options,
            transform,
            DrawStyle::Fill,
        );
    }

    pub(crate) fn render_descriptor(
        &self,
        descriptor: &TextNodeDescriptor,
        fonts: &Fonts,
    ) -> RenderResult<RenderedTextNode> {
        let entity = descriptor.entity;
        let is_bubble_text = descriptor.balloon_contour.is_some();
        let frame = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: descriptor.width,
            height: descriptor.height,
        };
        let bounds = inset(frame, descriptor.layout.insets);
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Err(Error::invalid(format!(
                "text inset leaves no layout area for entity {}",
                entity
            )));
        }
        let writing_mode = match descriptor.typography.writing_mode {
            SceneWritingMode::Horizontal => WritingMode::Horizontal,
            SceneWritingMode::Vertical => WritingMode::VerticalRl,
        };
        let alignment = match descriptor.typography.alignment {
            TextAlignment::Start => TextAlign::Left,
            TextAlignment::Center => TextAlign::Center,
            TextAlignment::End => TextAlign::Right,
            TextAlignment::Justify => TextAlign::Justify,
        };
        let font_style = match descriptor.typography.font_style {
            SceneFontStyle::Normal => crate::FontStyle::Normal,
            SceneFontStyle::Italic => crate::FontStyle::Italic,
            SceneFontStyle::Oblique => crate::FontStyle::Oblique,
        };
        let fonts = fonts
            .resolve(
                None,
                Some(descriptor.typography.font_weight),
                Some(font_style),
                &descriptor.typography.font_families,
                &descriptor.text,
                descriptor
                    .language
                    .as_ref()
                    .map(koharu_scene::LanguageTag::as_str),
            )
            .map_err(|source| Error::Font { entity, source })?;
        let maximum = descriptor.typography.size;
        let minimum = descriptor.typography.minimum_size.min(maximum);
        let mut layout = TextLayout::new(&fonts[0])
            .with_fallback_fonts(&fonts[1..])
            .with_writing_mode(writing_mode)
            .with_alignment(alignment)
            .with_line_height(descriptor.typography.line_height)
            .with_spacing(
                descriptor.typography.letter_spacing,
                descriptor.typography.word_spacing,
            )
            .with_compact_emphasis_punctuation(
                is_cjk_text(&descriptor.text)
                    || descriptor
                        .language
                        .as_ref()
                        .is_some_and(|language| is_cjk_language(language.as_str())),
            );
        let point_text = descriptor.layout.kind == TextLayoutKind::Point;
        if !point_text {
            layout = layout
                .with_max_width(bounds.width)
                .with_max_height(bounds.height);
        }
        if let Some(contour) = &descriptor.balloon_contour {
            let [top, _, _, left] = descriptor.layout.insets;
            layout = layout.with_comic_balloon(
                bounds.width,
                bounds.height,
                contour.iter().map(|&(x, y)| (x - left, y - top)).collect(),
                match descriptor.layout.vertical_alignment {
                    SceneVerticalAlignment::Top => 0.0,
                    SceneVerticalAlignment::Center => 0.5,
                    SceneVerticalAlignment::Bottom => 1.0,
                },
                descriptor.layout.insets.into_iter().fold(0.0, f32::max),
            );
        }
        if let Some(language) = &descriptor.language {
            layout = layout.with_hyphenation_language_tag(language.as_str());
        }
        if is_bubble_text && writing_mode == WritingMode::Horizontal {
            layout = layout.with_hyphenation_policy(HyphenationPolicy::LastResort);
        }
        let layout = if descriptor.typography.auto_fit && !point_text {
            layout
                .with_max_font_size(maximum)
                .with_min_font_size(minimum)
        } else {
            layout.with_font_size(maximum)
        };
        let layout = self
            .layout(&layout, &descriptor.text)
            .map_err(|source| Error::Layout { entity, source })?;
        let (mut x, mut y) = if point_text {
            (bounds.x, bounds.y)
        } else {
            placement(
                bounds,
                layout.width,
                layout.height,
                descriptor.layout.vertical_alignment,
            )
        };
        x += layout.placement_offset_x();
        y += layout.placement_offset_y();
        let transform = Affine::translate((f64::from(x), f64::from(y)));
        let color = descriptor.typography.color;
        let mut options = TextRenderOptions {
            color,
            stroke: None,
            ..TextRenderOptions::default()
        };
        let mut scene = Scene::new();
        if let Some(stroke) = descriptor.typography.stroke {
            options.stroke = Some(StrokeOptions {
                color: stroke.color,
                width_px: stroke.width,
            });
        }
        self.render(&mut scene, &layout, writing_mode, &options, transform);
        let mut diagnostics = Vec::new();
        if layout.font_size + f32::EPSILON < descriptor.typography.minimum_size {
            diagnostics.push(RenderDiagnostic::TextBelowReadableSize {
                entity,
                font_size: layout.font_size,
                minimum_font_size: descriptor.typography.minimum_size,
            });
        }
        if layout.overflowed() {
            diagnostics.push(RenderDiagnostic::TextOverflow {
                entity,
                available: bounds.into(),
                actual_width: layout.width,
                actual_height: layout.height,
                font_size: layout.font_size,
            });
        }
        let rendered_bounds = RenderBounds {
            x,
            y,
            width: layout.width,
            height: layout.height,
        };
        Ok(RenderedTextNode {
            scene: Arc::new(scene),
            local_bounds: RenderBounds {
                x: 0.0,
                y: 0.0,
                width: descriptor.width.max(x + layout.width),
                height: descriptor.height.max(y + layout.height),
            },
            metadata: RenderedTextNodeMetadata {
                rendered_bounds,
                layout_bounds: if point_text {
                    rendered_bounds
                } else {
                    bounds.into()
                },
                post_script_fonts: fonts
                    .iter()
                    .map(|font| font.post_script_name().to_owned())
                    .collect(),
                font_size: layout.font_size,
                color,
            },
            diagnostics,
        })
    }
}

fn is_cjk_language(language: &str) -> bool {
    language
        .split(['-', '_'])
        .next()
        .is_some_and(|primary| matches!(primary.to_ascii_lowercase().as_str(), "ja" | "ko" | "zh"))
}

fn inset(rect: LayoutBox, [top, right, bottom, left]: [f32; 4]) -> LayoutBox {
    LayoutBox {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    }
}

fn placement(
    rect: LayoutBox,
    width: f32,
    height: f32,
    vertical: SceneVerticalAlignment,
) -> (f32, f32) {
    let x = rect.x + (rect.width - width) * 0.5;
    let remaining = rect.height - height;
    let y = rect.y
        + match vertical {
            SceneVerticalAlignment::Top => 0.0,
            SceneVerticalAlignment::Center => remaining * 0.5,
            SceneVerticalAlignment::Bottom => remaining,
        };
    (x, y)
}

#[derive(Clone, Copy)]
enum DrawStyle {
    Stroke(StrokeOptions),
    Fill,
}

fn draw_layout(
    scene: &mut Scene,
    layout: &LayoutRun<'_>,
    writing_mode: WritingMode,
    options: &TextRenderOptions,
    transform: Affine,
    style: DrawStyle,
) {
    for line in &layout.lines {
        let (baseline_x, baseline_y) = match writing_mode {
            WritingMode::Horizontal | WritingMode::VerticalRl => line.baseline,
        };
        let mut pen_x = 0.0;
        let mut pen_y = 0.0;
        let mut start = 0;

        while start < line.glyphs.len() {
            let font = line.glyphs[start].font;
            let key = font_key(font);
            let mut end = start + 1;
            while end < line.glyphs.len() && font_key(line.glyphs[end].font) == key {
                end += 1;
            }

            let mut glyphs = Vec::with_capacity(end - start);
            for glyph in &line.glyphs[start..end] {
                glyphs.push(Glyph {
                    id: glyph.glyph_id,
                    x: options.padding + baseline_x + pen_x + glyph.x_offset,
                    y: options.padding + baseline_y + pen_y
                        - glyph.y_offset
                        - options.baseline_shift,
                });
                pen_x += glyph.x_advance;
                pen_y -= glyph.y_advance;
            }

            let font_data = font.vello_data();
            let normalized_coords = font.normalized_coords();
            let mut run = scene
                .draw_glyphs(&font_data)
                .font_size(layout.font_size)
                .transform(transform)
                .hint(options.hint_glyphs);
            if !normalized_coords.is_empty() {
                run = run.normalized_coords(normalized_coords);
            }
            if let Some(angle) = font.synthetic_skew() {
                run = run
                    .glyph_transform(Some(Affine::skew(-(angle.to_radians().tan() as f64), 0.0)));
            }
            if font.synthetic_bold() {
                run = run.font_embolden(FontEmbolden::new(Diagonal2::new(1.0, 1.0)));
            }

            match style {
                DrawStyle::Fill => run
                    .brush(rgba(options.color))
                    .draw(Fill::NonZero, glyphs.into_iter()),
                DrawStyle::Stroke(stroke) => {
                    let outline =
                        Stroke::new((stroke.width_px * 2.0) as f64).with_join(Join::Round);
                    run.brush(rgba(stroke.color))
                        .draw(&outline, glyphs.into_iter());
                }
            }
            start = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use koharu_scene::{EntityId, TextLayout as AuthoredTextLayout, Typography};

    use super::*;
    use crate::fonts::Fonts;

    #[tokio::test]
    async fn authored_auto_fit_respects_declared_size_range() {
        let fonts = Fonts::new();
        fonts
            .prepare(&[("Arial".to_owned(), 400, crate::FontStyle::Normal)])
            .await
            .unwrap();
        let descriptor = TextNodeDescriptor {
            entity: EntityId::new(),
            text: "A deliberately long line of dialogue for a small frame".to_owned(),
            language: None,
            width: 160.0,
            height: 64.0,
            balloon_contour: Some(vec![(0.0, 0.0), (160.0, 0.0), (160.0, 64.0), (0.0, 64.0)]),
            layout: AuthoredTextLayout {
                insets: [0.0; 4],
                ..AuthoredTextLayout::default()
            },
            typography: Typography {
                font_families: vec!["Arial".to_owned()],
                size: 32.0,
                minimum_size: 9.0,
                auto_fit: true,
                ..Typography::default()
            },
        };

        let rendered = TextRenderer::new()
            .render_descriptor(&descriptor, &fonts)
            .unwrap();

        assert!(rendered.metadata.font_size <= descriptor.typography.size);
        assert!(rendered.metadata.font_size >= descriptor.typography.minimum_size);
    }
}
