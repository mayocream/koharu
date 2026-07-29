//! Parallel preparation of render-plan layers into reusable Vello scenes.

use std::{collections::HashMap, sync::Arc};

use koharu_scene::{EntityId, Revision, SceneSnapshot};
use rayon::prelude::*;
use vello::{
    Scene,
    kurbo::{Affine, Rect, Vec2},
    peniko::{Blob, Fill, ImageAlphaType, ImageData, ImageFormat, Mix},
};

use crate::{
    Error, HyphenationPolicy, RenderDependency, RenderDiagnostic, RenderOptions, RenderPlan,
    RenderResources, RenderTheme, Result, VerticalAlignment, WritingMode,
    bubble::LayoutBox,
    plan::{ImageLayer, Layer, RenderBounds, TextLayer},
    raster::{DrawStyle, draw_layout},
    script::is_cjk_text,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RenderedEntityKind {
    Image,
    Text,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedEntity {
    pub entity: EntityId,
    pub kind: RenderedEntityKind,
    pub bounds: RenderBounds,
    pub font_size: Option<f32>,
}

pub struct PreparedPage {
    revision: Revision,
    page: EntityId,
    width: u32,
    height: u32,
    scene: Arc<Scene>,
    entity_scenes: HashMap<EntityId, Vec<Arc<Scene>>>,
    entities: Vec<RenderedEntity>,
    dependencies: Vec<RenderDependency>,
    diagnostics: Vec<RenderDiagnostic>,
}

impl std::fmt::Debug for PreparedPage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedPage")
            .field("revision", &self.revision)
            .field("page", &self.page)
            .field("size", &(self.width, self.height))
            .field("entities", &self.entities)
            .field("dependencies", &self.dependencies)
            .field("diagnostics", &self.diagnostics)
            .finish_non_exhaustive()
    }
}

impl PreparedPage {
    /// Shapes text and decodes image resources, then records an immutable Vello page.
    pub fn prepare(
        plan: &RenderPlan,
        snapshot: &SceneSnapshot,
        resources: &RenderResources,
        theme: &RenderTheme,
    ) -> Result<Self> {
        let prepared = plan
            .layers
            .par_iter()
            .map(|layer| prepare_layer(layer, plan, snapshot, resources, theme))
            .collect::<Result<Vec<_>>>()?;
        let mut scene = Scene::new();
        let mut entity_scenes = HashMap::<EntityId, Vec<Arc<Scene>>>::new();
        let mut entities = Vec::with_capacity(prepared.len());
        let mut diagnostics = plan.diagnostics.clone();
        for layer in prepared {
            let entity = layer.entity.entity;
            let layer_scene = Arc::new(layer.scene);
            scene.append(&layer_scene, None);
            entity_scenes.entry(entity).or_default().push(layer_scene);
            entities.push(layer.entity);
            diagnostics.extend(layer.diagnostics);
        }
        Ok(Self {
            revision: plan.revision,
            page: plan.page,
            width: plan.width,
            height: plan.height,
            scene: Arc::new(scene),
            entity_scenes,
            entities,
            dependencies: plan.dependencies.clone(),
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
    pub fn entities(&self) -> &[RenderedEntity] {
        &self.entities
    }

    #[must_use]
    pub fn dependencies(&self) -> &[RenderDependency] {
        &self.dependencies
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[RenderDiagnostic] {
        &self.diagnostics
    }

    /// Appends prepared content to a caller-owned Vello scene without readback.
    pub fn append_to(&self, scene: &mut Scene, transform: Option<Affine>) {
        scene.append(&self.scene, transform);
    }

    /// Appends every prepared layer owned by one entity.
    ///
    /// Interactive consumers use this to apply a transient transform without
    /// repeating image decoding, text shaping, or page preparation.
    pub fn append_entity_to(
        &self,
        entity: EntityId,
        scene: &mut Scene,
        transform: Option<Affine>,
    ) -> bool {
        let Some(prepared) = self.entity_scenes.get(&entity) else {
            return false;
        };
        for layer in prepared {
            scene.append(layer, transform);
        }
        true
    }

    pub(crate) fn scene(&self) -> &Scene {
        &self.scene
    }

    pub(crate) fn at_revision(&self, revision: Revision) -> Self {
        Self {
            revision,
            page: self.page,
            width: self.width,
            height: self.height,
            scene: self.scene.clone(),
            entity_scenes: self.entity_scenes.clone(),
            entities: self.entities.clone(),
            dependencies: self.dependencies.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn empty_for_test(
        revision: Revision,
        page: EntityId,
        dependencies: Vec<RenderDependency>,
    ) -> Self {
        Self {
            revision,
            page,
            width: 100,
            height: 100,
            scene: Arc::new(Scene::new()),
            entity_scenes: HashMap::new(),
            entities: Vec::new(),
            dependencies,
            diagnostics: Vec::new(),
        }
    }
}

struct PreparedLayer {
    scene: Scene,
    entity: RenderedEntity,
    diagnostics: Vec<RenderDiagnostic>,
}

fn prepare_layer(
    layer: &Layer,
    plan: &RenderPlan,
    snapshot: &SceneSnapshot,
    resources: &RenderResources,
    theme: &RenderTheme,
) -> Result<PreparedLayer> {
    match layer {
        Layer::Image(layer) => prepare_image(layer, plan, snapshot, resources),
        Layer::Text(layer) => prepare_text(layer, resources, theme),
    }
}

fn prepare_image(
    layer: &ImageLayer,
    plan: &RenderPlan,
    snapshot: &SceneSnapshot,
    resources: &RenderResources,
) -> Result<PreparedLayer> {
    let image = resources.image(snapshot, &layer.asset)?;
    if layer.is_base && (image.width != plan.width || image.height != plan.height) {
        return Err(Error::invalid(format!(
            "base image for page {} is {}x{}, expected {}x{}",
            plan.page, image.width, image.height, plan.width, plan.height
        )));
    }
    let pixels: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(ImageBytes(image.pixels.clone()));
    let data = ImageData {
        data: Blob::new(pixels),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: image.width,
        height: image.height,
    };
    let transform = Affine::scale_non_uniform(
        f64::from(layer.bounds.width) / f64::from(image.width),
        f64::from(layer.bounds.height) / f64::from(image.height),
    )
    .then_translate(Vec2::new(
        f64::from(layer.bounds.x),
        f64::from(layer.bounds.y),
    ));
    let mut scene = Scene::new();
    if layer.opacity < 1.0 {
        scene.push_layer(
            Fill::NonZero,
            Mix::Normal,
            layer.opacity,
            transform,
            &Rect::new(0.0, 0.0, f64::from(image.width), f64::from(image.height)),
        );
    }
    scene.draw_image(&data, transform);
    if layer.opacity < 1.0 {
        scene.pop_layer();
    }
    Ok(PreparedLayer {
        scene,
        entity: RenderedEntity {
            entity: layer.entity,
            kind: RenderedEntityKind::Image,
            bounds: layer.bounds.into(),
            font_size: None,
        },
        diagnostics: Vec::new(),
    })
}

fn prepare_text(
    layer: &TextLayer,
    resources: &RenderResources,
    theme: &RenderTheme,
) -> Result<PreparedLayer> {
    let is_bubble_text = layer.balloon_contour.is_some();
    let bounds = if is_bubble_text {
        inset(layer.bounds, theme.text_inset)
    } else {
        layer.bounds
    };
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return Err(Error::invalid(format!(
            "text inset leaves no layout area for entity {}",
            layer.entity
        )));
    }
    let fonts = resources
        .fonts()
        .resolve(
            layer.preferred_font.as_deref(),
            &theme.font_families,
            &layer.text,
            layer
                .language
                .as_ref()
                .map(koharu_scene::LanguageTag::as_str),
        )
        .map_err(|source| Error::Font {
            entity: layer.entity,
            source,
        })?;
    let maximum = layer.font_size.unwrap_or_else(|| {
        if is_bubble_text {
            if layer.writing_mode.is_vertical() {
                bounds.height
            } else {
                bounds.width
            }
        } else {
            theme.font_size
        }
    });
    let minimum = theme.minimum_font_size.min(maximum);
    let mut layout = crate::TextLayout::new(&fonts[0])
        .with_fallback_fonts(&fonts[1..])
        .with_writing_mode(layer.writing_mode)
        .with_alignment(layer.alignment)
        .with_line_height(theme.line_height)
        .with_spacing(theme.letter_spacing, theme.word_spacing)
        .with_max_width(bounds.width)
        .with_max_height(bounds.height)
        .with_compact_emphasis_punctuation(
            is_cjk_text(&layer.text)
                || layer
                    .language
                    .as_ref()
                    .is_some_and(|language| is_cjk_language(language.as_str())),
        );
    if let Some(contour) = &layer.balloon_contour {
        let [top, _, _, left] = theme.text_inset;
        layout = layout.with_comic_balloon(
            bounds.width,
            bounds.height,
            contour.iter().map(|&(x, y)| (x - left, y - top)).collect(),
            match theme.vertical_alignment {
                VerticalAlignment::Top => 0.0,
                VerticalAlignment::Center => 0.5,
                VerticalAlignment::Bottom => 1.0,
            },
            theme.text_inset.into_iter().fold(0.0, f32::max),
        );
    }
    if let Some(language) = &layer.language {
        layout = layout.with_hyphenation_language_tag(language.as_str());
        if is_bubble_text
            && layer.writing_mode == WritingMode::Horizontal
            && is_english(language.as_str())
        {
            layout = layout.with_hyphenation_policy(HyphenationPolicy::LastResort);
        }
    }
    let layout = if theme.auto_fit {
        layout
            .with_max_font_size(maximum)
            .with_min_font_size(minimum)
            .with_min_line_height(1.0)
    } else {
        layout.with_font_size(maximum)
    };
    let layout = layout.run(&layer.text).map_err(|source| Error::Layout {
        entity: layer.entity,
        source,
    })?;
    let (mut x, mut y) = placement(
        bounds,
        layout.width,
        layout.height,
        theme.vertical_alignment,
    );
    x += layout.placement_offset_x();
    y += layout.placement_offset_y();
    let layout_rect = Rect::new(
        f64::from(x),
        f64::from(y),
        f64::from(x + layout.width),
        f64::from(y + layout.height),
    );
    let angle = f64::from(layer.angle_degrees).to_radians();
    let center = layout_rect.center();
    let rotation = Affine::rotate_about(angle, center);
    let transform =
        Affine::translate((f64::from(x), f64::from(y))).then_rotate_about(angle, center);
    let options = RenderOptions {
        color: with_alpha(
            layer.foreground_color.unwrap_or(theme.text_color),
            layer.opacity,
        ),
        font_size: layout.font_size,
        stroke: None,
        ..RenderOptions::default()
    };
    let mut scene = Scene::new();
    scene.push_clip_layer(
        Fill::NonZero,
        rotation,
        &Rect::new(
            f64::from(bounds.x),
            f64::from(bounds.y),
            f64::from(bounds.x + bounds.width),
            f64::from(bounds.y + bounds.height),
        ),
    );
    if let Some(mut stroke) = theme.text_stroke {
        stroke.color = with_alpha(stroke.color, layer.opacity);
        draw_layout(
            &mut scene,
            &layout,
            layer.writing_mode,
            &options,
            transform,
            DrawStyle::Stroke(stroke),
        );
    }
    draw_layout(
        &mut scene,
        &layout,
        layer.writing_mode,
        &options,
        transform,
        DrawStyle::Fill,
    );
    scene.pop_layer();
    let rendered_bounds = rotation.transform_rect_bbox(layout_rect);
    let mut diagnostics = Vec::new();
    if layout.font_size + f32::EPSILON < theme.minimum_font_size {
        diagnostics.push(RenderDiagnostic::TextBelowReadableSize {
            entity: layer.entity,
            font_size: layout.font_size,
            minimum_font_size: theme.minimum_font_size,
        });
    }
    if layout.overflowed() {
        diagnostics.push(RenderDiagnostic::TextOverflow {
            entity: layer.entity,
            available: bounds.into(),
            actual_width: layout.width,
            actual_height: layout.height,
            font_size: layout.font_size,
        });
    }
    Ok(PreparedLayer {
        scene,
        entity: RenderedEntity {
            entity: layer.entity,
            kind: RenderedEntityKind::Text,
            bounds: RenderBounds {
                x: rendered_bounds.x0 as f32,
                y: rendered_bounds.y0 as f32,
                width: rendered_bounds.width() as f32,
                height: rendered_bounds.height() as f32,
            },
            font_size: Some(layout.font_size),
        },
        diagnostics,
    })
}

fn is_english(language: &str) -> bool {
    language
        .split(['-', '_'])
        .next()
        .is_some_and(|primary| primary.eq_ignore_ascii_case("en"))
}

fn is_cjk_language(language: &str) -> bool {
    language
        .split(['-', '_'])
        .next()
        .is_some_and(|primary| matches!(primary.to_ascii_lowercase().as_str(), "ja" | "ko" | "zh"))
}

struct ImageBytes(Arc<[u8]>);

impl AsRef<[u8]> for ImageBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

fn inset(rect: LayoutBox, [top, right, bottom, left]: [f32; 4]) -> LayoutBox {
    LayoutBox {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    }
}

fn placement(rect: LayoutBox, width: f32, height: f32, vertical: VerticalAlignment) -> (f32, f32) {
    let x = rect.x + (rect.width - width) * 0.5;
    let remaining = rect.height - height;
    let y = rect.y
        + match vertical {
            VerticalAlignment::Top => 0.0,
            VerticalAlignment::Center => remaining * 0.5,
            VerticalAlignment::Bottom => remaining,
        };
    (x, y)
}

fn with_alpha(mut color: [u8; 4], opacity: f32) -> [u8; 4] {
    color[3] = (f32::from(color[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
    color
}

#[cfg(test)]
mod tests {
    use koharu_scene::{
        At, Authored, Geometry, LanguageTag, Origin, PageDraft, Region, RegionKind, RelationKind,
        SceneSession, SourceText, TextAlignment, Typography,
    };

    use super::*;
    use crate::{BUBBLE_REGION_KIND, RenderRequest, TEXT_REGION_RELATION_KIND};

    fn text_fixture(
        balloon_width: f64,
        balloon_height: f64,
        text: &str,
        font_size: f32,
    ) -> (koharu_scene::SceneSnapshot, RenderPlan, EntityId) {
        let mut session = SceneSession::memory().unwrap();
        let mut text_entity = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(PageDraft::new("page", 300.0, 200.0), At::End)?;
                let bubble = edit.add_entity(page, At::End)?;
                edit.set(
                    bubble,
                    "default",
                    &Geometry::rectangle(10.0, 10.0, balloon_width, balloon_height),
                )?;
                edit.set(
                    bubble,
                    "default",
                    &Region {
                        origin: Origin::User,
                        kind: RegionKind::new(BUBBLE_REGION_KIND)?,
                        label: None,
                    },
                )?;
                let entity = edit.add_entity(page, At::End)?;
                edit.set(
                    entity,
                    "default",
                    &Geometry::rectangle(10.0, 10.0, balloon_width, balloon_height),
                )?;
                edit.set_source_text(
                    entity,
                    SourceText {
                        text: Authored::user(text.to_owned()),
                        language: Some(LanguageTag::new("en")?),
                    },
                )?;
                edit.set(
                    entity,
                    "default",
                    &Typography {
                        origin: Origin::User,
                        preferred_font: None,
                        size: Some(font_size),
                        alignment: Some(TextAlignment::Center),
                        writing_mode: Some(koharu_scene::WritingMode::Horizontal),
                        extensions: Default::default(),
                    },
                )?;
                edit.add_relation(
                    RelationKind::new(TEXT_REGION_RELATION_KIND)?,
                    entity,
                    bubble,
                )?;
                text_entity = Some(entity);
                Ok(())
            })
            .unwrap();
        let snapshot = session.commit(patch).unwrap().snapshot;
        let entity = text_entity.unwrap();
        let page = snapshot.parent(entity).unwrap().unwrap();
        let plan = RenderPlan::compile(&snapshot, &RenderRequest::transparent(page)).unwrap();
        (snapshot, plan, entity)
    }

    #[test]
    fn preparation_reports_balloon_overflow() {
        let (snapshot, plan, entity) = text_fixture(40.0, 18.0, "This dialogue cannot fit", 18.0);
        let theme = RenderTheme {
            auto_fit: false,
            text_inset: [0.0; 4],
            ..RenderTheme::default()
        };

        let prepared =
            PreparedPage::prepare(&plan, &snapshot, &RenderResources::new(), &theme).unwrap();

        assert!(prepared.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            RenderDiagnostic::TextOverflow { entity: found, .. } if *found == entity
        )));
    }

    #[test]
    fn free_text_auto_fits_the_exact_original_block_without_balloon_air() {
        let (snapshot, mut plan, entity) =
            text_fixture(40.0, 18.0, "This free text must shrink", 18.0);
        let Layer::Text(layer) = &mut plan.layers[0] else {
            panic!("expected a text layer");
        };
        layer.balloon_contour = None;
        let original_bounds = layer.bounds;
        let theme = RenderTheme {
            minimum_font_size: 1.0,
            text_inset: [100.0; 4],
            ..RenderTheme::default()
        };

        let prepared =
            PreparedPage::prepare(&plan, &snapshot, &RenderResources::new(), &theme).unwrap();
        let rendered = prepared
            .entities()
            .iter()
            .find(|rendered| rendered.entity == entity)
            .unwrap();

        assert!(rendered.font_size.unwrap() < 18.0);
        assert!(rendered.bounds.x >= original_bounds.x - f32::EPSILON);
        assert!(rendered.bounds.y >= original_bounds.y - f32::EPSILON);
        assert!(rendered.bounds.width <= original_bounds.width + f32::EPSILON);
        assert!(rendered.bounds.height <= original_bounds.height + f32::EPSILON);
        assert!(!prepared.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            RenderDiagnostic::TextOverflow { entity: found, .. } if *found == entity
        )));
    }

    #[test]
    fn automatic_balloon_text_can_grow_beyond_the_theme_font_size() {
        let (snapshot, mut plan, entity) = text_fixture(240.0, 120.0, "Hi", 18.0);
        let Layer::Text(layer) = &mut plan.layers[0] else {
            panic!("expected a text layer");
        };
        layer.font_size = None;
        let theme = RenderTheme {
            text_inset: [0.0; 4],
            ..RenderTheme::default()
        };

        let prepared =
            PreparedPage::prepare(&plan, &snapshot, &RenderResources::new(), &theme).unwrap();
        let rendered = prepared
            .entities()
            .iter()
            .find(|rendered| rendered.entity == entity)
            .unwrap();

        assert!(rendered.font_size.unwrap() > theme.font_size);
    }

    #[test]
    fn preparation_reports_text_below_the_readability_floor() {
        let (snapshot, plan, entity) = text_fixture(240.0, 120.0, "Small dialogue", 8.0);
        let theme = RenderTheme {
            auto_fit: false,
            minimum_font_size: 9.0,
            text_inset: [0.0; 4],
            ..RenderTheme::default()
        };

        let prepared =
            PreparedPage::prepare(&plan, &snapshot, &RenderResources::new(), &theme).unwrap();

        assert!(prepared.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            RenderDiagnostic::TextBelowReadableSize {
                entity: found,
                font_size,
                minimum_font_size,
            } if *found == entity && *font_size == 8.0 && *minimum_font_size == 9.0
        )));
    }

    #[test]
    fn preparation_rotates_text_and_reported_bounds() {
        let (snapshot, plan, entity) = text_fixture(240.0, 120.0, "Rotated text", 18.0);
        let theme = RenderTheme {
            auto_fit: false,
            text_inset: [0.0; 4],
            ..RenderTheme::default()
        };
        let resources = RenderResources::new();
        let baseline = PreparedPage::prepare(&plan, &snapshot, &resources, &theme).unwrap();
        let baseline_bounds = baseline
            .entities()
            .iter()
            .find(|rendered| rendered.entity == entity)
            .unwrap()
            .bounds;

        let mut rotated_plan = plan.clone();
        let Layer::Text(layer) = &mut rotated_plan.layers[0] else {
            panic!("expected a text layer");
        };
        layer.angle_degrees = 90.0;
        let rotated = PreparedPage::prepare(&rotated_plan, &snapshot, &resources, &theme).unwrap();
        let rotated_bounds = rotated
            .entities()
            .iter()
            .find(|rendered| rendered.entity == entity)
            .unwrap()
            .bounds;

        assert!((rotated_bounds.width - baseline_bounds.height).abs() < 1e-4);
        assert!((rotated_bounds.height - baseline_bounds.width).abs() < 1e-4);
        assert!(
            (rotated_bounds.x + rotated_bounds.width * 0.5
                - baseline_bounds.x
                - baseline_bounds.width * 0.5)
                .abs()
                < 1e-4
        );
        assert!(
            (rotated_bounds.y + rotated_bounds.height * 0.5
                - baseline_bounds.y
                - baseline_bounds.height * 0.5)
                .abs()
                < 1e-4
        );
    }
}
