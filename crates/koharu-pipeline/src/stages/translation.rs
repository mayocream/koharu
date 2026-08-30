use std::{collections::BTreeMap, sync::Mutex};

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use koharu_scene::{Authored, EntityId, LanguageTag, Origin, SourceText, Translation};
use koharu_translator::{TranslationContext, TranslationRequest, Translator};

use crate::TranslationConfig;

use super::{StageInput, StageProcessor, finish, generation};

const PRODUCER: &str = "dev.koharu.pipeline.translation";

pub(super) struct Processor {
    config: TranslationConfig,
    translator: Translator,
    cache: Mutex<BatchCache>,
}

#[derive(Default)]
struct BatchCache {
    run_id: u64,
    pages: BTreeMap<EntityId, CachedPage>,
}

struct CachedPage {
    targets: Vec<Target>,
    translations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Target {
    entity: EntityId,
    source: String,
}

impl Processor {
    pub(super) fn new(config: TranslationConfig, translator: Translator) -> Self {
        Self {
            config,
            translator,
            cache: Mutex::new(BatchCache::default()),
        }
    }

    pub(super) fn batch_pages(&self) -> usize {
        if !Translator::supports_context_memory(&self.config.model)
            || Translator::supports_vision(&self.config.model, &self.config.generation)
        {
            1
        } else {
            self.config.memory.batch_pages()
        }
    }

    fn uses_translation_hints(&self) -> bool {
        self.config.memory.translation_hints
            && Translator::supports_context_memory(&self.config.model)
    }

    fn targets(
        &self,
        input: &StageInput,
        page: EntityId,
    ) -> Result<(Vec<Target>, Vec<TranslationContext>)> {
        let mut targets = Vec::new();
        let mut context = Vec::new();
        if let Some(group) = input.scene.page(page)?.text_group()? {
            for layer in group.text_layers()? {
                if !input.contains_entity_on(page, layer.id())? {
                    continue;
                }
                let content = layer.content()?;
                let Some(source) = content.source()? else {
                    continue;
                };
                if source.text.value.trim().is_empty() {
                    continue;
                }
                if self.uses_translation_hints()
                    && let Some(translation) = content.translation()?
                    && matches!(translation.text.origin, Origin::User)
                    && !translation.text.value.trim().is_empty()
                {
                    context.push(TranslationContext::new(
                        source.text.value.clone(),
                        translation.text.value,
                    ));
                    continue;
                }
                targets.push(Target {
                    entity: content.id(),
                    source: source.text.value,
                });
            }
        }
        Ok((targets, context))
    }

    fn previous_context(
        &self,
        input: &StageInput,
        page: EntityId,
    ) -> Result<Vec<TranslationContext>> {
        let count = if self.uses_translation_hints() {
            self.config.memory.context_pages()
        } else {
            0
        };
        if count == 0 {
            return Ok(Vec::new());
        }
        let pages = input
            .scene
            .pages()
            .map(|page| page.id())
            .collect::<Vec<_>>();
        let Some(index) = pages.iter().position(|candidate| *candidate == page) else {
            return Ok(Vec::new());
        };
        let mut context = Vec::new();
        for page in &pages[index.saturating_sub(count)..index] {
            let Some(group) = input.scene.page(*page)?.text_group()? else {
                continue;
            };
            for layer in group.text_layers()? {
                let content = layer.content()?;
                let (Some(source), Some(translation)) = (content.source()?, content.translation()?)
                else {
                    continue;
                };
                if translation
                    .language
                    .as_ref()
                    .is_none_or(|language| language.as_str() != self.config.target_language.tag())
                    || source.text.value.trim().is_empty()
                    || translation.text.value.trim().is_empty()
                {
                    continue;
                }
                context.push(TranslationContext::new(
                    source.text.value,
                    translation.text.value,
                ));
            }
        }
        Ok(context)
    }

    fn take_cached(
        &self,
        run_id: u64,
        page: EntityId,
        targets: &[Target],
    ) -> Result<Option<Vec<String>>> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| anyhow::anyhow!("translation batch cache is poisoned"))?;
        if cache.run_id != run_id {
            cache.run_id = run_id;
            cache.pages.clear();
        }
        Ok(cache
            .pages
            .remove(&page)
            .filter(|cached| cached.targets == targets)
            .map(|cached| cached.translations))
    }

    fn store_cached(
        &self,
        run_id: u64,
        pages: impl IntoIterator<Item = (EntityId, CachedPage)>,
    ) -> Result<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| anyhow::anyhow!("translation batch cache is poisoned"))?;
        if cache.run_id != run_id {
            cache.run_id = run_id;
            cache.pages.clear();
        }
        cache.pages.extend(pages);
        Ok(())
    }

    fn patch(
        &self,
        input: &StageInput,
        targets: Vec<Target>,
        translations: Vec<String>,
        provider: &str,
    ) -> Result<koharu_scene::Patch> {
        anyhow::ensure!(
            targets.len() == translations.len(),
            "cached translation count does not match its page"
        );
        let language = LanguageTag::new(self.config.target_language.tag())?;
        let generated = generation(PRODUCER, provider)?;
        let mut edit = input.scene.edit_as(generated.clone());
        for target in &targets {
            edit.observe::<SourceText>(target.entity)?;
            edit.observe::<Translation>(target.entity)?;
        }
        for (target, text) in targets.into_iter().zip(translations) {
            if input
                .scene
                .component::<Translation>(target.entity)?
                .is_some_and(|value| matches!(value.text.origin, Origin::User))
            {
                continue;
            }
            let text = if target.source.trim() == "\u{2026}" {
                "\u{2026}".to_owned()
            } else {
                text
            };
            edit.set(
                target.entity,
                &Translation {
                    text: Authored::generated(text, generated.clone()),
                    language: Some(language.clone()),
                },
            )?;
        }
        finish(edit)
    }
}

#[async_trait]
impl StageProcessor for Processor {
    fn model(&self) -> &'static str {
        Translator::model(&self.config.model)
    }

    fn unload(&self) -> bool {
        self.translator.unload()
    }

    async fn load(&self) -> Result<()> {
        self.translator.load_model(&self.config.model).await
    }

    async fn process(&self, input: StageInput) -> Result<koharu_scene::Patch> {
        let (current_targets, current_context) = self.targets(&input, input.page)?;
        if self.config.memory.batch_pages() > 1
            && let Some(translations) =
                self.take_cached(input.run_id, input.page, &current_targets)?
        {
            return self.patch(&input, current_targets, translations, self.model());
        }

        let pages = if self.config.memory.batch_pages() > 1 {
            input.batch_pages.to_vec()
        } else {
            vec![input.page]
        };
        let mut page_targets = Vec::with_capacity(pages.len());
        let mut context = self.previous_context(&input, input.page)?;
        context.extend(current_context);
        for (index, page) in pages.iter().copied().enumerate() {
            let (targets, page_context) = if index == 0 {
                (current_targets.clone(), Vec::new())
            } else {
                self.targets(&input, page)?
            };
            context.extend(page_context);
            page_targets.push((page, targets));
        }
        let page_lengths = page_targets
            .iter()
            .map(|(_, targets)| targets.len())
            .collect::<Vec<_>>();
        let mut request = TranslationRequest::new(
            page_targets
                .iter()
                .flat_map(|(_, targets)| targets.iter().map(|target| target.source.clone())),
            self.config.target_language,
        )
        .with_page_lengths(page_lengths.clone())
        .with_context(context)
        .with_prefix_cache(self.config.memory.prefix_cache);
        if let Some(instructions) = self.config.instructions.as_deref() {
            request = request.with_instructions(instructions);
        }
        if Translator::supports_vision(&self.config.model, &self.config.generation)
            && let Some(image) = input.images.get(&input.scene, input.page, "source").await?
        {
            request = request.with_image(image);
        }
        let (provider, translated) = self
            .translator
            .translate(&self.config.model, self.config.generation, request)
            .await?;

        let mut offset = 0;
        let mut completed = Vec::with_capacity(page_targets.len());
        for ((page, targets), length) in page_targets.into_iter().zip(page_lengths) {
            let translations = translated[offset..offset + length].to_vec();
            offset += length;
            completed.push((
                page,
                CachedPage {
                    targets,
                    translations,
                },
            ));
        }
        let current = completed.remove(0).1;
        self.store_cached(input.run_id, completed)?;
        self.patch(&input, current.targets, current.translations, provider)
            .context("failed to apply translated page")
    }
}
