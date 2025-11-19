use std::ops::Range;

use anyhow::Result;
use swash::shape::ShapeContext;
use swash::shape::cluster::{Glyph, GlyphCluster};
use swash::shape::partition::{SelectedFont, Selector, ShapeOptions, shape};
use swash::text::cluster::{Boundary, CharCluster, CharInfo, Status, Token};
use swash::text::{Script, analyze};
use swash::{FontRef, Setting};

use crate::font::Font;
use crate::types::Point;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// This is the standard left-to-right, top-to-bottom text layout.
    Horizontal,
    /// This is used for vertical text layouts like traditional Chinese/Japanese.
    Vertical,
}

impl Orientation {
    fn is_vertical(self) -> bool {
        matches!(self, Orientation::Vertical)
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
            Orientation::Vertical => {
                // In vertical layout, primary_offset affects the y-coordinate.
                (baseline.0 + glyph.x, baseline.1 + primary_offset + glyph.y)
            }
        }
    }
}

/// Parameters shared by a layout request.
#[derive(Clone, Copy, Debug)]
pub struct LayoutRequest<'a> {
    pub text: &'a str,
    pub fonts: &'a [Font],
    pub font_size: f32,
    pub line_height: f32,
    pub script: Script,
    pub max_primary_axis: f32,
    pub direction: Orientation,
}

/// Resulting glyph arrangement for a piece of text.
pub type LayoutResult = Vec<LayoutLine>;

/// Glyphs for one line alongside metadata required by the renderer.
#[derive(Debug, Clone, Default)]
pub struct LayoutLine {
    /// Font used for this line.
    pub font: Font,
    /// Positioned glyphs in this line.
    pub glyphs: Vec<Glyph>,
    /// Range in the original text that this line covers.
    pub range: Range<usize>,
    /// Total advance (width for horizontal, height for vertical) of this line.
    pub advance: f32,
    /// Baseline position as (x, y) coordinates.
    pub baseline: Point,
}

struct FontSelector<'a> {
    fonts: &'a [Font],
}

impl<'a> Selector for FontSelector<'a> {
    type SelectedFont = Font;

    fn select_font(&mut self, cluster: &mut CharCluster) -> Option<Self::SelectedFont> {
        for font in self.fonts {
            let charmap = font.font_ref().ok()?.charmap();
            match cluster.map(|ch| charmap.map(ch)) {
                Status::Complete => return Some(font.clone()),
                _ => continue,
            }
        }
        None
    }
}

impl SelectedFont for Font {
    fn font(&self) -> FontRef<'_> {
        self.font_ref().unwrap()
    }
}

impl ShapeOptions for LayoutRequest<'_> {
    type Features = std::vec::IntoIter<Setting<u16>>;

    type Variations = std::iter::Empty<Setting<f32>>;

    fn script(&self) -> Script {
        self.script
    }

    fn size(&self) -> f32 {
        self.font_size
    }

    fn features(&self) -> Self::Features {
        let mut features = vec![];
        if self.direction.is_vertical() {
            features.push(("vert", 1).into());
            features.push(("vrt2", 1).into());
        }
        features.into_iter()
    }

    fn variations(&self) -> Self::Variations {
        std::iter::empty()
    }
}

pub struct Layouter {
    shape_context: ShapeContext,
}

impl Layouter {
    pub fn new() -> Self {
        Self {
            shape_context: ShapeContext::new(),
        }
    }

    pub fn layout(&mut self, request: &LayoutRequest<'_>) -> Result<LayoutResult> {
        let mut lines = Vec::new();
        let mut current_line = LayoutLine::default();
        let mut line_index = 0;
        let mut primary_offset = 0.0;

        let mut selector = FontSelector {
            fonts: request.fonts,
        };

        let tokens = request
            .text
            .char_indices()
            .zip(analyze(request.text.chars()))
            .map(|((i, ch), (props, boundary))| Token {
                ch,
                offset: i as u32,
                len: ch.len_utf8() as u8,
                info: CharInfo::new(props, boundary),
                data: 0,
            });

        // Shape the text using the partition API
        shape(
            &mut self.shape_context,
            &mut selector,
            request,
            tokens,
            |font, shaper| {
                shaper.shape_with(|cluster| {
                    // handle line breaking
                    if should_break_line(cluster, primary_offset, request.max_primary_axis) {
                        // Finalize the current line
                        if !current_line.glyphs.is_empty() {
                            current_line.baseline = request
                                .direction
                                .baseline_for_line(line_index, request.line_height);
                            lines.push(std::mem::take(&mut current_line));
                            line_index += 1;
                        }

                        // Start a new line
                        current_line = LayoutLine::default();
                        current_line.font = font.clone();
                        primary_offset = 0.0;
                    }

                    // ensure the line has a correct font
                    if current_line.glyphs.is_empty() {
                        current_line.font = font.clone();
                    }

                    // add glyphs from this cluster to the current line
                    let cluster_advance = add_cluster_to_line(
                        cluster,
                        &mut current_line,
                        primary_offset,
                        request.direction,
                    );

                    primary_offset += cluster_advance;
                    current_line.advance = primary_offset;

                    // update the range covered by this line
                    let source_range = cluster.source;
                    if current_line.range.is_empty() {
                        current_line.range = source_range.start as usize..source_range.end as usize;
                    } else {
                        current_line.range.end = source_range.end as usize;
                    }
                });
            },
        );

        // Add the last line if it has content
        if !current_line.glyphs.is_empty() {
            current_line.baseline = request
                .direction
                .baseline_for_line(line_index, request.line_height);
            lines.push(current_line);
        }

        Ok(lines)
    }
}

fn should_break_line(cluster: &GlyphCluster, current_offset: f32, max_primary_axis: f32) -> bool {
    // Handle hard line breaks (newlines)
    if cluster.info.boundary() == Boundary::Mandatory {
        return true;
    }

    // Check if we exceed the maximum width/height
    let cluster_advance = cluster.advance();
    let would_exceed = current_offset + cluster_advance > max_primary_axis;

    // Only break if we're at a valid break point and would exceed the limit
    would_exceed
        && (matches!(
            cluster.info.boundary(),
            Boundary::Line | Boundary::Mandatory
        ))
}

fn add_cluster_to_line(
    cluster: &swash::shape::cluster::GlyphCluster,
    line: &mut LayoutLine,
    primary_offset: f32,
    direction: Orientation,
) -> f32 {
    let baseline = line.baseline;
    let mut cluster_advance = 0.0;

    for glyph in cluster.glyphs {
        // Position the glyph
        let mut positioned_glyph = *glyph;
        let pos = direction.position_glyph(glyph, baseline, primary_offset + cluster_advance);

        positioned_glyph.x = pos.0;
        positioned_glyph.y = pos.1;

        line.glyphs.push(positioned_glyph);

        cluster_advance += glyph.advance;
    }

    cluster_advance
}
