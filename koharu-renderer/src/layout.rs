//! Text layout engine with line breaking and glyph positioning.
//!
//! This module handles the conversion of text strings into positioned glyphs,
//! including line breaking, text shaping, and glyph positioning for both
//! horizontal and vertical text layouts.

use std::{collections::VecDeque, ops::Range};

use anyhow::{Result, bail};
use rayon::prelude::*;
use swash::FontRef;
use swash::shape::cluster::Glyph;
use swash::shape::{Direction, ShapeContext};
use swash::text::Script;
use unicode_linebreak::{BreakOpportunity, linebreaks};

use crate::types::{Point, TextStyle};

/// Controls the primary flow of glyph advances and line progression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// Glyph advances grow along the X axis and lines progress on the Y axis.
    /// This is the standard left-to-right, top-to-bottom text layout.
    Horizontal,
    /// Glyph advances grow along the Y axis and lines progress on the X axis.
    /// This is used for vertical text layouts like traditional Chinese/Japanese.
    Vertical,
}

impl Default for Orientation {
    fn default() -> Self {
        Orientation::Horizontal
    }
}

impl Orientation {
    /// Returns the swash Direction for text shaping.
    fn to_swash_direction(self) -> Direction {
        match self {
            Orientation::Horizontal => Direction::LeftToRight,
            Orientation::Vertical => Direction::LeftToRight, // Vertical text still shapes LTR
        }
    }

    /// Calculates baseline position for a line.
    fn baseline_for_line(self, line_index: usize, line_height: f32) -> Point {
        let secondary_offset = line_index as f32 * line_height;
        match self {
            Orientation::Horizontal => (0.0, secondary_offset),
            Orientation::Vertical => (-secondary_offset, 0.0),
        }
    }

    /// Calculates final glyph position.
    fn position_glyph(self, glyph: &Glyph, baseline: Point, primary_offset: f32) -> Point {
        match self {
            Orientation::Horizontal => {
                (baseline.0 + primary_offset + glyph.x, baseline.1 + glyph.y)
            }
            Orientation::Vertical => (baseline.0 + glyph.x, baseline.1 + primary_offset + glyph.y),
        }
    }
}

/// Parameters shared by a layout request.
#[derive(Clone, Copy, Debug)]
pub struct LayoutRequest<'a> {
    pub style: TextStyle<'a>,
    pub text: &'a str,
    pub max_primary_axis: f32,
    pub direction: Orientation,
}

/// Resulting glyph arrangement for a piece of text.
pub type LayoutResult = Vec<LayoutLine>;

/// Glyphs for one line alongside metadata required by the renderer.
#[derive(Debug, Default)]
pub struct LayoutLine {
    /// Positioned glyphs in this line.
    pub glyphs: Vec<Glyph>,
    /// Range in the original text that this line covers.
    pub range: Range<usize>,
    /// Total advance (width for horizontal, height for vertical) of this line.
    pub advance: f32,
    /// Baseline position as (x, y) coordinates.
    pub baseline: Point,
}

/// Shapes text into positioned glyphs with optional line wrapping.
pub struct TextLayouter {
    shape_context: ShapeContext,
}

impl TextLayouter {
    pub fn new() -> Self {
        Self {
            shape_context: ShapeContext::new(),
        }
    }

    pub fn layout(&mut self, request: &LayoutRequest<'_>) -> Result<LayoutResult> {
        if request.style.line_height <= 0.0 {
            bail!("line height must be positive");
        }

        let script = request.style.script.unwrap_or(Script::Latin);
        let font_ref = request.style.font.font_ref()?;
        let shaped_lines = self.shape_lines(font_ref, request, script)?;

        let layout_lines =
            self.place_lines(shaped_lines, request.direction, request.style.line_height);

        Ok(layout_lines)
    }

    fn shape_lines(
        &mut self,
        font_ref: FontRef<'_>,
        request: &LayoutRequest<'_>,
        script: Script,
    ) -> Result<Vec<ShapedLine>> {
        let wrap_limit = wrap_limit(request.max_primary_axis);
        let mut pending: Option<PendingLine> = None;
        let mut shaped_lines = Vec::new();
        let mut start = 0usize;
        let mut breakpoints: VecDeque<_> =
            ensure_terminal_breaks(request.text, linebreaks(request.text).collect()).into();

        while let Some((pos, kind)) = breakpoints.pop_front() {
            if pos < start {
                continue;
            }

            let slice = &request.text[start..pos];
            let trimmed = slice.trim_end_matches(|ch| ch == '\n' || ch == '\r');
            let trimmed_end = start + trimmed.len();
            let shaped_line = self.shape_segment(
                font_ref,
                trimmed,
                script,
                request.style.font_size,
                start..trimmed_end,
                request.direction,
            )?;

            let exceeds =
                wrap_limit.is_some_and(|limit| shaped_line.advance > limit + f32::EPSILON);

            if exceeds {
                if let Some(previous) = pending.take() {
                    shaped_lines.push(previous.line);
                    start = previous.break_pos;
                    breakpoints.push_front((pos, kind));
                    continue;
                } else {
                    shaped_lines.push(shaped_line);
                    start = pos;
                    continue;
                }
            }

            pending = Some(PendingLine {
                break_pos: pos,
                line: shaped_line,
            });

            if matches!(kind, BreakOpportunity::Mandatory) {
                if let Some(next_start) = flush_pending(&mut pending, &mut shaped_lines) {
                    start = next_start;
                }
            }
        }

        flush_pending(&mut pending, &mut shaped_lines);
        Ok(shaped_lines)
    }

    fn shape_segment(
        &mut self,
        font_ref: FontRef<'_>,
        text: &str,
        script: Script,
        font_size: f32,
        range: Range<usize>,
        direction: Orientation,
    ) -> Result<ShapedLine> {
        let swash_direction = direction.to_swash_direction();
        let builder = self
            .shape_context
            .builder(font_ref)
            .script(script)
            .size(font_size.max(0.0))
            .direction(swash_direction);
        let builder = if matches!(direction, Orientation::Vertical) {
            builder.features([("vert", 1u16), ("vrt2", 1u16)])
        } else {
            builder
        };
        let mut shaper = builder.build();
        if !text.is_empty() {
            shaper.add_str(text);
        }
        let glyphs = {
            let mut temp = Vec::new();
            shaper.shape_with(|cluster| temp.extend(cluster.glyphs.iter().copied()));
            temp
        };
        let advance = glyphs.iter().map(|g| g.advance).sum();

        Ok(ShapedLine {
            glyphs,
            advance,
            range,
        })
    }

    /// Positions shaped lines into final layout coordinates with parallel processing.
    fn place_lines(
        &self,
        shaped_lines: Vec<ShapedLine>,
        direction: Orientation,
        line_height: f32,
    ) -> Vec<LayoutLine> {
        shaped_lines
            .into_par_iter()
            .enumerate()
            .map(|(line_index, shaped)| {
                self.place_single_line(shaped, line_index, direction, line_height)
            })
            .collect()
    }

    /// Positions a single line's glyphs with parallel processing.
    fn place_single_line(
        &self,
        shaped: ShapedLine,
        line_index: usize,
        direction: Orientation,
        line_height: f32,
    ) -> LayoutLine {
        let baseline = direction.baseline_for_line(line_index, line_height);

        // Calculate cumulative advances for parallel positioning
        let advances: Vec<f32> = shaped.glyphs.par_iter().map(|g| g.advance).collect();
        let cumulative_advances = self.calculate_cumulative_advances(&advances);

        let glyphs = shaped
            .glyphs
            .into_par_iter()
            .enumerate()
            .map(|(i, glyph)| {
                let primary_offset = cumulative_advances[i];
                let (x, y) = direction.position_glyph(&glyph, baseline, primary_offset);
                Glyph { x, y, ..glyph }
            })
            .collect();

        LayoutLine {
            glyphs,
            range: shaped.range,
            advance: shaped.advance,
            baseline,
        }
    }

    /// Calculates cumulative advances for positioning glyphs.
    fn calculate_cumulative_advances(&self, advances: &[f32]) -> Vec<f32> {
        let mut cumulative = vec![0.0f32; advances.len() + 1];
        for i in 0..advances.len() {
            cumulative[i + 1] = cumulative[i] + advances[i];
        }
        cumulative
    }
}

impl Default for TextLayouter {
    fn default() -> Self {
        Self::new()
    }
}

struct PendingLine {
    break_pos: usize,
    line: ShapedLine,
}

struct ShapedLine {
    glyphs: Vec<Glyph>,
    advance: f32,
    range: Range<usize>,
}

fn flush_pending(pending: &mut Option<PendingLine>, lines: &mut Vec<ShapedLine>) -> Option<usize> {
    pending.take().map(|pending_line| {
        let break_pos = pending_line.break_pos;
        lines.push(pending_line.line);
        break_pos
    })
}

fn wrap_limit(max_primary_axis: f32) -> Option<f32> {
    if max_primary_axis.is_finite() && max_primary_axis > 0.0 {
        Some(max_primary_axis)
    } else {
        None
    }
}

fn ensure_terminal_breaks(
    text: &str,
    mut breaks: Vec<(usize, BreakOpportunity)>,
) -> Vec<(usize, BreakOpportunity)> {
    if let Some((pos, _)) = breaks.last() {
        if *pos != text.len() {
            breaks.push((text.len(), BreakOpportunity::Mandatory));
        }
    } else {
        breaks.push((text.len(), BreakOpportunity::Mandatory));
    }
    breaks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontBook;
    use fontdb::{Family, Query, Stretch, Style, Weight};
    use swash::text::Script;

    fn default_query<'a>(families: &'a [Family<'a>]) -> Query<'a> {
        Query {
            families,
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        }
    }

    #[test]
    fn wraps_text_with_unicode_line_breaks() -> Result<()> {
        let mut book = FontBook::new();
        let mut engine = TextLayouter::new();
        let families = [Family::SansSerif];
        let font = book
            .query(&default_query(&families))?
            .expect("expected sans-serif font for test");
        let request = LayoutRequest {
            style: TextStyle {
                font: &font,
                font_size: 20.0,
                line_height: 28.0,
                color: [0, 0, 0, 255],
                script: Some(Script::Latin),
            },
            text: "Koharu renderer needs thoughtful wrapping for lengthy passages of text.",
            max_primary_axis: 160.0,
            direction: Orientation::Horizontal,
        };
        let result = engine.layout(&request)?;
        assert!(
            result.len() > 1,
            "expecting automatic wrapping to create multiple lines"
        );
        assert!(
            result
                .iter()
                .all(|line| line.advance <= request.max_primary_axis + f32::EPSILON),
            "each line should respect the configured width"
        );
        Ok(())
    }

    #[test]
    fn supports_vertical_layout() -> Result<()> {
        let mut book = FontBook::new();
        let mut engine = TextLayouter::new();
        let families = [Family::SansSerif];
        let font = book
            .query(&default_query(&families))?
            .expect("expected sans-serif font for test");
        let request = LayoutRequest {
            style: TextStyle {
                font: &font,
                font_size: 18.0,
                line_height: 24.0,
                color: [0, 0, 0, 255],
                script: Some(Script::Latin),
            },
            text: "Vertical writing is stacked by this layout engine.",
            max_primary_axis: 140.0,
            direction: Orientation::Vertical,
        };
        let result = engine.layout(&request)?;
        for (index, line) in result.iter().enumerate() {
            let expected = -(index as f32 * request.style.line_height);
            assert!(
                (line.baseline.0 - expected).abs() <= f32::EPSILON,
                "vertical layout should place columns right-to-left"
            );
        }
        Ok(())
    }
}
