use std::collections::HashMap;

use anyhow::{Result, bail};

use crate::{
    DetectionModel, InpaintingModel, OcrModel, PipelineConfig, SegmentationModel, Stage,
    TranslationModel, TypographyModel,
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ConfiguredModel {
    Detection(DetectionModel),
    Segmentation(SegmentationModel),
    Ocr(OcrModel),
    Translation(TranslationModel),
    Typography(TypographyModel),
    Inpainting(InpaintingModel),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct NodeKey {
    pub stage: Stage,
    pub index: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Output {
    Text,
    TextMask,
    BubbleMask,
    SourceText,
    Translation,
    Typography,
    Clean,
}

impl ConfiguredModel {
    pub(crate) const fn stage(&self) -> Stage {
        match self {
            Self::Detection(_) => Stage::Detection,
            Self::Segmentation(_) => Stage::Segmentation,
            Self::Ocr(_) => Stage::Ocr,
            Self::Translation(_) => Stage::Translation,
            Self::Typography(_) => Stage::Typography,
            Self::Inpainting(_) => Stage::Inpainting,
        }
    }

    pub(crate) const fn name(&self) -> &'static str {
        match self {
            Self::Detection(DetectionModel::ComicTextDetector(_)) => "ComicTextDetector",
            Self::Detection(DetectionModel::PPDocLayoutV3(_)) => "PPDocLayoutV3",
            Self::Segmentation(SegmentationModel::MangaTextSegmentation(_)) => {
                "MangaTextSegmentation"
            }
            Self::Segmentation(SegmentationModel::SpeechBubbleSegmentation(_)) => {
                "SpeechBubbleSegmentation"
            }
            Self::Ocr(OcrModel::PaddleOcrVl1_6(_)) => "PaddleOCR-VL 1.6",
            Self::Ocr(OcrModel::MangaOcr(_)) => "MangaOcr",
            Self::Translation(TranslationModel::Local(_)) => "LocalTranslator",
            Self::Translation(TranslationModel::OpenAi(_)) => "OpenAI",
            Self::Translation(TranslationModel::Gemini(_)) => "Gemini",
            Self::Translation(TranslationModel::Claude(_)) => "Claude",
            Self::Translation(TranslationModel::DeepSeek(_)) => "DeepSeek",
            Self::Translation(TranslationModel::OpenAiCompatible(_)) => "OpenAI-compatible",
            Self::Translation(TranslationModel::DeepL(_)) => "DeepL",
            Self::Translation(TranslationModel::GoogleCloudTranslation) => {
                "Google Cloud Translation"
            }
            Self::Translation(TranslationModel::Caiyun) => "Caiyun",
            Self::Typography(TypographyModel::FontDetector(_)) => "FontDetector",
            Self::Inpainting(InpaintingModel::LaMa(_)) => "LaMa",
            Self::Inpainting(InpaintingModel::AotInpainting(_)) => "AotInpainting",
            Self::Inpainting(InpaintingModel::Flux2Klein(_)) => "FLUX.2 Klein",
        }
    }

    pub(crate) const fn outputs(&self) -> &'static [Output] {
        match self {
            Self::Detection(DetectionModel::ComicTextDetector(_)) => {
                &[Output::Text, Output::TextMask]
            }
            Self::Detection(DetectionModel::PPDocLayoutV3(_)) => &[Output::Text],
            Self::Segmentation(SegmentationModel::MangaTextSegmentation(_)) => &[Output::TextMask],
            Self::Segmentation(SegmentationModel::SpeechBubbleSegmentation(_)) => {
                &[Output::BubbleMask]
            }
            Self::Ocr(_) => &[Output::SourceText],
            Self::Translation(_) => &[Output::Translation],
            Self::Typography(_) => &[Output::Typography],
            Self::Inpainting(_) => &[Output::Clean],
        }
    }

    pub(crate) const fn uses_accelerator(&self) -> bool {
        !matches!(
            self,
            Self::Translation(
                TranslationModel::OpenAi(_)
                    | TranslationModel::Gemini(_)
                    | TranslationModel::Claude(_)
                    | TranslationModel::DeepSeek(_)
                    | TranslationModel::OpenAiCompatible(_)
                    | TranslationModel::DeepL(_)
                    | TranslationModel::GoogleCloudTranslation
                    | TranslationModel::Caiyun
            )
        )
    }

    const fn dependencies(&self) -> &'static [Output] {
        match self {
            Self::Segmentation(SegmentationModel::MangaTextSegmentation(_)) => &[Output::Text],
            Self::Ocr(_) | Self::Typography(_) => &[Output::Text],
            Self::Translation(_) => &[Output::SourceText],
            Self::Inpainting(_) => &[Output::TextMask],
            _ => &[],
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Selection {
    All,
    Through(Stage),
    Only(Stage),
}

pub(crate) struct PlanNode {
    pub key: NodeKey,
    pub model: ConfiguredModel,
}

pub(crate) struct Plan {
    pub nodes: Vec<PlanNode>,
    pub waves: Vec<Vec<usize>>,
    dependencies: Vec<Vec<usize>>,
}

impl Plan {
    pub(crate) fn build(config: &PipelineConfig, selection: Selection) -> Result<Self> {
        let models = configured_models(config);
        let all_dependencies = dependencies(&models)?;
        let selected = selected_nodes(&models, &all_dependencies, selection)?;
        let mut remap = vec![None; models.len()];
        let mut nodes = Vec::new();
        for (old, (key, model)) in models.into_iter().enumerate() {
            if selected[old] {
                remap[old] = Some(nodes.len());
                nodes.push(PlanNode { key, model });
            }
        }
        let dependencies = all_dependencies
            .into_iter()
            .enumerate()
            .filter(|(old, _)| selected[*old])
            .map(|(_, dependencies)| {
                dependencies
                    .into_iter()
                    .filter_map(|old| remap[old])
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let waves = waves(&dependencies)?;
        Ok(Self {
            nodes,
            waves,
            dependencies,
        })
    }

    pub(crate) fn dot(&self) -> String {
        let mut output = String::from("digraph pipeline {\n");
        for (index, node) in self.nodes.iter().enumerate() {
            output.push_str(&format!(
                "  n{index} [label=\"{}: {}\"];\n",
                node.model.stage(),
                node.model.name()
            ));
            for dependency in &self.dependencies[index] {
                output.push_str(&format!("  n{dependency} -> n{index};\n"));
            }
        }
        output.push_str("}\n");
        output
    }
}

fn configured_models(config: &PipelineConfig) -> Vec<(NodeKey, ConfiguredModel)> {
    vec![
        (
            NodeKey {
                stage: Stage::Detection,
                index: 0,
            },
            ConfiguredModel::Detection(config.detection.clone()),
        ),
        (
            NodeKey {
                stage: Stage::Segmentation,
                index: 0,
            },
            ConfiguredModel::Segmentation(config.segmentation.clone()),
        ),
        (
            NodeKey {
                stage: Stage::Ocr,
                index: 0,
            },
            ConfiguredModel::Ocr(config.ocr.clone()),
        ),
        (
            NodeKey {
                stage: Stage::Translation,
                index: 0,
            },
            ConfiguredModel::Translation(config.translation.clone()),
        ),
        (
            NodeKey {
                stage: Stage::Typography,
                index: 0,
            },
            ConfiguredModel::Typography(config.typography.clone()),
        ),
        (
            NodeKey {
                stage: Stage::Inpainting,
                index: 0,
            },
            ConfiguredModel::Inpainting(config.inpainting.clone()),
        ),
    ]
}

fn dependencies(models: &[(NodeKey, ConfiguredModel)]) -> Result<Vec<Vec<usize>>> {
    let mut producers = HashMap::new();
    for (index, (_, model)) in models.iter().enumerate() {
        for output in model.outputs() {
            if let Some(previous) = producers.insert(*output, index) {
                bail!(
                    "models '{}' and '{}' both produce {output:?}",
                    models[previous].1.name(),
                    model.name()
                );
            }
        }
    }
    Ok(models
        .iter()
        .map(|(_, model)| {
            model
                .dependencies()
                .iter()
                .filter_map(|output| producers.get(output).copied())
                .collect()
        })
        .collect())
}

fn selected_nodes(
    models: &[(NodeKey, ConfiguredModel)],
    dependencies: &[Vec<usize>],
    selection: Selection,
) -> Result<Vec<bool>> {
    if matches!(selection, Selection::All) {
        return Ok(vec![true; models.len()]);
    }
    let stage = match selection {
        Selection::Through(stage) | Selection::Only(stage) => stage,
        Selection::All => unreachable!(),
    };
    let mut selected = models
        .iter()
        .map(|(key, _)| key.stage == stage)
        .collect::<Vec<_>>();
    if !selected.iter().any(|selected| *selected) {
        bail!("stage {stage} has no configured model");
    }
    if matches!(selection, Selection::Only(_)) {
        return Ok(selected);
    }
    let mut pending = selected
        .iter()
        .enumerate()
        .filter_map(|(index, selected)| selected.then_some(index))
        .collect::<Vec<_>>();
    while let Some(index) = pending.pop() {
        for &dependency in &dependencies[index] {
            if !selected[dependency] {
                selected[dependency] = true;
                pending.push(dependency);
            }
        }
    }
    Ok(selected)
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
            bail!("configured models produce a cyclic pipeline");
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
    use crate::{ChatTranslationConfig, MangaTextSegmentationConfig};

    #[test]
    fn through_selects_only_ancestors() {
        let config = PipelineConfig {
            detection: DetectionModel::ComicTextDetector(Default::default()),
            segmentation: SegmentationModel::SpeechBubbleSegmentation(Default::default()),
            ocr: OcrModel::MangaOcr(Default::default()),
            translation: TranslationModel::OpenAi(ChatTranslationConfig::default()),
            ..PipelineConfig::default()
        };
        let plan = Plan::build(&config, Selection::Through(Stage::Ocr)).unwrap();
        assert_eq!(
            plan.nodes
                .iter()
                .map(|node| node.key.stage)
                .collect::<Vec<_>>(),
            [Stage::Detection, Stage::Ocr]
        );
    }

    #[test]
    fn text_mask_waits_for_detection() {
        let config = PipelineConfig {
            detection: DetectionModel::PPDocLayoutV3(Default::default()),
            segmentation: SegmentationModel::MangaTextSegmentation(
                MangaTextSegmentationConfig::default(),
            ),
            ..PipelineConfig::default()
        };
        let plan = Plan::build(&config, Selection::All).unwrap();
        assert_eq!(plan.waves, vec![vec![0], vec![1, 2, 4], vec![3, 5]]);
    }

    #[test]
    fn only_does_not_schedule_ancestors() {
        let config = PipelineConfig {
            detection: DetectionModel::PPDocLayoutV3(Default::default()),
            ocr: OcrModel::MangaOcr(Default::default()),
            ..PipelineConfig::default()
        };
        let plan = Plan::build(&config, Selection::Only(Stage::Ocr)).unwrap();
        assert_eq!(plan.nodes.len(), 1);
        assert_eq!(plan.waves, vec![vec![0]]);
    }

    #[test]
    fn rejects_models_that_write_the_same_scene_result() {
        let config = PipelineConfig {
            detection: DetectionModel::ComicTextDetector(Default::default()),
            segmentation: SegmentationModel::MangaTextSegmentation(Default::default()),
            ..PipelineConfig::default()
        };
        assert!(Plan::build(&config, Selection::All).is_err());
    }
}
