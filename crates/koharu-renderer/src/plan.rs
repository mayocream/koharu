//! Compilation of semantic scene components into backend-independent layers.

use std::collections::BTreeSet;

use koharu_scene::{
    Asset, BlobId, EntityId, Geometry, LanguageTag, OcrAnalysis, Origin, Page, RelationId,
    Revision, SceneSnapshot, SourceText, TextAlignment, TextDirection, Translation, Typography,
    Visibility,
};

use crate::{
    Error, RenderRequest, Result, TextAlign, WritingMode,
    bubble::{LayoutBox, geometry_bounds},
    script::{is_cjk_text, shaping_direction_for_text},
};

const MAX_SURFACE_DIMENSION: u32 = 32_768;
const MAX_SURFACE_PIXELS: u64 = 268_435_456;
const FOREGROUND_COLOR_EXTENSION: &str = "dev.koharu.typography.foreground-color";
const ANGLE_DEGREES_EXTENSION: &str = "dev.koharu.typography.angle-degrees";

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
        locale: LanguageTag,
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
pub struct RenderPlan {
    pub(crate) revision: Revision,
    pub(crate) page: EntityId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) layers: Vec<Layer>,
    pub(crate) dependencies: Vec<RenderDependency>,
    pub(crate) diagnostics: Vec<RenderDiagnostic>,
}

impl RenderPlan {
    /// Resolves scene capabilities into owned, backend-independent layers.
    pub fn compile(snapshot: &SceneSnapshot, request: &RenderRequest) -> Result<Self> {
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
            let Some(asset) = snapshot.component::<Asset>(request.page, role.as_str())? else {
                continue;
            };
            dependencies.insert(RenderDependency::Blob(asset.blob));
            layers.push(Layer::Image(ImageLayer {
                entity: request.page,
                asset,
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
            let visibility = snapshot
                .component::<Visibility>(entity, "default")?
                .unwrap_or(Visibility {
                    origin: koharu_scene::Origin::User,
                    visible: true,
                    opacity: 1.0,
                });
            if !visibility.visible || visibility.opacity <= 0.0 {
                continue;
            }
            let Some(geometry) = snapshot.component::<Geometry>(entity, "default")? else {
                continue;
            };
            let Some(bounds) = geometry_bounds(&geometry) else {
                continue;
            };

            if request.include_images
                && let Some(asset) =
                    snapshot.component::<Asset>(entity, request.image_asset.as_str())?
            {
                dependencies.insert(RenderDependency::Blob(asset.blob));
                layers.push(Layer::Image(ImageLayer {
                    entity,
                    asset,
                    bounds,
                    opacity: visibility.opacity,
                    is_base: false,
                }));
            }

            let Some((text, language)) = resolve_text(snapshot, entity, request, &mut diagnostics)?
            else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            let typography = snapshot.component::<Typography>(entity, "default")?;
            let analysis = snapshot.component::<OcrAnalysis>(entity, "default")?;
            let writing_mode =
                resolve_writing_mode(&text, bounds, typography.as_ref(), analysis.as_ref());
            let mut layout_bounds = bounds;
            let mut balloon_contour = None;
            if let Some(bubble) = crate::bubble::resolve(
                snapshot,
                entity,
                &page_entities,
                &request.text_region_relation,
                &request.bubble_region,
            )? {
                layout_bounds = bubble.bounds;
                balloon_contour = Some(bubble.contour);
                dependencies.insert(RenderDependency::Relation(bubble.relation));
                dependencies.insert(RenderDependency::Entity(bubble.region));
            }
            let (direction, _) = shaping_direction_for_text(&text, writing_mode);
            let rtl = direction == harfrust::Direction::RightToLeft;
            let alignment =
                resolve_alignment(typography.as_ref().and_then(|value| value.alignment), rtl);
            let is_bubble_text = balloon_contour.is_some();
            layers.push(Layer::Text(TextLayer {
                entity,
                text,
                language,
                bounds: layout_bounds,
                balloon_contour,
                opacity: visibility.opacity,
                preferred_font: typography
                    .as_ref()
                    .and_then(|value| value.preferred_font.clone()),
                font_size: if is_bubble_text {
                    user_font_size(typography.as_ref())
                } else {
                    typography.as_ref().and_then(|value| value.size)
                },
                alignment,
                writing_mode,
                foreground_color: resolve_foreground_color(typography.as_ref()),
                angle_degrees: resolve_angle_degrees(typography.as_ref()),
            }));
        }

        Ok(Self {
            revision: snapshot.revision(),
            page: request.page,
            width,
            height,
            layers,
            dependencies: dependencies.into_iter().collect(),
            diagnostics,
        })
    }

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

#[derive(Clone, Debug)]
pub(crate) enum Layer {
    Image(ImageLayer),
    Text(TextLayer),
}

#[derive(Clone, Debug)]
pub(crate) struct ImageLayer {
    pub entity: EntityId,
    pub asset: Asset,
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
    pub font_size: Option<f32>,
    pub alignment: TextAlign,
    pub writing_mode: WritingMode,
    pub foreground_color: Option<[u8; 4]>,
    pub angle_degrees: f32,
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
    snapshot: &SceneSnapshot,
    entity: EntityId,
    request: &RenderRequest,
    diagnostics: &mut Vec<RenderDiagnostic>,
) -> Result<Option<(String, Option<LanguageTag>)>> {
    if let Some(locale) = &request.locale {
        if let Some(translation) = snapshot.component::<Translation>(entity, locale.as_str())? {
            return Ok(Some((translation.text.value, Some(locale.clone()))));
        }
        if !request.fallback_to_source_text {
            return Ok(None);
        }
        if let Some(source) = snapshot.component::<SourceText>(entity, "default")? {
            diagnostics.push(RenderDiagnostic::UsedSourceText {
                entity,
                locale: locale.clone(),
            });
            return Ok(Some((source.text.value, source.language)));
        }
        return Ok(None);
    }
    Ok(snapshot
        .component::<SourceText>(entity, "default")?
        .map(|source| (source.text.value, source.language)))
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

fn user_font_size(typography: Option<&Typography>) -> Option<f32> {
    typography
        .filter(|value| matches!(value.origin, Origin::User))
        .and_then(|value| value.size)
}

fn resolve_foreground_color(typography: Option<&Typography>) -> Option<[u8; 4]> {
    let value = typography?.extensions.get(FOREGROUND_COLOR_EXTENSION)?;
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, u8::MAX])
}

fn resolve_angle_degrees(typography: Option<&Typography>) -> f32 {
    typography
        .and_then(|value| value.extensions.get(ANGLE_DEGREES_EXTENSION))
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
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
        At, Authored, Generation, Geometry, Origin, PageDraft, ProducerId, Region, RegionKind,
        RelationKind, SceneSession, TextAlignment, Translation, Typography,
    };

    use super::*;

    struct Fixture {
        snapshot: SceneSnapshot,
        page: EntityId,
        text: EntityId,
        bubble: EntityId,
        relation: RelationId,
    }

    fn fixture() -> Fixture {
        let mut session = SceneSession::memory().unwrap();
        let mut ids = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(PageDraft::new("page", 200.0, 120.0), At::End)?;
                let bubble = edit.add_entity(page, At::End)?;
                edit.set(
                    bubble,
                    "default",
                    &Geometry::rectangle(20.0, 30.0, 100.0, 50.0),
                )?;
                edit.set(
                    bubble,
                    "default",
                    &Region {
                        origin: Origin::User,
                        kind: RegionKind::new(crate::request::BUBBLE_REGION_KIND)?,
                        label: None,
                    },
                )?;
                let text = edit.add_entity(page, At::End)?;
                edit.set(
                    text,
                    "default",
                    &Geometry::rectangle(30.0, 35.0, 80.0, 40.0),
                )?;
                edit.set_source_text(
                    text,
                    SourceText {
                        text: Authored::user("原文".to_owned()),
                        language: Some(LanguageTag::new("ja")?),
                    },
                )?;
                edit.set_translation(
                    text,
                    &LanguageTag::new("ar")?,
                    Translation {
                        text: Authored::user("مرحبا".to_owned()),
                    },
                )?;
                edit.set(
                    text,
                    "default",
                    &Typography {
                        origin: Origin::User,
                        preferred_font: None,
                        size: Some(18.0),
                        alignment: Some(TextAlignment::Start),
                        writing_mode: None,
                        extensions: [
                            (FOREGROUND_COLOR_EXTENSION.to_owned(), "#123456".to_owned()),
                            (ANGLE_DEGREES_EXTENSION.to_owned(), "12.5".to_owned()),
                        ]
                        .into_iter()
                        .collect(),
                    },
                )?;
                let relation = edit.add_relation(
                    RelationKind::new(crate::request::TEXT_REGION_RELATION_KIND)?,
                    text,
                    bubble,
                )?;
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
    fn compiles_locale_text_and_explicit_bubble_relation() {
        let fixture = fixture();
        let mut request = RenderRequest::transparent(fixture.page);
        request.locale = Some(LanguageTag::new("ar").unwrap());

        let plan = RenderPlan::compile(&fixture.snapshot, &request).unwrap();
        let Layer::Text(text) = &plan.layers[0] else {
            panic!("expected a text layer");
        };

        assert_eq!(text.entity, fixture.text);
        assert_eq!(text.text, "مرحبا");
        assert_eq!(text.language.as_ref().unwrap().as_str(), "ar");
        assert_eq!(text.alignment, TextAlign::Right);
        assert_eq!(text.writing_mode, WritingMode::Horizontal);
        assert_eq!(text.font_size, Some(18.0));
        assert!(text.balloon_contour.is_some());
        assert_eq!(text.foreground_color, Some([0x12, 0x34, 0x56, 0xff]));
        assert_eq!(text.angle_degrees, 12.5);
        assert!((text.bounds.x - 20.0).abs() < 1e-5);
        assert!((text.bounds.y - 30.0).abs() < 1e-5);
        assert!((text.bounds.width - 100.0).abs() < 1e-5);
        assert!((text.bounds.height - 50.0).abs() < 1e-5);
        assert_eq!(text.balloon_contour.as_ref().unwrap().len(), 4);
        assert!(
            plan.dependencies
                .contains(&RenderDependency::Relation(fixture.relation))
        );
        assert!(
            plan.dependencies
                .contains(&RenderDependency::Entity(fixture.bubble))
        );
    }

    #[test]
    fn records_when_a_missing_translation_falls_back_to_source() {
        let fixture = fixture();
        let mut request = RenderRequest::transparent(fixture.page);
        request.locale = Some(LanguageTag::new("fr").unwrap());

        let plan = RenderPlan::compile(&fixture.snapshot, &request).unwrap();
        let Layer::Text(text) = &plan.layers[0] else {
            panic!("expected a text layer");
        };

        assert_eq!(text.text, "原文");
        assert_eq!(text.language.as_ref().unwrap().as_str(), "ja");
        assert_eq!(
            plan.diagnostics,
            vec![RenderDiagnostic::UsedSourceText {
                entity: fixture.text,
                locale: LanguageTag::new("fr").unwrap(),
            }]
        );
    }

    #[test]
    fn missing_translation_can_be_skipped_without_losing_other_capabilities() {
        let fixture = fixture();
        let mut request = RenderRequest::transparent(fixture.page);
        request.locale = Some(LanguageTag::new("fr").unwrap());
        request.fallback_to_source_text = false;

        let plan = RenderPlan::compile(&fixture.snapshot, &request).unwrap();

        assert!(plan.layers.is_empty());
        assert!(plan.diagnostics.is_empty());
    }

    #[test]
    fn writing_mode_is_only_applied_to_cjk_text() {
        let typography = Typography {
            origin: Origin::User,
            preferred_font: None,
            size: None,
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
    fn generated_font_size_does_not_cap_balloon_fitting() {
        let mut typography = Typography {
            origin: Origin::User,
            preferred_font: None,
            size: Some(18.0),
            alignment: None,
            writing_mode: None,
            extensions: Default::default(),
        };
        assert_eq!(user_font_size(Some(&typography)), Some(18.0));

        typography.origin = Origin::Generated(Generation::new(
            ProducerId::new("dev.koharu.pipeline.detection").unwrap(),
        ));
        assert_eq!(user_font_size(Some(&typography)), None);
    }

    #[test]
    fn free_text_keeps_its_source_size_without_balloon_fitting() {
        let fixture = fixture();
        let mut request = RenderRequest::transparent(fixture.page);
        request.locale = Some(LanguageTag::new("ar").unwrap());
        request.text_region_relation =
            RelationKind::new("dev.koharu.relation.unused-text-region").unwrap();

        let plan = RenderPlan::compile(&fixture.snapshot, &request).unwrap();
        let Layer::Text(text) = &plan.layers[0] else {
            panic!("expected a text layer");
        };

        assert_eq!(text.font_size, Some(18.0));
        assert!(text.balloon_contour.is_none());
        assert_eq!(text.bounds.width, 80.0);
    }

    #[test]
    fn invalid_typography_extensions_use_renderer_defaults() {
        let typography = Typography {
            origin: Origin::User,
            preferred_font: None,
            size: None,
            alignment: None,
            writing_mode: None,
            extensions: [
                (FOREGROUND_COLOR_EXTENSION.to_owned(), "white".to_owned()),
                (ANGLE_DEGREES_EXTENSION.to_owned(), "NaN".to_owned()),
            ]
            .into_iter()
            .collect(),
        };

        assert_eq!(resolve_foreground_color(Some(&typography)), None);
        assert_eq!(resolve_angle_degrees(Some(&typography)), 0.0);
    }
}
