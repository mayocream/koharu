//! Artifact enum: the pipeline's dependency currency.
//!
//! Engines declare `needs: &[Artifact]` and `produces: &[Artifact]`; the DAG
//! resolver derives execution order from these. Artifacts are satisfied when
//! the corresponding scene node / field is present on the target page.

use koharu_core::{ImageRole, MaskRole, NodeKind, Page};

/// Every named "thing" a pipeline step depends on or writes.
///
/// These correspond to scene node kinds + role tags (see `§7.1` of the
/// data-model design). Textual artifacts (OcrText, Translations) are
/// satisfied when every Text node on the page has the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Artifact {
    /// `Image { role: Source }` — always present on a valid page.
    SourceImage,
    /// `Image { role: Inpainted }` node present.
    Inpainted,
    /// `Mask { role: Segment }` node present.
    SegmentMask,
    /// `Mask { role: Bubble }` node present — bubble-interior mask from
    /// `speech-bubble-segmentation`, consumed by the renderer to grow
    /// text layout boxes inside the bubble.
    BubbleMask,
    /// `Mask { role: BrushInpaint }` node present.
    BrushMask,
    /// At least one `Text` node exists on the page.
    TextBoxes,
    /// Every `Text` node has `text` set.
    OcrText,
    /// Every `Text` node has `font_prediction` set.
    FontPredictions,
    /// Every `Text` node has `translation` set (or has no OCR text).
    Translations,
    /// Every `Text` node with a non-empty translation has a rendered sprite.
    RenderedSprites,
    /// `Image { role: Rendered }` node present.
    FinalRender,
}

impl Artifact {
    /// Whether this artifact is satisfied by the given page's current state.
    pub fn ready(self, page: &Page) -> bool {
        match self {
            Artifact::SourceImage => has_image_role(page, ImageRole::Source),
            Artifact::Inpainted => has_image_role(page, ImageRole::Inpainted),
            Artifact::SegmentMask => has_mask_role(page, MaskRole::Segment),
            Artifact::BubbleMask => has_mask_role(page, MaskRole::Bubble),
            Artifact::BrushMask => has_mask_role(page, MaskRole::BrushInpaint),
            Artifact::TextBoxes => page
                .nodes
                .values()
                .any(|n| matches!(n.kind, NodeKind::Text(_))),
            Artifact::OcrText => every_text(page, |t| {
                t.text.as_ref().is_some_and(|s| !s.trim().is_empty())
            }),
            Artifact::FontPredictions => every_text(page, |t| t.font_prediction.is_some()),
            Artifact::Translations => every_text(page, |t| {
                // A text node with no OCR text needs no translation either.
                let has_ocr = t.text.as_ref().is_some_and(|s| !s.trim().is_empty());
                if !has_ocr {
                    return true;
                }
                t.translation.as_ref().is_some_and(|s| !s.trim().is_empty())
            }),
            Artifact::RenderedSprites => every_text(page, |t| {
                // The renderer only emits sprites for non-empty translations.
                let has_translation = t.translation.as_ref().is_some_and(|s| !s.trim().is_empty());
                !has_translation || t.sprite.is_some()
            }),
            Artifact::FinalRender => has_image_role(page, ImageRole::Rendered),
        }
    }
}

fn has_image_role(page: &Page, role: ImageRole) -> bool {
    page.nodes.values().any(|n| match &n.kind {
        NodeKind::Image(img) => img.role == role,
        _ => false,
    })
}

fn has_mask_role(page: &Page, role: MaskRole) -> bool {
    page.nodes.values().any(|n| match &n.kind {
        NodeKind::Mask(mask) => mask.role == role,
        _ => false,
    })
}

fn every_text(page: &Page, predicate: impl Fn(&koharu_core::TextData) -> bool) -> bool {
    let texts: Vec<_> = page
        .nodes
        .values()
        .filter_map(|n| match &n.kind {
            NodeKind::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    // Empty page trivially satisfies text artifacts.
    if texts.is_empty() {
        return true;
    }
    texts.into_iter().all(predicate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_core::{BlobRef, Node, NodeId, TextData, Transform};

    #[test]
    fn rendered_sprites_ignore_blank_translation_nodes() {
        let mut page = Page::new("page", 100, 100);
        page.nodes.insert(
            node_id(1),
            text_node(TextData {
                text: Some(String::new()),
                translation: Some("   ".to_string()),
                ..Default::default()
            }),
        );

        assert!(Artifact::RenderedSprites.ready(&page));
    }

    #[test]
    fn rendered_sprites_require_sprite_for_nonblank_translation() {
        let mut page = Page::new("page", 100, 100);
        page.nodes.insert(
            node_id(1),
            text_node(TextData {
                translation: Some("hello".to_string()),
                ..Default::default()
            }),
        );

        assert!(!Artifact::RenderedSprites.ready(&page));
    }

    #[test]
    fn rendered_sprites_accept_nonblank_translation_with_sprite() {
        let mut page = Page::new("page", 100, 100);
        page.nodes.insert(
            node_id(1),
            text_node(TextData {
                translation: Some("hello".to_string()),
                sprite: Some(BlobRef::new("sprite")),
                ..Default::default()
            }),
        );

        assert!(Artifact::RenderedSprites.ready(&page));
    }

    fn node_id(_value: u128) -> NodeId {
        NodeId::new()
    }

    fn text_node(data: TextData) -> Node {
        Node {
            id: NodeId::new(),
            transform: Transform::default(),
            visible: true,
            kind: NodeKind::Text(data),
        }
    }
}
