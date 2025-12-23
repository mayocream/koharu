use anyhow::Result;
use harfrust::{Direction, Feature, ShaperData, UnicodeBuffer};
use skrifa::raw::TableProvider;

use crate::font::{Font, select_font};

/// A glyph with positioning information.
/// clone of harfrust::PositionedGlyph with glyph_id and cluster
#[derive(Debug, Clone)]
pub struct PositionedGlyph<'a> {
    /// The glyph ID as per the font's glyph set.
    pub glyph_id: u32,
    /// The cluster index in the original text that this glyph corresponds to.
    pub cluster: u32,
    /// Font used to shape this glyph.
    pub font: &'a Font,
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
pub struct ShapedRun<'a> {
    pub glyphs: Vec<PositionedGlyph<'a>>,
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

    pub fn shape<'a>(
        &self,
        text: &str,
        font: &'a Font,
        options: &ShapingOptions,
    ) -> Result<ShapedRun<'a>> {
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
                font,
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

pub(crate) fn shape_segment_with_fallbacks<'a>(
    shaper: &TextShaper,
    segment: &str,
    fonts: &[&'a Font],
    options: &ShapingOptions,
) -> Result<ShapedRun<'a>> {
    if segment.is_empty() || fonts.is_empty() {
        return Ok(ShapedRun {
            glyphs: Vec::new(),
            x_advance: 0.0,
            y_advance: 0.0,
        });
    }

    if fonts.len() == 1 {
        return shaper.shape(segment, fonts[0], options);
    }

    let mut run_start = 0usize;
    let mut run_font_idx: Option<usize> = None;
    let mut combined = ShapedRun {
        glyphs: Vec::new(),
        x_advance: 0.0,
        y_advance: 0.0,
    };

    for (byte_idx, ch) in segment.char_indices() {
        let next_idx = byte_idx + ch.len_utf8();
        let cluster = &segment[byte_idx..next_idx];
        let font_idx = select_font(cluster, fonts);

        match run_font_idx {
            Some(current_idx) if current_idx == font_idx => {}
            Some(current_idx) => {
                let run_text = &segment[run_start..byte_idx];
                if !run_text.is_empty() {
                    let mut run = shaper.shape(run_text, fonts[current_idx], options)?;
                    for glyph in &mut run.glyphs {
                        glyph.cluster += run_start as u32;
                    }
                    combined.x_advance += run.x_advance;
                    combined.y_advance += run.y_advance;
                    combined.glyphs.extend(run.glyphs);
                }
                run_start = byte_idx;
                run_font_idx = Some(font_idx);
            }
            None => {
                run_start = byte_idx;
                run_font_idx = Some(font_idx);
            }
        }
    }

    if let Some(current_idx) = run_font_idx {
        let run_text = &segment[run_start..];
        if !run_text.is_empty() {
            let mut run = shaper.shape(run_text, fonts[current_idx], options)?;
            for glyph in &mut run.glyphs {
                glyph.cluster += run_start as u32;
            }
            combined.x_advance += run.x_advance;
            combined.y_advance += run.y_advance;
            combined.glyphs.extend(run.glyphs);
        }
    }

    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FamilyName, FontBook, Properties};

    const PRIMARY_FAMILIES: &[&str] = &[
        "Arial",
        "Segoe UI",
        "Noto Sans",
        "DejaVu Sans",
        "Liberation Sans",
        "Yu Gothic",
        "MS Gothic",
        "Times New Roman",
    ];
    const FALLBACK_FAMILIES: &[&str] = &[
        "Segoe UI Symbol",
        "Segoe UI Emoji",
        "Noto Sans Symbols",
        "Noto Sans Symbols2",
        "Noto Color Emoji",
        "Apple Color Emoji",
        "Apple Symbols",
        "Symbola",
        "Arial Unicode MS",
    ];
    const SYMBOLS: &[char] = &[
        '\u{1F4A9}',
        '\u{1F642}',
        '\u{2665}',
        '\u{2605}',
        '\u{2713}',
        '\u{2603}',
        '\u{262F}',
        '\u{2691}',
        '\u{26A0}',
    ];

    fn query_font(book: &mut FontBook, name: &str) -> Option<Font> {
        book.query(
            &[FamilyName::Title(name.to_string())],
            &Properties::default(),
        )
        .ok()
    }

    #[test]
    fn shape_segment_uses_fallback_font() -> Result<()> {
        let mut book = FontBook::new();
        let primary = PRIMARY_FAMILIES
            .iter()
            .find_map(|name| query_font(&mut book, name))
            .expect("no primary font available for fallback test");

        let mut chosen = None;
        for ch in SYMBOLS {
            if primary.has_glyph(*ch) {
                continue;
            }
            if let Some(fallback) = FALLBACK_FAMILIES.iter().find_map(|name| {
                let font = query_font(&mut book, name)?;
                font.has_glyph(*ch).then_some(font)
            }) {
                chosen = Some((fallback, *ch));
                break;
            }
        }

        let (fallback, symbol) = chosen.expect("no fallback font with missing primary glyph found");

        let text = symbol.to_string();
        let shaper = TextShaper::new();
        let opts = ShapingOptions {
            direction: harfrust::Direction::LeftToRight,
            font_size: 16.0,
            features: &[],
        };
        let shaped = shape_segment_with_fallbacks(&shaper, &text, &[&primary, &fallback], &opts)?;

        assert!(!shaped.glyphs.is_empty());
        assert!(
            shaped
                .glyphs
                .iter()
                .all(|g| std::ptr::eq(g.font, &fallback))
        );

        Ok(())
    }
}
