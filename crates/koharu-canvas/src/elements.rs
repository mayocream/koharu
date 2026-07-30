use koharu_renderer::{PreparedPage, RenderPlan, RenderRequest, RenderResources, RenderTheme};
use koharu_scene::{BlobId, LanguageTag, SceneSnapshot};
use vello::{
    Scene,
    kurbo::{Affine, Rect, Vec2},
    peniko::{Fill, Mix},
};

use crate::{
    ActiveTransform, CanvasDiagnostic, CanvasElement, CanvasPage, Resources, Result,
    transform::frame_transform,
};

/// Cached renderer preparation plus the cheap ordered composition used by the
/// interactive viewport. Text shaping is repeated only after a scene, locale,
/// theme, or font change. Image data stays in the canvas resource cache.
pub(crate) struct ElementScenes {
    renderer_resources: RenderResources,
    prepared_text: Option<PreparedPage>,
    combined: Option<Scene>,
}

pub(crate) struct ElementSceneContext<'a> {
    pub snapshot: &'a SceneSnapshot,
    pub page: &'a CanvasPage,
    pub resources: &'a mut Resources,
    pub text: &'a RenderTheme,
    pub locale: Option<&'a LanguageTag>,
    pub transform: Option<&'a ActiveTransform>,
    pub show_text: bool,
    pub diagnostics: &'a mut Vec<CanvasDiagnostic>,
}

impl ElementScenes {
    pub fn new() -> Result<Self> {
        Ok(Self {
            renderer_resources: RenderResources::new(),
            prepared_text: None,
            combined: None,
        })
    }

    pub fn clear(&mut self) {
        self.prepared_text = None;
        self.combined = None;
    }

    pub fn recompose(&mut self) {
        self.combined = None;
    }

    pub fn retain_page(&mut self, _page: &CanvasPage) {
        self.clear();
    }

    pub fn invalidate_text(&mut self) {
        self.prepared_text = None;
        self.recompose();
    }

    pub fn invalidate_image(&mut self, _blob: BlobId) {
        self.recompose();
    }

    pub fn scene(&mut self, mut context: ElementSceneContext<'_>) -> &Scene {
        if context.show_text && self.prepared_text.is_none() {
            self.prepare_text(&mut context);
        }
        if self.combined.is_none() {
            self.combined = Some(self.compose(&mut context));
        }
        self.combined
            .as_ref()
            .expect("element scene was composed above")
    }

    fn prepare_text(&mut self, context: &mut ElementSceneContext<'_>) {
        let mut request = RenderRequest::transparent(context.page.id);
        request.include_images = false;
        request.locale = context.locale.cloned();
        request.theme = context.text.clone();
        let prepared = RenderPlan::compile(context.snapshot, &request).and_then(|plan| {
            PreparedPage::prepare(
                &plan,
                context.snapshot,
                &self.renderer_resources,
                &request.theme,
            )
        });
        match prepared {
            Ok(prepared) => self.prepared_text = Some(prepared),
            Err(error) => context.diagnostics.push(CanvasDiagnostic {
                page: Some(context.page.id),
                element: None,
                blob: None,
                message: format!("failed to prepare page text: {error}"),
            }),
        }
    }

    fn compose(&self, context: &mut ElementSceneContext<'_>) -> Scene {
        let mut combined = Scene::new();
        for element in &context.page.elements {
            if !element.visible || element.opacity <= 0.0 {
                continue;
            }
            let preview = context
                .transform
                .and_then(|transform| transform.preview(element.id));
            let frame = preview.unwrap_or(element.frame);
            if let Some(blob) = element.image
                && let Some(image) = context.resources.color(blob)
            {
                append_image(&mut combined, element, frame, &image);
            }
            if context.show_text
                && element.has_text
                && let Some(prepared) = &self.prepared_text
            {
                let transform = preview.map(|preview| frame_transform(element.frame, preview));
                prepared.append_entity_to(element.id, &mut combined, transform);
            }
        }
        combined
    }
}

fn append_image(
    scene: &mut Scene,
    element: &CanvasElement,
    frame: crate::Frame,
    image: &vello::peniko::ImageData,
) {
    let transform = image_transform(frame, image.width, image.height);
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
    use super::*;
    use crate::{Frame, frame_corners};
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
