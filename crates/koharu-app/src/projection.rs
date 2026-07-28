//! Read-only adapters from semantic scene components to UI DTOs.

use koharu_scene::{
    Asset, EntityId, Geometry, LanguageTag, PageRef, Region, SceneSnapshot, SourceText,
    Translation, Typography, Visibility,
};

use crate::protocol::{
    AssetView, EntityView, GeometryView, PageSize, PageSummary, PageView, Point, RegionView,
    SourceTextView, TranslationView, TypographyIntent, VisibilityView,
};

pub(crate) fn page_summary(
    snapshot: &SceneSnapshot,
    page: PageRef<'_>,
) -> anyhow::Result<PageSummary> {
    let value = page.page()?;
    Ok(PageSummary {
        id: page.id(),
        label: value.label,
        size: PageSize {
            width: value.width,
            height: value.height,
        },
        source: asset_id(snapshot, page.id(), "source")?,
        clean: asset_id(snapshot, page.id(), "clean")?,
        entities: snapshot.descendants(page.id())?.count(),
    })
}

pub(crate) fn page_view(
    snapshot: &SceneSnapshot,
    page: EntityId,
    locale: Option<&LanguageTag>,
) -> anyhow::Result<PageView> {
    let value = snapshot.page(page)?.page()?;
    let entities = snapshot
        .descendants(page)?
        .map(|entity| entity_view(snapshot, entity.id(), locale))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(PageView {
        id: page,
        label: value.label,
        size: PageSize {
            width: value.width,
            height: value.height,
        },
        assets: page_assets(snapshot, page)?,
        entities,
    })
}

fn entity_view(
    snapshot: &SceneSnapshot,
    entity: EntityId,
    locale: Option<&LanguageTag>,
) -> anyhow::Result<EntityView> {
    let geometry = snapshot
        .component::<Geometry>(entity, "default")?
        .map(|geometry| GeometryView {
            points: geometry
                .points
                .into_iter()
                .map(|point| Point {
                    x: point.x,
                    y: point.y,
                })
                .collect(),
        });
    let visibility = snapshot.component::<Visibility>(entity, "default")?.map_or(
        VisibilityView {
            visible: true,
            opacity: 1.0,
        },
        |visibility| VisibilityView {
            visible: visibility.visible,
            opacity: visibility.opacity,
        },
    );
    let source_text = snapshot
        .component::<SourceText>(entity, "default")?
        .map(|source| SourceTextView {
            text: source.text.value,
            language: source.language.map(|language| language.to_string()),
        });
    let translation = locale
        .map(|locale| {
            snapshot
                .component::<Translation>(entity, locale.as_str())
                .map(|value| {
                    value.map(|translation| TranslationView {
                        locale: locale.to_string(),
                        text: translation.text.value,
                    })
                })
        })
        .transpose()?
        .flatten();
    let typography = snapshot
        .component::<Typography>(entity, "default")?
        .map(|typography| TypographyIntent {
            preferred_font: typography.preferred_font,
            size: typography.size,
            alignment: typography.alignment,
            writing_mode: typography.writing_mode,
        });
    let region = snapshot
        .component::<Region>(entity, "default")?
        .map(|region| RegionView {
            kind: region.kind.as_str().to_owned(),
            label: region.label,
        });
    Ok(EntityView {
        id: entity,
        parent: snapshot.parent(entity)?,
        geometry,
        visibility,
        image: asset_id(snapshot, entity, "source")?,
        source_text,
        translation,
        typography,
        region,
    })
}

fn page_assets(snapshot: &SceneSnapshot, page: EntityId) -> anyhow::Result<AssetView> {
    Ok(AssetView {
        source: asset_id(snapshot, page, "source")?,
        clean: asset_id(snapshot, page, "clean")?,
        rendered: asset_id(snapshot, page, "rendered")?,
        text_mask: asset_id(snapshot, page, "text-mask")?,
        coo_mask: asset_id(snapshot, page, "coo-mask")?,
        bubble_mask: asset_id(snapshot, page, "bubble-mask")?,
        brush_mask: asset_id(snapshot, page, "brush-mask")?,
    })
}

fn asset_id(
    snapshot: &SceneSnapshot,
    entity: EntityId,
    role: &str,
) -> anyhow::Result<Option<String>> {
    Ok(snapshot
        .component::<Asset>(entity, role)?
        .map(|asset| asset.blob.to_string()))
}
