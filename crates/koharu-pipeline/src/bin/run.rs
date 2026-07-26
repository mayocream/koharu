use std::{fs, path::PathBuf, sync::Arc};

use anyhow::{Context as _, Result};
use clap::{Parser, ValueEnum};
use koharu_config::Config;
use koharu_pipeline::{
    AotInpaintingConfig, BaberuOcrConfig, DetectionModel, Flux2KleinConfig, FontDetectorConfig,
    InpaintingModel, KoharuLayoutRFDetrSeg2XLConfig, LaMaConfig, MangaOcrConfig, OcrModel,
    PaddleOcrVl1_6Config, Pipeline, PipelineConfig, PipelineEvent, RoremMixedConfig,
    TypographyModel,
};
use koharu_renderer::{PageRenderOptions, Renderer};
use koharu_scene::{Command, ElementChange, PageId, Session};
use koharu_translator::{LocalConfig, Providers, TranslationConfig};

#[derive(Debug, Parser)]
#[command(version, about = "Run Koharu's complete pipeline and render one image")]
struct Arguments {
    #[arg(short, long, value_name = "INPUT", required_unless_present = "worker")]
    input: Option<PathBuf>,

    #[arg(short, long, value_name = "OUTPUT", required_unless_present = "worker")]
    output: Option<PathBuf>,

    #[arg(long, value_enum, default_value = "koharu-layout-rfdetr-seg-2xl")]
    detection: DetectionChoice,

    #[arg(long, value_enum, default_value = "paddleocr-vl-1.6")]
    ocr: OcrChoice,

    #[arg(
        long = "font-family",
        value_name = "FAMILY",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    font_families: Vec<String>,

    #[arg(long, value_enum, default_value = "lama")]
    inpainting: InpaintingChoice,

    #[arg(long, default_value = "en-US")]
    target_language: String,

    #[arg(long)]
    translation_instructions: Option<String>,

    #[arg(long, default_value = "gemma4-12b-it")]
    llm: String,

    #[arg(long, hide = true)]
    worker: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DetectionChoice {
    #[value(name = "koharu-layout-rfdetr-seg-2xl")]
    KoharuLayoutRFDetrSeg2XL,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OcrChoice {
    #[value(name = "paddleocr-vl-1.6")]
    PaddleOcrVl1_6,
    #[value(name = "manga-ocr")]
    MangaOcr,
    #[value(name = "baberu-ocr")]
    BaberuOcr,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum InpaintingChoice {
    #[value(name = "lama")]
    LaMa,
    #[value(name = "aot-inpainting")]
    AotInpainting,
    #[value(name = "flux2-klein")]
    Flux2Klein,
    #[value(name = "rorem-mixed")]
    RoremMixed,
}

impl Arguments {
    fn pipeline_config(&self) -> PipelineConfig {
        let detection = match self.detection {
            DetectionChoice::KoharuLayoutRFDetrSeg2XL => {
                DetectionModel::KoharuLayoutRFDetrSeg2XL(KoharuLayoutRFDetrSeg2XLConfig::default())
            }
        };
        let ocr = match self.ocr {
            OcrChoice::PaddleOcrVl1_6 => OcrModel::PaddleOcrVl1_6(PaddleOcrVl1_6Config::default()),
            OcrChoice::MangaOcr => OcrModel::MangaOcr(MangaOcrConfig::default()),
            OcrChoice::BaberuOcr => OcrModel::BaberuOcr(BaberuOcrConfig::default()),
        };
        let inpainting = match self.inpainting {
            InpaintingChoice::LaMa => InpaintingModel::LaMa(LaMaConfig::default()),
            InpaintingChoice::AotInpainting => {
                InpaintingModel::AotInpainting(AotInpaintingConfig::default())
            }
            InpaintingChoice::Flux2Klein => {
                InpaintingModel::Flux2Klein(Flux2KleinConfig::default())
            }
            InpaintingChoice::RoremMixed => {
                InpaintingModel::RoremMixed(RoremMixedConfig::default())
            }
        };
        PipelineConfig {
            detection,
            ocr,
            typography: TypographyModel::FontDetector(FontDetectorConfig::default()),
            inpainting,
        }
    }

    fn translation_config(&self) -> TranslationConfig {
        TranslationConfig {
            model: Providers::Local(LocalConfig {
                model: self.llm.clone(),
            }),
            target_language: self.target_language.clone(),
            instructions: self.translation_instructions.clone(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    if arguments.worker {
        return koharu_pipeline::serve_worker().await;
    }

    let input = arguments.input.as_deref().expect("input required by clap");
    let output = arguments
        .output
        .as_deref()
        .expect("output required by clap");
    let source = fs::read(input).with_context(|| format!("failed to read {}", input.display()))?;
    let page_name = input
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("input")
        .to_owned();

    let mut session = Session::memory().context("failed to create the scene session")?;
    let page = {
        let mut edit = session.edit();
        let page = edit
            .add_page(page_name, source)
            .context("failed to import the input image")?;
        edit.commit().context("failed to commit the input page")?;
        page
    };

    let target_language = arguments.target_language.clone();
    let pipeline = Pipeline::new(
        Config::memory(arguments.pipeline_config()),
        Config::memory(arguments.translation_config()),
    );
    let report = pipeline
        .run(&mut session)
        .pages([page])
        .events(Arc::new(|event| {
            if let PipelineEvent::Progress(progress) = event {
                eprintln!(
                    "[{}/{}] {}: {}",
                    progress.completed, progress.total, progress.phase, progress.model
                );
            }
        }))
        .execute()
        .await
        .context("pipeline failed")?;
    pipeline.unload_all().await?;
    apply_font_families(&mut session, page, &arguments.font_families)?;

    let page = session.page(page)?;
    let base = page.assets.clean.unwrap_or(page.source);
    let base = image::load_from_memory(&session.read_blob(base)?)?;
    let renderer = Renderer::new().context("failed to initialize koharu-renderer")?;
    let rendered = renderer.composite_page(
        &base,
        page,
        |blob| session.read_blob(blob).map_err(Into::into),
        &PageRenderOptions {
            target_language: Some(target_language),
            ..PageRenderOptions::default()
        },
    )?;
    rendered
        .image
        .save(output)
        .with_context(|| format!("failed to write {}", output.display()))?;
    eprintln!(
        "rendered {} after {} processors and {} committed revisions",
        output.display(),
        report.processors,
        report.revisions.len()
    );
    Ok(())
}

fn apply_font_families(session: &mut Session, page: PageId, families: &[String]) -> Result<()> {
    if families.is_empty() {
        return Ok(());
    }
    let styles = session
        .page(page)?
        .texts()
        .map(|(element, text)| {
            let mut style = text.style.clone();
            style.font_families = families.to_vec();
            (element.id, style)
        })
        .collect::<Vec<_>>();
    let mut commands = session.commands();
    for (element, style) in styles {
        commands.push(Command::EditElement {
            page,
            element,
            edit: ElementChange::Style(style),
        });
    }
    if !commands.as_slice().is_empty() {
        session
            .apply(commands)
            .context("failed to apply CLI font families")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_flags_select_models_with_default_configs() {
        let arguments = Arguments::try_parse_from([
            "run",
            "--input",
            "input.png",
            "--output",
            "output.png",
            "--ocr",
            "manga-ocr",
            "--font-family",
            "Noto Sans",
            "--font-family",
            "Noto Serif",
            "--inpainting",
            "flux2-klein",
        ])
        .unwrap();
        let config = arguments.pipeline_config();

        assert!(matches!(
            config.detection,
            DetectionModel::KoharuLayoutRFDetrSeg2XL(config)
                if config == KoharuLayoutRFDetrSeg2XLConfig::default()
        ));
        assert!(matches!(config.ocr, OcrModel::MangaOcr(_)));
        assert!(matches!(
            config.typography,
            TypographyModel::FontDetector(config) if config == FontDetectorConfig::default()
        ));
        assert_eq!(arguments.font_families, ["Noto Sans", "Noto Serif"]);
        assert!(matches!(
            config.inpainting,
            InpaintingModel::Flux2Klein(config)
                if config == Flux2KleinConfig::default()
        ));
        assert!(matches!(
            arguments.translation_config().model,
            Providers::Local(config) if config.model == "gemma4-12b-it"
        ));
    }

    #[test]
    fn worker_mode_does_not_require_input_or_output() {
        let arguments = Arguments::try_parse_from(["run", "--worker"]).unwrap();

        assert!(arguments.worker);
        assert!(arguments.input.is_none());
        assert!(arguments.output.is_none());
    }
}
