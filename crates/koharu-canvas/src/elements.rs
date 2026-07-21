use std::collections::HashMap;

use koharu_renderer::{ElementRenderKey, PageRenderOptions, SceneRenderer};
use koharu_scene::{BlobId, Element, ElementId, ElementKind, Frame, Page};
use vello::{Scene, kurbo::Affine};

use crate::{ActiveTransform, CanvasDiagnostic, Error, Resources, Result};

struct CachedElement {
    /// The committed element used to encode `scene`.
    element: Element,
    /// Renderer-owned dependencies, including related scene elements.
    renderer_key: ElementRenderKey,
    scene: Scene,
}

/// Retains the expensive Vello scene for each committed element.
///
/// `entries` are rebuilt only when an element or one of its resources changes.
/// `combined` is cheaper: it appends those entries in page order and applies
/// transient transform-preview affines. Pointer movement therefore recomposes
/// the page without repeating text layout or image encoding.
pub(crate) struct ElementScenes {
    renderer: SceneRenderer,
    entries: HashMap<ElementId, CachedElement>,
    combined: Option<Scene>,
}

/// Inputs needed to (re)build the visible page's element scene.
/// Grouping them makes the cache boundary explicit and avoids a long positional
/// argument list at the call site.
pub(crate) struct ElementSceneContext<'a> {
    pub page: &'a Page,
    pub resources: &'a mut Resources,
    pub text: &'a PageRenderOptions,
    pub transform: Option<&'a ActiveTransform>,
    pub show_text: bool,
    pub diagnostics: &'a mut Vec<CanvasDiagnostic>,
}

impl ElementScenes {
    pub fn new() -> Result<Self> {
        Ok(Self {
            renderer: SceneRenderer::new().map_err(|error| Error::Invalid(error.to_string()))?,
            entries: HashMap::new(),
            combined: None,
        })
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.combined = None;
    }

    /// Drops only the ordered composition. Cached element scenes remain valid.
    pub fn recompose(&mut self) {
        self.combined = None;
    }

    pub fn retain_page(&mut self, page: &Page) {
        self.entries.retain(|id, _| page.element(*id).is_some());
        self.recompose();
    }

    pub fn invalidate_text(&mut self) {
        self.entries
            .retain(|_, cached| cached.element.text().is_none());
        self.recompose();
    }

    pub fn invalidate_image(&mut self, blob: BlobId) {
        self.entries
            .retain(|_, cached| cached.element.image_data().map(|image| image.blob) != Some(blob));
        self.recompose();
    }

    pub fn scene(&mut self, mut context: ElementSceneContext<'_>) -> &Scene {
        if self.combined.is_none() {
            self.rebuild_entries(&mut context);
            self.combined = Some(self.compose(context.page, context.transform, context.show_text));
        }
        self.combined
            .as_ref()
            .expect("element scene was composed above")
    }

    fn rebuild_entries(&mut self, context: &mut ElementSceneContext<'_>) {
        let page = context.page;
        for element in &page.elements {
            if !element.visible || element.opacity <= 0.0 {
                continue;
            }
            if element.text().is_some() && !context.show_text {
                continue;
            }
            let renderer_key = self.renderer.element_render_key(page, element);
            let reusable = self
                .entries
                .get(&element.id)
                .is_some_and(|cached| cached.renderer_key == renderer_key);
            if reusable {
                continue;
            }

            let mut scene = Scene::new();
            match &element.kind {
                ElementKind::Text(_) if context.show_text => {
                    if let Err(error) =
                        self.renderer
                            .encode_text_element(&mut scene, page, element, context.text)
                    {
                        context.diagnostics.push(CanvasDiagnostic::element(
                            page.id,
                            element.id,
                            error.to_string(),
                        ));
                    }
                }
                ElementKind::Image(image) => {
                    let Some(data) = context.resources.color(image.blob) else {
                        continue;
                    };
                    SceneRenderer::encode_image_element(&mut scene, element, &data);
                }
                ElementKind::Text(_) | ElementKind::Region(_) => {}
            }
            self.entries.insert(
                element.id,
                CachedElement {
                    element: element.clone(),
                    renderer_key,
                    scene,
                },
            );
        }
        self.entries.retain(|id, _| page.element(*id).is_some());
    }

    fn compose(&self, page: &Page, transform: Option<&ActiveTransform>, show_text: bool) -> Scene {
        let mut combined = Scene::new();
        for element in &page.elements {
            if !element.visible
                || element.opacity <= 0.0
                || (element.text().is_some() && !show_text)
            {
                continue;
            }
            let Some(cached) = self.entries.get(&element.id) else {
                continue;
            };
            let preview = transform.and_then(|transform| transform.preview(element.id));
            combined.append(
                &cached.scene,
                preview.map(|frame| frame_transform(element.frame, frame)),
            );
        }
        combined
    }
}

/// Maps the committed element coordinate system onto its transient preview.
/// The cached scene already includes the committed translation and rotation, so
/// this affine removes that transform before applying the preview transform.
fn frame_transform(original: Frame, preview: Frame) -> Affine {
    let original_angle = f64::from(original.angle_degrees).to_radians();
    let preview_angle = f64::from(preview.angle_degrees).to_radians();
    let (original_sin, original_cos) = original_angle.sin_cos();
    let (preview_sin, preview_cos) = preview_angle.sin_cos();
    let scale_x = f64::from(preview.width / original.width);
    let scale_y = f64::from(preview.height / original.height);
    let a = preview_cos * scale_x * original_cos + preview_sin * scale_y * original_sin;
    let b = preview_sin * scale_x * original_cos - preview_cos * scale_y * original_sin;
    let c = preview_cos * scale_x * original_sin - preview_sin * scale_y * original_cos;
    let d = preview_sin * scale_x * original_sin + preview_cos * scale_y * original_cos;
    let original_center_x = f64::from(original.x + original.width * 0.5);
    let original_center_y = f64::from(original.y + original.height * 0.5);
    let preview_center_x = f64::from(preview.x + preview.width * 0.5);
    let preview_center_y = f64::from(preview.y + preview.height * 0.5);
    Affine::new([
        a,
        b,
        c,
        d,
        preview_center_x - a * original_center_x - c * original_center_y,
        preview_center_y - b * original_center_x - d * original_center_y,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_corners;
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
