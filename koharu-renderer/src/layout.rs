use std::{collections::HashMap, ops::Range};
use unicode_bidi::BidiInfo;

use anyhow::Result;
use harfrust::{Feature, Tag};
use skrifa::{
    MetadataProvider,
    instance::{LocationRef, Size},
};

use crate::font::{Font, font_key};
use crate::shape::shape_script_runs;
use crate::text::script::shaping_direction_for_text;
use crate::types::TextAlign;

pub use crate::segment::{LineBreakOpportunity, LineBreaker, LineSegment};
pub use crate::shape::{PositionedGlyph, ShapedRun, ShapingOptions, TextShaper};

/// Writing mode for text layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WritingMode {
    /// Horizontal text, left-to-right, lines flow top-to-bottom.
    #[default]
    Horizontal,
    /// Vertical text, right-to-left columns (traditional CJK).
    VerticalRl,
}

impl WritingMode {
    /// Returns true if the writing mode is vertical.
    pub fn is_vertical(&self) -> bool {
        matches!(self, WritingMode::VerticalRl)
    }
}

/// Glyphs for one line alongside metadata required by the renderer.
#[derive(Debug, Clone, Default)]
pub struct LayoutLine<'a> {
    /// Positioned glyphs in this line.
    pub glyphs: Vec<PositionedGlyph<'a>>,
    /// Range in the original text that this line covers.
    pub range: Range<usize>,
    /// Total advance (width for horizontal, height for vertical) of this line.
    pub advance: f32,
    /// Baseline position for this line (x, y).
    pub baseline: (f32, f32),
    /// Writing direction of this line.
    pub direction: harfrust::Direction,
}

/// A collection of laid out lines.
#[derive(Debug, Clone)]
pub struct LayoutRun<'a> {
    /// Lines in this layout run.
    pub lines: Vec<LayoutLine<'a>>,
    /// Total width of the layout.
    pub width: f32,
    /// Total height of the layout.
    pub height: f32,
    /// Font size used to generate this layout.
    pub font_size: f32,
}

#[derive(Clone)]
pub struct TextLayout<'a> {
    writing_mode: WritingMode,
    center_vertical_punctuation: bool,
    font: &'a Font,
    fallback_fonts: &'a [Font],
    font_size: Option<f32>,
    max_width: Option<f32>,
    max_height: Option<f32>,
    alignment: Option<TextAlign>,
}

impl<'a> TextLayout<'a> {
    pub fn new(font: &'a Font, font_size: Option<f32>) -> Self {
        Self {
            writing_mode: WritingMode::Horizontal,
            center_vertical_punctuation: true,
            font,
            fallback_fonts: &[],
            font_size,
            max_width: None,
            max_height: None,
            alignment: None,
        }
    }

    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = Some(size);
        self
    }

    pub fn with_writing_mode(mut self, mode: WritingMode) -> Self {
        self.writing_mode = mode;
        self
    }

    pub fn with_center_vertical_punctuation(mut self, enabled: bool) -> Self {
        self.center_vertical_punctuation = enabled;
        self
    }

    pub fn with_fallback_fonts(mut self, fonts: &'a [Font]) -> Self {
        self.fallback_fonts = fonts;
        self
    }

    pub fn with_max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    pub fn with_max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }

    pub fn with_alignment(mut self, alignment: TextAlign) -> Self {
        self.alignment = Some(alignment);
        self
    }

    pub fn run(&self, text: &str) -> Result<LayoutRun<'a>> {
        if let Some(font_size) = self.font_size {
            return self.run_with_size(text, font_size);
        }

        self.run_auto(text)
    }

    fn run_auto(&self, text: &str) -> Result<LayoutRun<'a>> {
        let _s = tracing::info_span!("auto_size").entered();
        let max_height = self.max_height.unwrap_or(f32::INFINITY);
        let max_width = self.max_width.unwrap_or(f32::INFINITY);

        let mut low = 6;
        let mut high = 300;
        let mut best: Option<LayoutRun<'a>> = None;
        let mut iterations = 0u32;

        while low <= high {
            iterations += 1;
            let mid = (low + high) / 2;
            let size = mid as f32;
            let layout = self.run_with_size(text, size)?;
            if layout.width <= max_width && layout.height <= max_height {
                best = Some(layout);
                low = mid + 1;
            } else {
                high = mid - 1;
            }
        }

        // If no size fits within constraints, fall back to the smallest size.
        // This ensures we always render something even if the box is very small.
        if best.is_none() {
            best = Some(self.run_with_size(text, 6.0)?);
        }
        tracing::info!(
            iterations,
            font_size = best.as_ref().map(|b| b.font_size as u32).unwrap_or(0),
            "auto_size done"
        );
        Ok(best.unwrap())
    }

    fn run_with_size(&self, text: &str, font_size: f32) -> Result<LayoutRun<'a>> {
        let _s = tracing::debug_span!("layout_size", font_size = font_size as u32).entered();
        let shaper = TextShaper::new();
        let line_breaker = LineBreaker::new();
        let normalized_punctuation;
        let text = if self.writing_mode.is_vertical() {
            normalized_punctuation = normalize_vertical_emphasis_punctuation(text);
            normalized_punctuation.as_str()
        } else {
            text
        };

        // Use real font metrics for consistent line sizing across modes.
        let font_ref = self.font.skrifa()?;
        let metrics = font_ref.metrics(Size::new(font_size), LocationRef::default());
        let ascent = metrics.ascent;
        let descent = -metrics.descent;
        let line_height = (ascent + descent + metrics.leading).max(font_size);

        let bidi_info = BidiInfo::new(text, None);

        let (direction, script) = shaping_direction_for_text(text, self.writing_mode);
        let opts = ShapingOptions {
            direction,
            script,
            font_size,
            features: if self.writing_mode.is_vertical() {
                &[
                    Feature::new(Tag::new(b"vert"), 1, ..),
                    Feature::new(Tag::new(b"vrt2"), 1, ..),
                ]
            } else {
                &[]
            },
        };

        let max_extent = if self.writing_mode.is_vertical() {
            self.max_height
        } else {
            self.max_width
        }
        .unwrap_or(f32::INFINITY);
        let max_extent_finite = max_extent.is_finite() && max_extent > 0.0;

        let segments = line_breaker.line_segments(text);

        let mut fonts: Vec<&Font> = Vec::with_capacity(1 + self.fallback_fonts.len());
        fonts.push(self.font);
        fonts.extend(self.fallback_fonts.iter());
        let mut lines: Vec<LayoutLine<'a>> = Vec::new();

        // Temporary storage for runs in the current line to be reordered
        struct LineRun<'a> {
            shaped: ShapedRun<'a>,
            level: unicode_bidi::Level,
        }
        let mut current_line_runs: Vec<LineRun<'a>> = Vec::new();
        let mut line_offset = 0usize;
        let mut current_advance = 0.0f32;

        let finalize_current_line = |runs: &mut Vec<LineRun<'a>>,
                                     offset: &mut usize,
                                     visible_end: usize,
                                     next_offset: usize,
                                     lines: &mut Vec<LayoutLine<'a>>,
                                     force_push: bool| {
            if runs.is_empty() && !force_push {
                *offset = next_offset;
                return;
            }

            let levels: Vec<unicode_bidi::Level> = runs.iter().map(|r| r.level).collect();
            let visual_indices = reorder_visual(&levels);

            let mut line = LayoutLine {
                range: *offset..visible_end,
                direction: if self.writing_mode.is_vertical() {
                    harfrust::Direction::TopToBottom
                } else {
                    bidi_info
                        .paragraphs
                        .iter()
                        .find(|p| *offset >= p.range.start && *offset <= p.range.end)
                        .map(|p| {
                            if p.level.is_rtl() {
                                harfrust::Direction::RightToLeft
                            } else {
                                harfrust::Direction::LeftToRight
                            }
                        })
                        .unwrap_or(harfrust::Direction::LeftToRight)
                },
                ..Default::default()
            };

            let mut pen_x = 0.0f32;
            let mut pen_y = 0.0f32;

            for idx in visual_indices {
                let run = &mut runs[idx];
                for glyph in std::mem::take(&mut run.shaped.glyphs) {
                    line.glyphs.push(glyph);
                }
                if self.writing_mode.is_vertical() {
                    pen_y -= run.shaped.y_advance;
                } else {
                    pen_x += run.shaped.x_advance;
                }
            }

            line.advance = if self.writing_mode.is_vertical() {
                pen_y.abs()
            } else {
                pen_x
            };

            lines.push(line);
            runs.clear();
            *offset = next_offset;
        };

        for segment in segments {
            let segment_text = &text[segment.range.clone()];

            let mut segment_runs = Vec::new();
            let mut segment_advance = 0.0f32;

            if !segment_text.is_empty() {
                // Subdivide segment into constant BiDi level runs.
                let mut char_indices = segment_text
                    .char_indices()
                    .map(|(id, _)| segment.range.start + id)
                    .peekable();

                while let Some(run_start) = char_indices.next() {
                    let level = bidi_info.levels[run_start];
                    let mut run_end = segment.range.end;

                    while let Some(&next_char_start) = char_indices.peek() {
                        if bidi_info.levels[next_char_start] != level {
                            run_end = next_char_start;
                            break;
                        }
                        char_indices.next();
                    }

                    let run_text = &text[run_start..run_end];
                    let mut run_opts = opts.clone();
                    run_opts.direction = if self.writing_mode.is_vertical() {
                        harfrust::Direction::TopToBottom
                    } else if level.is_rtl() {
                        harfrust::Direction::RightToLeft
                    } else {
                        harfrust::Direction::LeftToRight
                    };

                    let script_runs = shape_script_runs(&shaper, run_text, &fonts, &run_opts)?;
                    for mut shaped in script_runs {
                        if self.writing_mode.is_vertical() && self.center_vertical_punctuation {
                            self.center_vertical_fullwidth_punctuation(
                                font_size,
                                run_text,
                                &mut shaped.glyphs,
                            );
                        }

                        for glyph in &mut shaped.glyphs {
                            glyph.cluster += run_start as u32;
                        }

                        segment_advance += if self.writing_mode.is_vertical() {
                            shaped.y_advance
                        } else {
                            shaped.x_advance
                        };

                        segment_runs.push(LineRun { shaped, level });
                    }
                }
            }

            let would_overflow = if self.writing_mode.is_vertical() {
                current_advance.abs() + segment_advance.abs() > max_extent
            } else {
                current_advance + segment_advance > max_extent
            };

            if would_overflow && !current_line_runs.is_empty() {
                finalize_current_line(
                    &mut current_line_runs,
                    &mut line_offset,
                    segment.range.start,
                    segment.range.start,
                    &mut lines,
                    false,
                );
                current_advance = 0.0;
            }

            current_line_runs.extend(segment_runs);
            current_advance += segment_advance;

            if segment.is_mandatory {
                // Consecutive mandatory breaks (e.g. "A\n\nB") will drop the empty line:
                // when segment.is_mandatory and current_line_runs is empty, finalize_current_line
                // early-returns without pushing a LayoutLine. Consider special-casing mandatory
                // breaks to push an empty LayoutLine (advance=0, glyphs empty, range=offset..visible_end)
                // so blank lines are preserved.
                finalize_current_line(
                    &mut current_line_runs,
                    &mut line_offset,
                    segment.range.end,
                    segment.next_offset,
                    &mut lines,
                    true,
                );
                current_advance = 0.0;
            }
        }

        // Finalize last line
        finalize_current_line(
            &mut current_line_runs,
            &mut line_offset,
            text.len(),
            text.len(),
            &mut lines,
            false, // Don't force push a final empty line if the text didn't end with a break
        );

        // Baselines depend only on line index and metrics. For vertical text we compute absolute X
        // positions within the layout bounds (0..width) so the renderer can draw from the left.
        let line_count = lines.len();
        let effective_alignment = self.alignment.unwrap_or(if max_extent_finite {
            // Default to Center for established horizontal or vertical scripts
            // to match the visual style of speech bubbles.
            TextAlign::Center
        } else {
            TextAlign::Left
        });

        for (i, line) in lines.iter_mut().enumerate() {
            line.baseline = if self.writing_mode.is_vertical() {
                // Vertical-rl: first column is on the right, subsequent columns shift left.
                // Place the baseline at the center of each column. This avoids depending on
                // ascent/descent for X extents (which are Y metrics) and prevents right-edge clipping.
                let x = (line_count.saturating_sub(1) as f32 - i as f32) * line_height
                    + line_height * 0.5;
                (x, ascent)
            } else {
                (0.0, ascent + i as f32 * line_height)
            };
        }

        // Compute a tight ink bounding box using per-glyph bounds from the font tables (via skrifa),
        // then translate baselines so the top-left ink origin is (0, 0). This avoids clipping without
        // having to measure Skia paths in the renderer.
        let (mut width, mut height) = (0.0, 0.0);
        if let Some((mut min_x, mut min_y, mut max_x, mut max_y)) =
            self.ink_bounds(font_size, &lines)
        {
            // Keep a tiny safety pad for hinting/AA differences.
            const PAD: f32 = 1.0;
            min_x -= PAD;
            min_y -= PAD;
            max_x += PAD;
            max_y += PAD;

            for line in &mut lines {
                line.baseline.0 -= min_x;
                line.baseline.1 -= min_y;
            }
            let max_width_finite = self.max_width.is_some_and(|w| w.is_finite() && w > 0.0);
            if self.writing_mode.is_vertical() {
                let actual_width = (max_x - min_x).max(0.0);
                if max_width_finite {
                    width = actual_width.max(self.max_width.unwrap());
                    if effective_alignment != TextAlign::Left {
                        let remaining = (width - actual_width).max(0.0);
                        let offset = match effective_alignment {
                            TextAlign::Center => remaining * 0.5,
                            TextAlign::Right => remaining,
                            TextAlign::Left => 0.0,
                        };
                        if offset > 0.0 {
                            for line in &mut lines {
                                line.baseline.0 += offset;
                            }
                        }
                    }
                } else {
                    width = actual_width;
                }
            } else {
                width = (max_x - min_x).max(if max_extent.is_finite() {
                    max_extent
                } else {
                    0.0
                });
            }
            height = (max_y - min_y).max(0.0);

            // Center vertically if requested and we have a container height.
            if !self.writing_mode.is_vertical()
                && effective_alignment == TextAlign::Center
                && let Some(max_h) = self.max_height.filter(|&h| h.is_finite() && h > 0.0)
            {
                let offset = (max_h - height).max(0.0) * 0.5;
                if offset > 0.0 {
                    for line in &mut lines {
                        line.baseline.1 += offset;
                    }
                    height = height.max(max_h);
                }
            }

            // Apply horizontal alignment for horizontal writing mode (per-line alignment).
            if !self.writing_mode.is_vertical()
                && max_extent_finite
                && effective_alignment != TextAlign::Left
            {
                for line in &mut lines {
                    let remaining = (max_extent - line.advance).max(0.0);
                    let offset = match effective_alignment {
                        TextAlign::Left => 0.0,
                        TextAlign::Center => remaining * 0.5,
                        TextAlign::Right => remaining,
                    };
                    if offset > 0.0 {
                        line.baseline.0 += offset;
                    }
                }
            }
        }

        Ok(LayoutRun {
            lines,
            width,
            height,
            font_size,
        })
    }

    fn ink_bounds(&self, font_size: f32, lines: &[LayoutLine<'a>]) -> Option<(f32, f32, f32, f32)> {
        let mut metrics_cache = HashMap::new();

        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for line in lines {
            let (mut x, mut y) = line.baseline;
            for g in &line.glyphs {
                let key = font_key(g.font);
                let glyph_metrics = match metrics_cache.entry(key) {
                    std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        let Ok(font_ref) = g.font.skrifa() else {
                            x += g.x_advance;
                            y -= g.y_advance;
                            continue;
                        };
                        entry.insert(
                            font_ref.glyph_metrics(Size::new(font_size), LocationRef::default()),
                        )
                    }
                };

                let gid = skrifa::GlyphId::new(g.glyph_id);
                if let Some(b) = glyph_metrics.bounds(gid) {
                    let x0 = x + g.x_offset + b.x_min;
                    let x1 = x + g.x_offset + b.x_max;

                    // `b` is in a Y-up font coordinate system. Our layout coordinates are Y-down
                    // (matching the Skia canvas), so we flip by subtracting.
                    let y0 = (y - g.y_offset) - b.y_max;
                    let y1 = (y - g.y_offset) - b.y_min;

                    min_x = min_x.min(x0).min(x1);
                    max_x = max_x.max(x0).max(x1);
                    min_y = min_y.min(y0).min(y1);
                    max_y = max_y.max(y0).max(y1);
                }

                x += g.x_advance;
                y -= g.y_advance;
            }
        }

        if min_x.is_finite() {
            Some((min_x, min_y, max_x, max_y))
        } else {
            None
        }
    }

    fn center_vertical_fullwidth_punctuation(
        &self,
        font_size: f32,
        segment: &str,
        glyphs: &mut [PositionedGlyph<'a>],
    ) {
        if segment.is_empty() || glyphs.is_empty() {
            return;
        }

        let mut metrics_cache = HashMap::new();
        for glyph in glyphs {
            let cluster = glyph.cluster as usize;
            let Some(ch) = segment.get(cluster..).and_then(|tail| tail.chars().next()) else {
                continue;
            };
            if !is_fullwidth_punctuation(ch) {
                continue;
            }

            let key = font_key(glyph.font);
            let glyph_metrics = match metrics_cache.entry(key) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let Ok(font_ref) = glyph.font.skrifa() else {
                        continue;
                    };
                    entry.insert(
                        font_ref.glyph_metrics(Size::new(font_size), LocationRef::default()),
                    )
                }
            };

            let gid = skrifa::GlyphId::new(glyph.glyph_id);
            let Some(bounds) = glyph_metrics.bounds(gid) else {
                continue;
            };
            glyph.x_offset = centered_x_offset(bounds.x_min, bounds.x_max);
        }
    }
}

fn centered_x_offset(x_min: f32, x_max: f32) -> f32 {
    -((x_min + x_max) * 0.5)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EmphasisMark {
    Bang,
    Question,
}

fn emphasis_mark_kind(ch: char) -> Option<EmphasisMark> {
    match ch {
        '!' | '！' => Some(EmphasisMark::Bang),
        '?' | '？' => Some(EmphasisMark::Question),
        _ => None,
    }
}

fn emphasis_pair_symbol(left: EmphasisMark, right: EmphasisMark) -> char {
    match (left, right) {
        (EmphasisMark::Bang, EmphasisMark::Bang) => '‼',
        (EmphasisMark::Question, EmphasisMark::Question) => '⁇',
        (EmphasisMark::Bang, EmphasisMark::Question) => '⁉',
        (EmphasisMark::Question, EmphasisMark::Bang) => '⁈',
    }
}

fn normalize_vertical_emphasis_punctuation(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;

    while i < chars.len() {
        let Some(kind) = emphasis_mark_kind(chars[i]) else {
            out.push(chars[i]);
            i += 1;
            continue;
        };

        if i + 1 >= chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        let Some(next_kind) = emphasis_mark_kind(chars[i + 1]) else {
            out.push(chars[i]);
            i += 1;
            continue;
        };

        if kind == next_kind {
            out.push(emphasis_pair_symbol(kind, next_kind));
            i += 2;
            continue;
        }

        if i + 2 < chars.len()
            && let Some(lookahead_kind) = emphasis_mark_kind(chars[i + 2])
            && next_kind == lookahead_kind
        {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        out.push(emphasis_pair_symbol(kind, next_kind));
        i += 2;
    }

    out
}

fn is_fullwidth_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '\u{3001}' // Ideographic comma
            | '\u{3002}' // Ideographic full stop
            | '\u{3008}'..='\u{3011}' // Angle/corner brackets
            | '\u{3014}'..='\u{301F}' // Tortoise shell/white brackets and marks
            | '\u{3030}' // Wavy dash
            | '\u{30FB}' // Katakana middle dot
            | '\u{FF01}'..='\u{FF0F}' // Fullwidth punctuation block 1
            | '\u{FF1A}'..='\u{FF20}' // Fullwidth punctuation block 2
            | '\u{FF3B}'..='\u{FF40}' // Fullwidth punctuation block 3
            | '\u{FF5B}'..='\u{FF65}' // Fullwidth punctuation block 4
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{Font, FontBook};
    use skrifa::{
        MetadataProvider,
        instance::{LocationRef, Size},
    };

    fn any_system_font() -> Font {
        let mut book = FontBook::new();

        // Prefer fonts that are commonly available depending on OS/environment.
        // This is only used to construct a `TextLayout` for calling `compute_bounds`.
        let preferred = [
            "Yu Gothic",
            "MS Gothic",
            "Noto Sans CJK JP",
            "Noto Sans",
            "Arial",
            "DejaVu Sans",
            "Liberation Sans",
        ];

        for name in preferred {
            if let Some(post_script_name) = book
                .all_families()
                .into_iter()
                .find(|face| {
                    face.post_script_name == name
                        || face
                            .families
                            .iter()
                            .any(|(family, _)| family.as_str() == name)
                })
                .map(|face| face.post_script_name)
                .filter(|post_script_name| !post_script_name.is_empty())
                && let Ok(font) = book.query(&post_script_name)
            {
                return font;
            }
        }

        if let Some(face) = book
            .all_families()
            .into_iter()
            .find(|face| !face.post_script_name.is_empty())
        {
            return book
                .query(&face.post_script_name)
                .expect("failed to load first system font");
        }

        panic!("no system font available for tests");
    }

    fn assert_approx_eq(actual: f32, expected: f32) {
        if actual.is_infinite()
            && expected.is_infinite()
            && actual.is_sign_positive() == expected.is_sign_positive()
        {
            return;
        }
        let eps = 1e-4;
        assert!(
            (actual - expected).abs() <= eps,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn layout_baselines_horizontal_follow_font_metrics() -> anyhow::Result<()> {
        let font = any_system_font();
        let font_size = 16.0;
        let layout = TextLayout::new(&font, Some(font_size))
            .with_writing_mode(WritingMode::Horizontal)
            .run("A\nB\nC")?;

        assert!(layout.lines.len() >= 2);

        let metrics = font
            .skrifa()?
            .metrics(Size::new(font_size), LocationRef::default());
        let ascent = metrics.ascent;
        let descent = -metrics.descent;
        let line_height = (ascent + descent + metrics.leading).max(font_size);

        let base_x = layout.lines[0].baseline.0;
        for line in &layout.lines {
            assert_approx_eq(line.baseline.0, base_x);
        }
        for i in 1..layout.lines.len() {
            let dy = layout.lines[i].baseline.1 - layout.lines[i - 1].baseline.1;
            assert_approx_eq(dy, line_height);
        }

        Ok(())
    }

    #[test]
    fn mandatory_newlines_are_not_shaped_as_glyphs() -> anyhow::Result<()> {
        let font = any_system_font();
        let text = "A\nB\nC";
        let layout = TextLayout::new(&font, Some(16.0))
            .with_writing_mode(WritingMode::Horizontal)
            .run(text)?;

        assert_eq!(layout.lines.len(), 3);
        for (line, expected) in layout.lines.iter().zip(["A", "B", "C"]) {
            assert_eq!(&text[line.range.clone()], expected);
            assert_eq!(line.glyphs.len(), 1);
        }

        Ok(())
    }

    #[test]
    fn layout_baselines_vertical_follow_font_metrics() -> anyhow::Result<()> {
        let font = any_system_font();
        let font_size = 16.0;
        let layout = TextLayout::new(&font, Some(font_size))
            .with_writing_mode(WritingMode::VerticalRl)
            .run("A\nB\nC")?;

        assert!(layout.lines.len() >= 2);

        let metrics = font
            .skrifa()?
            .metrics(Size::new(font_size), LocationRef::default());
        let ascent = metrics.ascent;
        let descent = -metrics.descent;
        let line_height = (ascent + descent + metrics.leading).max(font_size);
        let base_y = layout.lines[0].baseline.1;
        for line in &layout.lines {
            assert_approx_eq(line.baseline.1, base_y);
        }

        for i in 1..layout.lines.len() {
            let dx = layout.lines[i - 1].baseline.0 - layout.lines[i].baseline.0;
            assert_approx_eq(dx, line_height);
        }

        Ok(())
    }

    #[test]
    fn vertical_layout_horizontal_alignment_works() -> anyhow::Result<()> {
        let font = any_system_font();
        let max_width = 100.0;
        let layout = TextLayout::new(&font, Some(16.0))
            .with_writing_mode(WritingMode::VerticalRl)
            .with_max_width(max_width)
            .with_alignment(TextAlign::Center)
            .run("A")?;

        assert_eq!(layout.width, max_width);
        // The block is centered horizontally.
        assert!(layout.lines[0].baseline.0 > 40.0);
        assert!(layout.lines[0].baseline.0 < 60.0);

        Ok(())
    }

    #[test]
    fn vertical_layout_left_alignment_expands_width() -> anyhow::Result<()> {
        let font = any_system_font();
        let max_width = 100.0;
        let layout = TextLayout::new(&font, Some(16.0))
            .with_writing_mode(WritingMode::VerticalRl)
            .with_max_width(max_width)
            .with_alignment(TextAlign::Left)
            .run("A")?;

        assert_eq!(layout.width, max_width);
        // The block should NOT be shifted horizontally.
        assert!(layout.lines[0].baseline.0 < 20.0);

        Ok(())
    }

    #[test]
    fn horizontal_center_alignment_centres_short_lines() -> anyhow::Result<()> {
        // Two lines of clearly different widths — a wide "HELLOWORLD" and
        // a narrow "HI". In a max_width wider than the long line, the
        // narrow line should be offset so its centre matches the long
        // line's centre (and the sprite centre).
        let font = any_system_font();
        let max_width = 400.0;
        let layout = TextLayout::new(&font, Some(20.0))
            .with_max_width(max_width)
            .with_alignment(TextAlign::Center)
            .run("HELLOWORLD\nHI")?;

        assert_eq!(layout.lines.len(), 2);
        let w0 = layout.lines[0].advance;
        let w1 = layout.lines[1].advance;
        let c0 = layout.lines[0].baseline.0 + w0 * 0.5;
        let c1 = layout.lines[1].baseline.0 + w1 * 0.5;
        // Line centres must coincide (within rounding / float slack).
        assert!(
            (c0 - c1).abs() < 1.0,
            "expected line centres to match, got c0={c0} c1={c1}",
        );
        Ok(())
    }

    #[test]
    fn fullwidth_punctuation_detection_works() {
        assert!(is_fullwidth_punctuation('。'));
        assert!(is_fullwidth_punctuation('（'));
        assert!(is_fullwidth_punctuation('！'));
        assert!(!is_fullwidth_punctuation('A'));
        assert!(!is_fullwidth_punctuation('中'));
    }

    #[test]
    fn vertical_punctuation_centering_enabled_by_default() {
        let font = any_system_font();
        let layout = TextLayout::new(&font, Some(16.0));
        assert!(layout.center_vertical_punctuation);
    }

    #[test]
    fn centered_x_offset_uses_absolute_center() {
        assert_approx_eq(centered_x_offset(2.0, 6.0), -4.0);
        assert_approx_eq(centered_x_offset(-3.0, 1.0), 1.0);
    }

    #[test]
    fn normalize_vertical_emphasis_punctuation_collapses_pairs() {
        assert_eq!(normalize_vertical_emphasis_punctuation("！！"), "‼");
        assert_eq!(normalize_vertical_emphasis_punctuation("!!"), "‼");
        assert_eq!(normalize_vertical_emphasis_punctuation("!!?"), "‼?");
        assert_eq!(normalize_vertical_emphasis_punctuation("?!!"), "?‼");
        assert_eq!(normalize_vertical_emphasis_punctuation("!?!"), "⁉!");
        assert_eq!(normalize_vertical_emphasis_punctuation("！？"), "⁉");
        assert_eq!(normalize_vertical_emphasis_punctuation("？！"), "⁈");
        assert_eq!(
            normalize_vertical_emphasis_punctuation("Hello!?!"),
            "Hello⁉!"
        );
    }
}

fn reorder_visual(levels: &[unicode_bidi::Level]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..levels.len()).collect();
    if levels.is_empty() {
        return indices;
    }

    let max_level = levels.iter().map(|l| l.number()).max().unwrap();
    let min_odd_level = levels
        .iter()
        .map(|l| l.number())
        .filter(|&n| n % 2 != 0)
        .min()
        .unwrap_or(u8::MAX);

    if min_odd_level == u8::MAX {
        return indices;
    }

    for level in (min_odd_level..=max_level).rev() {
        let mut i = 0;
        while i < levels.len() {
            if levels[i].number() >= level {
                let mut j = i;
                while j < levels.len() && levels[j].number() >= level {
                    j += 1;
                }
                indices[i..j].reverse();
                i = j;
            } else {
                i += 1;
            }
        }
    }
    indices
}
