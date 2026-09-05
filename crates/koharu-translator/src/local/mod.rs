use std::sync::Arc;

use anyhow::Context;
use koharu_ml::llm::{
    ChatMessage, ChatTemplateOptions, Input, Llm, LoadOptions, MtmdOptions, media_marker,
};

mod catalog;

pub use catalog::LocalConfig;
use catalog::LocalModelDescriptor;
pub(crate) use catalog::{DEFAULT_MODEL, DEFAULT_QUANTIZATION};

use crate::{
    Device, Error, GenerationConfig, Model, ModelSelection, Provider, Quantization, Result,
    TranslationRequest, prompt,
};

#[derive(Debug)]
pub struct LocalTranslator {
    descriptor: LocalModelDescriptor,
    llm: Arc<Llm>,
}

impl LocalTranslator {
    pub async fn load(device: Device, selection: &ModelSelection) -> Result<Self> {
        let model = selection
            .model
            .as_deref()
            .context("local translation requires a selected model")?;
        let descriptor = catalog::MODELS
            .iter()
            .copied()
            .find(|descriptor| descriptor.id == model)
            .with_context(|| format!("unknown local translator '{model}'"))?;
        let resolved = descriptor.resolve(selection).await?;
        let options = LoadOptions {
            mtmd: resolved.projector.map(MtmdOptions::new),
            ..LoadOptions::default()
        };
        let llm = Llm::load_with_options(device, resolved.model, options)
            .await
            .context("failed to load local translation model")?;
        if llm.capabilities().vision != descriptor.projector.is_some() {
            return Err(anyhow::anyhow!(
                "local translator vision capability does not match its catalog"
            )
            .into());
        }
        Ok(Self {
            descriptor,
            llm: Arc::new(llm),
        })
    }

    pub(crate) async fn translate(
        &self,
        request: TranslationRequest,
        generation: GenerationConfig,
    ) -> Result<Vec<String>> {
        let expected = request.segments.len();
        if expected == 0 {
            return Ok(Vec::new());
        }
        if !self
            .descriptor
            .target_languages
            .contains(request.target_language)
        {
            return Err(Error::UnsupportedLanguage {
                provider: "local",
                language: request.target_language,
            });
        }

        let image = request.image.clone();
        let prompt = self.render_prompt(
            &request,
            generation.reasoning.unwrap_or(false) && self.descriptor.reasoning,
        )?;
        let schema = prompt::output_schema(expected);
        let llm = Arc::clone(&self.llm);
        let generation = self.descriptor.generation.options(generation);
        let output = tokio::task::spawn_blocking(move || {
            let input = image.as_deref().map_or_else(
                || Input::new(&prompt),
                |image| Input::new(&prompt).with_image(image),
            );
            llm.inference_with_json_schema(&input, &generation, &schema)
        })
        .await
        .context("local translation task panicked")??;
        let segments = prompt::translations("local", &output.text, &request.segments)?;
        Ok(segments)
    }

    fn render_prompt(&self, request: &TranslationRequest, reasoning: bool) -> Result<String> {
        let (system, payload) = prompt::prompts(request)?;
        let payload = if request.image.is_some() {
            format!("{}\n{payload}", media_marker())
        } else {
            payload
        };
        Ok(self
            .llm
            .render_chat_prompt_with_options(
                &[ChatMessage::system(system), ChatMessage::user(payload)],
                ChatTemplateOptions {
                    add_generation_prompt: true,
                    enable_thinking: reasoning,
                },
            )
            .context("failed to render local translation prompt")?)
    }
}

pub(crate) fn models() -> Vec<Model> {
    catalog::MODELS
        .iter()
        .map(|descriptor| Model {
            provider: Provider::Local,
            model: Some(descriptor.id.to_owned()),
            name: descriptor.name.to_owned(),
            quantizations: descriptor
                .quantizations
                .iter()
                .map(|quantization| public_quantization(descriptor, quantization))
                .collect(),
            vision: descriptor.projector.is_some(),
            reasoning: descriptor.reasoning,
        })
        .collect()
}

fn public_quantization(
    descriptor: &catalog::LocalModelDescriptor,
    definition: &crate::QuantizationDefinition,
) -> Quantization {
    let gguf_present = koharu_runtime::HuggingFaceFile::pinned(
        descriptor.repository,
        descriptor.revision,
        definition.filename,
    )
    .path()
    .is_file();

    let downloaded = match descriptor.projector {
        None => gguf_present,
        Some(projector) => {
            gguf_present
                && koharu_runtime::HuggingFaceFile::pinned(
                    descriptor.repository,
                    descriptor.revision,
                    projector,
                )
                .path()
                .is_file()
        }
    };

    Quantization {
        id: definition.id.to_owned(),
        name: definition.name.to_owned(),
        downloaded,
    }
}

pub(crate) fn supports_vision(selection: &ModelSelection) -> bool {
    selection.model.as_deref().is_some_and(|model| {
        catalog::MODELS
            .iter()
            .find(|descriptor| descriptor.id == model)
            .is_some_and(|descriptor| descriptor.projector.is_some())
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;
    use crate::{ModelGeneration, QuantizationDefinition};
    use catalog::{LocalModelDescriptor, SupportedLanguages};

    const TEST_REVISION: &str = "c0de233c0de233c0de233c0de233c0de233c0de2";
    const QUANTS: &[QuantizationDefinition] =
        &[QuantizationDefinition::new("Q4", "Q4", "model.gguf")];

    struct RemoveOnDrop(Vec<PathBuf>);

    impl Drop for RemoveOnDrop {
        fn drop(&mut self) {
            for path in self.0.drain(..).rev() {
                let _ = fs::remove_file(&path);
            }
        }
    }

    fn touch(path: PathBuf, guard: &mut RemoveOnDrop) {
        fs::create_dir_all(path.parent().expect("cache file has a parent")).unwrap();
        fs::write(&path, b"downloaded").unwrap();
        guard.0.push(path);
    }

    fn descriptor(
        repository: &'static str,
        projector: Option<&'static str>,
    ) -> LocalModelDescriptor {
        LocalModelDescriptor {
            id: "issue-233-minimal",
            reasoning: false,
            name: "Issue 233 Minimal",
            quantizations: QUANTS,
            generation: ModelGeneration::default(),
            repository,
            revision: TEST_REVISION,
            projector,
            target_languages: SupportedLanguages::All,
        }
    }

    #[test]
    fn reports_not_downloaded_when_gguf_is_missing() {
        let descriptor = descriptor("koharu-test/issue-233-minimal-missing", None);
        let _ = fs::remove_file(
            koharu_runtime::HuggingFaceFile::pinned(
                descriptor.repository,
                descriptor.revision,
                descriptor.quantizations[0].filename,
            )
            .path(),
        );
        assert!(!public_quantization(&descriptor, &descriptor.quantizations[0]).downloaded);
    }

    #[test]
    fn reports_downloaded_when_required_files_exist() {
        let descriptor = descriptor("koharu-test/issue-233-minimal-ready", Some("mmproj.gguf"));
        let mut guard = RemoveOnDrop(Vec::new());
        touch(
            koharu_runtime::HuggingFaceFile::pinned(
                descriptor.repository,
                descriptor.revision,
                descriptor.quantizations[0].filename,
            )
            .path(),
            &mut guard,
        );
        touch(
            koharu_runtime::HuggingFaceFile::pinned(
                descriptor.repository,
                descriptor.revision,
                "mmproj.gguf",
            )
            .path(),
            &mut guard,
        );
        assert!(public_quantization(&descriptor, &descriptor.quantizations[0]).downloaded);
    }

    #[test]
    fn vision_model_requires_projector_file() {
        let descriptor = descriptor(
            "koharu-test/issue-233-minimal-projector-missing",
            Some("mmproj.gguf"),
        );
        let mut guard = RemoveOnDrop(Vec::new());
        touch(
            koharu_runtime::HuggingFaceFile::pinned(
                descriptor.repository,
                descriptor.revision,
                descriptor.quantizations[0].filename,
            )
            .path(),
            &mut guard,
        );
        let _ = fs::remove_file(
            koharu_runtime::HuggingFaceFile::pinned(
                descriptor.repository,
                descriptor.revision,
                "mmproj.gguf",
            )
            .path(),
        );
        assert!(!public_quantization(&descriptor, &descriptor.quantizations[0]).downloaded);
    }
}
