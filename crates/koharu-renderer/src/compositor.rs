//! Scene-aware composition and font discovery.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow, bail};
use fontique::{Attributes, FontStyle, FontWeight, FontWidth};
use image::{DynamicImage, RgbaImage, imageops};
use koharu_fonts::{GoogleFonts, parse_variant_query};
use koharu_scene::{
    BlendMode, BlobId, Element, ElementId, ElementKind, FontSlant, Frame, Page, StrokePosition,
    TextDirection as SceneTextDirection, TextEffectKind, TextFit, TextOverflow, TextStyle,
    WritingMode as SceneWritingMode,
};
use vello::{
    Scene,
    kurbo::{Affine, Line, Point, Rect, Stroke, Vec2},
    peniko::{Blob, Color, Fill, ImageAlphaType, ImageData, ImageFormat, Mix},
};

use crate::{
    TextAlign as RendererTextAlign,
    bubble::{LayoutBox, layout_box as bubble_layout_box},
    font::{Font, FontSystem},
    layout::{TextLayout, WritingMode},
    renderer::{DrawStyle, RasterOptions, RenderOptions, StrokeOptions, WgpuRenderer, draw_layout},
    script::{fontique_scripts, is_cjk_text, shaping_direction_for_text},
    types::{FontFaceInfo, FontFaceStyle, FontSource},
};

#[derive(Debug, Clone, Default)]
pub struct PageRenderOptions {
    pub document_font: Option<String>,
    pub target_language: Option<String>,
    pub raster: RasterOptions,
}

/// Opaque snapshot of the scene inputs used to encode one element.
///
/// Viewport caches compare this key without learning which related scene
/// elements affect renderer layout. Keeping dependency discovery here ensures
/// that rendering and cache invalidation follow the same rules.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementRenderKey {
    element: Element,
    related_region: Option<Element>,
}

#[derive(Debug)]
pub struct RenderedElement {
    pub element: ElementId,
    /// Laid-out text bounds in page coordinates. Rotation is stored separately in the frame.
    pub frame: Frame,
}

#[derive(Debug)]
pub struct RenderedPage {
    pub image: RgbaImage,
    pub elements: Vec<RenderedElement>,
}

/// Scene-aware renderer shared by the application and export paths.
#[derive(Clone)]
pub struct SceneRenderer {
    fonts: Arc<Mutex<FontSystem>>,
    google_fonts: Arc<GoogleFonts>,
}

pub struct Renderer {
    scene: SceneRenderer,
    rasterizer: WgpuRenderer,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            scene: SceneRenderer::new()?,
            rasterizer: WgpuRenderer::new()?,
        })
    }

    #[must_use]
    pub fn from_parts(
        fonts: FontSystem,
        google_fonts: GoogleFonts,
        rasterizer: WgpuRenderer,
    ) -> Self {
        Self {
            scene: SceneRenderer::from_parts(fonts, google_fonts),
            rasterizer,
        }
    }

    pub fn available_fonts(&self) -> Result<Vec<FontFaceInfo>> {
        self.scene.available_fonts()
    }

    /// Returns the Google Fonts catalog and cache used for font discovery.
    pub fn google_fonts(&self) -> &GoogleFonts {
        self.scene.google_fonts()
    }

    pub fn resolve_post_script_name(
        &self,
        style: &TextStyle,
        text: Option<&str>,
    ) -> Result<String> {
        self.scene.resolve_post_script_name(style, text)
    }

    /// Composites visible translated text elements over `base` in scene order.
    ///
    /// `base` is normally the page's clean/inpainted asset. Image elements are intentionally
    /// not loaded here because blob decoding belongs to the caller that owns the scene session.
    pub fn composite_text(
        &self,
        base: &DynamicImage,
        page: &Page,
        options: &PageRenderOptions,
    ) -> Result<RenderedPage> {
        if base.width() != page.size.width || base.height() != page.size.height {
            bail!(
                "base image is {}x{}, but page is {}x{}",
                base.width(),
                base.height(),
                page.size.width,
                page.size.height
            );
        }

        let mut scene = Scene::new();
        let mut elements = Vec::with_capacity(page.elements.len());
        for element in &page.elements {
            let Some(rendered) = self
                .scene
                .encode_text_element(&mut scene, page, element, options)?
            else {
                continue;
            };
            elements.push(rendered);
        }
        if elements.is_empty() {
            return Ok(RenderedPage {
                image: base.to_rgba8(),
                elements,
            });
        }
        // Submit and read back once per page rather than once per text element. This is the
        // important performance boundary for an editor page containing many translated blocks.
        let text = self.rasterizer.rasterize(
            &scene,
            page.size.width,
            page.size.height,
            [0, 0, 0, 0],
            options.raster,
        )?;
        let mut image = base.to_rgba8();
        imageops::overlay(&mut image, &text, 0, 0);
        Ok(RenderedPage { image, elements })
    }

    /// Composites every visible scene element over `base` in exact element order.
    /// Image bytes are resolved lazily by the caller that owns scene storage.
    pub fn composite_page(
        &self,
        base: &DynamicImage,
        page: &Page,
        mut resolve: impl FnMut(BlobId) -> Result<Arc<[u8]>>,
        options: &PageRenderOptions,
    ) -> Result<RenderedPage> {
        if base.width() != page.size.width || base.height() != page.size.height {
            bail!(
                "base image is {}x{}, but page is {}x{}",
                base.width(),
                base.height(),
                page.size.width,
                page.size.height
            );
        }

        let mut scene = Scene::new();
        let mut elements = Vec::with_capacity(page.elements.len());
        let mut has_content = false;
        for element in &page.elements {
            if !element.visible || element.opacity <= 0.0 {
                continue;
            }
            match &element.kind {
                ElementKind::Text(_) => {
                    if let Some(rendered) = self
                        .scene
                        .encode_text_element(&mut scene, page, element, options)?
                    {
                        elements.push(rendered);
                        has_content = true;
                    }
                }
                ElementKind::Image(image) => {
                    let decoded = image::load_from_memory(&resolve(image.blob)?)?.into_rgba8();
                    if decoded.width() != image.natural_size.width
                        || decoded.height() != image.natural_size.height
                    {
                        bail!(
                            "image element {} is {}x{}, but its natural size is {}x{}",
                            element.id,
                            decoded.width(),
                            decoded.height(),
                            image.natural_size.width,
                            image.natural_size.height
                        );
                    }
                    let width = decoded.width();
                    let height = decoded.height();
                    let pixels: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(decoded.into_raw());
                    let data = ImageData {
                        data: Blob::new(pixels),
                        format: ImageFormat::Rgba8,
                        alpha_type: ImageAlphaType::Alpha,
                        width,
                        height,
                    };
                    has_content = SceneRenderer::encode_image_element(&mut scene, element, &data);
                }
                ElementKind::Region(_) => {}
            }
        }
        if !has_content {
            return Ok(RenderedPage {
                image: base.to_rgba8(),
                elements,
            });
        }
        let content = self.rasterizer.rasterize(
            &scene,
            page.size.width,
            page.size.height,
            [0, 0, 0, 0],
            options.raster,
        )?;
        let mut output = base.to_rgba8();
        imageops::overlay(&mut output, &content, 0, 0);
        Ok(RenderedPage {
            image: output,
            elements,
        })
    }
}

impl SceneRenderer {
    pub fn new() -> Result<Self> {
        let google_fonts =
            GoogleFonts::new().context("failed to initialize Google Fonts service")?;
        Ok(Self::from_parts(FontSystem::new(), google_fonts))
    }

    #[must_use]
    pub fn from_parts(fonts: FontSystem, google_fonts: GoogleFonts) -> Self {
        Self {
            fonts: Arc::new(Mutex::new(fonts)),
            google_fonts: Arc::new(google_fonts),
        }
    }

    pub fn available_fonts(&self) -> Result<Vec<FontFaceInfo>> {
        let mut font_system = self
            .fonts
            .lock()
            .map_err(|_| anyhow!("font system lock was poisoned"))?;
        let mut fonts = font_system
            .system_faces()
            .into_iter()
            .filter(|face| !face.post_script_name.is_empty())
            .map(|face| FontFaceInfo {
                family_name: face.family_name,
                post_script_name: face.post_script_name,
                weight: face.weight,
                stretch: face.stretch,
                style: face.style,
                source: FontSource::System,
                category: None,
                cached: true,
            })
            .collect::<Vec<_>>();
        for entry in &self.google_fonts.catalog().fonts {
            for variant in &entry.variants {
                fonts.push(FontFaceInfo {
                    family_name: entry.family.clone(),
                    post_script_name: format!(
                        "{}:{}{}",
                        entry.family,
                        variant.weight,
                        if variant.style == "italic" { "i" } else { "" }
                    ),
                    weight: variant.weight,
                    stretch: 100,
                    style: if variant.style == "italic" {
                        FontFaceStyle::Italic
                    } else {
                        FontFaceStyle::Normal
                    },
                    source: FontSource::Google,
                    category: Some(entry.category.clone()),
                    cached: self.google_fonts.is_variant_cached(&entry.family, variant),
                });
            }
        }
        fonts.sort();
        fonts.dedup();
        Ok(fonts)
    }

    pub fn google_fonts(&self) -> &GoogleFonts {
        &self.google_fonts
    }

    pub async fn fetch_google_font(&self, family: &str, weight: u16, italic: bool) -> Result<()> {
        self.google_fonts
            .fetch_variant(family, weight, if italic { "italic" } else { "normal" })
            .await?;
        Ok(())
    }

    pub fn resolve_post_script_name(
        &self,
        style: &TextStyle,
        text: Option<&str>,
    ) -> Result<String> {
        self.resolve_fonts(style, text.unwrap_or_default(), None, None)?
            .into_iter()
            .next()
            .map(|font| font.post_script_name().to_owned())
            .context("no usable fonts are installed")
    }

    #[must_use]
    pub fn element_render_key(&self, page: &Page, element: &Element) -> ElementRenderKey {
        let related_region = element
            .text()
            .and_then(|text| text.bubble)
            .and_then(|id| page.element(id))
            .cloned();
        ElementRenderKey {
            element: element.clone(),
            related_region,
        }
    }

    pub fn encode_text_element(
        &self,
        scene: &mut Scene,
        page: &Page,
        element: &Element,
        options: &PageRenderOptions,
    ) -> Result<Option<RenderedElement>> {
        let Some(block) = element.text() else {
            return Ok(None);
        };
        let Some(text) = block
            .translation
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        else {
            return Ok(None);
        };
        if !element.visible || element.opacity <= 0.0 {
            return Ok(None);
        }

        let writing_mode = resolve_writing_mode(element, block.layout.writing_mode, text, block);
        let seed = LayoutBox {
            x: element.frame.x,
            y: element.frame.y,
            width: element.frame.width,
            height: element.frame.height,
        };
        let layout_box = if block.layout.fit == TextFit::Bubble {
            bubble_layout_box(page, element, writing_mode).unwrap_or(seed)
        } else {
            seed
        };
        let layout_box = inset(layout_box, block.layout.inset);
        if layout_box.width <= 0.0 || layout_box.height <= 0.0 {
            return Ok(None);
        }

        let fonts = self.resolve_fonts(
            &block.style,
            text,
            options.document_font.as_deref(),
            options.target_language.as_deref(),
        )?;
        let (base_direction, _) = shaping_direction_for_text(text, writing_mode);
        let rtl = base_direction == harfrust::Direction::RightToLeft;
        let align = match block.layout.horizontal_align {
            koharu_scene::TextAlign::Start if rtl => RendererTextAlign::Right,
            koharu_scene::TextAlign::Start => RendererTextAlign::Left,
            koharu_scene::TextAlign::Center => RendererTextAlign::Center,
            koharu_scene::TextAlign::End if rtl => RendererTextAlign::Left,
            koharu_scene::TextAlign::End => RendererTextAlign::Right,
            koharu_scene::TextAlign::Justify => RendererTextAlign::Justify,
        };
        let scale_x = block.style.horizontal_scale / 100.0;
        let scale_y = block.style.vertical_scale / 100.0;
        let mut layout = TextLayout::new(&fonts[0])
            .with_fallback_fonts(&fonts[1..])
            .with_writing_mode(writing_mode)
            .with_alignment(align)
            .with_line_height(block.style.line_height)
            .with_spacing(block.style.letter_spacing, block.style.word_spacing)
            .with_max_width(layout_box.width / scale_x)
            .with_max_height(layout_box.height / scale_y);
        layout = if block.layout.fit == TextFit::Bubble {
            layout
                .with_max_font_size(block.style.font_size)
                .with_min_font_size((block.style.font_size * 0.6).max(9.0))
                .with_min_line_height(1.0)
        } else {
            layout.with_font_size(block.style.font_size)
        };
        if let Some(language) = options.target_language.as_deref() {
            layout = layout.with_hyphenation_language_tag(language);
        }
        let layout = layout.run(text)?;
        let visual_width = layout.width * scale_x;
        let visual_height = layout.height * scale_y;
        let angle = element.frame.angle_degrees + block.style.angle_degrees;
        let (x, y) = placement(
            layout_box,
            visual_width,
            visual_height,
            block.layout.vertical_align,
        );
        let transform = Affine::scale_non_uniform(scale_x as f64, scale_y as f64)
            .then_rotate_about(
                angle.to_radians() as f64,
                Point::new((visual_width * 0.5) as f64, (visual_height * 0.5) as f64),
            )
            .then_translate(Vec2::new(x as f64, y as f64));
        let (fill, strokes) = supported_effects(&block.style, element.opacity)?;
        let render_options = RenderOptions {
            color: fill,
            font_size: layout.font_size,
            baseline_shift: block.style.baseline_shift,
            stroke: None,
            // Supersampling is applied to the completed page, not individual glyph runs.
            raster: RasterOptions::default(),
            ..Default::default()
        };
        let clip_to_layout_box =
            block.layout.fit == TextFit::Bubble || block.layout.overflow == TextOverflow::Clip;
        if clip_to_layout_box {
            let clip = Rect::new(
                ((layout_box.x - x) / scale_x) as f64,
                ((layout_box.y - y) / scale_y) as f64,
                ((layout_box.x + layout_box.width - x) / scale_x) as f64,
                ((layout_box.y + layout_box.height - y) / scale_y) as f64,
            );
            scene.push_clip_layer(Fill::NonZero, transform, &clip);
        }
        for stroke in strokes.into_iter().rev() {
            draw_layout(
                scene,
                &layout,
                writing_mode,
                &render_options,
                transform,
                DrawStyle::Stroke(stroke),
            );
        }
        draw_layout(
            scene,
            &layout,
            writing_mode,
            &render_options,
            transform,
            DrawStyle::Fill,
        );
        if block.style.decoration.underline || block.style.decoration.strikethrough {
            draw_decorations(
                scene,
                &layout,
                writing_mode,
                &render_options,
                transform,
                block.style.decoration,
            );
        }
        if clip_to_layout_box {
            scene.pop_layer();
        }
        Ok(Some(RenderedElement {
            element: element.id,
            frame: Frame {
                x,
                y,
                width: visual_width,
                height: visual_height,
                angle_degrees: angle,
            },
        }))
    }

    /// Appends an image element using the same frame, rotation, and opacity semantics as text.
    pub fn encode_image_element(scene: &mut Scene, element: &Element, data: &ImageData) -> bool {
        let Some(image) = element.image_data() else {
            return false;
        };
        if !element.visible || element.opacity <= 0.0 {
            return false;
        }
        let frame = element.frame;
        let center = Point::new(f64::from(frame.width) * 0.5, f64::from(frame.height) * 0.5);
        let transform = Affine::scale_non_uniform(
            f64::from(frame.width) / f64::from(image.natural_size.width),
            f64::from(frame.height) / f64::from(image.natural_size.height),
        )
        .then_rotate_about(f64::from(frame.angle_degrees).to_radians(), center)
        .then_translate(Vec2::new(f64::from(frame.x), f64::from(frame.y)));
        if element.opacity < 1.0 {
            scene.push_layer(
                Fill::NonZero,
                Mix::Normal,
                element.opacity,
                transform,
                &Rect::new(
                    0.0,
                    0.0,
                    f64::from(image.natural_size.width),
                    f64::from(image.natural_size.height),
                ),
            );
        }
        scene.draw_image(data, transform);
        if element.opacity < 1.0 {
            scene.pop_layer();
        }
        true
    }

    fn resolve_fonts(
        &self,
        style: &TextStyle,
        text: &str,
        document_font: Option<&str>,
        language: Option<&str>,
    ) -> Result<Vec<Font>> {
        let mut ordered_families = Vec::new();
        let mut registrations = Vec::new();
        for (priority, candidate) in font_candidates(style, document_font)
            .into_iter()
            .enumerate()
        {
            let (family, weight, slant) = parse_variant_query(&candidate);
            if candidate.contains(':')
                && let Some(data) = self
                    .google_fonts
                    .read_cached_variant(family, weight, slant)?
            {
                registrations.push((priority, candidate, data));
                continue;
            }
            if let Some(data) = self.google_fonts.read_cached_file(&candidate)? {
                registrations.push((priority, candidate, data));
                continue;
            }
            ordered_families.push((priority, candidate));
        }
        let mut fonts = self
            .fonts
            .lock()
            .map_err(|_| anyhow!("font system lock was poisoned"))?;
        for (priority, key, data) in registrations {
            ordered_families.extend(
                fonts
                    .register(&key, data)?
                    .into_iter()
                    .map(|family| (priority, family)),
            );
        }
        ordered_families.sort_by_key(|(priority, _)| *priority);
        let families = ordered_families
            .into_iter()
            .map(|(_, family)| family)
            .collect::<Vec<_>>();
        fonts.resolve(
            &families,
            font_attributes(style),
            &fontique_scripts(text),
            language,
        )
    }
}

fn font_candidates(style: &TextStyle, document_font: Option<&str>) -> Vec<String> {
    if !style.font_families.is_empty() {
        return style.font_families.clone();
    }
    if let Some(font) = document_font {
        return vec![font.to_owned()];
    }
    Vec::new()
}

fn resolve_writing_mode(
    element: &Element,
    requested: SceneWritingMode,
    text: &str,
    block: &koharu_scene::TextBlock,
) -> WritingMode {
    // Persisted scenes may contain the source typography detector's vertical mode. The rendered
    // translation owns the final direction, and non-CJK scripts must remain horizontal.
    if !is_cjk_text(text) {
        return WritingMode::Horizontal;
    }
    match requested {
        SceneWritingMode::Horizontal => WritingMode::Horizontal,
        SceneWritingMode::VerticalRightToLeft => WritingMode::VerticalRl,
        SceneWritingMode::VerticalLeftToRight => WritingMode::VerticalLr,
        SceneWritingMode::Auto => infer_writing_mode(
            &element.frame,
            text,
            block.source.as_ref().map(|source| source.direction),
        ),
    }
}

fn infer_writing_mode(
    frame: &Frame,
    text: &str,
    source_direction: Option<SceneTextDirection>,
) -> WritingMode {
    if !is_cjk_text(text) {
        return WritingMode::Horizontal;
    }
    match source_direction {
        Some(SceneTextDirection::Vertical) => WritingMode::VerticalRl,
        Some(SceneTextDirection::Horizontal) => WritingMode::Horizontal,
        Some(SceneTextDirection::Auto) | None if frame.height > frame.width => {
            WritingMode::VerticalRl
        }
        Some(SceneTextDirection::Auto) | None => WritingMode::Horizontal,
    }
}

fn font_attributes(style: &TextStyle) -> Attributes {
    let slant = match style.font_slant {
        FontSlant::Normal => FontStyle::Normal,
        FontSlant::Italic => FontStyle::Italic,
        FontSlant::Oblique { angle_degrees } => FontStyle::Oblique(Some(angle_degrees)),
    };
    Attributes::new(
        FontWidth::from_percentage(style.font_stretch),
        slant,
        FontWeight::new(f32::from(style.font_weight)),
    )
}

fn supported_effects(
    style: &TextStyle,
    element_opacity: f32,
) -> Result<([u8; 4], Vec<StrokeOptions>)> {
    let mut fill = style.color;
    let mut strokes = Vec::new();
    for effect in style
        .effects
        .iter()
        .filter(|effect| effect.enabled && effect.opacity > 0.0)
    {
        if effect.blend_mode != BlendMode::Normal {
            bail!(
                "text effect blend mode {:?} is not supported",
                effect.blend_mode
            );
        }
        match effect.kind {
            TextEffectKind::Stroke {
                color,
                width,
                position: StrokePosition::Center,
            } if width > 0.0 => strokes.push(StrokeOptions {
                color: with_alpha(color, effect.opacity * element_opacity),
                width_px: width,
            }),
            TextEffectKind::Stroke { position, .. } => {
                bail!("text stroke position {position:?} is not supported")
            }
            TextEffectKind::ColorOverlay { color } => {
                fill = mix_color(fill, color, effect.opacity);
            }
            ref unsupported => bail!("text effect {unsupported:?} is not supported"),
        }
    }
    Ok((with_alpha(fill, element_opacity), strokes))
}

fn mix_color(base: [u8; 4], overlay: [u8; 4], amount: f32) -> [u8; 4] {
    let amount = amount.clamp(0.0, 1.0) * (f32::from(overlay[3]) / 255.0);
    std::array::from_fn(|index| {
        if index == 3 {
            base[3]
        } else {
            (f32::from(base[index]) * (1.0 - amount) + f32::from(overlay[index]) * amount).round()
                as u8
        }
    })
}

fn draw_decorations(
    scene: &mut Scene,
    layout: &crate::layout::LayoutRun<'_>,
    writing_mode: WritingMode,
    options: &RenderOptions,
    transform: Affine,
    decoration: koharu_scene::TextDecoration,
) {
    let stroke = Stroke::new((layout.font_size / 16.0).max(1.0) as f64);
    let brush = Color::from_rgba8(
        options.color[0],
        options.color[1],
        options.color[2],
        options.color[3],
    );
    for line in &layout.lines {
        if writing_mode.is_vertical() {
            let y0 = line.baseline.1 - options.baseline_shift;
            if decoration.underline {
                let x = line.baseline.0 + layout.font_size * 0.1;
                scene.stroke(
                    &stroke,
                    transform,
                    brush,
                    None,
                    &Line::new(
                        (x as f64, y0 as f64),
                        (x as f64, (y0 + line.advance) as f64),
                    ),
                );
            }
            if decoration.strikethrough {
                let x = line.baseline.0 - layout.font_size * 0.3;
                scene.stroke(
                    &stroke,
                    transform,
                    brush,
                    None,
                    &Line::new(
                        (x as f64, y0 as f64),
                        (x as f64, (y0 + line.advance) as f64),
                    ),
                );
            }
        } else {
            let x0 = line.baseline.0;
            if decoration.underline {
                let y = line.baseline.1 + layout.font_size * 0.1 - options.baseline_shift;
                scene.stroke(
                    &stroke,
                    transform,
                    brush,
                    None,
                    &Line::new(
                        (x0 as f64, y as f64),
                        ((x0 + line.advance) as f64, y as f64),
                    ),
                );
            }
            if decoration.strikethrough {
                let y = line.baseline.1 - layout.font_size * 0.3 - options.baseline_shift;
                scene.stroke(
                    &stroke,
                    transform,
                    brush,
                    None,
                    &Line::new(
                        (x0 as f64, y as f64),
                        ((x0 + line.advance) as f64, y as f64),
                    ),
                );
            }
        }
    }
}

fn with_alpha(mut color: [u8; 4], opacity: f32) -> [u8; 4] {
    color[3] = (f32::from(color[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
    color
}

fn inset(rect: LayoutBox, [top, right, bottom, left]: [f32; 4]) -> LayoutBox {
    LayoutBox {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    }
}

fn placement(
    rect: LayoutBox,
    width: f32,
    height: f32,
    vertical: koharu_scene::VerticalAlign,
) -> (f32, f32) {
    let x = rect.x + (rect.width - width) * 0.5;
    let remaining = rect.height - height;
    let y = rect.y
        + match vertical {
            koharu_scene::VerticalAlign::Top => 0.0,
            koharu_scene::VerticalAlign::Center => remaining * 0.5,
            koharu_scene::VerticalAlign::Bottom => remaining,
        };
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_scene_insets_in_css_order() {
        assert_eq!(
            inset(
                LayoutBox {
                    x: 10.0,
                    y: 20.0,
                    width: 100.0,
                    height: 80.0,
                },
                [1.0, 2.0, 3.0, 4.0],
            ),
            LayoutBox {
                x: 14.0,
                y: 21.0,
                width: 94.0,
                height: 76.0,
            }
        );
    }

    #[test]
    fn writing_mode_prefers_source_direction_over_frame_shape() {
        let wide = Frame {
            width: 200.0,
            height: 60.0,
            ..Default::default()
        };
        let tall = Frame {
            width: 40.0,
            height: 120.0,
            ..Default::default()
        };

        assert_eq!(
            infer_writing_mode(&wide, "縦書き", Some(SceneTextDirection::Vertical)),
            WritingMode::VerticalRl
        );
        assert_eq!(
            infer_writing_mode(&tall, "横書き", Some(SceneTextDirection::Horizontal)),
            WritingMode::Horizontal
        );
    }

    #[test]
    fn writing_mode_uses_geometry_only_for_auto_cjk_text() {
        let tall = Frame {
            width: 40.0,
            height: 120.0,
            ..Default::default()
        };

        assert_eq!(
            infer_writing_mode(&tall, "縦書き", None),
            WritingMode::VerticalRl
        );
        assert_eq!(
            infer_writing_mode(&tall, "HELLO", Some(SceneTextDirection::Auto)),
            WritingMode::Horizontal
        );
    }

    #[test]
    fn latin_translation_overrides_a_persisted_vertical_source_layout() {
        let block = koharu_scene::TextBlock::default();
        let element = Element::new_text(
            ElementId::new(),
            Frame {
                width: 40.0,
                height: 120.0,
                ..Default::default()
            },
            block.clone(),
        );

        assert_eq!(
            resolve_writing_mode(
                &element,
                SceneWritingMode::VerticalRightToLeft,
                "English translation",
                &block,
            ),
            WritingMode::Horizontal
        );
    }
}
