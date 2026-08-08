//! Text layout and Vello glyph recording.

use anyhow::Result;
use vello::{
    FontEmbolden, Glyph, Scene,
    kurbo::{Affine, Diagonal2, Join, Rect, Stroke},
    peniko::Fill,
};

use crate::{
    Error, HyphenationPolicy, LayoutRun, RenderDiagnostic, RenderTheme, Result as RenderResult,
    TextLayout, VerticalAlignment, WritingMode,
    bubble::LayoutBox,
    compositor::{RenderBounds, TextLayer},
    fonts::{Fonts, font_key},
    rasterizer::rgba,
    scene_renderer::{RenderedLayer, VisualLayer, VisualLayerKind, VisualText},
    script::is_cjk_text,
};

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

    pub(crate) fn render_layer(
        &self,
        layer: &TextLayer,
        fonts: &Fonts,
        theme: &RenderTheme,
    ) -> RenderResult<RenderedLayer> {
        let is_bubble_text = layer.balloon_contour.is_some();
        let bounds = if is_bubble_text {
            inset(layer.bounds, theme.text_inset)
        } else {
            layer.bounds
        };
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Err(Error::invalid(format!(
                "text inset leaves no layout area for entity {}",
                layer.entity
            )));
        }
        let fonts = fonts
            .resolve(
                layer.preferred_font.as_deref(),
                layer.font_weight,
                layer.font_style,
                &theme.font_families,
                &layer.text,
                layer
                    .language
                    .as_ref()
                    .map(koharu_scene::LanguageTag::as_str),
            )
            .map_err(|source| Error::Font {
                entity: layer.entity,
                source,
            })?;
        let automatic_maximum = || {
            if is_bubble_text {
                if layer.writing_mode.is_vertical() {
                    bounds.height
                } else {
                    bounds.width
                }
            } else {
                theme.font_size
            }
        };
        let maximum = if layer.auto_fit {
            automatic_maximum()
        } else {
            layer.font_size.unwrap_or_else(automatic_maximum)
        };
        let minimum = theme.minimum_font_size.min(maximum);
        let mut layout = TextLayout::new(&fonts[0])
            .with_fallback_fonts(&fonts[1..])
            .with_writing_mode(layer.writing_mode)
            .with_alignment(layer.alignment)
            .with_line_height(theme.line_height)
            .with_spacing(theme.letter_spacing, theme.word_spacing)
            .with_compact_emphasis_punctuation(
                is_cjk_text(&layer.text)
                    || layer
                        .language
                        .as_ref()
                        .is_some_and(|language| is_cjk_language(language.as_str())),
            );
        if !layer.point_text {
            layout = layout
                .with_max_width(bounds.width)
                .with_max_height(bounds.height);
        }
        if let Some(contour) = &layer.balloon_contour {
            let [top, _, _, left] = theme.text_inset;
            layout = layout.with_comic_balloon(
                bounds.width,
                bounds.height,
                contour.iter().map(|&(x, y)| (x - left, y - top)).collect(),
                match theme.vertical_alignment {
                    VerticalAlignment::Top => 0.0,
                    VerticalAlignment::Center => 0.5,
                    VerticalAlignment::Bottom => 1.0,
                },
                theme.text_inset.into_iter().fold(0.0, f32::max),
            );
        }
        if let Some(language) = &layer.language {
            layout = layout.with_hyphenation_language_tag(language.as_str());
            if is_bubble_text
                && layer.writing_mode == WritingMode::Horizontal
                && is_english(language.as_str())
            {
                layout = layout.with_hyphenation_policy(HyphenationPolicy::LastResort);
            }
        }
        let layout = if layer.auto_fit && !layer.point_text {
            layout
                .with_max_font_size(maximum)
                .with_min_font_size(minimum)
        } else {
            layout.with_font_size(layer.font_size.unwrap_or(maximum))
        };
        let layout = self
            .layout(&layout, &layer.text)
            .map_err(|source| Error::Layout {
                entity: layer.entity,
                source,
            })?;
        let (mut x, mut y) = if layer.point_text {
            (bounds.x, bounds.y)
        } else {
            placement(
                bounds,
                layout.width,
                layout.height,
                theme.vertical_alignment,
            )
        };
        x += layout.placement_offset_x();
        y += layout.placement_offset_y();
        let layout_rect = Rect::new(
            f64::from(x),
            f64::from(y),
            f64::from(x + layout.width),
            f64::from(y + layout.height),
        );
        let angle = f64::from(layer.angle_degrees).to_radians();
        let center = layout_rect.center();
        let rotation = Affine::rotate_about(angle, center);
        let transform =
            Affine::translate((f64::from(x), f64::from(y))).then_rotate_about(angle, center);
        let color = with_alpha(
            layer.foreground_color.unwrap_or(theme.text_color),
            layer.opacity,
        );
        let mut options = TextRenderOptions {
            color,
            stroke: None,
            ..TextRenderOptions::default()
        };
        let mut scene = Scene::new();
        if let Some(mut stroke) = layer.stroke.or(theme.text_stroke) {
            stroke.color = with_alpha(stroke.color, layer.opacity);
            options.stroke = Some(stroke);
        }
        self.render(&mut scene, &layout, layer.writing_mode, &options, transform);
        let rendered_bounds = rotation.transform_rect_bbox(layout_rect);
        let mut diagnostics = Vec::new();
        if layout.font_size + f32::EPSILON < theme.minimum_font_size {
            diagnostics.push(RenderDiagnostic::TextBelowReadableSize {
                entity: layer.entity,
                font_size: layout.font_size,
                minimum_font_size: theme.minimum_font_size,
            });
        }
        if layout.overflowed() {
            diagnostics.push(RenderDiagnostic::TextOverflow {
                entity: layer.entity,
                available: bounds.into(),
                actual_width: layout.width,
                actual_height: layout.height,
                font_size: layout.font_size,
            });
        }
        Ok(RenderedLayer {
            scene,
            layer: VisualLayer {
                entity: layer.entity,
                kind: VisualLayerKind::Text,
                name: None,
                bounds: RenderBounds {
                    x: rendered_bounds.x0 as f32,
                    y: rendered_bounds.y0 as f32,
                    width: rendered_bounds.width() as f32,
                    height: rendered_bounds.height() as f32,
                },
                font_size: Some(layout.font_size),
                text: Some(VisualText {
                    text: layer.text.clone(),
                    language: layer.language.clone(),
                    rendered_bounds: RenderBounds {
                        x,
                        y,
                        width: layout.width,
                        height: layout.height,
                    },
                    layout_bounds: if layer.point_text {
                        RenderBounds {
                            x,
                            y,
                            width: layout.width,
                            height: layout.height,
                        }
                    } else {
                        bounds.into()
                    },
                    post_script_fonts: fonts
                        .iter()
                        .map(|font| font.post_script_name().to_owned())
                        .collect(),
                    font_size: layout.font_size,
                    color,
                    alignment: layer.alignment,
                    writing_mode: layer.writing_mode,
                    angle_degrees: layer.angle_degrees,
                }),
            },
            diagnostics,
        })
    }
}

fn is_english(language: &str) -> bool {
    language
        .split(['-', '_'])
        .next()
        .is_some_and(|primary| primary.eq_ignore_ascii_case("en"))
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

fn placement(rect: LayoutBox, width: f32, height: f32, vertical: VerticalAlignment) -> (f32, f32) {
    let x = rect.x + (rect.width - width) * 0.5;
    let remaining = rect.height - height;
    let y = rect.y
        + match vertical {
            VerticalAlignment::Top => 0.0,
            VerticalAlignment::Center => remaining * 0.5,
            VerticalAlignment::Bottom => remaining,
        };
    (x, y)
}

fn with_alpha(mut color: [u8; 4], opacity: f32) -> [u8; 4] {
    color[3] = (f32::from(color[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
    color
}

#[derive(Clone, Copy)]
enum DrawStyle {
    Stroke(crate::StrokeOptions),
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
            WritingMode::Horizontal | WritingMode::VerticalRl | WritingMode::VerticalLr => {
                line.baseline
            }
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
    use koharu_scene::EntityId;

    use super::*;
    use crate::{TextAlign, compositor::TextLayer, fonts::Fonts};

    #[test]
    fn automatic_balloon_size_ignores_stored_size() {
        let layer = TextLayer {
            entity: EntityId::new(),
            text: "Hi".to_owned(),
            language: None,
            bounds: LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 240.0,
                height: 120.0,
            },
            balloon_contour: Some(vec![(0.0, 0.0), (240.0, 0.0), (240.0, 120.0), (0.0, 120.0)]),
            opacity: 1.0,
            preferred_font: Some("Arial".to_owned()),
            font_weight: None,
            font_style: None,
            font_size: Some(6.0),
            auto_fit: true,
            alignment: TextAlign::Center,
            writing_mode: WritingMode::Horizontal,
            foreground_color: None,
            stroke: None,
            angle_degrees: 0.0,
            point_text: false,
        };
        let theme = RenderTheme {
            font_families: vec!["Arial".to_owned()],
            text_inset: [0.0; 4],
            ..RenderTheme::default()
        };

        let rendered = TextRenderer::new()
            .render_layer(&layer, &Fonts::shared(), &theme)
            .unwrap();

        assert!(rendered.layer.font_size.unwrap() > theme.font_size);
    }
}
