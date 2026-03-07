use std::{collections::HashMap, ops::Range};

use anyhow::Result;
use harfrust::{Direction, Feature, Tag};
use skrifa::{
    MetadataProvider,
    instance::{LocationRef, Size},
};

use crate::font::{Font, font_key};
use crate::shape::shape_segment_with_fallbacks;

pub use crate::segment::{LineBreakOpportunity, LineBreaker};
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

impl From<WritingMode> for Direction {
    fn from(mode: WritingMode) -> Self {
        match mode {
            WritingMode::Horizontal => Direction::LeftToRight,
            WritingMode::VerticalRl => Direction::TopToBottom,
        }
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

pub struct TextLayout<'a> {
    writing_mode: WritingMode,
    center_vertical_punctuation: bool,
    font: &'a Font,
    fallback_fonts: &'a [Font],
    font_size: Option<f32>,
    max_width: Option<f32>,
    max_height: Option<f32>,
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

    pub fn run(&self, text: &str) -> Result<LayoutRun<'a>> {
        if let Some(font_size) = self.font_size {
            return self.run_with_size(text, font_size);
        }

        self.run_auto(text)
    }

    fn run_auto(&self, text: &str) -> Result<LayoutRun<'a>> {
        let max_height = self.max_height.unwrap_or(f32::INFINITY);
        let max_width = self.max_width.unwrap_or(f32::INFINITY);

        let mut low = 6;
        let mut high = 300;
        let mut best: Option<LayoutRun<'a>> = None;

        while low <= high {
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

        best.ok_or_else(|| anyhow::anyhow!("failed to layout text within constraints"))
    }

    fn run_with_size(&self, text: &str, font_size: f32) -> Result<LayoutRun<'a>> {
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

        let opts = ShapingOptions {
            direction: self.writing_mode.into(),
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

        let breaks = line_breaker.line_break_opportunities(text);

        let mut fonts: Vec<&Font> = Vec::with_capacity(1 + self.fallback_fonts.len());
        fonts.push(self.font);
        fonts.extend(self.fallback_fonts.iter());
        let mut lines: Vec<LayoutLine<'a>> = Vec::new();
        let mut current = LayoutLine::default();
        let mut line_offset = 0usize;

        for window in breaks.windows(2) {
            let (start, end) = (window[0].offset, window[1].offset);
            let segment = &text[start..end];

            let mut shaped = if fonts.len() == 1 {
                shaper.shape(segment, self.font, &opts)?
            } else {
                shape_segment_with_fallbacks(&shaper, segment, &fonts, &opts)?
            };
            if self.writing_mode.is_vertical() && self.center_vertical_punctuation {
                self.center_vertical_fullwidth_punctuation(font_size, segment, &mut shaped.glyphs);
            }
            let advance = if self.writing_mode.is_vertical() {
                shaped.y_advance
            } else {
                shaped.x_advance
            };

            let would_overflow = if self.writing_mode.is_vertical() {
                // For vertical text, advance is negative (downward), so we check absolute values
                current.advance.abs() + advance.abs() > max_extent
            } else {
                current.advance + advance > max_extent
            };
            let has_content = !current.glyphs.is_empty();
            let is_mandatory = window[1].is_mandatory; // Check if the END of segment is mandatory

            if (is_mandatory || would_overflow) && has_content {
                // Finalize current line
                current.range = line_offset..start;
                lines.push(current);

                // Start new line
                current = LayoutLine::default();
                line_offset = start;
            }

            // Adjust cluster indices and add glyphs to current line
            for mut glyph in shaped.glyphs {
                glyph.cluster += start as u32;
                current.glyphs.push(glyph);
            }
            current.advance += advance;
        }

        // Finalize last line
        if !current.glyphs.is_empty() {
            current.range = line_offset..text.len();
            lines.push(current);
        }

        // Baselines depend only on line index and metrics. For vertical text we compute absolute X
        // positions within the layout bounds (0..width) so the renderer can draw from the left.
        let line_count = lines.len();
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
        let (mut width, mut height) = self.compute_bounds(&lines, line_height, descent);
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
            width = (max_x - min_x).max(0.0);
            height = (max_y - min_y).max(0.0);
        }

        Ok(LayoutRun {
            lines,
            width,
            height,
            font_size,
        })
    }

    fn compute_bounds(
        &self,
        lines: &[LayoutLine<'a>],
        line_height: f32,
        descent: f32,
    ) -> (f32, f32) {
        if lines.is_empty() {
            return (0.0, 0.0);
        }

        match self.writing_mode {
            WritingMode::Horizontal => {
                let w = lines.iter().map(|l| l.advance).fold(0.0f32, f32::max);
                let h = (lines.len() - 1) as f32 * line_height + lines[0].baseline.1 + descent;
                (w, h)
            }
            WritingMode::VerticalRl => {
                // Each line is a column; `line_height` is used as the column pitch (width).
                let w = lines.len() as f32 * line_height;
                // Like horizontal layout, account for the baseline offset (top padding via ascent)
                // and the descent so glyphs don't get clipped after converting to a Y-down canvas.
                let h = lines.iter().map(|l| l.advance.abs()).fold(0.0f32, f32::max)
                    + lines[0].baseline.1
                    + descent;
                (w, h)
            }
        }
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
    use crate::font::{FamilyName, Font, FontBook, Properties};
    use skrifa::{
        MetadataProvider,
        instance::{LocationRef, Size},
    };

    fn any_system_font() -> Font {
        let mut book = FontBook::new();
        let props = Properties::default();

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
            if let Ok(font) = book.query(&[FamilyName::Title(name.to_string())], &props) {
                return font;
            }
        }

        panic!("no system font available for tests");
    }

    fn assert_approx_eq(actual: f32, expected: f32) {
        let eps = 1e-4;
        assert!(
            (actual - expected).abs() <= eps,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn compute_bounds_horizontal_uses_max_advance_and_baseline() {
        let font = any_system_font();
        let layout = TextLayout::new(&font, Some(16.0)).with_writing_mode(WritingMode::Horizontal);

        let lines = vec![
            LayoutLine {
                advance: 100.0,
                baseline: (0.0, 12.0),
                ..Default::default()
            },
            LayoutLine {
                advance: 250.0,
                baseline: (0.0, 32.0),
                ..Default::default()
            },
            LayoutLine {
                advance: 180.0,
                baseline: (0.0, 52.0),
                ..Default::default()
            },
        ];

        let line_height = 20.0;
        let descent = 5.0;
        let (w, h) = layout.compute_bounds(&lines, line_height, descent);

        assert_approx_eq(w, 250.0);
        // (len-1)*line_height + first_baseline_y + descent
        assert_approx_eq(h, 2.0 * line_height + 12.0 + descent);
    }

    #[test]
    fn compute_bounds_vertical_accounts_for_baseline_and_descent() {
        let font = any_system_font();
        let layout = TextLayout::new(&font, Some(16.0)).with_writing_mode(WritingMode::VerticalRl);

        let lines = vec![
            LayoutLine {
                // Vertical advances are typically negative in Y-up space; bounds use abs().
                advance: -100.0,
                baseline: (0.0, 12.0),
                ..Default::default()
            },
            LayoutLine {
                advance: -80.0,
                baseline: (-20.0, 12.0),
                ..Default::default()
            },
            LayoutLine {
                advance: -90.0,
                baseline: (-40.0, 12.0),
                ..Default::default()
            },
        ];

        let line_height = 20.0;
        let descent = 5.0;
        let (w, h) = layout.compute_bounds(&lines, line_height, descent);

        assert_approx_eq(w, 3.0 * line_height);
        // max(|advance|) + first_baseline_y + descent
        assert_approx_eq(h, 100.0 + 12.0 + descent);
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
