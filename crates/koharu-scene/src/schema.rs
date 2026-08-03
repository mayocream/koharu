//! Koharu document schema and cross-component invariants.
//!
//! The state kernel owns storage, hierarchy, indexing, and relations as data.
//! This module owns the meaning of Koharu's component combinations and typed
//! relation endpoints.

use crate::{
    DetectionAnalysis, EntityId, EntityOrigin, Error, Geometry, Group, OcrAnalysis, Page, Project,
    RasterLayer, Region, Relation, Result, SourceText, TextContent, TextGroup, TextLayout,
    TextRole, Translation, Typography, Visibility,
    component::{Component, ComponentRecord, ValidationContext, decode, key},
    components::Assets,
    state::{Components, State},
};

pub(crate) fn validate_components(
    components: &Components,
    context: &ValidationContext<'_>,
) -> Result<()> {
    for (key, raw) in components {
        validate_component(&key.kind, raw, context)?;
    }
    Ok(())
}

pub(crate) fn validate_entity(state: &State, id: EntityId) -> Result<()> {
    let entity = state.entity(id)?;
    let has = |kind: &str| entity.components.iter().any(|(key, _)| key.kind == kind);
    let has_source = has(SourceText::KIND);
    let has_content = has(TextContent::KIND);
    let has_translation = has(Translation::KIND);
    let has_region = has(Region::KIND);
    let has_geometry = has(Geometry::KIND);
    let has_raster = has(RasterLayer::KIND);
    let has_assets = has(Assets::KIND);
    let has_layout = has(TextLayout::KIND);
    let has_typography = has(Typography::KIND);
    let has_detection = has(DetectionAnalysis::KIND);
    let has_ocr = has(OcrAnalysis::KIND);
    let has_group = has(Group::KIND);
    let has_text_group = has(TextGroup::KIND);
    let parent = state.parent_and_position(id)?.0;
    let parent_is_text_group = parent.is_some_and(|parent| {
        state.entity(parent).is_ok_and(|entity| {
            entity
                .components
                .iter()
                .any(|(key, _)| key.kind == TextGroup::KIND)
        })
    });

    if (has_source || has_translation || has(TextRole::KIND)) && !has_content {
        Err(Error::invalid(format!(
            "entity {id} carries text content data but is not text content"
        )))
    } else if has_translation && !has_source {
        Err(Error::invalid(format!(
            "entity {id} has a translation but no source text"
        )))
    } else if has_text_group && !has_group {
        Err(Error::invalid(format!(
            "text group {id} does not carry the group component"
        )))
    } else if has_group
        && (has(Page::KIND)
            || has_content
            || has_region
            || has_geometry
            || has_raster
            || has_assets
            || has_layout
            || has_typography
            || has_detection
            || has_ocr)
    {
        Err(Error::invalid(format!(
            "group {id} also carries content, analysis, or layer components"
        )))
    } else if has_text_group {
        let page = state.page_for(id)?;
        if parent != Some(page) {
            return Err(Error::invalid(format!(
                "text group {id} is not a direct child of its page"
            )));
        }
        let groups = state.page(page)?.entities_with(&key::<TextGroup>()?);
        if groups.len() != 1 {
            return Err(Error::invalid(format!(
                "page {page} must not contain multiple text groups"
            )));
        }
        Ok(())
    } else if parent_is_text_group && !has_layout {
        Err(Error::invalid(format!(
            "text group child {id} is not a text layer"
        )))
    } else if has_layout && !parent_is_text_group {
        Err(Error::invalid(format!(
            "text layer {id} is not contained by the page text group"
        )))
    } else if has_content
        && (has_region || has_geometry || has_layout || has_typography || has_detection || has_ocr)
    {
        Err(Error::invalid(format!(
            "text content entity {id} also carries analysis or presentation components"
        )))
    } else if has_region && (has_layout || has_typography || has_source || has_translation) {
        Err(Error::invalid(format!(
            "analysis region {id} also carries text presentation or content components"
        )))
    } else if has_region && !has_geometry {
        Err(Error::invalid(format!(
            "analysis region {id} has no geometry"
        )))
    } else if has_layout
        && (has_source || has_translation || has_region || has_detection || has_ocr)
    {
        Err(Error::invalid(format!(
            "text layer {id} also carries content or analysis components"
        )))
    } else if has_typography && !has_layout {
        Err(Error::invalid(format!(
            "entity {id} has typography but is not a text layer"
        )))
    } else if (has_detection || has_ocr) && !has_region {
        Err(Error::invalid(format!(
            "entity {id} has analysis results but is not a region"
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_relation(
    state: &State,
    relation: &Relation,
    context: &ValidationContext<'_>,
) -> Result<()> {
    validate_relation_endpoints(state, relation, context)?;
    if is_functional(&relation.kind)
        && state
            .outgoing
            .get(&relation.source)
            .into_iter()
            .flat_map(|relations| relations.iter())
            .filter(|relation_id| state.relations[*relation_id].value.kind == relation.kind)
            .count()
            > 1
    {
        return Err(Error::invalid(format!(
            "entity {} has multiple {} relations",
            relation.source,
            relation.kind.as_str()
        )));
    }
    Ok(())
}

pub(crate) fn validate_new_relation(
    state: &State,
    relation: &Relation,
    context: &ValidationContext<'_>,
) -> Result<()> {
    validate_relation_endpoints(state, relation, context)?;
    if is_functional(&relation.kind)
        && state
            .outgoing
            .get(&relation.source)
            .into_iter()
            .flat_map(|relations| relations.iter())
            .any(|relation_id| state.relations[relation_id].value.kind == relation.kind)
    {
        return Err(Error::invalid(format!(
            "entity {} already has a {} relation",
            relation.source,
            relation.kind.as_str()
        )));
    }
    Ok(())
}

fn validate_relation_endpoints(
    state: &State,
    relation: &Relation,
    context: &ValidationContext<'_>,
) -> Result<()> {
    relation.validate(context)?;
    let has = |entity, kind: &str| {
        state
            .entity(entity)
            .is_ok_and(|entity| entity.components.iter().any(|(key, _)| key.kind == kind))
    };
    let valid = match relation.kind.as_str() {
        <crate::Presents as crate::RelationSpec>::KIND => {
            has(relation.source, TextLayout::KIND) && has(relation.target, TextContent::KIND)
        }
        <crate::RecognizedFrom as crate::RelationSpec>::KIND => {
            has(relation.source, TextContent::KIND) && has(relation.target, Region::KIND)
        }
        <crate::FitsTo as crate::RelationSpec>::KIND => {
            has(relation.source, TextLayout::KIND)
                && has(relation.target, Region::KIND)
                && has(relation.target, Geometry::KIND)
        }
        <crate::Inside as crate::RelationSpec>::KIND => {
            has(relation.source, Region::KIND)
                && has(relation.target, Region::KIND)
                && has(relation.source, Geometry::KIND)
                && has(relation.target, Geometry::KIND)
        }
        _ => true,
    };
    if !valid {
        Err(Error::invalid(format!(
            "relation {} has incompatible endpoints",
            relation.kind.as_str()
        )))
    } else {
        Ok(())
    }
}

fn is_functional(kind: &crate::RelationKind) -> bool {
    matches!(
        kind.as_str(),
        <crate::Presents as crate::RelationSpec>::KIND
            | <crate::RecognizedFrom as crate::RelationSpec>::KIND
            | <crate::FitsTo as crate::RelationSpec>::KIND
    )
}

fn validate_component(
    kind: &str,
    raw: &ComponentRecord,
    context: &ValidationContext<'_>,
) -> Result<()> {
    macro_rules! validate {
        ($type:ty) => {
            if kind == <$type as Component>::KIND {
                decode::<$type>(raw, context)?;
                return Ok(());
            }
        };
    }
    validate!(Project);
    validate!(Page);
    validate!(Group);
    validate!(TextGroup);
    validate!(Geometry);
    validate!(RasterLayer);
    validate!(Visibility);
    validate!(SourceText);
    validate!(TextContent);
    validate!(TextLayout);
    validate!(OcrAnalysis);
    validate!(Translation);
    validate!(TextRole);
    validate!(Typography);
    validate!(Region);
    validate!(DetectionAnalysis);
    validate!(Assets);
    validate!(EntityOrigin);
    Ok(())
}
