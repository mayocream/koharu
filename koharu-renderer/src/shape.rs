use anyhow::Result;
use harfrust::{Direction, Feature, ShaperData, UnicodeBuffer};
use skrifa::raw::TableProvider;

use crate::font::Font;

/// A glyph with positioning information.
/// clone of harfrust::PositionedGlyph with glyph_id and cluster
#[derive(Debug, Clone)]
pub struct PositionedGlyph {
    /// The glyph ID as per the font's glyph set.
    pub glyph_id: u32,
    /// The cluster index in the original text that this glyph corresponds to.
    pub cluster: u32,
    /// How much the line advances after drawing this glyph when setting text in
    /// horizontal direction.
    pub x_advance: f32,
    /// How much the line advances after drawing this glyph when setting text in
    /// vertical direction.
    pub y_advance: f32,
    /// How much the glyph moves on the X-axis before drawing it, this should
    /// not affect how much the line advances.
    pub x_offset: f32,
    /// How much the glyph moves on the Y-axis before drawing it, this should
    /// not affect how much the line advances.
    pub y_offset: f32,
}

/// A shaped run of text, containing positioned glyphs and overall advance.
#[derive(Debug, Clone)]
pub struct ShapedRun {
    pub glyphs: Vec<PositionedGlyph>,
    pub x_advance: f32,
    pub y_advance: f32,
}

/// Options for shaping text.
#[derive(Debug, Clone)]
pub struct ShapingOptions<'a> {
    pub direction: Direction,
    pub font_size: f32,
    pub features: &'a [Feature],
}

/// Text shaper using HarfRust.
///
/// TODO: add shaper plan cache
#[derive(Debug, Clone, Default)]
pub struct TextShaper;

impl TextShaper {
    pub fn new() -> Self {
        Self
    }

    pub fn shape(&self, text: &str, font: &Font, options: &ShapingOptions) -> Result<ShapedRun> {
        let font_ref = font.harfrust()?;

        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.set_direction(options.direction);
        buffer.guess_segment_properties();

        let shaper_data = ShaperData::new(&font_ref);
        let shaper = shaper_data
            .shaper(&font_ref)
            .point_size(Some(options.font_size))
            .build();
        let output = shaper.shape(buffer, options.features);

        let glyph_positions = output.glyph_positions();
        let glyph_infos = output.glyph_infos();

        // Scale factor to convert font units to pixels
        let upem = font.skrifa()?.head()?.units_per_em() as f32;
        let scale = options.font_size / upem;

        let mut positioned_glyphs = Vec::with_capacity(glyph_infos.len());
        for (info, pos) in glyph_infos.iter().zip(glyph_positions.iter()) {
            positioned_glyphs.push(PositionedGlyph {
                glyph_id: info.glyph_id,
                cluster: info.cluster,
                x_offset: (pos.x_offset as f32) * scale,
                y_offset: (pos.y_offset as f32) * scale,
                x_advance: (pos.x_advance as f32) * scale,
                y_advance: (pos.y_advance as f32) * scale,
            });
        }

        Ok(ShapedRun {
            glyphs: positioned_glyphs,
            x_advance: glyph_positions
                .iter()
                .map(|p| (p.x_advance as f32) * scale)
                .sum(),
            y_advance: glyph_positions
                .iter()
                .map(|p| (p.y_advance as f32) * scale)
                .sum(),
        })
    }
}
