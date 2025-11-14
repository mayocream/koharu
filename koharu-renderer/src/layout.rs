use crate::font::{Font, FontBook, FontQuery};
use anyhow::{Result, anyhow, bail};
use std::ops::Range;
use swash::shape::{Direction, ShapeContext};
use swash::text::Script;
use unicode_linebreak::{BreakOpportunity, linebreaks};

/// Controls the primary flow of glyph advances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutOrientation {
    /// Glyph advances grow along the X axis and lines progress on the Y axis.
    Horizontal,
    /// Glyph advances grow along the Y axis and lines progress on the X axis.
    Vertical,
}

impl Default for LayoutOrientation {
    fn default() -> Self {
        LayoutOrientation::Horizontal
    }
}

/// Parameters shared by a layout request.
#[derive(Clone, Copy, Debug)]
pub struct LayoutOptions<'a> {
    pub text: &'a str,
    pub font_query: FontQuery<'a>,
    pub font_size: f32,
    pub max_primary_axis: f32,
    pub line_height: f32,
    pub direction: LayoutOrientation,
}

/// Resulting glyph arrangement for a piece of text.
#[derive(Debug, Default)]
pub struct LayoutOutput {
    pub lines: Vec<LayoutLine>,
    pub direction: LayoutOrientation,
    pub bounds: LayoutBounds,
    pub font_size: f32,
}

/// Captures both the font and layout output for a shaping run.
pub struct LayoutSession {
    pub font: Font,
    pub output: LayoutOutput,
}

/// Bounding box of the entire layout.
#[derive(Debug, Default, Clone, Copy)]
pub struct LayoutBounds {
    pub width: f32,
    pub height: f32,
}

/// Glyphs for one line alongside metadata required by the renderer.
#[derive(Debug, Default)]
pub struct LayoutLine {
    pub glyphs: Vec<PositionedGlyph>,
    pub range: Range<usize>,
    pub advance: f32,
    pub baseline: (f32, f32),
}

/// Positioned glyph ready for rasterization.
#[derive(Debug, Clone, Copy)]
pub struct PositionedGlyph {
    pub id: u16,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
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

    pub fn layout(
        &mut self,
        font_book: &mut FontBook,
        request: &LayoutOptions<'_>,
    ) -> Result<LayoutSession> {
        if request.line_height <= 0.0 {
            bail!("line height must be positive");
        }

        let font = font_book
            .query_font(&request.font_query)?
            .ok_or_else(|| anyhow!("no font matched the provided query"))?;
        let script = request.font_query.script.unwrap_or(Script::Latin);
        let raw_breaks: Vec<_> = linebreaks(request.text).collect();
        let breaks = ensure_terminal_breaks(request.text, raw_breaks);
        let mut pending: Option<PendingLine> = None;
        let mut start = 0usize;
        let mut shaped_lines = Vec::new();
        let unlimited = !request.max_primary_axis.is_finite() || request.max_primary_axis <= 0.0;
        let mut index = 0usize;

        while index < breaks.len() {
            let (pos, kind) = breaks[index];
            if pos < start {
                index += 1;
                continue;
            }

            let slice = &request.text[start..pos];
            let trimmed = slice.trim_end_matches(|ch| ch == '\n' || ch == '\r');
            let trimmed_end = start + trimmed.len();
            let shaped_line = self.shape_segment(
                &font,
                trimmed,
                script,
                request.font_size,
                start..trimmed_end,
                request.direction,
            )?;

            let exceeds =
                !unlimited && shaped_line.advance > request.max_primary_axis + f32::EPSILON;

            if exceeds {
                if let Some(previous) = pending.take() {
                    shaped_lines.push(previous.line);
                    start = previous.break_pos;
                    continue;
                } else {
                    shaped_lines.push(shaped_line);
                    start = pos;
                    index += 1;
                    continue;
                }
            }

            pending = Some(PendingLine {
                break_pos: pos,
                line: shaped_line,
            });

            if matches!(kind, BreakOpportunity::Mandatory) {
                if let Some(line) = pending.take() {
                    shaped_lines.push(line.line);
                    start = line.break_pos;
                }
            }

            index += 1;
        }

        if let Some(line) = pending.take() {
            shaped_lines.push(line.line);
        }

        let layout_lines = self.place_lines(shaped_lines, request.direction, request.line_height);
        let bounds = compute_bounds(&layout_lines, request.direction, request.line_height);

        Ok(LayoutSession {
            font,
            output: LayoutOutput {
                lines: layout_lines,
                direction: request.direction,
                bounds,
                font_size: request.font_size,
            },
        })
    }

    fn shape_segment(
        &mut self,
        font: &Font,
        text: &str,
        script: Script,
        font_size: f32,
        range: Range<usize>,
        direction: LayoutOrientation,
    ) -> Result<ShapedLine> {
        let font_ref = font.font_ref()?;
        let mut builder = self.shape_context.builder(font_ref).script(script);
        builder = builder.size(font_size.max(0.0));
        let swash_direction = match direction {
            LayoutOrientation::Horizontal => Direction::LeftToRight,
            LayoutOrientation::Vertical => Direction::LeftToRight,
        };
        builder = builder.direction(swash_direction);
        if matches!(direction, LayoutOrientation::Vertical) {
            builder = builder.features([("vert", 1u16), ("vrt2", 1u16)]);
        }
        let mut shaper = builder.build();
        if !text.is_empty() {
            shaper.add_str(text);
        }
        let mut glyphs = Vec::new();
        shaper.shape_with(|cluster| {
            for glyph in cluster.glyphs {
                glyphs.push(RawGlyph {
                    id: glyph.id,
                    x: glyph.x,
                    y: glyph.y,
                    advance: glyph.advance,
                });
            }
        });
        let advance = glyphs.iter().map(|g| g.advance).sum();

        Ok(ShapedLine {
            glyphs,
            advance,
            range,
        })
    }

    fn place_lines(
        &self,
        shaped_lines: Vec<ShapedLine>,
        direction: LayoutOrientation,
        line_height: f32,
    ) -> Vec<LayoutLine> {
        let mut positioned = Vec::with_capacity(shaped_lines.len());
        let mut secondary_offset = 0.0f32;
        for shaped in shaped_lines {
            let baseline = match direction {
                LayoutOrientation::Horizontal => (0.0, secondary_offset),
                LayoutOrientation::Vertical => (-secondary_offset, 0.0),
            };
            let mut glyphs = Vec::with_capacity(shaped.glyphs.len());
            let mut primary_offset = 0.0f32;
            for glyph in shaped.glyphs {
                let (x, y) = match direction {
                    LayoutOrientation::Horizontal => {
                        (baseline.0 + primary_offset + glyph.x, baseline.1 + glyph.y)
                    }
                    LayoutOrientation::Vertical => {
                        (baseline.0 + glyph.x, baseline.1 + primary_offset + glyph.y)
                    }
                };
                glyphs.push(PositionedGlyph {
                    id: glyph.id,
                    x,
                    y,
                    advance: glyph.advance,
                });
                primary_offset += glyph.advance;
            }
            positioned.push(LayoutLine {
                glyphs,
                range: shaped.range,
                advance: shaped.advance,
                baseline,
            });
            secondary_offset += line_height;
        }
        positioned
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
    glyphs: Vec<RawGlyph>,
    advance: f32,
    range: Range<usize>,
}

struct RawGlyph {
    id: u16,
    x: f32,
    y: f32,
    advance: f32,
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

fn compute_bounds(
    lines: &[LayoutLine],
    direction: LayoutOrientation,
    line_height: f32,
) -> LayoutBounds {
    if lines.is_empty() {
        return LayoutBounds {
            width: 0.0,
            height: 0.0,
        };
    }
    let max_primary = lines.iter().fold(0.0f32, |acc, line| acc.max(line.advance));
    let secondary = line_height * lines.len() as f32;
    match direction {
        LayoutOrientation::Horizontal => LayoutBounds {
            width: max_primary,
            height: secondary,
        },
        LayoutOrientation::Vertical => LayoutBounds {
            width: secondary,
            height: max_primary,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontBook, FontQuery};
    use fontdb::Family;
    use swash::text::Script;

    #[test]
    fn wraps_text_with_unicode_line_breaks() -> Result<()> {
        let mut book = FontBook::new();
        let mut engine = TextLayouter::new();
        let families = [Family::SansSerif];
        let request = LayoutOptions {
            text: "Koharu renderer needs thoughtful wrapping for lengthy passages of text.",
            font_query: FontQuery::new(&families).with_script(Script::Latin),
            font_size: 20.0,
            max_primary_axis: 160.0,
            line_height: 28.0,
            direction: LayoutOrientation::Horizontal,
        };
        let LayoutSession { output, .. } = engine.layout(&mut book, &request)?;
        assert!(
            output.lines.len() > 1,
            "expecting automatic wrapping to create multiple lines"
        );
        assert!(
            output
                .lines
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
        let request = LayoutOptions {
            text: "Vertical writing is stacked by this layout engine.",
            font_query: FontQuery::new(&families).with_script(Script::Latin),
            font_size: 18.0,
            max_primary_axis: 140.0,
            line_height: 24.0,
            direction: LayoutOrientation::Vertical,
        };
        let LayoutSession { output, .. } = engine.layout(&mut book, &request)?;
        for (index, line) in output.lines.iter().enumerate() {
            let expected = -(index as f32 * request.line_height);
            assert!(
                (line.baseline.0 - expected).abs() <= f32::EPSILON,
                "vertical layout should place columns right-to-left"
            );
        }
        Ok(())
    }
}
