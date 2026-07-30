//! Unicode-aware text shaping, line breaking, and layout.

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};
use unicode_bidi::BidiInfo;

use anyhow::Result;
use harfrust::{Feature, Tag};
use hypher::Lang;
use skrifa::{
    MetadataProvider,
    instance::Size,
    outline::{DrawSettings, OutlinePen},
};

use crate::{
    font::{Font, font_key},
    script::shaping_direction_for_text,
    segment::{LineBreakSuffix, LineBreaker, hyphenation_lang_from_tag},
    shape::{PositionedGlyph, ShapedRun, ShapingOptions, TextShaper, shape_script_runs},
    types::TextAlign,
};

const HYPHENATION_MIN_WORD_LEN: usize = 8;
const LINE_BREAK_HYPHEN_PENALTY: f32 = 2_000.0;
const LINE_BREAK_OVERFLOW_MULTIPLIER: f32 = 10_000.0;
const COMIC_LINE_OVERFLOW_PENALTY: f32 = 1_000_000.0;
const COMIC_MAX_LINES: usize = 64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HyphenationPolicy {
    /// Do not introduce discretionary hyphenation opportunities.
    Disabled,
    /// Use a discretionary hyphen only when the unhyphenated text overflows.
    LastResort,
    /// Consider discretionary hyphens during normal line optimization.
    #[default]
    Normal,
}

/// Writing mode for text layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WritingMode {
    /// Horizontal text, left-to-right, lines flow top-to-bottom.
    #[default]
    Horizontal,
    /// Vertical text, right-to-left columns (traditional CJK).
    VerticalRl,
    /// Vertical text, left-to-right columns.
    VerticalLr,
}

impl WritingMode {
    /// Returns true if the writing mode is vertical.
    pub const fn is_vertical(self) -> bool {
        matches!(self, WritingMode::VerticalRl | WritingMode::VerticalLr)
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
    overflowed: bool,
    placement_offset_x: f32,
    placement_offset_y: f32,
}

impl LayoutRun<'_> {
    #[must_use]
    pub const fn overflowed(&self) -> bool {
        self.overflowed
    }

    pub(crate) const fn placement_offset_x(&self) -> f32 {
        self.placement_offset_x
    }

    pub(crate) const fn placement_offset_y(&self) -> f32 {
        self.placement_offset_y
    }
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
    break_penalty: f32,
    break_suffix: Option<ShapedBreakSuffix<'a>>,
}

#[derive(Clone, Copy, Debug)]
struct LineBreakMeasure {
    advance: f32,
    break_suffix_advance: f32,
    break_penalty: f32,
    is_mandatory: bool,
}

#[derive(Clone, Copy, Debug)]
struct LineProfile {
    width: f32,
    center_offset: f32,
}

#[derive(Debug)]
struct LineBreakResult {
    breaks: Vec<usize>,
    profiles: Vec<LineProfile>,
    overflowed: bool,
    cost: f32,
}

#[derive(Clone, Debug)]
struct ComicBalloon {
    width: f32,
    height: f32,
    contour: Vec<(f32, f32)>,
    vertical_alignment: f32,
    minimum_air: f32,
    edge_pixels: Arc<[(i32, i32)]>,
}

#[derive(Clone)]
pub struct TextLayout<'a> {
    writing_mode: WritingMode,
    center_vertical_punctuation: bool,
    hyphenation_lang: Option<Lang>,
    hyphenation_policy: HyphenationPolicy,
    comic_balloon: Option<ComicBalloon>,
    font: &'a Font,
    fallback_fonts: &'a [Font],
    font_size: Option<f32>,
    min_font_size: Option<f32>,
    max_font_size: Option<f32>,
    max_width: Option<f32>,
    max_height: Option<f32>,
    alignment: Option<TextAlign>,
    line_height: Option<f32>,
    min_line_height: Option<f32>,
    letter_spacing: f32,
    word_spacing: f32,
    compact_emphasis_punctuation: bool,
}

fn largest_fitting_font_size<T>(
    minimum: f32,
    maximum: f32,
    mut layout_at: impl FnMut(f32) -> Result<T>,
    fits: impl Fn(&T) -> bool,
) -> Result<Option<T>> {
    let step = ((maximum - minimum) / 64.0).max(0.25);
    let mut size = maximum;
    let mut larger_non_fit = None;

    loop {
        let candidate = layout_at(size)?;
        if fits(&candidate) {
            let Some(mut high) = larger_non_fit else {
                return Ok(Some(candidate));
            };
            let mut low = size;
            let mut best = candidate;
            let mut iterations = 0u32;
            while high - low > 0.01 && iterations < 12 {
                iterations += 1;
                let midpoint = (low + high) * 0.5;
                let candidate = layout_at(midpoint)?;
                if fits(&candidate) {
                    best = candidate;
                    low = midpoint;
                } else {
                    high = midpoint;
                }
            }
            return Ok(Some(best));
        }

        larger_non_fit = Some(size);
        if size <= minimum {
            return Ok(None);
        }
        size = (size - step).max(minimum);
    }
}

fn rasterize_edge_segment(
    pixels: &mut HashSet<(i32, i32)>,
    (x0, y0): (f32, f32),
    (x1, y1): (f32, f32),
) {
    let steps = ((x1 - x0).abs().max((y1 - y0).abs()) * 2.0).ceil().max(1.0) as usize;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        pixels.insert((
            (x0 + (x1 - x0) * t).round() as i32,
            (y0 + (y1 - y0) * t).round() as i32,
        ));
    }
}

fn rasterize_contour_edge(width: f32, height: f32, contour: &[(f32, f32)]) -> Vec<(i32, i32)> {
    let mut pixels = HashSet::new();
    if contour.len() >= 3 {
        for index in 0..contour.len() {
            rasterize_edge_segment(
                &mut pixels,
                contour[index],
                contour[(index + 1) % contour.len()],
            );
        }
    } else {
        let center = (width * 0.5, height * 0.5);
        let radii = (width * 0.5, height * 0.5);
        let samples = ((width + height) * std::f32::consts::PI).ceil().max(16.0) as usize;
        let mut previous = (center.0 + radii.0, center.1);
        for sample in 1..=samples {
            let angle = std::f32::consts::TAU * sample as f32 / samples as f32;
            let point = (
                center.0 + radii.0 * angle.cos(),
                center.1 + radii.1 * angle.sin(),
            );
            rasterize_edge_segment(&mut pixels, previous, point);
            previous = point;
        }
    }
    let mut pixels = pixels.into_iter().collect::<Vec<_>>();
    pixels.sort_unstable();
    pixels
}

struct EdgePixelPen<'a> {
    pixels: &'a mut HashSet<(i32, i32)>,
    origin: (f32, f32),
    current: (f32, f32),
    start: (f32, f32),
}

impl EdgePixelPen<'_> {
    fn screen_point(&self, point: (f32, f32)) -> (f32, f32) {
        (self.origin.0 + point.0, self.origin.1 - point.1)
    }

    fn line_to_point(&mut self, point: (f32, f32)) {
        rasterize_edge_segment(
            self.pixels,
            self.screen_point(self.current),
            self.screen_point(point),
        );
        self.current = point;
    }

    fn curve_steps(points: &[(f32, f32)]) -> usize {
        points
            .windows(2)
            .map(|pair| {
                (pair[1].0 - pair[0].0)
                    .abs()
                    .max((pair[1].1 - pair[0].1).abs())
            })
            .sum::<f32>()
            .ceil()
            .clamp(4.0, 256.0) as usize
    }
}

impl OutlinePen for EdgePixelPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.current = (x, y);
        self.start = (x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.line_to_point((x, y));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let start = self.current;
        let control = (cx, cy);
        let end = (x, y);
        let steps = Self::curve_steps(&[start, control, end]);
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let one_minus_t = 1.0 - t;
            self.line_to_point((
                one_minus_t * one_minus_t * start.0
                    + 2.0 * one_minus_t * t * control.0
                    + t * t * end.0,
                one_minus_t * one_minus_t * start.1
                    + 2.0 * one_minus_t * t * control.1
                    + t * t * end.1,
            ));
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let start = self.current;
        let control0 = (cx0, cy0);
        let control1 = (cx1, cy1);
        let end = (x, y);
        let steps = Self::curve_steps(&[start, control0, control1, end]);
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let one_minus_t = 1.0 - t;
            self.line_to_point((
                one_minus_t.powi(3) * start.0
                    + 3.0 * one_minus_t * one_minus_t * t * control0.0
                    + 3.0 * one_minus_t * t * t * control1.0
                    + t.powi(3) * end.0,
                one_minus_t.powi(3) * start.1
                    + 3.0 * one_minus_t * one_minus_t * t * control0.1
                    + 3.0 * one_minus_t * t * t * control1.1
                    + t.powi(3) * end.1,
            ));
        }
    }

    fn close(&mut self) {
        self.line_to_point(self.start);
    }
}

impl<'a> TextLayout<'a> {
    #[must_use]
    pub fn new(font: &'a Font) -> Self {
        Self {
            writing_mode: WritingMode::Horizontal,
            center_vertical_punctuation: true,
            hyphenation_lang: Some(Lang::English),
            hyphenation_policy: HyphenationPolicy::Normal,
            comic_balloon: None,
            font,
            fallback_fonts: &[],
            font_size: None,
            min_font_size: None,
            max_font_size: None,
            max_width: None,
            max_height: None,
            alignment: None,
            line_height: None,
            min_line_height: None,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            compact_emphasis_punctuation: false,
        }
    }

    #[must_use]
    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = Some(size);
        self.min_font_size = None;
        self.max_font_size = None;
        self
    }

    /// Automatically fit the text to its bounds without growing beyond `size`.
    #[must_use]
    pub fn with_max_font_size(mut self, size: f32) -> Self {
        self.font_size = None;
        self.max_font_size = Some(size);
        self
    }

    /// Prevent automatically fitted text from becoming unreadably small.
    #[must_use]
    pub fn with_min_font_size(mut self, size: f32) -> Self {
        self.font_size = None;
        self.min_font_size = Some(size);
        self
    }

    #[must_use]
    pub fn with_writing_mode(mut self, mode: WritingMode) -> Self {
        self.writing_mode = mode;
        self
    }

    #[must_use]
    pub fn with_center_vertical_punctuation(mut self, enabled: bool) -> Self {
        self.center_vertical_punctuation = enabled;
        self
    }

    #[must_use]
    pub fn with_hyphenation_language(mut self, lang: Lang) -> Self {
        self.hyphenation_lang = Some(lang);
        self
    }

    #[must_use]
    pub fn with_hyphenation_language_tag(mut self, tag: &str) -> Self {
        self.hyphenation_lang = hyphenation_lang_from_tag(tag);
        self
    }

    #[must_use]
    pub fn without_hyphenation(mut self) -> Self {
        self.hyphenation_lang = None;
        self.hyphenation_policy = HyphenationPolicy::Disabled;
        self
    }

    #[must_use]
    pub fn with_hyphenation_policy(mut self, policy: HyphenationPolicy) -> Self {
        self.hyphenation_policy = policy;
        self
    }

    pub(crate) fn with_comic_balloon(
        mut self,
        width: f32,
        height: f32,
        contour: Vec<(f32, f32)>,
        vertical_alignment: f32,
        minimum_air: f32,
    ) -> Self {
        let edge_pixels = rasterize_contour_edge(width, height, &contour);
        self.comic_balloon = Some(ComicBalloon {
            width,
            height,
            contour,
            vertical_alignment: vertical_alignment.clamp(0.0, 1.0),
            minimum_air: minimum_air.max(0.0),
            edge_pixels: edge_pixels.into(),
        });
        self
    }

    #[must_use]
    pub fn with_fallback_fonts(mut self, fonts: &'a [Font]) -> Self {
        self.fallback_fonts = fonts;
        self
    }

    #[must_use]
    pub fn with_max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    #[must_use]
    pub fn with_max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }

    #[must_use]
    pub fn with_alignment(mut self, alignment: TextAlign) -> Self {
        self.alignment = Some(alignment);
        self
    }

    #[must_use]
    pub fn with_line_height(mut self, ratio: f32) -> Self {
        self.line_height = Some(ratio);
        self
    }

    /// Allow auto-fit to tighten leading only when the readable size floor still overflows.
    #[must_use]
    pub fn with_min_line_height(mut self, ratio: f32) -> Self {
        self.min_line_height = Some(ratio);
        self
    }

    #[must_use]
    pub fn with_spacing(mut self, letter: f32, word: f32) -> Self {
        self.letter_spacing = letter;
        self.word_spacing = word;
        self
    }

    pub(crate) fn with_compact_emphasis_punctuation(mut self, enabled: bool) -> Self {
        self.compact_emphasis_punctuation = enabled;
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
        let maximum = self.max_font_size.unwrap_or(300.0).max(0.5);
        let minimum = self
            .min_font_size
            .unwrap_or(maximum.min(1.0))
            .max(0.5)
            .min(maximum);
        let fits = |layout: &LayoutRun<'_>| {
            !layout.overflowed()
                && layout.width <= max_width + f32::EPSILON
                && layout.height <= max_height + f32::EPSILON
        };

        if self.comic_balloon.is_some() {
            // A balloon's usable width changes when the text reflows to a different
            // number of lines, so a smaller font can fail even though a larger one
            // fits. Search from largest to smallest instead of assuming monotonicity.
            if let Some(best) = largest_fitting_font_size(
                minimum,
                maximum,
                |size| self.run_with_size(text, size),
                fits,
            )? {
                if best.lines.len() > 1
                    && max_height.is_finite()
                    && best.height < max_height * 0.6
                    && let Some(preferred_line_height) = self.line_height
                {
                    let mut relaxed = self.clone();
                    let maximum_line_height = preferred_line_height * 1.125;
                    let mut low = preferred_line_height;
                    let mut high = maximum_line_height;
                    let mut relaxed_best = best.clone();
                    let mut iterations = 0u32;
                    while high - low > 0.001 && iterations < 12 {
                        iterations += 1;
                        let line_height = (low + high) * 0.5;
                        relaxed.line_height = Some(line_height);
                        let candidate = relaxed.run_with_size(text, best.font_size)?;
                        if fits(&candidate) {
                            relaxed_best = candidate;
                            low = line_height;
                        } else {
                            high = line_height;
                        }
                    }
                    tracing::info!(
                        iterations,
                        font_size = relaxed_best.font_size,
                        line_height = low,
                        "auto_size loosened leading"
                    );
                    return Ok(relaxed_best);
                }
                tracing::info!(font_size = best.font_size, "auto_size done");
                return Ok(best);
            }

            let Some(preferred_line_height) = self.line_height else {
                return self.run_with_size(text, minimum);
            };
            let minimum_line_height = self
                .min_line_height
                .unwrap_or(preferred_line_height)
                .max(0.1)
                .min(preferred_line_height);
            if minimum_line_height >= preferred_line_height {
                return self.run_with_size(text, minimum);
            }

            let mut compact = self.clone();
            compact.line_height = Some(minimum_line_height);
            let Some(mut best) = largest_fitting_font_size(
                minimum,
                maximum,
                |size| compact.run_with_size(text, size),
                fits,
            )?
            else {
                return compact.run_with_size(text, minimum);
            };

            let font_size = best.font_size;
            let mut low = minimum_line_height;
            let mut high = preferred_line_height;
            let mut iterations = 0u32;
            while high - low > 0.001 && iterations < 12 {
                iterations += 1;
                let ratio = (low + high) * 0.5;
                compact.line_height = Some(ratio);
                let layout = compact.run_with_size(text, font_size)?;
                if fits(&layout) {
                    best = layout;
                    low = ratio;
                } else {
                    high = ratio;
                }
            }
            tracing::info!(
                iterations,
                font_size = best.font_size,
                line_height = low,
                "auto_size tightened leading"
            );
            return Ok(best);
        }

        let maximum_layout = self.run_with_size(text, maximum)?;
        if fits(&maximum_layout) {
            return Ok(maximum_layout);
        }

        let mut best = self.run_with_size(text, minimum)?;
        if !fits(&best) {
            let Some(preferred_line_height) = self.line_height else {
                return Ok(best);
            };
            let minimum_line_height = self
                .min_line_height
                .unwrap_or(preferred_line_height)
                .max(0.1)
                .min(preferred_line_height);
            if minimum_line_height >= preferred_line_height {
                return Ok(best);
            }

            let mut compact = self.clone();
            compact.line_height = Some(minimum_line_height);
            best = compact.run_with_size(text, minimum)?;
            if !fits(&best) {
                return Ok(best);
            }

            let mut low = minimum_line_height;
            let mut high = preferred_line_height;
            let mut iterations = 0u32;
            while high - low > 0.001 && iterations < 12 {
                iterations += 1;
                let ratio = (low + high) * 0.5;
                compact.line_height = Some(ratio);
                let layout = compact.run_with_size(text, minimum)?;
                if fits(&layout) {
                    best = layout;
                    low = ratio;
                } else {
                    high = ratio;
                }
            }
            tracing::info!(
                iterations,
                font_size = best.font_size,
                line_height = low,
                "auto_size tightened leading"
            );
            return Ok(best);
        }

        let mut low = minimum;
        let mut high = maximum;
        let mut iterations = 0u32;
        while high - low > 0.01 && iterations < 16 {
            iterations += 1;
            let size = (low + high) * 0.5;
            let layout = self.run_with_size(text, size)?;
            if fits(&layout) {
                best = layout;
                low = size;
            } else {
                high = size;
            }
        }
        tracing::info!(iterations, font_size = best.font_size, "auto_size done");
        Ok(best)
    }

    fn run_with_size(&self, text: &str, font_size: f32) -> Result<LayoutRun<'a>> {
        let _s = tracing::debug_span!("layout_size", font_size = font_size as u32).entered();
        let shaper = TextShaper::new();
        let mut line_breaker = LineBreaker::new().with_chinese_word_segmentation();
        if !self.writing_mode.is_vertical()
            && self.hyphenation_policy != HyphenationPolicy::Disabled
            && let Some(lang) = self.hyphenation_lang
        {
            line_breaker = line_breaker.with_hyphenation(lang, HYPHENATION_MIN_WORD_LEN);
        }
        // Use real font metrics for consistent line sizing across modes.
        let font_ref = self.font.skrifa_ref()?;
        let metrics = font_ref.metrics(Size::new(font_size), self.font.location());
        let ascent = metrics.ascent;
        let descent = -metrics.descent;
        let line_height = self.line_height.map_or_else(
            || (ascent + descent + metrics.leading).max(font_size),
            |ratio| font_size * ratio,
        );

        let bidi_info = BidiInfo::new(text, None);

        let (direction, script) = shaping_direction_for_text(text, self.writing_mode);
        let options = ShapingOptions {
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
        let balloon_air = self
            .comic_balloon
            .as_ref()
            .map_or(0.0, |balloon| balloon.air(font_size));

        let mut max_extent = if self.writing_mode.is_vertical() {
            self.max_height
        } else {
            self.max_width
        }
        .unwrap_or(f32::INFINITY);
        if self.comic_balloon.is_some() && self.writing_mode.is_vertical() {
            max_extent = (max_extent - balloon_air * 2.0).max(1.0);
        }
        let max_extent_finite = max_extent.is_finite() && max_extent > 0.0;

        let mut fonts: Vec<&Font> = Vec::with_capacity(1 + self.fallback_fonts.len());
        fonts.push(self.font);
        fonts.extend(self.fallback_fonts.iter());

        let shape_break_suffix = |suffix: LineBreakSuffix,
                                  level: unicode_bidi::Level,
                                  cluster: usize|
         -> Result<ShapedBreakSuffix<'a>> {
            let mut suffix_options = options.clone();
            suffix_options.direction = if level.is_rtl() {
                harfrust::Direction::RightToLeft
            } else {
                harfrust::Direction::LeftToRight
            };

            let mut runs = Vec::new();
            let mut advance = 0.0f32;
            for mut shaped in shape_script_runs(&shaper, suffix.as_str(), &fonts, &suffix_options)?
            {
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
                    let mut run_options = options.clone();
                    run_options.direction = if self.writing_mode.is_vertical() {
                        harfrust::Direction::TopToBottom
                    } else if level.is_rtl() {
                        harfrust::Direction::RightToLeft
                    } else {
                        harfrust::Direction::LeftToRight
                    };

                    let script_runs = shape_script_runs(&shaper, run_text, &fonts, &run_options)?;
                    for mut shaped in script_runs {
                        self.apply_spacing(run_text, &mut shaped);
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
                break_penalty: if self.comic_balloon.is_some() && !self.writing_mode.is_vertical() {
                    comic_break_penalty(text, segment.next_offset)
                } else {
                    0.0
                },
                break_suffix: segment_break_suffix,
            });
        }

        let mut lines: Vec<LayoutLine<'a>> = Vec::new();
        let mut line_profiles = Vec::new();
        let mut contour_overflowed = false;
        let mut line_offset = 0usize;
        if self.comic_balloon.is_some() && !self.writing_mode.is_vertical() {
            contour_overflowed = self.append_balanced_segment_lines(
                &shaped_segments,
                &mut line_offset,
                text.len(),
                false,
                max_extent,
                line_height,
                balloon_air,
                balloon_air,
                &bidi_info,
                &mut lines,
                &mut line_profiles,
            );
        } else {
            let mut paragraph_start = 0usize;
            for (index, segment) in shaped_segments.iter().enumerate() {
                if !segment.is_mandatory {
                    continue;
                }
                contour_overflowed |= self.append_balanced_segment_lines(
                    &shaped_segments[paragraph_start..=index],
                    &mut line_offset,
                    segment.next_offset,
                    true,
                    max_extent,
                    line_height,
                    balloon_air,
                    balloon_air,
                    &bidi_info,
                    &mut lines,
                    &mut line_profiles,
                );
                paragraph_start = index + 1;
            }
            if paragraph_start < shaped_segments.len() {
                contour_overflowed |= self.append_balanced_segment_lines(
                    &shaped_segments[paragraph_start..],
                    &mut line_offset,
                    text.len(),
                    false,
                    max_extent,
                    line_height,
                    balloon_air,
                    balloon_air,
                    &bidi_info,
                    &mut lines,
                    &mut line_profiles,
                );
            }
        }

        // Baselines depend only on line index and metrics. For vertical text we compute absolute X
        // positions within the layout bounds (0..width) so the renderer can draw from the left.
        let line_count = lines.len();
        let effective_alignment = self.alignment.unwrap_or(TextAlign::Left);

        for (i, line) in lines.iter_mut().enumerate() {
            line.baseline = match self.writing_mode {
                WritingMode::VerticalRl => (
                    (line_count.saturating_sub(1) as f32 - i as f32) * line_height
                        + line_height * 0.5,
                    ascent,
                ),
                WritingMode::VerticalLr => (i as f32 * line_height + line_height * 0.5, ascent),
                WritingMode::Horizontal => {
                    let x = if self.comic_balloon.is_some() {
                        let profile = line_profiles.get(i).copied().unwrap_or(LineProfile {
                            width: line.advance,
                            center_offset: 0.0,
                        });
                        match effective_alignment {
                            TextAlign::Left | TextAlign::Justify => {
                                profile.center_offset - profile.width * 0.5
                            }
                            TextAlign::Center => profile.center_offset - line.advance * 0.5,
                            TextAlign::Right => {
                                profile.center_offset + profile.width * 0.5 - line.advance
                            }
                        }
                    } else {
                        0.0
                    };
                    (x, ascent + i as f32 * line_height)
                }
            };
        }

        if effective_alignment == TextAlign::Justify && !self.writing_mode.is_vertical() {
            justify_lines(text, &mut lines, max_extent, &line_profiles);
        }

        // Compute a tight ink bounding box using per-glyph bounds from the font tables (via skrifa),
        // then translate baselines so the top-left ink origin is (0, 0). This avoids clipping without
        // having to measure glyph outlines in the renderer.
        let (mut width, mut height) = (0.0, 0.0);
        let mut placement_offset_x = 0.0;
        let mut placement_offset_y = 0.0;
        if let Some((mut min_x, mut min_y, mut max_x, mut max_y)) =
            self.ink_bounds(font_size, &lines)
        {
            // Keep a tiny safety pad for hinting/AA differences.
            const PAD: f32 = 1.0;
            min_x -= PAD;
            min_y -= PAD;
            max_x += PAD;
            max_y += PAD;

            if self.comic_balloon.is_some() && !self.writing_mode.is_vertical() {
                placement_offset_x = (min_x + max_x) * 0.5;
            }

            if let Some(balloon) = &self.comic_balloon {
                let ink_height = (max_y - min_y).max(0.0);
                if self.writing_mode.is_vertical() {
                    placement_offset_y = balloon_air * (1.0 - 2.0 * balloon.vertical_alignment);
                } else {
                    let block_height = lines.len() as f32 * line_height;
                    let desired_top = balloon.block_top(block_height, balloon_air);
                    let default_top = (balloon.height - ink_height) * balloon.vertical_alignment;
                    placement_offset_y = desired_top + min_y - default_top;
                }
            }

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
                            TextAlign::Left | TextAlign::Justify => 0.0,
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

            // Apply horizontal alignment for horizontal writing mode (per-line alignment).
            if !self.writing_mode.is_vertical()
                && max_extent_finite
                && !matches!(effective_alignment, TextAlign::Left | TextAlign::Justify)
                && self.comic_balloon.is_none()
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
                        TextAlign::Justify => 0.0,
                    };
                    if offset > 0.0 {
                        line.baseline.0 += offset;
                    }
                }
            }
        }

        let mut overflowed = self.comic_balloon.is_none()
            && (contour_overflowed
                || self
                    .max_width
                    .is_some_and(|maximum| width > maximum + f32::EPSILON)
                || self
                    .max_height
                    .is_some_and(|maximum| height > maximum + f32::EPSILON));

        let layout = LayoutRun {
            lines,
            width,
            height,
            font_size,
            overflowed,
            placement_offset_x,
            placement_offset_y,
        };
        if !overflowed && !self.comic_edge_clearance(font_size, &layout)? {
            overflowed = true;
        }

        Ok(LayoutRun {
            overflowed,
            ..layout
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
        line_height: f32,
        balloon_air_x: f32,
        balloon_air_y: f32,
        bidi_info: &BidiInfo<'_>,
        lines: &mut Vec<LayoutLine<'a>>,
        line_profiles: &mut Vec<LineProfile>,
    ) -> bool {
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
                line_profiles.push(LineProfile {
                    width: max_extent,
                    center_offset: 0.0,
                });
            }
            return false;
        }

        let measures = segments
            .iter()
            .map(|segment| LineBreakMeasure {
                advance: segment.advance,
                break_suffix_advance: segment
                    .break_suffix
                    .as_ref()
                    .map_or(0.0, |suffix| suffix.advance),
                break_penalty: segment.break_penalty,
                is_mandatory: segment.is_mandatory,
            })
            .collect::<Vec<_>>();
        let result = if let Some(balloon) = self
            .comic_balloon
            .as_ref()
            .filter(|_| !self.writing_mode.is_vertical())
        {
            comic_line_breaks(
                &measures,
                balloon,
                line_height,
                balloon_air_x,
                balloon_air_y,
                self.hyphenation_policy,
            )
        } else if max_extent.is_finite() && max_extent > 0.0 {
            line_breaks_with_policy(&measures, max_extent, self.hyphenation_policy)
        } else {
            LineBreakResult {
                breaks: vec![segments.len()],
                profiles: vec![LineProfile {
                    width: measures.iter().map(|measure| measure.advance).sum(),
                    center_offset: 0.0,
                }],
                overflowed: false,
                cost: 0.0,
            }
        };

        let mut start = 0usize;
        for (line_index, end) in result.breaks.iter().copied().enumerate() {
            if end <= start || end > segments.len() {
                continue;
            }
            let final_line = end == segments.len();
            let mandatory_line = segments[end - 1].is_mandatory;
            let visible_end = segments[end - 1].range.end;
            let next_offset = if mandatory_line {
                segments[end - 1].next_offset
            } else if final_line {
                final_next_offset
            } else {
                segments[end].range.start
            };
            let break_suffix = if final_line || mandatory_line {
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
                mandatory_line || (force_final_line && final_line),
                bidi_info,
                lines,
            );
            line_profiles.push(
                result
                    .profiles
                    .get(line_index)
                    .copied()
                    .unwrap_or(LineProfile {
                        width: max_extent,
                        center_offset: 0.0,
                    }),
            );
            start = end;
        }
        result.overflowed
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
                        let Ok(font_ref) = g.font.skrifa_ref() else {
                            x += g.x_advance;
                            y -= g.y_advance;
                            continue;
                        };
                        entry
                            .insert(font_ref.glyph_metrics(Size::new(font_size), g.font.location()))
                    }
                };

                let gid = skrifa::GlyphId::new(g.glyph_id);
                if let Some(b) = glyph_metrics.bounds(gid) {
                    let x0 = x + g.x_offset + b.x_min;
                    let x1 = x + g.x_offset + b.x_max;
                    let synthetic_pad = g.font.synthetic_skew().map_or(0.0, |_| font_size * 0.25)
                        + if g.font.synthetic_bold() {
                            font_size * 0.05
                        } else {
                            0.0
                        };

                    // `b` is in a Y-up font coordinate system. Our layout coordinates are Y-down
                    // while screen-space Y grows downward, so we flip by subtracting.
                    let y0 = (y - g.y_offset) - b.y_max;
                    let y1 = (y - g.y_offset) - b.y_min;

                    min_x = min_x.min(x0 - synthetic_pad).min(x1 - synthetic_pad);
                    max_x = max_x.max(x0 + synthetic_pad).max(x1 + synthetic_pad);
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

    fn comic_edge_clearance(&self, font_size: f32, layout: &LayoutRun<'a>) -> Result<bool> {
        let Some(balloon) = &self.comic_balloon else {
            return Ok(true);
        };

        let origin_x = (balloon.width - layout.width) * 0.5 + layout.placement_offset_x;
        let origin_y = (balloon.height - layout.height) * balloon.vertical_alignment
            + layout.placement_offset_y;
        let mut text_edge_pixels = HashSet::new();

        for line in &layout.lines {
            let (mut x, mut y) = line.baseline;
            for glyph in &line.glyphs {
                let font_ref = glyph.font.skrifa_ref()?;
                if let Some(outline) = font_ref
                    .outline_glyphs()
                    .get(skrifa::GlyphId::new(glyph.glyph_id))
                {
                    let mut pen = EdgePixelPen {
                        pixels: &mut text_edge_pixels,
                        origin: (origin_x + x + glyph.x_offset, origin_y + y - glyph.y_offset),
                        current: (0.0, 0.0),
                        start: (0.0, 0.0),
                    };
                    outline
                        .draw(
                            DrawSettings::unhinted(Size::new(font_size), glyph.font.location()),
                            &mut pen,
                        )
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "failed to read glyph {} outline for balloon layout: {error}",
                                glyph.glyph_id
                            )
                        })?;
                }
                x += glyph.x_advance;
                y -= glyph.y_advance;
            }
        }

        Ok(text_edge_pixels
            .into_iter()
            .all(|pixel| balloon.contains_with_clearance(pixel, balloon.air(font_size))))
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
                    let Ok(font_ref) = glyph.font.skrifa_ref() else {
                        continue;
                    };
                    entry
                        .insert(font_ref.glyph_metrics(Size::new(font_size), glyph.font.location()))
                }
            };

            let gid = skrifa::GlyphId::new(glyph.glyph_id);
            let Some(bounds) = glyph_metrics.bounds(gid) else {
                continue;
            };
            glyph.x_offset = centered_x_offset(bounds.x_min, bounds.x_max);
        }
    }

    fn apply_spacing(&self, text: &str, shaped: &mut ShapedRun<'a>) {
        if self.letter_spacing == 0.0
            && self.word_spacing == 0.0
            && !self.compact_emphasis_punctuation
        {
            return;
        }
        for glyph in &mut shaped.glyphs {
            let character = text
                .get(glyph.cluster as usize..)
                .and_then(|tail| tail.chars().next());
            let advance = if self.writing_mode.is_vertical() {
                glyph.y_advance
            } else {
                glyph.x_advance
            };
            let compact_spacing = self
                .compact_emphasis_punctuation
                .then(|| emphasis_run_length(text, glyph.cluster as usize))
                .flatten()
                .map_or(0.0, |run_length| {
                    -advance.abs() * (1.0 - 1.0 / run_length as f32)
                });
            let extra = self.letter_spacing
                + character
                    .filter(|character| character.is_whitespace())
                    .map_or(0.0, |_| self.word_spacing)
                + compact_spacing;
            if self.writing_mode.is_vertical() {
                glyph.y_advance = extend_advance(glyph.y_advance, extra);
            } else {
                glyph.x_advance = extend_advance(glyph.x_advance, extra);
            }
        }
        shaped.x_advance = shaped.glyphs.iter().map(|glyph| glyph.x_advance).sum();
        shaped.y_advance = shaped.glyphs.iter().map(|glyph| glyph.y_advance).sum();
    }
}

fn extend_advance(advance: f32, extra: f32) -> f32 {
    if advance < 0.0 {
        advance - extra
    } else {
        advance + extra
    }
}

fn justify_lines(
    text: &str,
    lines: &mut [LayoutLine<'_>],
    max_width: f32,
    profiles: &[LineProfile],
) {
    if profiles.is_empty() && (!max_width.is_finite() || max_width <= 0.0) {
        return;
    }
    let last = lines.len().saturating_sub(1);
    for (index, line) in lines.iter_mut().enumerate() {
        if index == last
            || text
                .get(line.range.end..)
                .and_then(|tail| tail.chars().next())
                .is_some_and(|character| {
                    matches!(
                        character,
                        '\n' | '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}'
                    )
                })
        {
            continue;
        }
        let target_width = profiles
            .get(index)
            .map_or(max_width, |profile| profile.width);
        if !target_width.is_finite() || target_width <= 0.0 {
            continue;
        }
        let is_space = |glyph: &PositionedGlyph<'_>| {
            text.get(glyph.cluster as usize..)
                .and_then(|tail| tail.chars().next())
                .is_some_and(char::is_whitespace)
        };
        let spaces = line.glyphs.iter().filter(|glyph| is_space(glyph)).count();
        if spaces == 0 || line.advance >= target_width {
            continue;
        }
        let extra = (target_width - line.advance) / spaces as f32;
        for glyph in &mut line.glyphs {
            if is_space(glyph) {
                glyph.x_advance = extend_advance(glyph.x_advance, extra);
            }
        }
        line.advance = target_width;
    }
}

fn line_breaks_with_policy(
    segments: &[LineBreakMeasure],
    max_extent: f32,
    policy: HyphenationPolicy,
) -> LineBreakResult {
    if policy == HyphenationPolicy::LastResort {
        let without_hyphens = optimal_uniform_line_breaks(segments, max_extent, false);
        if !without_hyphens.overflowed {
            return without_hyphens;
        }
    }
    optimal_uniform_line_breaks(segments, max_extent, policy != HyphenationPolicy::Disabled)
}

#[cfg(test)]
fn optimal_line_breaks(segments: &[LineBreakMeasure], max_extent: f32) -> Vec<usize> {
    optimal_uniform_line_breaks(segments, max_extent, true).breaks
}

fn optimal_uniform_line_breaks(
    segments: &[LineBreakMeasure],
    max_extent: f32,
    allow_hyphenation: bool,
) -> LineBreakResult {
    let len = segments.len();
    if len == 0 {
        return LineBreakResult {
            breaks: Vec::new(),
            profiles: Vec::new(),
            overflowed: false,
            cost: 0.0,
        };
    }
    if !max_extent.is_finite() || max_extent <= 0.0 {
        return LineBreakResult {
            breaks: vec![len],
            profiles: vec![LineProfile {
                width: max_extent,
                center_offset: 0.0,
            }],
            overflowed: false,
            cost: 0.0,
        };
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
            let hyphenated_break = end < len && suffix_advance > 0.0;
            if hyphenated_break && !allow_hyphenation {
                continue;
            }
            let mut cost = dp[start] + line_break_badness(line_advance, max_extent);
            if end < len && suffix_advance > 0.0 {
                cost += LINE_BREAK_HYPHEN_PENALTY;
            }
            if end < len {
                cost += segments[end - 1].break_penalty;
            }

            if cost < dp[end] {
                dp[end] = cost;
                prev[end] = Some(start);
            }
            if segments[end - 1].is_mandatory || advance > max_extent {
                break;
            }
        }
    }

    if !dp[len].is_finite() {
        return LineBreakResult {
            breaks: vec![len],
            profiles: vec![LineProfile {
                width: max_extent,
                center_offset: 0.0,
            }],
            overflowed: segments.iter().map(|segment| segment.advance).sum::<f32>() > max_extent,
            cost: f32::INFINITY,
        };
    }

    let mut breaks = Vec::new();
    let mut index = len;
    while index > 0 {
        breaks.push(index);
        let Some(previous) = prev[index] else {
            return LineBreakResult {
                breaks: vec![len],
                profiles: vec![LineProfile {
                    width: max_extent,
                    center_offset: 0.0,
                }],
                overflowed: true,
                cost: f32::INFINITY,
            };
        };
        index = previous;
    }
    breaks.reverse();
    let overflowed = breaks_overflow(segments, &breaks, &[max_extent; 1]);
    let profiles = vec![
        LineProfile {
            width: max_extent,
            center_offset: 0.0,
        };
        breaks.len()
    ];
    LineBreakResult {
        breaks,
        profiles,
        overflowed,
        cost: dp[len],
    }
}

fn comic_line_breaks(
    segments: &[LineBreakMeasure],
    balloon: &ComicBalloon,
    line_height: f32,
    air_x: f32,
    air_y: f32,
    policy: HyphenationPolicy,
) -> LineBreakResult {
    let available_height = (balloon.height - air_y * 2.0).max(0.0);
    let maximum_lines = ((available_height / line_height).floor() as usize)
        .min(COMIC_MAX_LINES)
        .min(segments.len());
    if maximum_lines == 0 {
        let mut fallback =
            line_breaks_with_policy(segments, (balloon.width - air_x * 2.0).max(1.0), policy);
        fallback.overflowed = true;
        return fallback;
    }

    let select = |allow_hyphenation| {
        (1..=maximum_lines)
            .filter_map(|line_count| {
                let profiles = balloon.line_profiles(line_count, line_height, air_x, air_y)?;
                exact_profiled_line_breaks(segments, profiles, allow_hyphenation)
            })
            .min_by(|left, right| {
                left.overflowed
                    .cmp(&right.overflowed)
                    .then_with(|| left.cost.total_cmp(&right.cost))
            })
    };

    if policy == HyphenationPolicy::LastResort
        && let Some(without_hyphens) = select(false)
        && !without_hyphens.overflowed
    {
        return without_hyphens;
    }

    select(policy != HyphenationPolicy::Disabled).unwrap_or_else(|| {
        let mut fallback =
            line_breaks_with_policy(segments, (balloon.width - air_x * 2.0).max(1.0), policy);
        fallback.overflowed = true;
        fallback
    })
}

fn exact_profiled_line_breaks(
    segments: &[LineBreakMeasure],
    profiles: Vec<LineProfile>,
    allow_hyphenation: bool,
) -> Option<LineBreakResult> {
    let len = segments.len();
    let line_count = profiles.len();
    if line_count == 0 || line_count > len {
        return None;
    }
    let mut dp = vec![vec![f32::INFINITY; len + 1]; line_count + 1];
    let mut previous = vec![vec![None; len + 1]; line_count + 1];
    dp[0][0] = 0.0;

    for line in 0..line_count {
        let remaining_lines = line_count - line - 1;
        for start in line..len {
            if !dp[line][start].is_finite() {
                continue;
            }
            let mut advance = 0.0f32;
            let last_end = len - remaining_lines;
            for end in start + 1..=last_end {
                advance += segments[end - 1].advance;
                let suffix = if end < len {
                    segments[end - 1].break_suffix_advance
                } else {
                    0.0
                };
                let hyphenated_break = end < len && suffix > 0.0;
                if hyphenated_break && !allow_hyphenation {
                    continue;
                }
                let line_advance = advance + suffix;
                let width = profiles[line].width.max(1.0);
                let overflow = (line_advance - width).max(0.0) / width;
                let slack = (width - line_advance).max(0.0) / width;
                let mut cost = dp[line][start]
                    + slack * slack * 1_000.0
                    + overflow * overflow * COMIC_LINE_OVERFLOW_PENALTY;
                if hyphenated_break {
                    cost += LINE_BREAK_HYPHEN_PENALTY;
                }
                if end < len {
                    cost += segments[end - 1].break_penalty;
                }
                if cost < dp[line + 1][end] {
                    dp[line + 1][end] = cost;
                    previous[line + 1][end] = Some(start);
                }
                if segments[end - 1].is_mandatory || advance > width {
                    break;
                }
            }
        }
    }

    let mut cost = dp[line_count][len];
    if !cost.is_finite() {
        return None;
    }
    cost = cost / line_count as f32 + line_count as f32 * 8.0;
    let mut breaks = Vec::with_capacity(line_count);
    let mut end = len;
    for line in (1..=line_count).rev() {
        breaks.push(end);
        end = previous[line][end]?;
    }
    if end != 0 {
        return None;
    }
    breaks.reverse();
    let widths = profiles
        .iter()
        .map(|profile| profile.width)
        .collect::<Vec<_>>();
    let overflowed = breaks_overflow(segments, &breaks, &widths);
    Some(LineBreakResult {
        breaks,
        profiles,
        overflowed,
        cost,
    })
}

fn breaks_overflow(segments: &[LineBreakMeasure], breaks: &[usize], widths: &[f32]) -> bool {
    let mut start = 0usize;
    for (line, end) in breaks.iter().copied().enumerate() {
        let mut advance = segments[start..end]
            .iter()
            .map(|segment| segment.advance)
            .sum::<f32>();
        if end < segments.len() {
            advance += segments[end - 1].break_suffix_advance;
        }
        let Some(width) = widths
            .get(line)
            .copied()
            .or_else(|| widths.first().copied())
        else {
            return true;
        };
        if advance > width + f32::EPSILON {
            return true;
        }
        start = end;
    }
    false
}

impl ComicBalloon {
    fn air(&self, font_size: f32) -> f32 {
        self.minimum_air.max(font_size)
    }

    fn contains_with_clearance(&self, pixel: (i32, i32), air: f32) -> bool {
        let point = (pixel.0 as f32 + 0.5, pixel.1 as f32 + 0.5);
        if !self.contains(point) {
            return false;
        }
        if air <= 0.0 {
            return true;
        }

        let radius = air.ceil() as i32;
        let first = self
            .edge_pixels
            .partition_point(|&(x, _)| x < pixel.0 - radius);
        let last = self
            .edge_pixels
            .partition_point(|&(x, _)| x <= pixel.0 + radius);
        let minimum_distance_squared = air * air;
        !self.edge_pixels[first..last].iter().any(|&(x, y)| {
            let dx = (pixel.0 - x) as f32;
            let dy = (pixel.1 - y) as f32;
            dx * dx + dy * dy < minimum_distance_squared
        })
    }

    fn contains(&self, point: (f32, f32)) -> bool {
        if self.contour.len() < 3 {
            let radius_x = self.width * 0.5;
            let radius_y = self.height * 0.5;
            if radius_x <= 0.0 || radius_y <= 0.0 {
                return false;
            }
            let x = (point.0 - radius_x) / radius_x;
            let y = (point.1 - radius_y) / radius_y;
            return x * x + y * y <= 1.0;
        }

        let mut inside = false;
        let mut previous = self.contour[self.contour.len() - 1];
        for &current in &self.contour {
            if (current.1 > point.1) != (previous.1 > point.1) {
                let intersection_x = (previous.0 - current.0) * (point.1 - current.1)
                    / (previous.1 - current.1)
                    + current.0;
                if point.0 < intersection_x {
                    inside = !inside;
                }
            }
            previous = current;
        }
        inside
    }

    fn block_top(&self, block_height: f32, air: f32) -> f32 {
        let available_height = (self.height - air * 2.0).max(0.0);
        air + (available_height - block_height).max(0.0) * self.vertical_alignment
    }

    fn line_profiles(
        &self,
        line_count: usize,
        line_height: f32,
        air_x: f32,
        air_y: f32,
    ) -> Option<Vec<LineProfile>> {
        let rx = self.width * 0.5 - air_x;
        let ry = self.height * 0.5 - air_y;
        let block_height = line_count as f32 * line_height;
        if rx <= 0.0 || ry <= 0.0 || block_height > ry * 2.0 + f32::EPSILON {
            return None;
        }
        let top = self.block_top(block_height, air_y);
        let center_x = self.width * 0.5;
        let mut profiles = Vec::with_capacity(line_count);
        for line in 0..line_count {
            let line_top = top + line as f32 * line_height;
            let mut left = f32::NEG_INFINITY;
            let mut right = f32::INFINITY;
            // A center-only sample lets glyph corners enter a narrowing contour.
            // Intersect several spans across the complete line box instead.
            for sample in 0..=4 {
                let y = line_top + line_height * sample as f32 / 4.0;
                let normalized_y = ((y - self.height * 0.5) / ry).clamp(-1.0, 1.0);
                let ellipse_half_width = rx * (1.0 - normalized_y * normalized_y).sqrt();
                let (sample_left, sample_right) = self.contour_span(y).map_or(
                    (center_x - ellipse_half_width, center_x + ellipse_half_width),
                    |(contour_left, contour_right)| (contour_left + air_x, contour_right - air_x),
                );
                left = left.max(sample_left);
                right = right.min(sample_right);
            }
            if right <= left {
                return None;
            }
            profiles.push(LineProfile {
                width: right - left,
                center_offset: (left + right) * 0.5 - center_x,
            });
        }
        Some(profiles)
    }

    fn contour_span(&self, y: f32) -> Option<(f32, f32)> {
        if self.contour.len() < 3 {
            return None;
        }
        let mut intersections = Vec::new();
        for index in 0..self.contour.len() {
            let first = self.contour[index];
            let second = self.contour[(index + 1) % self.contour.len()];
            if (first.1 <= y && second.1 > y) || (second.1 <= y && first.1 > y) {
                let fraction = (y - first.1) / (second.1 - first.1);
                intersections.push(first.0 + (second.0 - first.0) * fraction);
            }
        }
        intersections.sort_by(f32::total_cmp);
        intersections
            .chunks_exact(2)
            .map(|pair| (pair[0], pair[1]))
            .max_by(|left, right| (left.1 - left.0).total_cmp(&(right.1 - right.0)))
    }
}

fn comic_break_penalty(text: &str, boundary: usize) -> f32 {
    let boundary = boundary.min(text.len());
    let before = text[..boundary].trim_end();
    let after = text[boundary..].trim_start();
    match before.chars().next_back() {
        Some('.' | '!' | '?' | '…' | '‼' | '⁇' | '⁈' | '⁉') => return 0.0,
        Some(',' | ';' | ':' | '—' | '–') => return 20.0,
        _ => {}
    }
    let next_word = after
        .split(|character: char| !character.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    if matches!(
        next_word.to_ascii_lowercase().as_str(),
        "and" | "but" | "or" | "so" | "because" | "although" | "while" | "then"
    ) {
        return 40.0;
    }
    let previous_word = before
        .rsplit(|character: char| !character.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    if matches!(
        previous_word.to_ascii_lowercase().as_str(),
        "a" | "an" | "the" | "to" | "of" | "for" | "in" | "on" | "at" | "with" | "from"
    ) {
        300.0
    } else {
        100.0
    }
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

fn is_emphasis_mark(character: char) -> bool {
    matches!(character, '!' | '?' | '！' | '？')
}

fn emphasis_run_length(text: &str, offset: usize) -> Option<usize> {
    let character = text.get(offset..)?.chars().next()?;
    if !is_emphasis_mark(character) {
        return None;
    }
    let before = text
        .get(..offset)?
        .chars()
        .rev()
        .take_while(|character| is_emphasis_mark(*character))
        .count();
    let after = text
        .get(offset..)?
        .chars()
        .take_while(|character| is_emphasis_mark(*character))
        .count();
    let length = before + after;
    (length > 1).then_some(length)
}

fn is_fullwidth_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '\u{203C}' // Double exclamation mark
            | '\u{2047}'..='\u{2049}' // Double/mixed question and exclamation marks
            | '\u{3001}' // Ideographic comma
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
    use crate::font::{Font, FontSystem};
    use skrifa::{MetadataProvider, instance::Size};

    fn any_system_font() -> Font {
        let mut fonts = FontSystem::new();

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
            if let Ok(font) = fonts.query_family(name) {
                return font;
            }
        }
        fonts
            .first_font()
            .expect("no system font available for tests")
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

    fn comic_balloon(
        width: f32,
        height: f32,
        contour: Vec<(f32, f32)>,
        vertical_alignment: f32,
        minimum_air: f32,
    ) -> ComicBalloon {
        let edge_pixels = rasterize_contour_edge(width, height, &contour);
        ComicBalloon {
            width,
            height,
            contour,
            vertical_alignment,
            minimum_air,
            edge_pixels: edge_pixels.into(),
        }
    }

    #[test]
    fn capped_auto_size_shrinks_text_to_fit_the_available_height() -> anyhow::Result<()> {
        let font = any_system_font();
        let preferred_size = 24.0;
        let fixed = TextLayout::new(&font)
            .with_font_size(preferred_size)
            .with_max_width(1_000.0)
            .run("First\nSecond\nThird")?;
        let max_height = fixed.height * 0.65;

        let fitted = TextLayout::new(&font)
            .with_max_font_size(preferred_size)
            .with_max_width(1_000.0)
            .with_max_height(max_height)
            .run("First\nSecond\nThird")?;

        assert!(fitted.font_size < preferred_size);
        assert!(fitted.width <= 1_000.0);
        assert!(fitted.height <= max_height + 0.01);
        Ok(())
    }

    #[test]
    fn capped_auto_size_does_not_enlarge_text_that_already_fits() -> anyhow::Result<()> {
        let font = any_system_font();
        let preferred_size = 18.0;

        let fitted = TextLayout::new(&font)
            .with_max_font_size(preferred_size)
            .with_max_width(1_000.0)
            .with_max_height(1_000.0)
            .run("Fits")?;

        assert_eq!(fitted.font_size, preferred_size);
        Ok(())
    }

    #[test]
    fn capped_auto_size_respects_the_readable_floor() -> anyhow::Result<()> {
        let font = any_system_font();

        let fitted = TextLayout::new(&font)
            .with_max_font_size(16.0)
            .with_min_font_size(8.0)
            .with_max_width(12.0)
            .with_max_height(12.0)
            .run("This translation cannot fit")?;

        assert_eq!(fitted.font_size, 8.0);
        Ok(())
    }

    #[test]
    fn capped_auto_size_prefers_default_leading_before_tightening() -> anyhow::Result<()> {
        let font = any_system_font();
        let text = "First\nSecond\nThird\nFourth";
        let loose = TextLayout::new(&font)
            .with_font_size(16.0)
            .with_line_height(1.2)
            .run(text)?;
        let tight = TextLayout::new(&font)
            .with_font_size(16.0)
            .with_line_height(1.0)
            .run(text)?;
        let max_height = (loose.height + tight.height) * 0.5;

        let fitted = TextLayout::new(&font)
            .with_max_font_size(16.0)
            .with_min_font_size(16.0)
            .with_line_height(1.2)
            .with_min_line_height(1.0)
            .with_max_width(1_000.0)
            .with_max_height(max_height)
            .run(text)?;

        assert_eq!(fitted.font_size, 16.0);
        assert!(fitted.height <= max_height + 0.01);
        assert!(fitted.height > tight.height);
        Ok(())
    }

    #[test]
    fn font_size_search_handles_non_monotonic_balloon_fit() -> anyhow::Result<()> {
        let fitted = largest_fitting_font_size(
            9.0,
            24.0,
            |size| Ok(size),
            |size| (10.0..=12.0).contains(size),
        )?
        .expect("the larger fitting range should be found");

        assert!((11.99..=12.0).contains(&fitted));
        Ok(())
    }

    #[test]
    fn optimal_line_breaks_balance_ragged_lines() {
        let segments = vec![
            LineBreakMeasure {
                advance: 30.0,
                break_suffix_advance: 0.0,
                break_penalty: 0.0,
                is_mandatory: false,
            };
            7
        ];

        assert_eq!(optimal_line_breaks(&segments, 100.0), vec![2, 4, 7]);
    }

    #[test]
    fn comic_profiles_are_widest_in_the_middle() {
        let balloon = comic_balloon(200.0, 120.0, Vec::new(), 0.5, 0.0);
        let profiles = balloon.line_profiles(5, 16.0, 10.0, 10.0).unwrap();

        assert!(profiles[0].width < profiles[1].width);
        assert!(profiles[1].width < profiles[2].width);
        assert!((profiles[0].width - profiles[4].width).abs() < 0.001);
        assert!((profiles[1].width - profiles[3].width).abs() < 0.001);
    }

    #[test]
    fn comic_profiles_use_the_detected_contour_without_an_extra_ellipse() {
        let balloon = comic_balloon(
            200.0,
            120.0,
            vec![(0.0, 0.0), (200.0, 0.0), (200.0, 120.0), (0.0, 120.0)],
            0.5,
            0.0,
        );
        let profiles = balloon.line_profiles(5, 16.0, 10.0, 10.0).unwrap();

        for profile in profiles {
            assert_approx_eq(profile.width, 180.0);
        }
    }

    #[test]
    fn comic_profiles_reserve_air_across_the_entire_line_box() {
        let balloon = comic_balloon(
            200.0,
            120.0,
            vec![(100.0, 0.0), (200.0, 60.0), (100.0, 120.0), (0.0, 60.0)],
            0.5,
            0.0,
        );
        let profile = balloon.line_profiles(1, 40.0, 0.0, 10.0).unwrap()[0];

        assert!((130.0..=135.0).contains(&profile.width));
    }

    #[test]
    fn comic_profiles_respect_asymmetric_contours() {
        let balloon = comic_balloon(
            200.0,
            120.0,
            vec![(0.0, 0.0), (140.0, 0.0), (140.0, 120.0), (0.0, 120.0)],
            0.5,
            0.0,
        );
        let profile = balloon.line_profiles(1, 16.0, 10.0, 10.0).unwrap()[0];

        assert!(profile.center_offset < -20.0);
        assert!(profile.width < 130.0);
    }

    #[test]
    fn comic_layout_preserves_the_contour_center_during_final_placement() -> anyhow::Result<()> {
        let font = any_system_font();
        let layout = TextLayout::new(&font)
            .with_font_size(16.0)
            .with_alignment(TextAlign::Center)
            .with_max_width(200.0)
            .with_max_height(120.0)
            .with_comic_balloon(
                200.0,
                120.0,
                vec![(0.0, 0.0), (140.0, 0.0), (140.0, 120.0), (0.0, 120.0)],
                0.5,
                0.0,
            )
            .run("Hello")?;

        assert!(layout.placement_offset_x() < -10.0);
        Ok(())
    }

    #[test]
    fn comic_auto_size_only_moderately_loosens_underfilled_leading() -> anyhow::Result<()> {
        let font = any_system_font();
        let font_size = 16.0;
        let layout = TextLayout::new(&font)
            .with_max_font_size(font_size)
            .with_min_font_size(font_size)
            .with_line_height(1.2)
            .with_alignment(TextAlign::Center)
            .with_max_width(200.0)
            .with_max_height(150.0)
            .with_comic_balloon(
                200.0,
                150.0,
                vec![(0.0, 0.0), (200.0, 0.0), (200.0, 150.0), (0.0, 150.0)],
                0.5,
                10.0,
            )
            .run("First\nSecond\nThird")?;

        let leading = layout.lines[1].baseline.1 - layout.lines[0].baseline.1;
        assert!(leading > font_size * 1.2);
        assert!(leading <= font_size * 1.35 + 0.01);
        Ok(())
    }

    #[test]
    fn comic_auto_size_preserves_air_for_an_already_filled_layout() -> anyhow::Result<()> {
        let font = any_system_font();
        let font_size = 16.0;
        let layout = TextLayout::new(&font)
            .with_max_font_size(font_size)
            .with_min_font_size(font_size)
            .with_line_height(1.2)
            .with_alignment(TextAlign::Center)
            .with_max_width(200.0)
            .with_max_height(150.0)
            .with_comic_balloon(
                200.0,
                150.0,
                vec![(0.0, 0.0), (200.0, 0.0), (200.0, 150.0), (0.0, 150.0)],
                0.5,
                10.0,
            )
            .run("First\nSecond\nThird\nFourth\nFifth\nSixth")?;

        let leading = layout.lines[1].baseline.1 - layout.lines[0].baseline.1;
        assert_approx_eq(leading, font_size * 1.2);
        Ok(())
    }

    #[test]
    fn comic_breaks_prefer_natural_pauses() {
        let mut segments = vec![
            LineBreakMeasure {
                advance: 30.0,
                break_suffix_advance: 0.0,
                break_penalty: 0.0,
                is_mandatory: false,
            };
            5
        ];
        segments[2].break_penalty = 500.0;

        assert_eq!(optimal_line_breaks(&segments, 90.0), vec![2, 5]);
        assert!(comic_break_penalty("Stop! Now", 6) < comic_break_penalty("go and", 3));
        assert!(comic_break_penalty("go and", 3) < comic_break_penalty("hello world", 6));
        assert!(comic_break_penalty("hello world", 6) < comic_break_penalty("the word", 4));
    }

    #[test]
    fn last_resort_hyphenation_is_used_only_to_avoid_overflow() {
        let fits_without_hyphen = [
            LineBreakMeasure {
                advance: 30.0,
                break_suffix_advance: 5.0,
                break_penalty: 0.0,
                is_mandatory: false,
            },
            LineBreakMeasure {
                advance: 30.0,
                break_suffix_advance: 0.0,
                break_penalty: 0.0,
                is_mandatory: false,
            },
        ];
        let unbroken =
            line_breaks_with_policy(&fits_without_hyphen, 70.0, HyphenationPolicy::LastResort);
        assert_eq!(unbroken.breaks, [2]);
        assert!(!unbroken.overflowed);

        let needs_hyphen = [
            LineBreakMeasure {
                advance: 55.0,
                break_suffix_advance: 10.0,
                break_penalty: 0.0,
                is_mandatory: false,
            },
            LineBreakMeasure {
                advance: 55.0,
                break_suffix_advance: 0.0,
                break_penalty: 0.0,
                is_mandatory: false,
            },
        ];
        let hyphenated =
            line_breaks_with_policy(&needs_hyphen, 70.0, HyphenationPolicy::LastResort);
        assert_eq!(hyphenated.breaks, [1, 2]);
        assert!(!hyphenated.overflowed);
    }

    #[test]
    fn mandatory_breaks_are_respected_by_the_global_balloon_profile() {
        let segments = [
            LineBreakMeasure {
                advance: 20.0,
                break_suffix_advance: 0.0,
                break_penalty: 0.0,
                is_mandatory: true,
            },
            LineBreakMeasure {
                advance: 20.0,
                break_suffix_advance: 0.0,
                break_penalty: 0.0,
                is_mandatory: false,
            },
            LineBreakMeasure {
                advance: 5.0,
                break_suffix_advance: 0.0,
                break_penalty: 0.0,
                is_mandatory: false,
            },
        ];
        let profiles = vec![
            LineProfile {
                width: 50.0,
                center_offset: 0.0,
            },
            LineProfile {
                width: 25.0,
                center_offset: 0.0,
            },
        ];

        let result = exact_profiled_line_breaks(&segments, profiles, true).unwrap();

        assert_eq!(result.breaks, [1, 3]);
    }

    #[test]
    fn a_discretionary_suffix_does_not_hide_a_later_unbroken_fit() {
        let segments = [
            LineBreakMeasure {
                advance: 30.0,
                break_suffix_advance: 5.0,
                break_penalty: 0.0,
                is_mandatory: false,
            },
            LineBreakMeasure {
                advance: 25.0,
                break_suffix_advance: 20.0,
                break_penalty: 0.0,
                is_mandatory: false,
            },
            LineBreakMeasure {
                advance: 5.0,
                break_suffix_advance: 0.0,
                break_penalty: 0.0,
                is_mandatory: false,
            },
        ];

        let result = line_breaks_with_policy(&segments, 65.0, HyphenationPolicy::Normal);

        assert_eq!(result.breaks, [3]);
        assert!(!result.overflowed);
    }

    #[test]
    fn comic_balloon_reports_when_relative_air_cannot_fit() -> anyhow::Result<()> {
        let font = any_system_font();
        let layout = TextLayout::new(&font)
            .with_font_size(16.0)
            .with_max_width(100.0)
            .with_max_height(10.0)
            .with_comic_balloon(
                100.0,
                10.0,
                vec![(0.0, 0.0), (100.0, 0.0), (100.0, 10.0), (0.0, 10.0)],
                0.5,
                4.0,
            )
            .run("Hi")?;

        assert!(layout.overflowed());
        Ok(())
    }

    #[test]
    fn comic_balloon_measures_polygon_edge_pixel_clearance() {
        let balloon = comic_balloon(
            100.0,
            100.0,
            vec![(50.0, 0.0), (100.0, 50.0), (50.0, 100.0), (0.0, 50.0)],
            0.5,
            8.0,
        );

        assert!(balloon.contains_with_clearance((50, 20), 8.0));
        assert!(!balloon.contains_with_clearance((50, 5), 8.0));
        assert!(!balloon.contains_with_clearance((10, 10), 8.0));
    }

    #[test]
    fn comic_balloon_air_scales_with_the_candidate_font_size() {
        let balloon = comic_balloon(100.0, 100.0, Vec::new(), 0.5, 4.0);

        assert_approx_eq(balloon.air(12.0), 12.0);
        assert_approx_eq(balloon.air(24.0), 24.0);
        assert_approx_eq(balloon.air(3.0), 4.0);
    }

    #[test]
    fn layout_baselines_horizontal_follow_font_metrics() -> anyhow::Result<()> {
        let font = any_system_font();
        let font_size = 16.0;
        let layout = TextLayout::new(&font)
            .with_font_size(font_size)
            .with_writing_mode(WritingMode::Horizontal)
            .run("A\nB\nC")?;

        assert!(layout.lines.len() >= 2);

        let metrics = font
            .skrifa_ref()?
            .metrics(Size::new(font_size), font.location());
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
        let layout = TextLayout::new(&font)
            .with_font_size(16.0)
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
        let layout = TextLayout::new(&font)
            .with_font_size(font_size)
            .with_writing_mode(WritingMode::VerticalRl)
            .run("A\nB\nC")?;

        assert!(layout.lines.len() >= 2);

        let metrics = font
            .skrifa_ref()?
            .metrics(Size::new(font_size), font.location());
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
    fn vertical_lr_columns_flow_left_to_right() -> anyhow::Result<()> {
        let font = any_system_font();
        let layout = TextLayout::new(&font)
            .with_font_size(16.0)
            .with_writing_mode(WritingMode::VerticalLr)
            .run("A\nB\nC")?;

        assert_eq!(layout.lines.len(), 3);
        assert!(
            layout
                .lines
                .windows(2)
                .all(|pair| { pair[0].baseline.0 < pair[1].baseline.0 })
        );
        Ok(())
    }

    #[test]
    fn vertical_layout_preserves_original_source_offsets() -> anyhow::Result<()> {
        let font = any_system_font();
        let text = "！！A";
        let layout = TextLayout::new(&font)
            .with_font_size(16.0)
            .with_writing_mode(WritingMode::VerticalRl)
            .run(text)?;

        assert_eq!(layout.lines[0].range, 0..text.len());
        assert!(layout.lines[0].glyphs.iter().all(|glyph| {
            let cluster = glyph.cluster as usize;
            cluster <= text.len() && text.is_char_boundary(cluster)
        }));
        Ok(())
    }

    #[test]
    fn vertical_layout_horizontal_alignment_works() -> anyhow::Result<()> {
        let font = any_system_font();
        let max_width = 100.0;
        let layout = TextLayout::new(&font)
            .with_font_size(16.0)
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
        let layout = TextLayout::new(&font)
            .with_font_size(16.0)
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
        let layout = TextLayout::new(&font)
            .with_font_size(20.0)
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
        let unwrapped = TextLayout::new(&font).with_font_size(font_size).run(text)?;
        let max_width = (unwrapped.lines[0].advance * 0.45).max(font_size * 4.0);

        let layout = TextLayout::new(&font)
            .with_font_size(font_size)
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
        let unwrapped = TextLayout::new(&font).with_font_size(font_size).run(text)?;
        let layout = TextLayout::new(&font)
            .with_font_size(font_size)
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
        assert!(is_fullwidth_punctuation('‼'));
        assert!(is_fullwidth_punctuation('⁇'));
        assert!(is_fullwidth_punctuation('⁈'));
        assert!(is_fullwidth_punctuation('⁉'));
        assert!(!is_fullwidth_punctuation('A'));
        assert!(!is_fullwidth_punctuation('中'));
    }

    #[test]
    fn cjk_emphasis_punctuation_keeps_every_symbol_and_uses_tighter_spacing() -> anyhow::Result<()>
    {
        let font = any_system_font();
        let text = "後??!";
        let regular = TextLayout::new(&font).with_font_size(16.0).run(text)?;
        let compact = TextLayout::new(&font)
            .with_font_size(16.0)
            .with_compact_emphasis_punctuation(true)
            .run(text)?;
        let clusters = compact.lines[0]
            .glyphs
            .iter()
            .map(|glyph| glyph.cluster)
            .collect::<Vec<_>>();

        assert_eq!(compact.lines[0].range, 0..text.len());
        assert!(clusters.contains(&3));
        assert!(clusters.contains(&4));
        assert!(clusters.contains(&5));
        assert!(compact.lines[0].advance < regular.lines[0].advance);
        Ok(())
    }

    #[test]
    fn emphasis_run_length_includes_marks_on_both_sides() {
        let text = "後？？!続";

        assert_eq!(emphasis_run_length(text, 3), Some(3));
        assert_eq!(emphasis_run_length(text, 6), Some(3));
        assert_eq!(emphasis_run_length(text, 9), Some(3));
        assert_eq!(emphasis_run_length(text, 0), None);
    }

    #[test]
    fn vertical_punctuation_centering_enabled_by_default() {
        let font = any_system_font();
        let layout = TextLayout::new(&font).with_font_size(16.0);
        assert!(layout.center_vertical_punctuation);
    }

    #[test]
    fn centered_x_offset_uses_absolute_center() {
        assert_approx_eq(centered_x_offset(2.0, 6.0), -4.0);
        assert_approx_eq(centered_x_offset(-3.0, 1.0), 1.0);
    }

    #[test]
    fn horizontal_center_alignment_with_overflow_is_aligned_relative_to_widest()
    -> anyhow::Result<()> {
        let font = any_system_font();
        // A very narrow container.
        let max_width = 20.0;
        // A very long word that is guaranteed to overflow 20px in any font.
        let text = "LONGWORDTHATWILLOVERFLOW,\nHI";
        let layout = TextLayout::new(&font)
            .with_font_size(20.0)
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
