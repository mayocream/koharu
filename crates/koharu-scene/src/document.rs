//! Resolved document views and intent-level edits over the generic scene kernel.

use crate::{
    At, DetectionAnalysis, Edit, EntityId, Geometry, OcrAnalysis, Origin, Presents, Region,
    RegionSpec, Result, Snapshot, SourceText, TextContent, TextLayout, TextRole, Translation,
    Typography, Visibility,
};

#[derive(Copy, Clone)]
pub struct TextLayerRef<'a> {
    snapshot: &'a Snapshot,
    id: EntityId,
}

impl<'a> TextLayerRef<'a> {
    #[must_use]
    pub const fn id(self) -> EntityId {
        self.id
    }

    pub fn layout(self) -> Result<TextLayout> {
        required(self.snapshot.component(self.id)?, self.id, "text layout")
    }

    pub fn typography(self) -> Result<Option<Typography>> {
        self.snapshot.component(self.id)
    }

    pub fn visibility(self) -> Result<Option<Visibility>> {
        self.snapshot.component(self.id)
    }

    pub fn content(self) -> Result<TextContentRef<'a>> {
        let relation = self
            .snapshot
            .relation_from::<Presents>(self.id)?
            .ok_or_else(|| {
                crate::Error::invalid(format!("text layer {} has no content", self.id))
            })?;
        self.snapshot.text_content(relation.value().target)
    }

    pub fn fit_target(self) -> Result<Option<AnalysisRegionRef<'a>>> {
        self.snapshot
            .relation_from::<crate::FitsTo>(self.id)?
            .map(|relation| self.snapshot.analysis_region(relation.value().target))
            .transpose()
    }

    /// Returns the authored presentation frame, or the current automatic fit target.
    pub fn frame(self) -> Result<Option<Geometry>> {
        if let Some(frame) = self.snapshot.component(self.id)? {
            return Ok(Some(frame));
        }
        self.fit_target()?
            .map(AnalysisRegionRef::geometry)
            .transpose()
    }
}

#[derive(Copy, Clone)]
pub struct TextContentRef<'a> {
    snapshot: &'a Snapshot,
    id: EntityId,
}

impl<'a> TextContentRef<'a> {
    #[must_use]
    pub const fn id(self) -> EntityId {
        self.id
    }

    pub fn source(self) -> Result<Option<SourceText>> {
        self.snapshot.component(self.id)
    }

    pub fn translation(self) -> Result<Option<Translation>> {
        self.snapshot.component(self.id)
    }

    pub fn role(self) -> Result<Option<TextRole>> {
        self.snapshot.component(self.id)
    }

    pub fn source_region(self) -> Result<Option<AnalysisRegionRef<'a>>> {
        self.snapshot
            .relation_from::<crate::RecognizedFrom>(self.id)?
            .map(|relation| self.snapshot.analysis_region(relation.value().target))
            .transpose()
    }
}

#[derive(Copy, Clone)]
pub struct AnalysisRegionRef<'a> {
    snapshot: &'a Snapshot,
    id: EntityId,
}

impl AnalysisRegionRef<'_> {
    #[must_use]
    pub const fn id(self) -> EntityId {
        self.id
    }

    pub fn region(self) -> Result<Region> {
        required(self.snapshot.component(self.id)?, self.id, "region")
    }

    pub fn geometry(self) -> Result<Geometry> {
        required(self.snapshot.component(self.id)?, self.id, "geometry")
    }

    pub fn detection(self) -> Result<Option<DetectionAnalysis>> {
        self.snapshot.component(self.id)
    }

    pub fn ocr(self) -> Result<Option<OcrAnalysis>> {
        self.snapshot.component(self.id)
    }
}

impl Snapshot {
    pub fn text_layer(&self, id: EntityId) -> Result<TextLayerRef<'_>> {
        required(self.component::<TextLayout>(id)?, id, "text layout")?;
        Ok(TextLayerRef { snapshot: self, id })
    }

    pub fn text_content(&self, id: EntityId) -> Result<TextContentRef<'_>> {
        required(self.component::<TextContent>(id)?, id, "text content")?;
        Ok(TextContentRef { snapshot: self, id })
    }

    pub fn analysis_region(&self, id: EntityId) -> Result<AnalysisRegionRef<'_>> {
        required(self.component::<Region>(id)?, id, "analysis region")?;
        Ok(AnalysisRegionRef { snapshot: self, id })
    }

    pub fn text_layers(&self) -> Result<impl ExactSizeIterator<Item = TextLayerRef<'_>>> {
        Ok(self
            .entities_with::<TextLayout>()?
            .map(|entity| TextLayerRef {
                snapshot: self,
                id: entity.id(),
            }))
    }
}

impl Edit {
    pub fn add_text_content(&mut self, parent: EntityId, at: At) -> Result<EntityId> {
        let entity = self.add_entity(parent, at)?;
        self.set(
            entity,
            &TextContent {
                origin: Origin::User,
            },
        )?;
        Ok(entity)
    }

    pub fn add_text_layer(
        &mut self,
        parent: EntityId,
        at: At,
        content: EntityId,
        layout: &TextLayout,
    ) -> Result<EntityId> {
        let layer = self.add_entity(parent, at)?;
        self.set(layer, layout)?;
        self.relate::<Presents>(layer, content)?;
        Ok(layer)
    }

    pub fn add_analysis_region<R: RegionSpec>(
        &mut self,
        parent: EntityId,
        at: At,
        geometry: &Geometry,
        label: Option<String>,
    ) -> Result<EntityId> {
        let entity = self.add_entity(parent, at)?;
        self.set(entity, geometry)?;
        self.set(
            entity,
            &Region {
                origin: Origin::User,
                kind: R::kind(),
                label,
            },
        )?;
        Ok(entity)
    }
}

fn required<T>(value: Option<T>, id: EntityId, role: &str) -> Result<T> {
    value.ok_or_else(|| crate::Error::invalid(format!("entity {id} is not {role}")))
}
