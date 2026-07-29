mod detection;
mod inpainting;
mod ocr;
mod translation;

use std::{collections::BTreeSet, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use koharu_scene::{
    Children, EntityId, Generation, ProducerId, SceneEdit, ScenePatch, SceneSnapshot,
};

pub use detection::KoharuLayoutRFDetrSeg2XLConfig;
pub use inpainting::{Flux2KleinConfig, RoremMixedConfig};

use crate::{Bounds, ImageCache, ModelCell, PipelineConfig, Stage};

#[derive(Clone)]
pub(crate) struct StageInput {
    scene: koharu_scene::SceneSnapshot,
    page: EntityId,
    entities: Option<Arc<BTreeSet<EntityId>>>,
    region: Option<Bounds>,
    images: Arc<ImageCache>,
}

impl StageInput {
    pub(crate) fn new(
        scene: SceneSnapshot,
        page: EntityId,
        entities: Option<Arc<BTreeSet<EntityId>>>,
        region: Option<Bounds>,
        images: Arc<ImageCache>,
    ) -> Self {
        Self {
            scene,
            page,
            entities,
            region,
            images,
        }
    }

    pub(crate) fn page(&self) -> EntityId {
        self.page
    }

    fn contains_entity(&self, entity: EntityId) -> Result<bool> {
        crate::scope::contains_entity(
            &self.scene,
            self.page,
            self.entities.as_deref(),
            self.region,
            entity,
        )
    }
}

trait ModelState: Send + Sync {
    fn loaded(&self) -> bool;
    fn unload(&self) -> bool;
    fn touch(&self, sequence: u64);
    fn last_used(&self) -> u64;
}

impl<M: Send> ModelState for ModelCell<M> {
    fn loaded(&self) -> bool {
        ModelCell::loaded(self)
    }

    fn unload(&self) -> bool {
        ModelCell::unload(self)
    }

    fn touch(&self, sequence: u64) {
        ModelCell::touch(self, sequence);
    }

    fn last_used(&self) -> u64 {
        ModelCell::last_used(self)
    }
}

struct ModelRef<'a> {
    name: &'static str,
    state: &'a dyn ModelState,
}

impl<'a> ModelRef<'a> {
    fn new(name: &'static str, state: &'a dyn ModelState) -> Self {
        Self { name, state }
    }
}

#[async_trait]
trait StageProcessor: Send + Sync {
    fn model(&self) -> ModelRef<'_>;
    async fn load(&self) -> Result<()>;
    async fn process(&self, input: StageInput) -> Result<ScenePatch>;
}

pub(crate) struct Stages {
    detection: detection::Processor,
    ocr: ocr::Processor,
    translation: translation::Processor,
    inpainting: inpainting::Processor,
}

impl Stages {
    pub(crate) fn new(
        config: &PipelineConfig,
        translation: &koharu_translator::TranslationConfig,
        device: &koharu_ml::Device,
    ) -> Result<Self> {
        Ok(Self {
            detection: detection::Processor::new(config.detection.clone(), device.clone())?,
            ocr: ocr::Processor::new(config.ocr.clone(), device.clone()),
            translation: translation::Processor::new(translation.clone(), device.clone())?,
            inpainting: inpainting::Processor::new(config.inpainting.clone(), device.clone())?,
        })
    }

    fn processor(&self, stage: Stage) -> &dyn StageProcessor {
        match stage {
            Stage::Detection => &self.detection,
            Stage::Ocr => &self.ocr,
            Stage::Translation => &self.translation,
            Stage::Inpainting => &self.inpainting,
        }
    }

    pub(crate) fn model(&self, stage: Stage) -> &'static str {
        self.processor(stage).model().name
    }

    pub(crate) async fn load(&self, stage: Stage) -> Result<()> {
        self.processor(stage).load().await
    }

    pub(crate) async fn process(&self, stage: Stage, input: StageInput) -> Result<ScenePatch> {
        self.processor(stage).process(input).await
    }

    pub(crate) fn loaded(&self, stage: Stage) -> bool {
        self.processor(stage).model().state.loaded()
    }

    pub(crate) fn unload(&self, stage: Stage) -> bool {
        self.processor(stage).model().state.unload()
    }

    pub(crate) fn touch(&self, stage: Stage, sequence: u64) {
        self.processor(stage).model().state.touch(sequence);
    }

    pub(crate) fn last_used(&self, stage: Stage) -> u64 {
        self.processor(stage).model().state.last_used()
    }
}

fn generation(producer: &str, model: &str) -> Result<Generation> {
    let mut generation = Generation::new(ProducerId::new(producer)?);
    generation.model = Some(model.to_owned());
    Ok(generation)
}

fn finish(edit: SceneEdit) -> Result<ScenePatch> {
    edit.finish().map_err(Into::into)
}

fn observe_page_hierarchy(
    edit: &mut SceneEdit,
    scene: &SceneSnapshot,
    page: EntityId,
) -> Result<Vec<EntityId>> {
    let entities = scene
        .subtree(page)?
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    for entity in &entities {
        edit.observe::<Children>(*entity, "default")?;
    }
    Ok(entities)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(bytes: &'static [u8]) -> koharu_scene::AssetInput {
        koharu_scene::AssetInput::new(
            bytes,
            "image/png",
            koharu_scene::AssetMetadata {
                width: Some(1),
                height: Some(1),
                attributes: std::collections::BTreeMap::new(),
            },
        )
    }

    #[test]
    fn registry_contains_every_stage() {
        let stages = Stages::new(
            &PipelineConfig::default(),
            &koharu_translator::TranslationConfig::default(),
            &koharu_ml::Device::cpu(),
        )
        .unwrap();

        assert_eq!(
            Stage::ALL.map(|stage| stages.model(stage)),
            [
                "koharu-layout-rfdetr-seg-2xl",
                "paddleocr-vl-1.6",
                "local-translation",
                "lama",
            ]
        );
    }

    #[test]
    fn text_and_image_branches_compose_on_one_page() {
        let mut session = koharu_scene::SceneSession::memory().unwrap();
        let mut setup = session.snapshot().edit();
        let page = setup
            .add_page(
                koharu_scene::PageDraft::new("page", 1.0, 1.0),
                koharu_scene::At::End,
            )
            .unwrap();
        let text = setup.add_entity(page, koharu_scene::At::End).unwrap();
        setup
            .set_source_text(
                text,
                koharu_scene::SourceText {
                    text: koharu_scene::Authored::user("before".to_owned()),
                    language: None,
                },
            )
            .unwrap();
        setup
            .set_asset(
                page,
                &koharu_scene::AssetRole::new("source").unwrap(),
                asset(b"source"),
            )
            .unwrap();
        session.commit(setup.finish().unwrap()).unwrap();
        let base = session.snapshot();

        let mut text_edit = base.edit();
        for entity in observe_page_hierarchy(&mut text_edit, &base, page).unwrap() {
            text_edit
                .observe::<koharu_scene::SourceText>(entity, "default")
                .unwrap();
        }
        text_edit
            .set_source_text(
                text,
                koharu_scene::SourceText {
                    text: koharu_scene::Authored::user("after".to_owned()),
                    language: None,
                },
            )
            .unwrap();
        let text_patch = text_edit.finish().unwrap();

        let mut image_edit = base.edit();
        image_edit
            .observe::<koharu_scene::Asset>(page, "source")
            .unwrap();
        for role in ["text-mask", "coo-mask", "brush-mask"] {
            image_edit
                .observe::<koharu_scene::Asset>(page, role)
                .unwrap();
        }
        image_edit
            .set_asset(
                page,
                &koharu_scene::AssetRole::new("clean").unwrap(),
                asset(b"clean"),
            )
            .unwrap();
        let image_patch = image_edit.finish().unwrap();

        session.commit(text_patch).unwrap();
        assert!(image_patch.rebase_on(&session.snapshot()).is_ok());
    }
}
