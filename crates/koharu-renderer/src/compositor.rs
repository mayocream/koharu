//! Compilation of semantic scene components into a reusable composition.

use std::collections::BTreeSet;

use koharu_scene::{
    Asset, BlobId, EntityId, Geometry, LanguageTag, OcrAnalysis, Page, RasterLayer,
    RasterLayerKind, RelationId, Revision, Snapshot, SourceText, TextAlignment, TextDirection,
    TextLayout as SceneTextLayout, TextLayoutKind, Translation, Typography, Visibility,
};

use crate::{
    Error, LayerPresentation, RenderRequest, Result, StrokeOptions, TextAlign, WritingMode,
    bubble::{LayoutBox, geometry_bounds, geometry_frame},
    script::{is_cjk_text, shaping_direction_for_text},
};

const MAX_SURFACE_DIMENSION: u32 = 32_768;
const MAX_SURFACE_PIXELS: u64 = 268_435_456;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RenderDependency {
    Entity(EntityId),
    Relation(RelationId),
    Blob(BlobId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderDiagnostic {
    MissingBaseAsset {
        roles: Vec<String>,
    },
    UsedSourceText {
        entity: EntityId,
    },
    TextOverflow {
        entity: EntityId,
        available: RenderBounds,
        actual_width: f32,
        actual_height: f32,
        font_size: f32,
    },
    TextBelowReadableSize {
        entity: EntityId,
        font_size: f32,
        minimum_font_size: f32,
    },
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RenderBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<LayoutBox> for RenderBounds {
    fn from(value: LayoutBox) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Composition {
    pub(crate) revision: Revision,
    pub(crate) page: EntityId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) layers: Vec<Layer>,
    pub(crate) dependencies: Vec<RenderDependency>,
    pub(crate) diagnostics: Vec<RenderDiagnostic>,
    pub(crate) request: RenderRequest,
}

impl Composition {
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub const fn page(&self) -> EntityId {
        self.page
    }

    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    #[must_use]
    pub fn dependencies(&self) -> &[RenderDependency] {
        &self.dependencies
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[RenderDiagnostic] {
        &self.diagnostics
    }
}

/// Compiles a semantic Koharu page into ordered visual layers.
#[derive(Clone, Copy, Debug, Default)]
pub struct Compositor;

impl Compositor {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Resolves scene capabilities into an owned, backend-independent composition.
    pub fn compile(&self, snapshot: &Snapshot, request: &RenderRequest) -> Result<Composition> {
        compile(snapshot, request)
    }
}

fn compile(snapshot: &Snapshot, request: &RenderRequest) -> Result<Composition> {
    validate_request(request)?;
    let page = snapshot.page(request.page)?.page()?;
    let (width, height) = surface_size(&page)?;
    let page_entities = snapshot
        .subtree(request.page)?
        .map(|entity| entity.id())
        .collect::<BTreeSet<_>>();
    let mut dependencies = BTreeSet::from([RenderDependency::Entity(request.page)]);
    let mut diagnostics = Vec::new();
    let mut layers = Vec::new();

    let mut base_found = false;
    for role in &request.base_assets {
        let Some(asset) = snapshot.asset(request.page, role)? else {
            continue;
        };
        dependencies.insert(RenderDependency::Blob(asset.blob));
        layers.push(Layer::Image(ImageLayer {
            entity: request.page,
            asset,
            name: None,
            kind: crate::VisualLayerKind::Image,
            bounds: LayoutBox {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
            },
            opacity: 1.0,
            is_base: true,
        }));
        base_found = true;
        break;
    }
    if !request.base_assets.is_empty() && !base_found {
        diagnostics.push(RenderDiagnostic::MissingBaseAsset {
            roles: request
                .base_assets
                .iter()
                .map(|role| role.as_str().to_owned())
                .collect(),
        });
    }

    for entity_ref in snapshot.subtree(request.page)?.skip(1) {
        let entity = entity_ref.id();
        dependencies.insert(RenderDependency::Entity(entity));
        let mut visibility = snapshot
            .component::<Visibility>(entity)?
            .unwrap_or(Visibility {
                origin: koharu_scene::Origin::User,
                visible: true,
                opacity: 1.0,
            });
        if request.presentation == LayerPresentation::Deferred {
            visibility.visible = true;
            visibility.opacity = 1.0;
        }
        if !visibility.visible || visibility.opacity <= 0.0 {
            continue;
        }
        if let Some(raster) = snapshot.component::<RasterLayer>(entity)? {
            if request.include_images
                && let Some(asset) = snapshot.asset(entity, &request.image_asset)?
            {
                dependencies.insert(RenderDependency::Blob(asset.blob));
                layers.push(Layer::Image(ImageLayer {
                    entity,
                    asset,
                    name: Some(raster.name),
                    kind: match raster.kind {
                        RasterLayerKind::Cleanup => crate::VisualLayerKind::Cleanup,
                        RasterLayerKind::Paint => crate::VisualLayerKind::Paint,
                    },
                    bounds: LayoutBox {
                        x: 0.0,
                        y: 0.0,
                        width: width as f32,
                        height: height as f32,
                    },
                    opacity: visibility.opacity,
                    is_base: false,
                }));
            }
            continue;
        }
        if let Some(text_layout) = snapshot.component::<SceneTextLayout>(entity)? {
            if request
                .text_entities
                .as_ref()
                .is_some_and(|entities| !entities.contains(&entity))
            {
                continue;
            }
            let text_layer = snapshot.text_layer(entity)?;
            let content = text_layer.content()?.id();
            let presents = snapshot
                .relation_from::<koharu_scene::Presents>(entity)?
                .expect("validated text layers present content");
            dependencies.insert(RenderDependency::Relation(presents.id()));
            dependencies.insert(RenderDependency::Entity(content));

            let geometry = snapshot.component::<Geometry>(entity)?;
            let (text_frame, balloon_contour) = if let Some(geometry) = geometry.as_ref() {
                let Some(frame) = geometry_frame(geometry) else {
                    continue;
                };
                (frame, None)
            } else {
                let Some(fit) = crate::bubble::resolve(snapshot, entity, &page_entities)? else {
                    continue;
                };
                dependencies.insert(RenderDependency::Relation(fit.relation));
                dependencies.insert(RenderDependency::Entity(fit.region));
                (fit.frame, fit.balloon_contour)
            };

            let Some((text, language)) =
                resolve_text(snapshot, content, entity, request, &mut diagnostics)?
            else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            let typography = text_layer.typography()?;
            let analysis = if let Some(recognized) =
                snapshot.relation_from::<koharu_scene::RecognizedFrom>(content)?
            {
                dependencies.insert(RenderDependency::Relation(recognized.id()));
                dependencies.insert(RenderDependency::Entity(recognized.value().target));
                snapshot.component::<OcrAnalysis>(recognized.value().target)?
            } else {
                None
            };
            let writing_mode = resolve_writing_mode(
                &text,
                text_frame.bounds,
                typography.as_ref(),
                analysis.as_ref(),
            );
            let (direction, _) = shaping_direction_for_text(&text, writing_mode);
            let rtl = direction == harfrust::Direction::RightToLeft;
            let alignment =
                resolve_alignment(typography.as_ref().and_then(|value| value.alignment), rtl);
            let is_bubble_text = balloon_contour.is_some();
            layers.push(Layer::Text(TextLayer {
                entity,
                text,
                language,
                bounds: text_frame.bounds,
                balloon_contour,
                opacity: visibility.opacity,
                preferred_font: typography
                    .as_ref()
                    .and_then(|value| value.preferred_font.clone()),
                font_weight: typography.as_ref().and_then(|value| value.font_weight),
                font_size: typography.as_ref().and_then(|value| value.size),
                auto_fit: typography.as_ref().is_none_or(|value| value.auto_fit),
                alignment,
                writing_mode,
                foreground_color: typography.as_ref().and_then(|value| value.color),
                stroke: resolve_stroke(typography.as_ref()),
                angle_degrees: text_frame.angle_degrees,
                point_text: !is_bubble_text && text_layout.kind == TextLayoutKind::Point,
            }));
            continue;
        }

        let Some(geometry) = snapshot.component::<Geometry>(entity)? else {
            continue;
        };
        let Some(bounds) = geometry_bounds(&geometry) else {
            continue;
        };
        if request.include_images
            && let Some(asset) = snapshot.asset(entity, &request.image_asset)?
        {
            dependencies.insert(RenderDependency::Blob(asset.blob));
            layers.push(Layer::Image(ImageLayer {
                entity,
                asset,
                name: None,
                kind: crate::VisualLayerKind::Image,
                bounds,
                opacity: visibility.opacity,
                is_base: false,
            }));
        }
    }

    Ok(Composition {
        revision: snapshot.revision(),
        page: request.page,
        width,
        height,
        layers,
        dependencies: dependencies.into_iter().collect(),
        diagnostics,
        request: request.clone(),
    })
}

#[derive(Clone, Debug)]
pub(crate) enum Layer {
    Image(ImageLayer),
    Text(TextLayer),
}

#[derive(Clone, Debug)]
pub(crate) struct ImageLayer {
    pub entity: EntityId,
    pub asset: Asset,
    pub name: Option<String>,
    pub kind: crate::VisualLayerKind,
    pub bounds: LayoutBox,
    pub opacity: f32,
    pub is_base: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct TextLayer {
    pub entity: EntityId,
    pub text: String,
    pub language: Option<LanguageTag>,
    pub bounds: LayoutBox,
    pub balloon_contour: Option<Vec<(f32, f32)>>,
    pub opacity: f32,
    pub preferred_font: Option<String>,
    pub font_weight: Option<u16>,
    pub font_size: Option<f32>,
    pub auto_fit: bool,
    pub alignment: TextAlign,
    pub writing_mode: WritingMode,
    pub foreground_color: Option<[u8; 4]>,
    pub stroke: Option<StrokeOptions>,
    pub angle_degrees: f32,
    pub point_text: bool,
}

fn validate_request(request: &RenderRequest) -> Result<()> {
    let theme = &request.theme;
    let valid = theme.font_size.is_finite()
        && theme.font_size > 0.0
        && theme.minimum_font_size.is_finite()
        && theme.minimum_font_size > 0.0
        && theme.minimum_font_size <= theme.font_size
        && theme.line_height.is_finite()
        && theme.line_height > 0.0
        && theme.letter_spacing.is_finite()
        && theme.word_spacing.is_finite()
        && theme
            .text_inset
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0)
        && theme
            .text_stroke
            .is_none_or(|stroke| stroke.width_px.is_finite() && stroke.width_px >= 0.0);
    if valid {
        Ok(())
    } else {
        Err(Error::invalid("render theme contains invalid dimensions"))
    }
}

fn surface_size(page: &Page) -> Result<(u32, u32)> {
    let width = page.width.ceil() as u32;
    let height = page.height.ceil() as u32;
    if width == 0
        || height == 0
        || width > MAX_SURFACE_DIMENSION
        || height > MAX_SURFACE_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_SURFACE_PIXELS
    {
        return Err(Error::invalid(format!(
            "page surface {width}x{height} exceeds renderer limits"
        )));
    }
    Ok((width, height))
}

fn resolve_text(
    snapshot: &Snapshot,
    content: EntityId,
    layer: EntityId,
    request: &RenderRequest,
    diagnostics: &mut Vec<RenderDiagnostic>,
) -> Result<Option<(String, Option<LanguageTag>)>> {
    if let Some(translation) = snapshot.component::<Translation>(content)? {
        return Ok(Some((translation.text.value, translation.language)));
    }
    if !request.fallback_to_source_text {
        return Ok(None);
    }
    if let Some(source) = snapshot.component::<SourceText>(content)? {
        diagnostics.push(RenderDiagnostic::UsedSourceText { entity: layer });
        return Ok(Some((source.text.value, source.language)));
    }
    Ok(None)
}

fn resolve_writing_mode(
    text: &str,
    bounds: LayoutBox,
    typography: Option<&Typography>,
    analysis: Option<&OcrAnalysis>,
) -> WritingMode {
    if !is_cjk_text(text) {
        return WritingMode::Horizontal;
    }
    if let Some(mode) = typography.and_then(|value| value.writing_mode) {
        return match mode {
            koharu_scene::WritingMode::Horizontal => WritingMode::Horizontal,
            koharu_scene::WritingMode::Vertical => WritingMode::VerticalRl,
        };
    }
    match analysis.map(|value| value.direction) {
        Some(TextDirection::Vertical) => WritingMode::VerticalRl,
        Some(TextDirection::Horizontal) => WritingMode::Horizontal,
        Some(TextDirection::Auto) | None if bounds.height > bounds.width => WritingMode::VerticalRl,
        Some(TextDirection::Auto) | None => WritingMode::Horizontal,
    }
}

fn resolve_stroke(typography: Option<&Typography>) -> Option<StrokeOptions> {
    let typography = typography?;
    let width_px = typography.stroke_width.filter(|width| *width > 0.0)?;
    Some(StrokeOptions {
        color: typography.stroke_color.unwrap_or([u8::MAX; 4]),
        width_px,
    })
}

fn resolve_alignment(alignment: Option<TextAlignment>, rtl: bool) -> TextAlign {
    match alignment.unwrap_or(TextAlignment::Center) {
        TextAlignment::Start if rtl => TextAlign::Right,
        TextAlignment::Start => TextAlign::Left,
        TextAlignment::Center => TextAlign::Center,
        TextAlignment::End if rtl => TextAlign::Left,
        TextAlignment::End => TextAlign::Right,
        TextAlignment::Justify => TextAlign::Justify,
    }
}

#[cfg(test)]
mod tests {
    use koharu_scene::{
        AssetInput, AssetMetadata, AssetRole, At, Authored, BubbleRegion, FitsTo, Geometry, Origin,
        PageDraft, Point, RasterLayer, RasterLayerKind, RecognizedFrom, Session, TextAlignment,
        TextLayout, TextLayoutKind, TextRegion, Translation, Typography, Visibility,
    };

    use super::*;

    struct Fixture {
        snapshot: Snapshot,
        page: EntityId,
        text: EntityId,
        bubble: EntityId,
        relation: RelationId,
    }

    fn fixture(include_translation: bool) -> Fixture {
        fixture_with_visibility(include_translation, None, Some(18.0), false)
    }

    fn rotated_geometry(x: f64, y: f64, width: f64, height: f64, degrees: f64) -> Geometry {
        let center_x = x + width * 0.5;
        let center_y = y + height * 0.5;
        let (sin, cos) = degrees.to_radians().sin_cos();
        Geometry {
            origin: Origin::User,
            points: [
                (-width * 0.5, -height * 0.5),
                (width * 0.5, -height * 0.5),
                (width * 0.5, height * 0.5),
                (-width * 0.5, height * 0.5),
            ]
            .map(|(x, y)| Point {
                x: center_x + x * cos - y * sin,
                y: center_y + x * sin + y * cos,
            })
            .into(),
        }
    }

    fn fixture_with_visibility(
        include_translation: bool,
        visibility: Option<Visibility>,
        font_size: Option<f32>,
        manual_frame: bool,
    ) -> Fixture {
        let mut session = Session::memory().unwrap();
        let mut ids = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(PageDraft::new("page", 200.0, 120.0), At::End)?;
                let bubble = edit.add_analysis_region::<BubbleRegion>(
                    page,
                    At::End,
                    &Geometry::rectangle(20.0, 30.0, 100.0, 50.0),
                    None,
                )?;
                let source_region = edit.add_analysis_region::<TextRegion>(
                    page,
                    At::End,
                    &rotated_geometry(30.0, 35.0, 80.0, 40.0, 12.5),
                    None,
                )?;
                let content = edit.add_text_content(page, At::End)?;
                edit.set(
                    content,
                    &SourceText {
                        text: Authored::user("原文".to_owned()),
                        language: Some(LanguageTag::new("ja")?),
                    },
                )?;
                if include_translation {
                    edit.set(
                        content,
                        &Translation {
                            text: Authored::user("مرحبا".to_owned()),
                            language: Some(LanguageTag::new("ar")?),
                        },
                    )?;
                }
                let text = edit.add_text_layer(
                    page,
                    At::End,
                    content,
                    &TextLayout {
                        origin: Origin::User,
                        kind: TextLayoutKind::Paragraph,
                    },
                )?;
                if manual_frame {
                    edit.set(text, &rotated_geometry(30.0, 35.0, 80.0, 40.0, 12.5))?;
                }
                edit.set(
                    text,
                    &Typography {
                        origin: Origin::User,
                        preferred_font: None,
                        font_weight: Some(600),
                        size: font_size.filter(|size| *size > 0.0),
                        auto_fit: font_size.is_none_or(|size| size <= 0.0),
                        color: Some([0x12, 0x34, 0x56, 0xff]),
                        stroke_color: Some([0xff, 0xff, 0xff, 0xff]),
                        stroke_width: Some(1.5),
                        alignment: Some(TextAlignment::Start),
                        writing_mode: None,
                        extensions: Default::default(),
                    },
                )?;
                if let Some(visibility) = &visibility {
                    edit.set(text, visibility)?;
                }
                let relation = edit.relate::<FitsTo>(text, bubble)?;
                edit.relate::<RecognizedFrom>(content, source_region)?;
                ids = Some((page, text, bubble, relation));
                Ok(())
            })
            .unwrap();
        let snapshot = session.commit(patch).unwrap().snapshot;
        let (page, text, bubble, relation) = ids.unwrap();
        Fixture {
            snapshot,
            page,
            text,
            bubble,
            relation,
        }
    }

    #[test]
    fn deferred_presentation_retains_hidden_text_at_full_opacity() {
        let fixture = fixture_with_visibility(
            true,
            Some(Visibility {
                origin: Origin::User,
                visible: false,
                opacity: 0.25,
            }),
            Some(18.0),
            false,
        );
        let compositor = Compositor::new();

        let resolved = compositor
            .compile(&fixture.snapshot, &RenderRequest::transparent(fixture.page))
            .unwrap();
        assert!(resolved.layers.is_empty());

        let mut deferred = RenderRequest::transparent(fixture.page);
        deferred.presentation = LayerPresentation::Deferred;
        let deferred = compositor.compile(&fixture.snapshot, &deferred).unwrap();
        let Layer::Text(text) = &deferred.layers[0] else {
            panic!("expected retained text layer");
        };
        assert_eq!(text.entity, fixture.text);
        assert_eq!(text.opacity, 1.0);
    }

    #[test]
    fn compiles_translation_and_explicit_bubble_relation() {
        let fixture = fixture(true);
        let request = RenderRequest::transparent(fixture.page);

        let composition = Compositor::new()
            .compile(&fixture.snapshot, &request)
            .unwrap();
        let Layer::Text(text) = &composition.layers[0] else {
            panic!("expected a text layer");
        };

        assert_eq!(text.entity, fixture.text);
        assert_eq!(text.text, "مرحبا");
        assert_eq!(text.language.as_ref().unwrap().as_str(), "ar");
        assert_eq!(text.alignment, TextAlign::Right);
        assert_eq!(text.writing_mode, WritingMode::Horizontal);
        assert_eq!(text.font_size, Some(18.0));
        assert_eq!(text.font_weight, Some(600));
        assert!(text.balloon_contour.is_some());
        assert_eq!(text.foreground_color, Some([0x12, 0x34, 0x56, 0xff]));
        assert_eq!(
            text.stroke,
            Some(StrokeOptions {
                color: [0xff; 4],
                width_px: 1.5,
            })
        );
        assert_eq!(text.angle_degrees, 0.0);
        assert!((text.bounds.x - 20.0).abs() < 1e-5);
        assert!((text.bounds.y - 30.0).abs() < 1e-5);
        assert!((text.bounds.width - 100.0).abs() < 1e-5);
        assert!((text.bounds.height - 50.0).abs() < 1e-5);
        assert_eq!(text.balloon_contour.as_ref().unwrap().len(), 4);
        assert!(
            composition
                .dependencies
                .contains(&RenderDependency::Relation(fixture.relation))
        );
        assert!(
            composition
                .dependencies
                .contains(&RenderDependency::Entity(fixture.bubble))
        );
    }

    #[test]
    fn automatic_font_size_is_an_explicit_layout_mode() {
        for size in [None, Some(0.0)] {
            let fixture = fixture_with_visibility(true, None, size, false);
            let composition = Compositor::new()
                .compile(&fixture.snapshot, &RenderRequest::transparent(fixture.page))
                .unwrap();
            let Layer::Text(text) = &composition.layers[0] else {
                panic!("expected a text layer");
            };
            assert_eq!(text.font_size, None);
            assert!(text.auto_fit);
        }
    }

    #[test]
    fn compiles_raster_paint_as_its_own_visual_layer() {
        let mut session = Session::memory().unwrap();
        let mut ids = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(PageDraft::new("page", 100.0, 100.0), At::End)?;
                let drawing = edit.add_entity(page, At::End)?;
                edit.set(
                    drawing,
                    &RasterLayer {
                        origin: Origin::User,
                        name: "Drawing 1".to_owned(),
                        kind: RasterLayerKind::Paint,
                    },
                )?;
                edit.set_asset(
                    drawing,
                    &AssetRole::new("source")?,
                    AssetInput::new(
                        std::sync::Arc::<[u8]>::from(&b"png"[..]),
                        "image/png",
                        AssetMetadata {
                            width: Some(100),
                            height: Some(100),
                            attributes: Default::default(),
                        },
                    ),
                )?;
                ids = Some((page, drawing));
                Ok(())
            })
            .unwrap();
        let snapshot = session.commit(patch).unwrap().snapshot;
        let (page, drawing) = ids.unwrap();

        let composition = Compositor::new()
            .compile(&snapshot, &RenderRequest::transparent(page))
            .unwrap();
        let Layer::Image(layer) = &composition.layers[0] else {
            panic!("expected paint layer");
        };
        assert_eq!(layer.entity, drawing);
        assert_eq!(layer.name.as_deref(), Some("Drawing 1"));
        assert_eq!(layer.kind, crate::VisualLayerKind::Paint);
        assert_eq!(layer.bounds.width, 100.0);
        assert_eq!(layer.bounds.height, 100.0);
    }

    #[test]
    fn records_when_a_missing_translation_falls_back_to_source() {
        let fixture = fixture(false);
        let request = RenderRequest::transparent(fixture.page);

        let composition = Compositor::new()
            .compile(&fixture.snapshot, &request)
            .unwrap();
        let Layer::Text(text) = &composition.layers[0] else {
            panic!("expected a text layer");
        };

        assert_eq!(text.text, "原文");
        assert_eq!(text.language.as_ref().unwrap().as_str(), "ja");
        assert_eq!(
            composition.diagnostics,
            vec![RenderDiagnostic::UsedSourceText {
                entity: fixture.text,
            }]
        );
    }

    #[test]
    fn missing_translation_can_be_skipped_without_losing_other_capabilities() {
        let fixture = fixture(false);
        let mut request = RenderRequest::transparent(fixture.page);
        request.fallback_to_source_text = false;

        let composition = Compositor::new()
            .compile(&fixture.snapshot, &request)
            .unwrap();

        assert!(composition.layers.is_empty());
        assert!(composition.diagnostics.is_empty());
    }

    #[test]
    fn writing_mode_is_only_applied_to_cjk_text() {
        let typography = Typography {
            origin: Origin::User,
            preferred_font: None,
            font_weight: None,
            size: None,
            auto_fit: true,
            color: None,
            stroke_color: None,
            stroke_width: None,
            alignment: None,
            writing_mode: Some(koharu_scene::WritingMode::Vertical),
            extensions: Default::default(),
        };
        let bounds = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 80.0,
        };

        assert_eq!(
            resolve_writing_mode("Latin", bounds, Some(&typography), None),
            WritingMode::Horizontal
        );
        assert_eq!(
            resolve_writing_mode("日本語", bounds, Some(&typography), None),
            WritingMode::VerticalRl
        );
        assert_eq!(
            resolve_writing_mode("한국어", bounds, Some(&typography), None),
            WritingMode::VerticalRl
        );
    }

    #[test]
    fn manual_text_frame_overrides_automatic_region_fitting() {
        let fixture = fixture_with_visibility(true, None, Some(18.0), true);
        let composition = Compositor::new()
            .compile(&fixture.snapshot, &RenderRequest::transparent(fixture.page))
            .unwrap();
        let Layer::Text(text) = &composition.layers[0] else {
            panic!("expected a text layer");
        };

        assert_eq!(text.font_size, Some(18.0));
        assert!(text.balloon_contour.is_none());
        assert_eq!(text.bounds.width, 80.0);
        assert_eq!(text.angle_degrees, 12.5);
    }
}
