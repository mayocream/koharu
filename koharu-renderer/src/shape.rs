use anyhow::Result;
use harfrust::{Direction, Feature, Script, ShaperData, UnicodeBuffer};
use skrifa::raw::TableProvider;
use unicode_bidi::BidiInfo;

use crate::font::Font;

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
    pub script: Option<Script>,
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

    #[tracing::instrument(level = "debug", skip_all)]
    pub fn shape<'a>(
        &self,
        text: &str,
        font: &'a Font,
        options: &ShapingOptions,
    ) -> Result<ShapedRun<'a>> {
        let font_ref = font.harfrust()?;

        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        buffer.set_direction(options.direction);
        if let Some(script) = options.script {
            buffer.set_script(script);
        }

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

#[tracing::instrument(level = "debug", skip_all)]
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

    // Perform proper BiDi resolution and itemize by script runs.
    let bidi_info = BidiInfo::new(segment, None);

    if bidi_info.paragraphs.is_empty() {
        return Ok(ShapedRun {
            glyphs: Vec::new(),
            x_advance: 0.0,
            y_advance: 0.0,
        });
    }

    // Perform proper BiDi resolution and itemize by script runs.
    let bidi_info = BidiInfo::new(segment, None);

    if bidi_info.paragraphs.is_empty() {
        return Ok(ShapedRun {
            glyphs: Vec::new(),
            x_advance: 0.0,
            y_advance: 0.0,
        });
    }

    let para = &bidi_info.paragraphs[0];
    let line = para.range.clone();
    let (run_levels, visual_runs) = bidi_info.visual_runs(para, line);
    
    let script_map = icu::properties::CodePointMapData::<icu::properties::props::Script>::new();
    let mut all_glyphs = Vec::new();
    let (mut total_x_advance, mut total_y_advance) = (0.0, 0.0);

    for (i, run_range) in visual_runs.into_iter().enumerate() {
        let run_text = &segment[run_range.clone()];
        let run_level = run_levels[i];

        let run_direction = if run_level.is_rtl() {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        };

        // Sub-itemize the bidi run by script to handle font fallbacks correctly.
        let mut char_iter = run_text.char_indices().peekable();
        while let Some((start_in_run, ch)) = char_iter.next() {
            let mut script = script_map.get(ch);
            let mut end_in_run = start_in_run + ch.len_utf8();

            while let Some(&(next_start, next_ch)) = char_iter.peek() {
                let next_script = script_map.get(next_ch);
                if next_script == script
                    || next_script == icu::properties::props::Script::Common
                    || next_script == icu::properties::props::Script::Inherited
                {
                    char_iter.next();
                    end_in_run = next_start + next_ch.len_utf8();
                } else if script == icu::properties::props::Script::Common
                    || script == icu::properties::props::Script::Inherited
                {
                    script = next_script;
                    char_iter.next();
                    end_in_run = next_start + next_ch.len_utf8();
                } else {
                    break;
                }
            }

            let script_run_text = &run_text[start_in_run..end_in_run];
            let absolute_start = run_range.start + start_in_run;

            // Find the best font for this script run.
            let mut chosen_font = fonts[0];
            for font in fonts {
                if script_run_text.chars().all(|c| font.has_glyph(c)) {
                    chosen_font = font;
                    break;
                }
            }

            let mut run_opts = options.clone();
            run_opts.direction = run_direction;

            let mut shaped = shaper.shape(script_run_text, chosen_font, &run_opts)?;
            for glyph in &mut shaped.glyphs {
                glyph.cluster += absolute_start as u32;
            }
            
            total_x_advance += shaped.x_advance;
            total_y_advance += shaped.y_advance;
            all_glyphs.extend(shaped.glyphs);
        }
    }

    Ok(ShapedRun {
        glyphs: all_glyphs,
        x_advance: total_x_advance,
        y_advance: total_y_advance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontBook;

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
        let post_script_name = book
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
            .filter(|post_script_name| !post_script_name.is_empty())?;
        book.query(&post_script_name).ok()
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
            script: None,
            font_size: 16.0,
            features: &[],
        };
        let shaped = shape_segment_with_fallbacks(&shaper, &text, &[&primary, &fallback], &opts)?;

        assert!(!shaped.glyphs.is_empty());
        assert!(shaped
            .glyphs
            .iter()
            .all(|g| std::ptr::eq(g.font, &fallback)));

        Ok(())
    }

    #[test]
    fn shape_arabic_with_fallback_preserves_context() -> Result<()> {
        let mut book = FontBook::new();
        let primary = PRIMARY_FAMILIES
            .iter()
            .find_map(|name| query_font(&mut book, name))
            .expect("no primary font available");
        let fallback = FALLBACK_FAMILIES
            .iter()
            .find_map(|name| query_font(&mut book, name))
            .expect("no fallback font available");

        let shaper = TextShaper::new();
        let opts = ShapingOptions {
            direction: harfrust::Direction::RightToLeft,
            script: Some(harfrust::Script::from_iso15924_tag(harfrust::Tag::new(b"Arab")).unwrap()),
            font_size: 16.0,
            features: &[],
        };

        // "مرحبا" (Marhaba)
        let text = "مرحبا";
        let shaped = shape_segment_with_fallbacks(&shaper, text, &[&primary, &fallback], &opts)?;

        assert!(!shaped.glyphs.is_empty());
        // Verify it used a consistent font for the whole run to ensure joining.
        let font_at_start = shaped.glyphs[0].font;
        assert!(shaped
            .glyphs
            .iter()
            .all(|g| std::ptr::eq(g.font, font_at_start)));

        Ok(())
    }

    #[test]
    fn shape_rtl_multi_run_preserves_visual_order() -> Result<()> {
        let mut book = FontBook::new();
        let font = PRIMARY_FAMILIES
            .iter()
            .find_map(|name| query_font(&mut book, name))
            .expect("no font available");

        let shaper = TextShaper::new();
        let opts = ShapingOptions {
            direction: harfrust::Direction::RightToLeft,
            script: Some(harfrust::Script::from_iso15924_tag(harfrust::Tag::new(b"Arab")).unwrap()),
            font_size: 16.0,
            features: &[],
        };

        // Mixed script: Arabic + Hebrew (will be detected as separate script runs).
        let text = "مرحبا שלום";
        let shaped = shape_segment_with_fallbacks(&shaper, text, &[&font], &opts)?;

        let clusters: Vec<u32> = shaped.glyphs.iter().map(|g| g.cluster).collect();
        // In RTL, visual order is reverse logical. 
        // The first visual glyph should be from the Hebrew part (latter part of string).
        assert!(!clusters.is_empty());
        assert!(clusters[0] > clusters[clusters.len() - 1]);

        Ok(())
    }
}
