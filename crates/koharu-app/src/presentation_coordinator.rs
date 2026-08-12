use std::sync::Arc;

use anyhow::{Context as _, Result};
use koharu_protocol::{AppEvent, CanvasState};
use koharu_scene::{EntityId, ProjectId, Revision, Snapshot};
use tokio::sync::Mutex;

use crate::{
    PageRenderer, Presentation, PresentationUpdate, ViewDisposition, event_hub::EventHub,
    project::Project,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PresentationTarget {
    project: ProjectId,
    revision: Revision,
    page: Option<EntityId>,
}

impl PresentationTarget {
    fn new(snapshot: &Snapshot, page: Option<EntityId>) -> Self {
        Self {
            project: snapshot.project_id(),
            revision: snapshot.revision(),
            page,
        }
    }
}

/// Serializes application renders and rejects results prepared from obsolete
/// project state before they can replace the native canvas frame.
#[derive(Clone)]
pub(crate) struct PresentationCoordinator {
    project: Arc<Mutex<Option<Project>>>,
    renderer: Arc<dyn PageRenderer>,
    presentation: Arc<dyn Presentation>,
    events: EventHub,
    serial: Arc<Mutex<()>>,
}

impl PresentationCoordinator {
    pub(crate) fn new(
        project: Arc<Mutex<Option<Project>>>,
        renderer: Arc<dyn PageRenderer>,
        presentation: Arc<dyn Presentation>,
        events: EventHub,
    ) -> Self {
        Self {
            project,
            renderer,
            presentation,
            events,
            serial: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) async fn synchronize(
        &self,
        view: ViewDisposition,
        publish: bool,
    ) -> Result<CanvasState> {
        let _serial = self.serial.lock().await;
        loop {
            let (target, render) = {
                let project = self.project.lock().await;
                match project.as_ref() {
                    Some(project) => {
                        let snapshot = project.snapshot();
                        let page = project.active_page();
                        let target = Some(PresentationTarget::new(&snapshot, page));
                        (target, page.map(|page| (snapshot, page)))
                    }
                    None => (None, None),
                }
            };
            let update = match render {
                Some((snapshot, page)) => PresentationUpdate::Frame {
                    frame: self.renderer.render(&snapshot, page).await?,
                    view,
                },
                None => PresentationUpdate::Clear,
            };

            let project = self.project.lock().await;
            if current_target(project.as_ref()) != target {
                continue;
            }
            let cleared = matches!(update, PresentationUpdate::Clear);
            let canvas = self.presentation.apply(update).await?;
            if cleared {
                self.renderer.discard_retained_nodes();
            }
            if publish {
                self.events.publish(AppEvent::Project {
                    project: project.as_ref().map(Project::info),
                });
                self.events.publish(AppEvent::Canvas {
                    state: canvas.clone(),
                });
            }
            return Ok(canvas);
        }
    }

    pub(crate) async fn replace(&self, project: Project) -> Result<()> {
        let _serial = self.serial.lock().await;
        let snapshot = project.snapshot();
        let update = match project.active_page() {
            Some(page) => PresentationUpdate::Frame {
                frame: self.renderer.render(&snapshot, page).await?,
                view: ViewDisposition::Fit,
            },
            None => PresentationUpdate::Clear,
        };
        let cleared = matches!(update, PresentationUpdate::Clear);
        let mut current = self.project.lock().await;
        let canvas = self.presentation.apply(update).await?;
        if cleared {
            self.renderer.discard_retained_nodes();
        }
        *current = Some(project);
        self.events.publish(AppEvent::Project {
            project: current.as_ref().map(Project::info),
        });
        self.events.publish(AppEvent::Canvas { state: canvas });
        Ok(())
    }

    pub(crate) async fn close(&self) -> Result<()> {
        let _serial = self.serial.lock().await;
        let mut project = self.project.lock().await;
        let canvas = self.presentation.apply(PresentationUpdate::Clear).await?;
        self.renderer.discard_retained_nodes();
        project.take();
        self.events.publish(AppEvent::Project { project: None });
        self.events.publish(AppEvent::Canvas { state: canvas });
        Ok(())
    }

    pub(crate) async fn select_page(&self, page: EntityId) -> Result<()> {
        let _serial = self.serial.lock().await;
        loop {
            let (target, snapshot) = {
                let project = self.project.lock().await;
                let project = project.as_ref().context("no project is open")?;
                let snapshot = project.snapshot();
                snapshot.page(page)?;
                let target = PresentationTarget::new(&snapshot, project.active_page());
                (target, snapshot)
            };
            let frame = self.renderer.render(&snapshot, page).await?;

            let mut current = self.project.lock().await;
            if current_target(current.as_ref()) != Some(target) {
                continue;
            }
            let project = current
                .as_mut()
                .context("project closed while selecting a page")?;
            let canvas = self
                .presentation
                .apply(PresentationUpdate::Frame {
                    frame,
                    view: ViewDisposition::Fit,
                })
                .await?;
            project.select_page(page)?;
            self.events.publish(AppEvent::Project {
                project: Some(project.info()),
            });
            self.events.publish(AppEvent::Canvas { state: canvas });
            return Ok(());
        }
    }
}

fn current_target(project: Option<&Project>) -> Option<PresentationTarget> {
    project.map(|project| {
        let snapshot = project.snapshot();
        PresentationTarget::new(&snapshot, project.active_page())
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        io::Cursor,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    use async_trait::async_trait;
    use koharu_protocol::CanvasState;
    use koharu_renderer::{ImageKind, LayerKind};
    use koharu_scene::{
        AssetInput, AssetMetadata, AssetRole, At, Origin, PageDraft, RasterLayer, RasterLayerKind,
        Session,
    };
    use tokio::sync::Notify;

    use super::*;
    use crate::{CanvasOperation, CanvasOutput};

    struct DelayedRenderer {
        inner: koharu_renderer::Renderer,
        delayed_revision: Revision,
        delayed: AtomicBool,
        started: Notify,
        resume: Notify,
    }

    #[async_trait]
    impl PageRenderer for DelayedRenderer {
        async fn render(
            &self,
            snapshot: &Snapshot,
            page: EntityId,
        ) -> Result<koharu_renderer::Frame> {
            if snapshot.revision() == self.delayed_revision
                && !self.delayed.swap(true, Ordering::SeqCst)
            {
                self.started.notify_one();
                self.resume.notified().await;
            }
            Ok(self.inner.render(snapshot, page).await?)
        }

        async fn rasterize(&self, frame: &koharu_renderer::Frame) -> Result<image::RgbaImage> {
            Ok(self
                .inner
                .rasterize(frame, koharu_renderer::RasterOptions::default())
                .await?
                .image)
        }

        async fn export_psd(
            &self,
            snapshot: &Snapshot,
            frame: &koharu_renderer::Frame,
        ) -> Result<Vec<u8>> {
            Ok(koharu_psd::export_page(
                &self.inner,
                snapshot,
                frame,
                &koharu_psd::PsdExportOptions::default(),
            )
            .await?)
        }

        async fn available_fonts(&self) -> Result<Vec<koharu_renderer::FontFamily>> {
            Ok(Vec::new())
        }

        async fn font_preview(&self, _family_name: &str) -> Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn discard_retained_nodes(&self) {
            self.inner.discard_retained_nodes();
        }
    }

    #[derive(Default)]
    struct RecordingPresentation {
        applied: parking_lot::Mutex<Vec<(Revision, bool)>>,
    }

    #[async_trait]
    impl Presentation for RecordingPresentation {
        async fn apply(&self, update: PresentationUpdate) -> Result<CanvasState> {
            match update {
                PresentationUpdate::Frame { frame, .. } => {
                    let cleanup = frame.layers().iter().any(|layer| {
                        matches!(
                            layer.kind(),
                            LayerKind::Image(metadata) if metadata.kind == ImageKind::Cleanup
                        )
                    });
                    self.applied.lock().push((frame.revision(), cleanup));
                }
                PresentationUpdate::Clear => {}
            }
            Ok(CanvasState::default())
        }

        async fn canvas(&self, _operation: CanvasOperation) -> Result<CanvasOutput> {
            Ok(CanvasOutput::State(CanvasState::default()))
        }
    }

    fn png(image: image::RgbaImage) -> Arc<[u8]> {
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner().into()
    }

    #[tokio::test]
    async fn obsolete_render_is_retried_before_reaching_the_canvas() {
        let source = AssetRole::new("source").unwrap();
        let mut session = Session::memory().await.unwrap();
        let mut page = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let created = edit.add_page(PageDraft::new("page", 4.0, 4.0), At::End)?;
                edit.set_asset(
                    created,
                    &source,
                    AssetInput::new(
                        png(image::RgbaImage::from_pixel(
                            4,
                            4,
                            image::Rgba([20, 30, 40, 255]),
                        )),
                        "image/png",
                        AssetMetadata {
                            width: Some(4),
                            height: Some(4),
                            attributes: BTreeMap::new(),
                        },
                    ),
                )?;
                page = Some(created);
                Ok(())
            })
            .unwrap();
        let first = session.commit(patch).await.unwrap();
        let first_revision = first.revision;
        let page = page.unwrap();
        let project = Arc::new(Mutex::new(Some(Project {
            session,
            name: "test".to_owned(),
            active_page: Some(page),
            undo: Vec::new(),
            redo: Vec::new(),
        })));
        let renderer = Arc::new(DelayedRenderer {
            inner: koharu_renderer::Renderer::new().unwrap(),
            delayed_revision: first_revision,
            delayed: AtomicBool::new(false),
            started: Notify::new(),
            resume: Notify::new(),
        });
        let presentation = Arc::new(RecordingPresentation::default());
        let coordinator = PresentationCoordinator::new(
            Arc::clone(&project),
            renderer.clone(),
            presentation.clone(),
            EventHub::default(),
        );

        let synchronization = tokio::spawn({
            let coordinator = coordinator.clone();
            async move {
                coordinator
                    .synchronize(ViewDisposition::Preserve, true)
                    .await
            }
        });
        renderer.started.notified().await;

        let latest_revision = {
            let mut project = project.lock().await;
            let project = project.as_mut().unwrap();
            let cleanup = png(image::RgbaImage::from_pixel(
                4,
                4,
                image::Rgba([200, 210, 220, 255]),
            ));
            let patch = project
                .snapshot()
                .patch(|edit| {
                    let layer = edit.add_entity(page, At::Start)?;
                    edit.set(
                        layer,
                        &RasterLayer {
                            origin: Origin::User,
                            name: "Cleanup".to_owned(),
                            kind: RasterLayerKind::Cleanup,
                        },
                    )?;
                    edit.set_asset(
                        layer,
                        &source,
                        AssetInput::new(
                            cleanup,
                            "image/png",
                            AssetMetadata {
                                width: Some(4),
                                height: Some(4),
                                attributes: BTreeMap::new(),
                            },
                        ),
                    )?;
                    Ok(())
                })
                .unwrap();
            let commit = project.session.commit(patch).await.unwrap();
            project.record_commit(&commit);
            commit.revision
        };
        renderer.resume.notify_one();
        synchronization.await.unwrap().unwrap();

        assert_eq!(
            presentation.applied.lock().as_slice(),
            &[(latest_revision, true)]
        );
    }
}
