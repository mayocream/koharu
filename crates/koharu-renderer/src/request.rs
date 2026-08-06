use std::collections::BTreeSet;

use koharu_scene::{AssetRole, EntityId};

use crate::StrokeOptions;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum VerticalAlignment {
    Top,
    #[default]
    Center,
    Bottom,
}

/// Whether layer visibility and opacity are resolved into the rendered frame.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum LayerPresentation {
    /// Omit hidden layers and bake opacity into each rendered layer.
    #[default]
    Resolved,
    /// Retain every layer at full opacity so an interactive compositor can
    /// apply visibility and opacity without repeating layout or shaping.
    Deferred,
}

/// Non-persistent visual policy applied after scene intent has been resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderTheme {
    pub font_families: Vec<String>,
    pub font_size: f32,
    pub minimum_font_size: f32,
    pub text_color: [u8; 4],
    pub text_stroke: Option<StrokeOptions>,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    /// Insets in top, right, bottom, left order.
    pub text_inset: [f32; 4],
    pub vertical_alignment: VerticalAlignment,
}

impl Default for RenderTheme {
    fn default() -> Self {
        Self {
            font_families: vec!["CCWildWords".to_owned(), "Arial".to_owned()],
            font_size: 24.0,
            minimum_font_size: 9.0,
            text_color: [0, 0, 0, 255],
            text_stroke: None,
            line_height: 1.2,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_inset: [4.0; 4],
            vertical_alignment: VerticalAlignment::Center,
        }
    }
}

/// Everything needed to deterministically render one page revision.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderRequest {
    pub page: EntityId,
    /// Page asset roles in preference order. An empty list renders transparently.
    pub base_assets: Vec<AssetRole>,
    /// Asset role used by image entities.
    pub image_asset: AssetRole,
    pub include_images: bool,
    /// Restricts text composition to specific entities while retaining page context.
    pub text_entities: Option<BTreeSet<EntityId>>,
    pub presentation: LayerPresentation,
    pub fallback_to_source_text: bool,
    pub theme: RenderTheme,
}

impl RenderRequest {
    #[must_use]
    pub fn new(page: EntityId) -> Self {
        Self {
            page,
            base_assets: vec![asset_role("source")],
            image_asset: asset_role("source"),
            include_images: true,
            text_entities: None,
            presentation: LayerPresentation::Resolved,
            fallback_to_source_text: true,
            theme: RenderTheme::default(),
        }
    }

    #[must_use]
    pub fn transparent(page: EntityId) -> Self {
        let mut request = Self::new(page);
        request.base_assets.clear();
        request
    }
}

fn asset_role(value: &str) -> AssetRole {
    AssetRole::new(value).expect("the built-in asset role is valid")
}
