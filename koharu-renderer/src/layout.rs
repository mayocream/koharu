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

#[derive(Default)]
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
        // Buffers used for horizontal layout to allow early breaking on word boundaries.
        let mut pending_word: Vec<OwnedCluster> = Vec::new();
        let mut pending_whitespace: Vec<OwnedCluster> = Vec::new();

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
                    let min_size = if request.direction == Orientation::Vertical {
                        request.font_size as u32
                    } else {
                        0 // in horizontal layout, we don't enforce a minimum advance
                    };
                    if request.direction.is_vertical() {
                        if should_break_line(
                            cluster,
                            primary_offset,
                            request.max_primary_axis,
                            min_size,
                        ) {
                            finalize_line(
                                &mut current_line,
                                &mut lines,
                                &mut line_index,
                                request,
                            );
                            current_line.font = font.clone();
                            primary_offset = 0.0;
                        }

                        if current_line.glyphs.is_empty() {
                            current_line.font = font.clone();
                        }

                        let cluster_advance = add_cluster_to_line(
                            cluster,
                            &mut current_line,
                            primary_offset,
                            request,
                        );

                        primary_offset += cluster_advance;
                        current_line.advance = primary_offset;

                        let source_range = cluster.source;
                        if current_line.range.is_empty() {
                            current_line.range =
                                source_range.start as usize..source_range.end as usize;
                        } else {
                            current_line.range.end = source_range.end as usize;
                        }
                        return;
                    }

                    let owned_cluster = OwnedCluster::new(cluster, font.clone());

                    // Handle hard line breaks immediately.
                    if cluster.info.boundary() == Boundary::Mandatory {
                        flush_word(
                            &mut pending_whitespace,
                            &mut pending_word,
                            &mut current_line,
                            &mut lines,
                            &mut line_index,
                            &mut primary_offset,
                            request,
                        );
                        finalize_line(
                            &mut current_line,
                            &mut lines,
                            &mut line_index,
                            request,
                        );
                        pending_whitespace.clear();
                        pending_word.clear();
                        primary_offset = 0.0;
                        return;
                    }

                    if cluster.info.is_whitespace() {
                        // Word boundary reached; decide whether to place the buffered word.
                        flush_word(
                            &mut pending_whitespace,
                            &mut pending_word,
                            &mut current_line,
                            &mut lines,
                            &mut line_index,
                            &mut primary_offset,
                            request,
                        );
                        pending_whitespace.push(owned_cluster);
                        return;
                    }

                    // Non-whitespace: keep buffering the current word so we can measure it as a whole.
                    pending_word.push(owned_cluster);
                });
            },
        );

        if !request.direction.is_vertical() {
            flush_word(
                &mut pending_whitespace,
                &mut pending_word,
                &mut current_line,
                &mut lines,
                &mut line_index,
                &mut primary_offset,
                request,
            );
        }

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

/// Calculate the bounding box of the layout
pub fn calculate_bounds(layout: &LayoutResult) -> (f32, f32, f32, f32) {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    for line in layout {
        let baseline_x = line.baseline.0;
        let baseline_y = line.baseline.1;

        for glyph in &line.glyphs {
            let glyph_x = baseline_x + glyph.x;
            let glyph_y = baseline_y + glyph.y;

            min_x = min_x.min(glyph_x);
            min_y = min_y.min(glyph_y);
            max_x = max_x.max(glyph_x + glyph.advance);
            max_y = max_y.max(glyph_y);
        }
    }

    (min_x, min_y, max_x, max_y)
}

#[derive(Clone)]
struct OwnedCluster {
    glyphs: Vec<Glyph>,
    source_range: Range<usize>,
    font: Font,
}

impl OwnedCluster {
    fn new(cluster: &GlyphCluster, font: Font) -> Self {
        Self {
            glyphs: cluster.glyphs.to_vec(),
            source_range: cluster.source.to_range(),
            font,
        }
    }
}

fn cluster_advance_for_layout(cluster: &OwnedCluster, request: &LayoutRequest<'_>) -> f32 {
    cluster.glyphs.iter().fold(0.0, |advance, glyph| {
        advance
            + if request.direction.is_vertical() {
                request.font_size.max(glyph.advance) * 1.08
            } else {
                glyph.advance
            }
    })
}

fn add_owned_cluster_to_line(
    cluster: &OwnedCluster,
    line: &mut LayoutLine,
    primary_offset: f32,
    request: &LayoutRequest<'_>,
) -> f32 {
    if line.glyphs.is_empty() {
        line.font = cluster.font.clone();
    }

    let baseline = line.baseline;
    let mut cluster_advance = 0.0;

    for glyph in &cluster.glyphs {
        let mut positioned_glyph = *glyph;
        let pos = request
            .direction
            .position_glyph(glyph, baseline, primary_offset + cluster_advance);

        positioned_glyph.x = pos.0;
        positioned_glyph.y = pos.1;

        line.glyphs.push(positioned_glyph);

        cluster_advance += if request.direction.is_vertical() {
            request.font_size.max(glyph.advance) * 1.08
        } else {
            glyph.advance
        };
    }

    if line.range.is_empty() {
        line.range = cluster.source_range.clone();
    } else {
        line.range.end = cluster.source_range.end;
    }

    cluster_advance
}

fn finalize_line(
    current_line: &mut LayoutLine,
    lines: &mut Vec<LayoutLine>,
    line_index: &mut usize,
    request: &LayoutRequest<'_>,
) {
    if current_line.glyphs.is_empty() {
        return;
    }

    current_line.baseline = request
        .direction
        .baseline_for_line(*line_index, request.line_height);
    lines.push(std::mem::take(current_line));
    *line_index += 1;
}

fn flush_word(
    pending_whitespace: &mut Vec<OwnedCluster>,
    pending_word: &mut Vec<OwnedCluster>,
    current_line: &mut LayoutLine,
    lines: &mut Vec<LayoutLine>,
    line_index: &mut usize,
    primary_offset: &mut f32,
    request: &LayoutRequest<'_>,
) {
    if pending_word.is_empty() {
        return;
    }

    let whitespace_advance: f32 = pending_whitespace
        .iter()
        .map(|cluster| cluster_advance_for_layout(cluster, request))
        .sum();
    let word_advance: f32 = pending_word
        .iter()
        .map(|cluster| cluster_advance_for_layout(cluster, request))
        .sum();

    let would_exceed =
        *primary_offset + whitespace_advance + word_advance > request.max_primary_axis;
    let line_has_content = !current_line.glyphs.is_empty();

    if request.direction == Orientation::Horizontal && line_has_content && would_exceed {
        finalize_line(current_line, lines, line_index, request);
        *primary_offset = 0.0;
        // Drop buffered whitespace so we don't start the next line with spaces.
        pending_whitespace.clear();
    }

    for cluster in pending_whitespace.iter().chain(pending_word.iter()) {
        let advance =
            add_owned_cluster_to_line(cluster, current_line, *primary_offset, request);
        *primary_offset += advance;
    }

    current_line.advance = *primary_offset;
    pending_whitespace.clear();
    pending_word.clear();
}

fn should_break_line(
    cluster: &GlyphCluster,
    current_offset: f32,
    max_primary_axis: f32,
    min_advance: u32,
) -> bool {
    // Handle hard line breaks (newlines)
    if cluster.info.boundary() == Boundary::Mandatory {
        return true;
    }

    // Check if we exceed the maximum width/height
    let mut cluster_advance = 0.;

    cluster.glyphs.iter().for_each(|glyph| {
        cluster_advance += glyph.advance.max(min_advance as f32);
    });
    let would_exceed = current_offset + cluster_advance > max_primary_axis;

    // Only break if we're at a valid break point and would exceed the limit
    would_exceed
        && (matches!(
            cluster.info.boundary(),
            Boundary::Line | Boundary::Mandatory
        ) || cluster.info.is_whitespace())
}

fn add_cluster_to_line(
    cluster: &swash::shape::cluster::GlyphCluster,
    line: &mut LayoutLine,
    primary_offset: f32,
    request: &LayoutRequest<'_>,
) -> f32 {
    let baseline = line.baseline;
    let mut cluster_advance = 0.0;

    for glyph in cluster.glyphs {
        // Position the glyph
        let mut positioned_glyph = *glyph;
        let pos =
            request
                .direction
                .position_glyph(glyph, baseline, primary_offset + cluster_advance);

        positioned_glyph.x = pos.0;
        positioned_glyph.y = pos.1;

        line.glyphs.push(positioned_glyph);

        cluster_advance += if request.direction.is_vertical() {
            // Right now latin characters inside non-latin text is rotated, so we always use font_size as advance
            request.font_size.max(glyph.advance) * 1.08
        } else {
            glyph.advance
        }
    }

    cluster_advance
}
