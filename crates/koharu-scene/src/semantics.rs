//! Typed document vocabulary layered over the generic scene kernel.

use crate::{RegionKind, RelationKind};

pub trait RegionSpec {
    const KIND: &'static str;

    fn kind() -> RegionKind {
        RegionKind::new(Self::KIND).expect("built-in region kind is valid")
    }
}

pub struct TextRegion;
impl RegionSpec for TextRegion {
    const KIND: &'static str = "dev.koharu.region.text";
}

pub struct BubbleRegion;
impl RegionSpec for BubbleRegion {
    const KIND: &'static str = "dev.koharu.region.bubble";
}

pub struct PanelRegion;
impl RegionSpec for PanelRegion {
    const KIND: &'static str = "dev.koharu.region.panel";
}

pub trait RelationSpec {
    const KIND: &'static str;

    fn kind() -> RelationKind {
        RelationKind::new(Self::KIND).expect("built-in relation kind is valid")
    }
}

/// A relation with at most one target for each source entity.
pub trait FunctionalRelation: RelationSpec {}

/// A text presentation layer displays semantic text content.
pub struct Presents;
impl RelationSpec for Presents {
    const KIND: &'static str = "dev.koharu.relation.presents";
}
impl FunctionalRelation for Presents {}

/// Semantic text content was recognized from a source-analysis region.
pub struct RecognizedFrom;
impl RelationSpec for RecognizedFrom {
    const KIND: &'static str = "dev.koharu.relation.recognized-from";
}
impl FunctionalRelation for RecognizedFrom {}

/// A text presentation layer automatically derives its frame from its detected text region.
pub struct FitsTo;
impl RelationSpec for FitsTo {
    const KIND: &'static str = "dev.koharu.relation.fits-to";
}
impl FunctionalRelation for FitsTo {}

/// A text presentation layer participates in the joint layout of a dialogue balloon.
pub struct FlowsIn;
impl RelationSpec for FlowsIn {
    const KIND: &'static str = "dev.koharu.relation.flows-in";
}
impl FunctionalRelation for FlowsIn {}

/// A source-analysis region is spatially contained by another region.
pub struct Inside;
impl RelationSpec for Inside {
    const KIND: &'static str = "dev.koharu.relation.inside";
}
