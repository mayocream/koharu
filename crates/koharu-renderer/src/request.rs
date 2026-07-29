use koharu_scene::{AssetRole, EntityId, LanguageTag, RegionKind, RelationKind};

use crate::{RasterOptions, StrokeOptions};

/// Default relation from a text entity to the region that constrains its layout.
pub const TEXT_REGION_RELATION_KIND: &str = "dev.koharu.relation.text-region";
/// Default semantic region kind used for speech-bubble safe areas.
pub const BUBBLE_REGION_KIND: &str = "dev.koharu.region.bubble";

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum VerticalAlignment {
    Top,
    #[default]
    Center,
    Bottom,
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
    pub auto_fit: bool,
}

impl Default for RenderTheme {
    fn default() -> Self {
        Self {
            font_families: Vec::new(),
            font_size: 24.0,
            minimum_font_size: 9.0,
            text_color: [0, 0, 0, 255],
            text_stroke: None,
            line_height: 1.2,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_inset: [4.0; 4],
            vertical_alignment: VerticalAlignment::Center,
            auto_fit: true,
        }
    }
}

/// Everything needed to deterministically render one page revision.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderRequest {
    pub page: EntityId,
    pub locale: Option<LanguageTag>,
    /// Page asset roles in preference order. An empty list renders transparently.
    pub base_assets: Vec<AssetRole>,
    /// Asset role used by image entities.
    pub image_asset: AssetRole,
    pub include_images: bool,
    pub fallback_to_source_text: bool,
    pub text_region_relation: RelationKind,
    pub bubble_region: RegionKind,
    pub theme: RenderTheme,
    pub raster: RasterOptions,
}

impl RenderRequest {
    #[must_use]
    pub fn new(page: EntityId) -> Self {
        Self {
            page,
            locale: None,
            base_assets: vec![asset_role("clean"), asset_role("source")],
            image_asset: asset_role("source"),
            include_images: true,
            fallback_to_source_text: true,
            text_region_relation: RelationKind::new(TEXT_REGION_RELATION_KIND)
                .expect("the built-in relation kind is valid"),
            bubble_region: RegionKind::new(BUBBLE_REGION_KIND)
                .expect("the built-in region kind is valid"),
            theme: RenderTheme::default(),
            raster: RasterOptions::default(),
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
