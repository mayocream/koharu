use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use koharu_translator::Providers;
use serde::{Deserialize, Serialize};

use crate::{Artifact, Phase, PipelineConfig, ProcessorConfig, ProcessorId, RunTarget};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) enum ConfiguredModel {
    Processor(ProcessorConfig),
    Translation(Providers),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModelRuntime {
    None,
    Torch,
    Llama,
    Diffusion,
}

impl ConfiguredModel {
    pub(crate) const fn id(&self) -> ProcessorId {
        match self {
            Self::Processor(ProcessorConfig::ComicTextDetector(_)) => {
                ProcessorId::ComicTextDetector
            }
            Self::Processor(ProcessorConfig::PPDocLayoutV3(_)) => ProcessorId::PPDocLayoutV3,
            Self::Processor(ProcessorConfig::ComicLayoutYolo26s(_)) => {
                ProcessorId::ComicLayoutYolo26s
            }
            Self::Processor(ProcessorConfig::MangaTextMask(_)) => ProcessorId::MangaTextMask,
            Self::Processor(ProcessorConfig::SpeechBubbleYoloV8m(_)) => {
                ProcessorId::SpeechBubbleYoloV8m
            }
            Self::Processor(ProcessorConfig::SpeechBubbleYolo11n(_)) => {
                ProcessorId::SpeechBubbleYolo11n
            }
            Self::Processor(ProcessorConfig::ComicOnomatopoeia(_)) => {
                ProcessorId::ComicOnomatopoeia
            }
            Self::Processor(ProcessorConfig::MaskFusion(_)) => ProcessorId::MaskFusion,
            Self::Processor(ProcessorConfig::PaddleOcrVl1_6(_)) => ProcessorId::PaddleOcrVl1_6,
            Self::Processor(ProcessorConfig::MangaOcr(_)) => ProcessorId::MangaOcr,
            Self::Processor(ProcessorConfig::BaberuOcr(_)) => ProcessorId::BaberuOcr,
            Self::Translation(_) => ProcessorId::Translation,
            Self::Processor(ProcessorConfig::FontDetector(_)) => ProcessorId::FontDetector,
            Self::Processor(ProcessorConfig::LaMa(_)) => ProcessorId::LaMa,
            Self::Processor(ProcessorConfig::AotInpainting(_)) => ProcessorId::AotInpainting,
            Self::Processor(ProcessorConfig::Flux2Klein(_)) => ProcessorId::Flux2Klein,
            Self::Processor(ProcessorConfig::RoremMixed(_)) => ProcessorId::RoremMixed,
        }
    }

    pub(crate) const fn name(&self) -> &'static str {
        match self {
            Self::Processor(ProcessorConfig::ComicTextDetector(_)) => "ComicTextDetector",
            Self::Processor(ProcessorConfig::PPDocLayoutV3(_)) => "PPDocLayoutV3",
            Self::Processor(ProcessorConfig::ComicLayoutYolo26s(_)) => "ComicLayoutYolo26s",
            Self::Processor(ProcessorConfig::MangaTextMask(_)) => "MangaTextMask",
            Self::Processor(ProcessorConfig::SpeechBubbleYoloV8m(_)) => "SpeechBubbleYoloV8m",
            Self::Processor(ProcessorConfig::SpeechBubbleYolo11n(_)) => "SpeechBubbleYolo11n",
            Self::Processor(ProcessorConfig::ComicOnomatopoeia(_)) => "ComicOnomatopoeia",
            Self::Processor(ProcessorConfig::MaskFusion(_)) => "MaskFusion",
            Self::Processor(ProcessorConfig::PaddleOcrVl1_6(_)) => "PaddleOCR-VL 1.6",
            Self::Processor(ProcessorConfig::MangaOcr(_)) => "MangaOcr",
            Self::Processor(ProcessorConfig::BaberuOcr(_)) => "BaberuOcr",
            Self::Translation(Providers::Local(_)) => "LocalTranslator",
            Self::Translation(Providers::OpenAi(_)) => "OpenAI",
            Self::Translation(Providers::Gemini(_)) => "Gemini",
            Self::Translation(Providers::Claude(_)) => "Claude",
            Self::Translation(Providers::DeepSeek(_)) => "DeepSeek",
            Self::Translation(Providers::OpenAiCompatible(_)) => "OpenAI-compatible",
            Self::Translation(Providers::OpenRouter(_)) => "OpenRouter",
            Self::Translation(Providers::LmStudio(_)) => "LM Studio",
            Self::Translation(Providers::DeepL(_)) => "DeepL",
            Self::Translation(Providers::GoogleCloudTranslation(_)) => "Google Cloud Translation",
            Self::Translation(Providers::Caiyun(_)) => "Caiyun",
            Self::Processor(ProcessorConfig::FontDetector(_)) => "FontDetector",
            Self::Processor(ProcessorConfig::LaMa(_)) => "LaMa",
            Self::Processor(ProcessorConfig::AotInpainting(_)) => "AotInpainting",
            Self::Processor(ProcessorConfig::Flux2Klein(_)) => "FLUX.2 Klein",
            Self::Processor(ProcessorConfig::RoremMixed(_)) => "RORem Mixed",
        }
    }

    pub(crate) const fn inputs(&self) -> &'static [Artifact] {
        match self {
            Self::Processor(ProcessorConfig::ComicTextDetector(_))
            | Self::Processor(ProcessorConfig::PPDocLayoutV3(_))
            | Self::Processor(ProcessorConfig::ComicLayoutYolo26s(_))
            | Self::Processor(ProcessorConfig::MangaTextMask(_))
            | Self::Processor(ProcessorConfig::SpeechBubbleYoloV8m(_))
            | Self::Processor(ProcessorConfig::SpeechBubbleYolo11n(_)) => &[Artifact::SourceImage],
            Self::Processor(ProcessorConfig::ComicOnomatopoeia(_)) => {
                &[Artifact::SourceImage, Artifact::TextRegion]
            }
            Self::Processor(ProcessorConfig::MaskFusion(_)) => &[
                Artifact::TextMaskCandidate,
                Artifact::LayoutTextMask,
                Artifact::TextRegion,
                Artifact::CooRegion,
            ],
            Self::Processor(ProcessorConfig::PaddleOcrVl1_6(_))
            | Self::Processor(ProcessorConfig::MangaOcr(_))
            | Self::Processor(ProcessorConfig::BaberuOcr(_))
            | Self::Processor(ProcessorConfig::FontDetector(_)) => {
                &[Artifact::SourceImage, Artifact::TextRegion]
            }
            Self::Translation(_) => &[Artifact::SourceText, Artifact::CooText],
            Self::Processor(ProcessorConfig::LaMa(_))
            | Self::Processor(ProcessorConfig::AotInpainting(_))
            | Self::Processor(ProcessorConfig::Flux2Klein(_))
            | Self::Processor(ProcessorConfig::RoremMixed(_)) => &[
                Artifact::SourceImage,
                Artifact::TextMask,
                Artifact::CooMask,
                Artifact::BrushMask,
            ],
        }
    }

    pub(crate) const fn outputs(&self) -> &'static [Artifact] {
        match self {
            Self::Processor(ProcessorConfig::ComicTextDetector(_))
            | Self::Processor(ProcessorConfig::PPDocLayoutV3(_)) => {
                &[Artifact::TextRegion, Artifact::TextMaskCandidate]
            }
            Self::Processor(ProcessorConfig::ComicLayoutYolo26s(_)) => &[
                Artifact::PanelRegion,
                Artifact::BubbleRegion,
                Artifact::TextRegion,
                Artifact::LayoutTextMask,
                Artifact::BubbleMask,
            ],
            Self::Processor(ProcessorConfig::MangaTextMask(_)) => &[Artifact::TextMaskCandidate],
            Self::Processor(
                ProcessorConfig::SpeechBubbleYoloV8m(_) | ProcessorConfig::SpeechBubbleYolo11n(_),
            ) => &[Artifact::BubbleMask],
            Self::Processor(ProcessorConfig::ComicOnomatopoeia(_)) => {
                &[Artifact::CooRegion, Artifact::CooText]
            }
            Self::Processor(ProcessorConfig::MaskFusion(_)) => {
                &[Artifact::TextMask, Artifact::CooMask]
            }
            Self::Processor(ProcessorConfig::PaddleOcrVl1_6(_))
            | Self::Processor(ProcessorConfig::MangaOcr(_))
            | Self::Processor(ProcessorConfig::BaberuOcr(_)) => &[Artifact::SourceText],
            Self::Translation(_) => &[Artifact::Translation],
            Self::Processor(ProcessorConfig::FontDetector(_)) => &[Artifact::Typography],
            Self::Processor(ProcessorConfig::LaMa(_))
            | Self::Processor(ProcessorConfig::AotInpainting(_))
            | Self::Processor(ProcessorConfig::Flux2Klein(_))
            | Self::Processor(ProcessorConfig::RoremMixed(_)) => &[Artifact::CleanImage],
        }
    }

    pub(crate) const fn supports_element_scope(&self) -> bool {
        matches!(
            self,
            Self::Processor(ProcessorConfig::PaddleOcrVl1_6(_))
                | Self::Processor(ProcessorConfig::MangaOcr(_))
                | Self::Processor(ProcessorConfig::BaberuOcr(_))
                | Self::Translation(_)
                | Self::Processor(ProcessorConfig::FontDetector(_))
        )
    }

    pub(crate) const fn uses_accelerator(&self) -> bool {
        !matches!(self.runtime(), ModelRuntime::None)
    }

    pub(crate) const fn runtime(&self) -> ModelRuntime {
        match self {
            Self::Translation(Providers::Local(_)) => ModelRuntime::Llama,
            Self::Processor(ProcessorConfig::Flux2Klein(_) | ProcessorConfig::RoremMixed(_)) => {
                ModelRuntime::Diffusion
            }
            Self::Processor(ProcessorConfig::MaskFusion(_)) => ModelRuntime::None,
            Self::Translation(_) => ModelRuntime::None,
            _ => ModelRuntime::Torch,
        }
    }
}

#[derive(Clone)]
pub(crate) struct PlanNode {
    pub id: ProcessorId,
    pub model: ConfiguredModel,
    pub phases: Vec<Phase>,
    pub phase: Phase,
}

pub(crate) struct Plan {
    pub nodes: Vec<PlanNode>,
    pub waves: Vec<Vec<usize>>,
    pub targets: Vec<bool>,
    pub required: Vec<Vec<Artifact>>,
    dependencies: Vec<Vec<usize>>,
    bindings: Vec<Vec<(Artifact, usize)>>,
    producers: HashMap<Artifact, usize>,
}

impl Plan {
    pub(crate) fn build(config: &PipelineConfig, translation: &Providers) -> Result<Self> {
        let nodes = configured_models(config, translation)?;
        let (dependencies, bindings, producers) = dependencies(&nodes)?;
        let waves = waves(&dependencies)?;
        Ok(Self {
            targets: vec![true; nodes.len()],
            required: nodes
                .iter()
                .map(|node| node.model.outputs().to_vec())
                .collect(),
            nodes,
            waves,
            dependencies,
            bindings,
            producers,
        })
    }

    pub(crate) fn select(mut self, target: &RunTarget) -> Result<Self> {
        let mut targets = vec![false; self.nodes.len()];
        let mut required = vec![Vec::new(); self.nodes.len()];
        match target {
            RunTarget::All => {
                targets.fill(true);
                for (index, node) in self.nodes.iter().enumerate() {
                    required[index].extend(
                        node.model
                            .outputs()
                            .iter()
                            .filter(|artifact| self.producers.get(artifact) == Some(&index))
                            .copied(),
                    );
                }
            }
            RunTarget::Phase { phase } => {
                for (index, node) in self.nodes.iter_mut().enumerate() {
                    targets[index] = node.phases.contains(phase);
                    if targets[index] {
                        node.phase = *phase;
                        required[index].extend(
                            node.model
                                .outputs()
                                .iter()
                                .filter(|artifact| artifact.phase() == Some(*phase))
                                .copied(),
                        );
                    }
                }
                if !targets.iter().any(|selected| *selected) {
                    bail!("phase {phase} has no configured processor");
                }
            }
            RunTarget::Processors { processors } => {
                for processor in processors {
                    let index = self
                        .nodes
                        .iter()
                        .position(|node| node.id == *processor)
                        .ok_or_else(|| {
                            anyhow::anyhow!("processor {processor} is not configured")
                        })?;
                    targets[index] = true;
                    required[index].extend_from_slice(self.nodes[index].model.outputs());
                }
            }
            RunTarget::Artifacts { artifacts } => {
                for artifact in artifacts {
                    let index = self.producers.get(artifact).copied().ok_or_else(|| {
                        anyhow::anyhow!("artifact {artifact} has no configured producer")
                    })?;
                    targets[index] = true;
                    required[index].push(*artifact);
                    if let Some(phase) = artifact.phase() {
                        self.nodes[index].phase = phase;
                    }
                }
            }
        }
        self.retain_with_ancestors(targets, required)
    }

    pub(crate) fn retain(self, retained: &[bool]) -> Result<Self> {
        if retained.len() != self.nodes.len() {
            bail!("execution mask does not match the pipeline plan");
        }
        let targets = self.targets.clone();
        let required = self.required.clone();
        self.remap(retained, targets, required)
    }

    pub(crate) fn dependency_ran(&self, index: usize, scheduled: &[bool]) -> bool {
        self.dependencies[index]
            .iter()
            .any(|dependency| scheduled[*dependency])
    }

    pub(crate) fn dot(&self) -> String {
        let mut output = String::from("digraph pipeline {\n");
        for (index, node) in self.nodes.iter().enumerate() {
            let phases = node
                .phases
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str(&format!(
                "  n{index} [label=\"{}: {}\"];\n",
                phases,
                node.model.name()
            ));
            for (artifact, dependency) in &self.bindings[index] {
                output.push_str(&format!(
                    "  n{dependency} -> n{index} [label=\"{artifact}\"];\n"
                ));
            }
        }
        output.push_str("}\n");
        output
    }

    fn retain_with_ancestors(
        self,
        targets: Vec<bool>,
        mut required: Vec<Vec<Artifact>>,
    ) -> Result<Self> {
        let mut retained = targets.clone();
        let mut pending = retained
            .iter()
            .enumerate()
            .filter_map(|(index, selected)| selected.then_some(index))
            .collect::<Vec<_>>();
        while let Some(index) = pending.pop() {
            for &(artifact, dependency) in &self.bindings[index] {
                if !required[dependency].contains(&artifact) {
                    required[dependency].push(artifact);
                }
                if !retained[dependency] {
                    retained[dependency] = true;
                    pending.push(dependency);
                }
            }
        }
        self.remap(&retained, targets, required)
    }

    fn remap(
        self,
        retained: &[bool],
        targets: Vec<bool>,
        required: Vec<Vec<Artifact>>,
    ) -> Result<Self> {
        if retained.iter().all(|retained| *retained) {
            return Ok(Self {
                targets,
                required,
                ..self
            });
        }
        let mut remap = vec![None; self.nodes.len()];
        let mut nodes = Vec::new();
        let mut new_targets = Vec::new();
        let mut new_required = Vec::new();
        for (old, node) in self.nodes.into_iter().enumerate() {
            if retained[old] {
                remap[old] = Some(nodes.len());
                nodes.push(node);
                new_targets.push(targets[old]);
                new_required.push(required[old].clone());
            }
        }
        let dependencies = self
            .dependencies
            .into_iter()
            .enumerate()
            .filter(|(old, _)| retained[*old])
            .map(|(_, dependencies)| {
                dependencies
                    .into_iter()
                    .filter_map(|old| remap[old])
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let bindings = self
            .bindings
            .into_iter()
            .enumerate()
            .filter(|(old, _)| retained[*old])
            .map(|(_, bindings)| {
                bindings
                    .into_iter()
                    .filter_map(|(artifact, old)| remap[old].map(|new| (artifact, new)))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let producers = self
            .producers
            .into_iter()
            .filter_map(|(artifact, old)| remap[old].map(|new| (artifact, new)))
            .collect();
        let waves = waves(&dependencies)?;
        Ok(Self {
            nodes,
            waves,
            targets: new_targets,
            required: new_required,
            dependencies,
            bindings,
            producers,
        })
    }
}

fn configured_models(config: &PipelineConfig, translation: &Providers) -> Result<Vec<PlanNode>> {
    let mut nodes = Vec::<PlanNode>::new();
    for processor in &config.processors {
        let model = ConfiguredModel::Processor(processor.clone());
        if nodes.iter().any(|node| node.id == model.id()) {
            bail!("processor {} is configured more than once", model.id());
        }
        let mut phases = model
            .outputs()
            .iter()
            .filter_map(|artifact| artifact.phase())
            .collect::<Vec<_>>();
        phases.sort_unstable();
        phases.dedup();
        let phase = *phases
            .first()
            .ok_or_else(|| anyhow::anyhow!("processor {} has no phased output", model.id()))?;
        nodes.push(PlanNode {
            id: model.id(),
            model,
            phases,
            phase,
        });
    }
    nodes.push(PlanNode {
        id: ProcessorId::Translation,
        model: ConfiguredModel::Translation(translation.clone()),
        phases: vec![Phase::Translation],
        phase: Phase::Translation,
    });
    Ok(nodes)
}

fn dependencies(
    nodes: &[PlanNode],
) -> Result<(
    Vec<Vec<usize>>,
    Vec<Vec<(Artifact, usize)>>,
    HashMap<Artifact, usize>,
)> {
    let mut producers = HashMap::new();
    let mut dependencies = Vec::<Vec<usize>>::with_capacity(nodes.len());
    let mut bindings = Vec::<Vec<(Artifact, usize)>>::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        let model = &node.model;
        let node_bindings = model
            .inputs()
            .iter()
            .filter_map(|input| {
                producers
                    .get(input)
                    .copied()
                    .map(|producer| (*input, producer))
            })
            .collect::<Vec<_>>();
        let mut node_dependencies = node_bindings
            .iter()
            .map(|(_, producer)| *producer)
            .collect::<Vec<_>>();
        for output in model.outputs() {
            if let Some(previous) = producers.get(output).copied()
                && !depends_on(previous, &node_dependencies, &dependencies)
            {
                node_dependencies.push(previous);
            }
            producers.insert(*output, index);
        }
        node_dependencies.sort_unstable();
        node_dependencies.dedup();
        dependencies.push(node_dependencies);
        bindings.push(node_bindings);
    }
    Ok((dependencies, bindings, producers))
}

fn depends_on(target: usize, roots: &[usize], dependencies: &[Vec<usize>]) -> bool {
    let mut pending = roots.to_vec();
    let mut seen = HashSet::new();
    while let Some(index) = pending.pop() {
        if index == target {
            return true;
        }
        if seen.insert(index) {
            pending.extend(&dependencies[index]);
        }
    }
    false
}

fn waves(dependencies: &[Vec<usize>]) -> Result<Vec<Vec<usize>>> {
    fn depth(
        index: usize,
        dependencies: &[Vec<usize>],
        depths: &mut [Option<usize>],
        visiting: &mut [bool],
    ) -> Result<usize> {
        if let Some(depth) = depths[index] {
            return Ok(depth);
        }
        if std::mem::replace(&mut visiting[index], true) {
            bail!("configured processors produce a cyclic pipeline");
        }
        let value = dependencies[index]
            .iter()
            .map(|&dependency| depth(dependency, dependencies, depths, visiting))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .max()
            .map_or(0, |depth| depth + 1);
        visiting[index] = false;
        depths[index] = Some(value);
        Ok(value)
    }

    let mut depths = vec![None; dependencies.len()];
    let mut visiting = vec![false; dependencies.len()];
    for index in 0..dependencies.len() {
        depth(index, dependencies, &mut depths, &mut visiting)?;
    }
    let mut waves = Vec::<Vec<usize>>::new();
    for (index, depth) in depths.into_iter().enumerate() {
        let depth = depth.expect("every node was visited");
        if waves.len() <= depth {
            waves.resize_with(depth + 1, Vec::new);
        }
        waves[depth].push(index);
    }
    Ok(waves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_translator::{OpenAiConfig, TranslationConfig};

    #[test]
    fn phase_selects_only_ancestors() {
        let config = PipelineConfig {
            processors: vec![
                ProcessorConfig::ComicTextDetector(Default::default()),
                ProcessorConfig::MangaOcr(Default::default()),
            ],
        };
        let translation = Providers::OpenAi(OpenAiConfig::default());
        let plan = Plan::build(&config, &translation)
            .unwrap()
            .select(&RunTarget::Phase { phase: Phase::Ocr })
            .unwrap();
        assert_eq!(
            plan.nodes.iter().map(|node| node.phase).collect::<Vec<_>>(),
            [Phase::Detection, Phase::Ocr]
        );
        assert_eq!(plan.targets, [false, true]);
        assert_eq!(plan.required[0], [Artifact::TextRegion]);
    }

    #[test]
    fn text_mask_writers_are_ordered_without_becoming_inputs() {
        let config = PipelineConfig {
            processors: vec![
                ProcessorConfig::PPDocLayoutV3(Default::default()),
                ProcessorConfig::MangaTextMask(Default::default()),
            ],
        };
        let plan = Plan::build(&config, &TranslationConfig::default().model).unwrap();
        assert!(plan.dependencies[1].contains(&0));
        assert!(plan.bindings[1].is_empty());
        assert_eq!(plan.producers[&Artifact::TextMaskCandidate], 1);

        let all = Plan::build(&config, &TranslationConfig::default().model)
            .unwrap()
            .select(&RunTarget::All)
            .unwrap();
        assert_eq!(all.required[0], [Artifact::TextRegion]);

        let segmentation = plan
            .select(&RunTarget::Phase {
                phase: Phase::Segmentation,
            })
            .unwrap();
        assert_eq!(segmentation.nodes.len(), 2);
        assert_eq!(segmentation.nodes[1].id, ProcessorId::MangaTextMask);
    }

    #[test]
    fn artifact_target_selects_its_latest_producer() {
        let config = PipelineConfig {
            processors: vec![
                ProcessorConfig::MangaTextMask(Default::default()),
                ProcessorConfig::MaskFusion(Default::default()),
            ],
        };
        let plan = Plan::build(&config, &TranslationConfig::default().model)
            .unwrap()
            .select(&RunTarget::Artifacts {
                artifacts: vec![Artifact::TextMask],
            })
            .unwrap();
        assert_eq!(plan.nodes.last().unwrap().id, ProcessorId::MaskFusion);
        assert!(plan.targets.last().copied().unwrap());
    }

    #[test]
    fn processors_that_write_the_same_artifact_are_ordered() {
        let config = PipelineConfig {
            processors: vec![
                ProcessorConfig::ComicTextDetector(Default::default()),
                ProcessorConfig::PPDocLayoutV3(Default::default()),
            ],
        };
        let plan = Plan::build(&config, &TranslationConfig::default().model).unwrap();
        assert_eq!(plan.dependencies[1], [0]);
        assert!(plan.bindings[1].is_empty());
        assert_eq!(plan.producers[&Artifact::TextRegion], 1);
        assert_eq!(plan.producers[&Artifact::TextMaskCandidate], 1);
    }

    #[test]
    fn one_model_can_belong_to_multiple_phases() {
        let config = PipelineConfig {
            processors: vec![ProcessorConfig::ComicLayoutYolo26s(Default::default())],
        };
        let plan = Plan::build(&config, &TranslationConfig::default().model).unwrap();
        let yolo = plan
            .nodes
            .iter()
            .find(|node| node.id == ProcessorId::ComicLayoutYolo26s)
            .unwrap();
        assert_eq!(yolo.phases, [Phase::Detection, Phase::Segmentation]);
        assert_eq!(
            plan.nodes
                .iter()
                .filter(|node| node.id == ProcessorId::ComicLayoutYolo26s)
                .count(),
            1
        );

        let detection = Plan::build(&config, &TranslationConfig::default().model)
            .unwrap()
            .select(&RunTarget::Phase {
                phase: Phase::Detection,
            })
            .unwrap();
        assert_eq!(detection.nodes[0].phase, Phase::Detection);
        assert_eq!(
            detection.required[0],
            [
                Artifact::PanelRegion,
                Artifact::BubbleRegion,
                Artifact::TextRegion,
            ]
        );

        let segmentation = Plan::build(&config, &TranslationConfig::default().model)
            .unwrap()
            .select(&RunTarget::Phase {
                phase: Phase::Segmentation,
            })
            .unwrap();
        assert_eq!(segmentation.nodes[0].phase, Phase::Segmentation);
        assert_eq!(
            segmentation.required[0],
            [Artifact::LayoutTextMask, Artifact::BubbleMask]
        );
    }

    #[test]
    fn a_processor_cannot_be_configured_twice() {
        let config = PipelineConfig {
            processors: vec![
                ProcessorConfig::PPDocLayoutV3(crate::PPDocLayoutV3Config { confidence: 0.25 }),
                ProcessorConfig::PPDocLayoutV3(crate::PPDocLayoutV3Config { confidence: 0.5 }),
            ],
        };
        let error = Plan::build(&config, &TranslationConfig::default().model)
            .err()
            .unwrap();
        assert!(error.to_string().contains("configured more than once"));
    }

    #[test]
    fn rorem_mixed_is_a_diffusion_inpainting_processor() {
        let model = ConfiguredModel::Processor(ProcessorConfig::RoremMixed(Default::default()));

        assert_eq!(model.id(), ProcessorId::RoremMixed);
        assert_eq!(model.name(), "RORem Mixed");
        assert_eq!(model.runtime(), ModelRuntime::Diffusion);
        assert_eq!(
            model.inputs(),
            [
                Artifact::SourceImage,
                Artifact::TextMask,
                Artifact::CooMask,
                Artifact::BrushMask,
            ]
        );
        assert_eq!(model.outputs(), [Artifact::CleanImage]);
    }

    #[test]
    fn speech_bubble_yolo11n_is_a_torch_segmentation_processor() {
        let model =
            ConfiguredModel::Processor(ProcessorConfig::SpeechBubbleYolo11n(Default::default()));

        assert_eq!(model.id(), ProcessorId::SpeechBubbleYolo11n);
        assert_eq!(model.name(), "SpeechBubbleYolo11n");
        assert_eq!(model.runtime(), ModelRuntime::Torch);
        assert_eq!(model.inputs(), [Artifact::SourceImage]);
        assert_eq!(model.outputs(), [Artifact::BubbleMask]);
    }
}
