use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use koharu_renderer::{
    Compositor, Frame as RenderFrame, LayerPresentation, RenderRequest, RenderTheme, SceneRenderer,
};
use koharu_scene::{BlobId, EntityId, Snapshot};
use vello::{
    Scene,
    kurbo::{Affine, Rect, Vec2},
    peniko::{Fill, Mix},
};

use crate::{
    ActiveTransform, CanvasDiagnostic, CanvasElement, CanvasPage, ElementFrame, Resources, Result,
};

/// Cached vector frame plus the cheap ordered composition used by the
/// interactive viewport. Text shaping is repeated only after a scene, theme,
/// or font change. Image data stays in the canvas resource cache.
pub(crate) struct ElementScenes {
    compositor: Compositor,
    scene_renderer: SceneRenderer,
    text_frame: Option<Arc<RenderFrame>>,
    text_overrides: HashMap<EntityId, Option<Arc<RenderFrame>>>,
    dirty_text: BTreeSet<EntityId>,
    combined: Option<Scene>,
}

pub(crate) struct ElementSceneContext<'a> {
    pub snapshot: &'a Snapshot,
    pub page: &'a CanvasPage,
    pub resources: &'a mut Resources,
    pub text: &'a RenderTheme,
    pub transform: Option<&'a ActiveTransform>,
    pub show_text: bool,
    pub diagnostics: &'a mut Vec<CanvasDiagnostic>,
}

impl ElementScenes {
    pub fn new() -> Result<Self> {
        Ok(Self {
            compositor: Compositor::new(),
            scene_renderer: SceneRenderer::new(),
            text_frame: None,
            text_overrides: HashMap::new(),
            dirty_text: BTreeSet::new(),
            combined: None,
        })
    }

    pub fn clear(&mut self) {
        self.text_frame = None;
        self.text_overrides.clear();
        self.dirty_text.clear();
        self.combined = None;
    }

    pub fn recompose(&mut self) {
        self.combined = None;
    }

    pub fn invalidate_text(&mut self) {
        self.text_frame = None;
        self.text_overrides.clear();
        self.dirty_text.clear();
        self.recompose();
    }

    pub fn invalidate_text_entities(&mut self, entities: impl IntoIterator<Item = EntityId>) {
        self.dirty_text.extend(entities);
        self.recompose();
    }

    pub fn invalidate_image(&mut self, _blob: BlobId) {
        self.recompose();
    }

    pub fn scene(&mut self, mut context: ElementSceneContext<'_>) -> &Scene {
        if context.show_text {
            self.ensure_text(
                context.snapshot,
                context.page,
                context.text,
                context.diagnostics,
            );
        }
        if self.combined.is_none() {
            self.combined = Some(self.compose(&mut context));
        }
        self.combined
            .as_ref()
            .expect("element scene was composed above")
    }

    pub fn element_frames(
        &mut self,
        snapshot: &Snapshot,
        page: &CanvasPage,
        text: &RenderTheme,
        diagnostics: &mut Vec<CanvasDiagnostic>,
    ) -> Vec<ElementFrame> {
        self.ensure_text(snapshot, page, text, diagnostics);
        page.elements
            .iter()
            .filter(|element| element.has_text)
            .filter_map(|element| {
                let text = self.visual_text(element.id)?;
                let bounds = text.rendered_bounds;
                Some(ElementFrame {
                    element: element.id,
                    frame: crate::Frame {
                        x: bounds.x,
                        y: bounds.y,
                        width: bounds.width,
                        height: bounds.height,
                        angle_degrees: text.angle_degrees,
                    },
                })
            })
            .collect()
    }

    fn ensure_text(
        &mut self,
        snapshot: &Snapshot,
        page: &CanvasPage,
        text: &RenderTheme,
        diagnostics: &mut Vec<CanvasDiagnostic>,
    ) {
        if self.text_frame.is_none() {
            self.dirty_text.clear();
            self.render_text(snapshot, page, text, diagnostics, None);
            return;
        }
        if !self.dirty_text.is_empty() {
            let entities = std::mem::take(&mut self.dirty_text);
            self.render_text(snapshot, page, text, diagnostics, Some(entities));
        }
    }

    fn render_text(
        &mut self,
        snapshot: &Snapshot,
        page: &CanvasPage,
        text: &RenderTheme,
        diagnostics: &mut Vec<CanvasDiagnostic>,
        entities: Option<BTreeSet<EntityId>>,
    ) {
        let mut request = RenderRequest::transparent(page.id);
        request.include_images = false;
        request.text_entities = entities.clone();
        request.presentation = LayerPresentation::Deferred;
        request.fallback_to_source_text = false;
        request.theme = text.clone();
        let frame = self
            .compositor
            .compile(snapshot, &request)
            .and_then(|composition| self.scene_renderer.render(snapshot, &composition));
        match frame {
            Ok(frame) => match entities {
                Some(entities) => {
                    for entity in entities {
                        let rendered = frame.layers().iter().any(|layer| layer.entity == entity);
                        self.text_overrides
                            .insert(entity, rendered.then(|| frame.clone()));
                    }
                }
                None => {
                    self.text_frame = Some(frame);
                    self.text_overrides.clear();
                }
            },
            Err(error) => diagnostics.push(CanvasDiagnostic {
                page: Some(page.id),
                element: None,
                blob: None,
                message: format!("failed to render page text: {error}"),
            }),
        }
    }

    fn text_frame_for(&self, entity: EntityId) -> Option<&Arc<RenderFrame>> {
        match self.text_overrides.get(&entity) {
            Some(frame) => frame.as_ref(),
            None => self.text_frame.as_ref(),
        }
    }

    fn visual_text(&self, entity: EntityId) -> Option<&koharu_renderer::VisualText> {
        self.text_frame_for(entity)?
            .layers()
            .iter()
            .find(|layer| layer.entity == entity)?
            .text
            .as_ref()
    }

    fn compose(&self, context: &mut ElementSceneContext<'_>) -> Scene {
        let mut combined = Scene::new();
        for element in &context.page.elements {
            if !element.visible || element.opacity <= 0.0 {
                continue;
            }
            let transform = context
                .transform
                .and_then(|transform| transform.affine(element.id));
            if let Some(blob) = element.image
                && let Some(image) = context.resources.color(blob)
            {
                append_image(&mut combined, element, &image, transform);
            }
            if context.show_text
                && element.has_text
                && let Some(frame) = self.text_frame_for(element.id)
            {
                if element.opacity < 1.0 {
                    combined.push_layer(
                        Fill::NonZero,
                        Mix::Normal,
                        element.opacity,
                        Affine::IDENTITY,
                        &Rect::new(
                            0.0,
                            0.0,
                            f64::from(context.page.size.width),
                            f64::from(context.page.size.height),
                        ),
                    );
                }
                frame.append_entity_to(element.id, &mut combined, transform);
                if element.opacity < 1.0 {
                    combined.pop_layer();
                }
            }
        }
        combined
    }
}

fn append_image(
    scene: &mut Scene,
    element: &CanvasElement,
    image: &vello::peniko::ImageData,
    preview: Option<Affine>,
) {
    let transform = preview.map_or_else(
        || image_transform(element.frame, image.width, image.height),
        |preview| preview * image_transform(element.frame, image.width, image.height),
    );
    if element.opacity < 1.0 {
        scene.push_layer(
            Fill::NonZero,
            Mix::Normal,
            element.opacity,
            transform,
            &Rect::new(0.0, 0.0, f64::from(image.width), f64::from(image.height)),
        );
    }
    scene.draw_image(image, transform);
    if element.opacity < 1.0 {
        scene.pop_layer();
    }
}

fn image_transform(frame: crate::Frame, width: u32, height: u32) -> Affine {
    let center_x = f64::from(frame.x + frame.width * 0.5);
    let center_y = f64::from(frame.y + frame.height * 0.5);
    Affine::scale_non_uniform(
        f64::from(frame.width) / f64::from(width),
        f64::from(frame.height) / f64::from(height),
    )
    .then_translate(Vec2::new(
        -f64::from(frame.width) * 0.5,
        -f64::from(frame.height) * 0.5,
    ))
    .then_rotate(f64::from(frame.angle_degrees).to_radians())
    .then_translate(Vec2::new(center_x, center_y))
}

#[cfg(test)]
mod tests {
    use crate::{Frame, frame_corners, transform::frame_transform};
    use vello::kurbo::Point;

    #[test]
    fn preview_affine_maps_every_transformed_corner() {
        let original = Frame {
            angle_degrees: 37.0,
            ..Frame::new(10.0, 20.0, 100.0, 50.0)
        };
        let preview = Frame {
            angle_degrees: -23.0,
            ..Frame::new(70.0, 40.0, 60.0, 120.0)
        };
        let affine = frame_transform(original, preview);

        for (source, expected) in frame_corners(original)
            .into_iter()
            .zip(frame_corners(preview))
        {
            let actual = affine * Point::new(source.x, source.y);
            assert!((actual.x - expected.x).abs() < 1e-4);
            assert!((actual.y - expected.y).abs() < 1e-4);
        }
    }
}
