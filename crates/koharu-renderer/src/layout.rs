use std::{collections::HashMap, ops::Range};
use unicode_bidi::BidiInfo;

use anyhow::Result;
use harfrust::{Feature, Tag};
use hypher::Lang;
use skrifa::{
    MetadataProvider,
    instance::{LocationRef, Size},
};

use crate::font::{Font, font_key};
use crate::shape::shape_script_runs;
use crate::text::script::shaping_direction_for_text;
use crate::types::TextAlign;

pub use crate::segment::{LineBreakOpportunity, LineBreaker, LineSegment};
pub use crate::segment::{LineBreakSuffix, hyphenation_lang_from_tag};
pub use crate::shape::{PositionedGlyph, ShapedRun, ShapingOptions, TextShaper};

const HYPHENATION_MIN_WORD_LEN: usize = 8;
const LINE_BREAK_HYPHEN_PENALTY: f32 = 2_000.0;
const LINE_BREAK_OVERFLOW_MULTIPLIER: f32 = 10_000.0;

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
struct LineRun<'a> {
    shaped: ShapedRun<'a>,
    level: unicode_bidi::Level,
}

#[derive(Clone)]
struct ShapedBreakSuffix<'a> {
    runs: Vec<LineRun<'a>>,
    advance: f32,
}

#[derive(Clone)]
struct ShapedSegment<'a> {
    range: Range<usize>,
    next_offset: usize,
    is_mandatory: bool,
    runs: Vec<LineRun<'a>>,
    advance: f32,
    break_suffix: Option<ShapedBreakSuffix<'a>>,
}

#[derive(Clone, Copy, Debug)]
struct LineBreakMeasure {
    advance: f32,
    break_suffix_advance: f32,
}

#[derive(Clone)]
pub struct TextLayout<'a> {
    writing_mode: WritingMode,
    center_vertical_punctuation: bool,
    hyphenation_lang: Option<Lang>,
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
            hyphenation_lang: Some(Lang::English),
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

    pub fn with_hyphenation_language(mut self, lang: Lang) -> Self {
        self.hyphenation_lang = Some(lang);
        self
    }

    pub fn with_hyphenation_language_tag(mut self, tag: &str) -> Self {
        self.hyphenation_lang = hyphenation_lang_from_tag(tag);
        self
    }

    pub fn without_hyphenation(mut self) -> Self {
        self.hyphenation_lang = None;
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
        let mut line_breaker = LineBreaker::new().with_chinese_word_segmentation();
        if !self.writing_mode.is_vertical()
            && let Some(lang) = self.hyphenation_lang
        {
            line_breaker = line_breaker.with_hyphenation(lang, HYPHENATION_MIN_WORD_LEN);
        }
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

        let mut fonts: Vec<&Font> = Vec::with_capacity(1 + self.fallback_fonts.len());
        fonts.push(self.font);
        fonts.extend(self.fallback_fonts.iter());

        let shape_break_suffix = |suffix: LineBreakSuffix,
                                  level: unicode_bidi::Level,
                                  cluster: usize|
         -> Result<ShapedBreakSuffix<'a>> {
            let mut suffix_opts = opts.clone();
            suffix_opts.direction = if level.is_rtl() {
                harfrust::Direction::RightToLeft
            } else {
                harfrust::Direction::LeftToRight
            };

            let mut runs = Vec::new();
            let mut advance = 0.0f32;
            for mut shaped in shape_script_runs(&shaper, suffix.as_str(), &fonts, &suffix_opts)? {
                for glyph in &mut shaped.glyphs {
                    glyph.cluster += cluster as u32;
                }
                advance += shaped.x_advance.abs();
                runs.push(LineRun { shaped, level });
            }

            Ok(ShapedBreakSuffix { runs, advance })
        };

        let mut shaped_segments = Vec::new();
        for segment in line_breaker.line_segments(text) {
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
                            shaped.y_advance.abs()
                        } else {
                            shaped.x_advance.abs()
                        };

                        segment_runs.push(LineRun { shaped, level });
                    }
                }
            }
            let segment_break_suffix = if let (Some(suffix), Some(level)) =
                (segment.break_suffix, segment_runs.last().map(|r| r.level))
            {
                Some(shape_break_suffix(suffix, level, segment.range.end)?)
            } else {
                None
            };

            shaped_segments.push(ShapedSegment {
                range: segment.range,
                next_offset: segment.next_offset,
                is_mandatory: segment.is_mandatory,
                runs: segment_runs,
                advance: segment_advance,
                break_suffix: segment_break_suffix,
            });
        }

        let mut lines: Vec<LayoutLine<'a>> = Vec::new();
        let mut line_offset = 0usize;
        let mut paragraph_start = 0usize;
        for (index, segment) in shaped_segments.iter().enumerate() {
            if !segment.is_mandatory {
                continue;
            }
            self.append_balanced_segment_lines(
                &shaped_segments[paragraph_start..=index],
                &mut line_offset,
                segment.next_offset,
                true,
                max_extent,
                &bidi_info,
                &mut lines,
            );
            paragraph_start = index + 1;
        }
        if paragraph_start < shaped_segments.len() {
            self.append_balanced_segment_lines(
                &shaped_segments[paragraph_start..],
                &mut line_offset,
                text.len(),
                false,
                max_extent,
                &bidi_info,
                &mut lines,
            );
        }

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
                    // Use tight bounds for Center alignment to ensure visual balance.
                    width = if effective_alignment == TextAlign::Center {
                        actual_width
                    } else {
                        actual_width.max(self.max_width.unwrap())
                    };

                    if effective_alignment != TextAlign::Left {
                        let anchor = if effective_alignment == TextAlign::Center {
                            actual_width
                        } else {
                            width
                        };
                        let remaining = (anchor - actual_width).max(0.0);
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
                let actual_width = (max_x - min_x).max(0.0);
                width = if effective_alignment == TextAlign::Center && max_extent_finite {
                    actual_width
                } else {
                    actual_width.max(if max_extent.is_finite() {
                        max_extent
                    } else {
                        0.0
                    })
                };
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
                // Anchor to the run width. If Center, this is a tight width.
                // If Right, this is the container width.
                let anchor = width;
                for line in &mut lines {
                    let remaining = (anchor - line.advance).max(0.0);
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

    #[allow(clippy::too_many_arguments)]
    fn append_balanced_segment_lines(
        &self,
        segments: &[ShapedSegment<'a>],
        line_offset: &mut usize,
        final_next_offset: usize,
        force_final_line: bool,
        max_extent: f32,
        bidi_info: &BidiInfo<'_>,
        lines: &mut Vec<LayoutLine<'a>>,
    ) {
        if segments.is_empty() {
            if force_final_line {
                *line_offset = self.push_layout_line(
                    Vec::new(),
                    *line_offset,
                    *line_offset,
                    final_next_offset,
                    None,
                    true,
                    bidi_info,
                    lines,
                );
            }
            return;
        }

        let break_indices = if max_extent.is_finite() && max_extent > 0.0 {
            let measures = segments
                .iter()
                .map(|segment| LineBreakMeasure {
                    advance: segment.advance,
                    break_suffix_advance: segment
                        .break_suffix
                        .as_ref()
                        .map_or(0.0, |suffix| suffix.advance),
                })
                .collect::<Vec<_>>();
            optimal_line_breaks(&measures, max_extent)
        } else {
            vec![segments.len()]
        };

        let mut start = 0usize;
        for end in break_indices {
            if end <= start || end > segments.len() {
                continue;
            }
            let final_line = end == segments.len();
            let visible_end = segments[end - 1].range.end;
            let next_offset = if final_line {
                final_next_offset
            } else {
                segments[end].range.start
            };
            let break_suffix = if final_line {
                None
            } else {
                segments[end - 1].break_suffix.clone()
            };
            let runs = segments[start..end]
                .iter()
                .flat_map(|segment| segment.runs.iter().cloned())
                .collect::<Vec<_>>();
            *line_offset = self.push_layout_line(
                runs,
                *line_offset,
                visible_end,
                next_offset,
                break_suffix,
                force_final_line && final_line,
                bidi_info,
                lines,
            );
            start = end;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_layout_line(
        &self,
        mut runs: Vec<LineRun<'a>>,
        offset: usize,
        visible_end: usize,
        next_offset: usize,
        break_suffix: Option<ShapedBreakSuffix<'a>>,
        force_push: bool,
        bidi_info: &BidiInfo<'_>,
        lines: &mut Vec<LayoutLine<'a>>,
    ) -> usize {
        if runs.is_empty() && !force_push {
            return next_offset;
        }

        if let Some(mut suffix) = break_suffix {
            runs.append(&mut suffix.runs);
        }

        let levels: Vec<unicode_bidi::Level> = runs.iter().map(|r| r.level).collect();
        let visual_indices = reorder_visual(&levels);

        let mut line = LayoutLine {
            range: offset..visible_end,
            direction: if self.writing_mode.is_vertical() {
                harfrust::Direction::TopToBottom
            } else {
                bidi_info
                    .paragraphs
                    .iter()
                    .find(|p| offset >= p.range.start && offset <= p.range.end)
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
        next_offset
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

fn optimal_line_breaks(segments: &[LineBreakMeasure], max_extent: f32) -> Vec<usize> {
    let len = segments.len();
    if len == 0 {
        return Vec::new();
    }
    if !max_extent.is_finite() || max_extent <= 0.0 {
        return vec![len];
    }

    let mut dp = vec![f32::INFINITY; len + 1];
    let mut prev = vec![None; len + 1];
    dp[0] = 0.0;

    for start in 0..len {
        if !dp[start].is_finite() {
            continue;
        }
        let mut advance = 0.0f32;
        for end in start + 1..=len {
            advance += segments[end - 1].advance;
            let suffix_advance = if end < len {
                segments[end - 1].break_suffix_advance
            } else {
                0.0
            };
            let line_advance = advance + suffix_advance;
            let is_single_segment = end == start + 1;
            if line_advance > max_extent && !is_single_segment {
                break;
            }

            let mut cost = dp[start] + line_break_badness(line_advance, max_extent);
            if end < len && suffix_advance > 0.0 {
                cost += LINE_BREAK_HYPHEN_PENALTY;
            }

            if cost < dp[end] {
                dp[end] = cost;
                prev[end] = Some(start);
            }
        }
    }

    if !dp[len].is_finite() {
        return vec![len];
    }

    let mut breaks = Vec::new();
    let mut index = len;
    while index > 0 {
        breaks.push(index);
        let Some(previous) = prev[index] else {
            return vec![len];
        };
        index = previous;
    }
    breaks.reverse();
    breaks
}

fn line_break_badness(line_advance: f32, max_extent: f32) -> f32 {
    if line_advance <= max_extent {
        (max_extent - line_advance).powi(3)
    } else {
        (line_advance - max_extent).powi(3) * LINE_BREAK_OVERFLOW_MULTIPLIER
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
    fn optimal_line_breaks_balance_ragged_lines() {
        let segments = vec![
            LineBreakMeasure {
                advance: 30.0,
                break_suffix_advance: 0.0,
            };
            7
        ];

        assert_eq!(optimal_line_breaks(&segments, 100.0), vec![2, 4, 7]);
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

        // Under the new tight-bounds strategy, the run width is now the actual content width (tightly cropped).
        // The visual centering on the page is handled by the renderer centering this tight sprite.
        assert!(layout.width < max_width);
        assert!(layout.width > 10.0); // Should be around one line height (16px+)

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
    fn horizontal_layout_hyphenates_long_words() -> anyhow::Result<()> {
        let font = any_system_font();
        let text = "antidisestablishmentarianism";
        let font_size = 24.0;
        let unwrapped = TextLayout::new(&font, Some(font_size)).run(text)?;
        let max_width = (unwrapped.lines[0].advance * 0.45).max(font_size * 4.0);

        let layout = TextLayout::new(&font, Some(font_size))
            .with_max_width(max_width)
            .run(text)?;

        assert!(
            layout.lines.len() > 1,
            "expected hyphenation to wrap long word, got {layout:?}"
        );
        for line in layout.lines.iter().take(layout.lines.len() - 1) {
            assert!(
                line.advance <= max_width + 1.0,
                "hyphenated line should fit max width {max_width}, got {}",
                line.advance
            );
        }
        assert!(
            layout
                .lines
                .iter()
                .take(layout.lines.len() - 1)
                .any(|line| line
                    .glyphs
                    .iter()
                    .any(|glyph| glyph.cluster as usize == line.range.end)),
            "expected a synthetic hyphen glyph at a discretionary break"
        );

        Ok(())
    }

    #[test]
    fn horizontal_layout_wraps_chinese_on_jieba_word_boundaries() -> anyhow::Result<()> {
        let font = any_system_font();
        let text = "\u{5357}\u{4eac}\u{5e02}\u{957f}\u{6c5f}\u{5927}\u{6865}";
        let font_size = 24.0;
        let unwrapped = TextLayout::new(&font, Some(font_size)).run(text)?;
        let layout = TextLayout::new(&font, Some(font_size))
            .with_max_width(unwrapped.lines[0].advance * 0.5)
            .run(text)?;

        assert!(
            layout.lines.len() > 1,
            "expected Chinese text to wrap, got {layout:?}"
        );
        assert_eq!(
            &text[layout.lines[0].range.clone()],
            "\u{5357}\u{4eac}\u{5e02}"
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

    #[test]
    fn horizontal_center_alignment_with_overflow_is_aligned_relative_to_widest()
    -> anyhow::Result<()> {
        let font = any_system_font();
        // A very narrow container.
        let max_width = 20.0;
        // A very long word that is guaranteed to overflow 20px in any font.
        let text = "LONGWORDTHATWILLOVERFLOW,\nHI";
        let layout = TextLayout::new(&font, Some(20.0))
            .with_max_width(max_width)
            .with_alignment(TextAlign::Center)
            .run(text)?;

        let w0 = layout.lines[0].advance;
        let w1 = layout.lines[1].advance;

        // Ensure we are actually testing the overflow case.
        assert!(
            w0 > max_width,
            "Test error: widest line {w0} did not overflow max_width {max_width}"
        );

        let c0 = layout.lines[0].baseline.0 + w0 * 0.5;
        let c1 = layout.lines[1].baseline.0 + w1 * 0.5;

        // In a fixed system, the center of the short line should match the center
        // of the overflowing line, NOT the center of the original max_width constraint.
        assert!(
            (c0 - c1).abs() < 1.0,
            "expected line centres to match even with overflow, got c0={c0} c1={c1} (max_width={max_width})",
        );
        Ok(())
    }
}
