use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use koharu_llm::paddleocr_vl::{
    DEFAULT_REPETITION_PENALTY, PaddleOcrVl, PaddleOcrVlGenerateOptions, PaddleOcrVlOutput,
    PaddleOcrVlTask,
};
use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TaskArg {
    Ocr,
    Table,
    Formula,
    Chart,
    Spotting,
    Seal,
}

impl From<TaskArg> for PaddleOcrVlTask {
    fn from(value: TaskArg) -> Self {
        match value {
            TaskArg::Ocr => Self::Ocr,
            TaskArg::Table => Self::Table,
            TaskArg::Formula => Self::Formula,
            TaskArg::Chart => Self::Chart,
            TaskArg::Spotting => Self::Spotting,
            TaskArg::Seal => Self::Seal,
        }
    }
}

#[derive(Parser)]
struct Cli {
    #[arg(long, value_name = "FILE", num_args = 1..)]
    input: Vec<PathBuf>,

    #[arg(long, value_name = "DIR")]
    model_dir: Option<PathBuf>,

    #[arg(long, value_enum, default_value = "ocr")]
    task: TaskArg,

    #[arg(long, default_value_t = 128)]
    max_new_tokens: usize,

    #[arg(long, default_value_t = DEFAULT_REPETITION_PENALTY)]
    repetition_penalty: f32,

    #[arg(long, value_name = "FILE")]
    json_output: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    cpu: bool,

    #[arg(long, value_name = "DIR")]
    dataset_root: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    ground_truth: Option<PathBuf>,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(long, default_value_t = 0)]
    offset: usize,

    #[arg(long, default_value_t = 10)]
    sample_errors: usize,
}

impl Cli {
    fn generate_options(&self) -> PaddleOcrVlGenerateOptions {
        PaddleOcrVlGenerateOptions {
            max_new_tokens: self.max_new_tokens,
            repetition_penalty: self.repetition_penalty,
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OutputEnvelope {
    outputs: Vec<PaddleOcrVlOutput>,
}

#[derive(Debug, Clone)]
struct GroundTruthRecord {
    relative_path: PathBuf,
    text: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EvaluationError {
    image: String,
    expected: String,
    predicted: String,
    char_distance: usize,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EvaluationSummary {
    dataset_root: String,
    ground_truth: String,
    evaluated_examples: usize,
    exact_matches: usize,
    exact_match_rate: f64,
    total_reference_chars: usize,
    total_char_distance: usize,
    cer: f64,
    elapsed_seconds: f64,
    sample_errors: Vec<EvaluationError>,
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let runtime = RuntimeManager::new(
        default_app_data_root(),
        if cli.cpu {
            ComputePolicy::CpuOnly
        } else {
            ComputePolicy::PreferGpu
        },
    )?;
    runtime
        .prepare()
        .await
        .context("failed to initialize runtime libraries")?;
    koharu_llm::sys::initialize(&runtime)
        .context("failed to initialize llama.cpp runtime bindings")?;
    let task: PaddleOcrVlTask = cli.task.into();
    let backend = Arc::new(LlamaBackend::init().context("unable to initialize llama.cpp backend")?);

    if cli.input.is_empty() && cli.dataset_root.is_none() {
        bail!("provide either --input or --dataset-root");
    }

    let mut model = if let Some(model_dir) = &cli.model_dir {
        PaddleOcrVl::load_from_dir(&runtime, model_dir, cli.cpu, Arc::clone(&backend))?
    } else {
        PaddleOcrVl::load(&runtime, cli.cpu, backend).await?
    };
    let generate_options = cli.generate_options();

    if let Some(dataset_root) = &cli.dataset_root {
        let summary = evaluate_dataset(&mut model, dataset_root, &cli, task, &generate_options)?;
        println!("{}", serde_json::to_string_pretty(&summary)?);
        if let Some(path) = &cli.json_output {
            std::fs::write(path, serde_json::to_string_pretty(&summary)?)?;
        }
        return Ok(());
    }

    let images = cli
        .input
        .iter()
        .map(image::open)
        .collect::<Result<Vec<_>, _>>()?;
    let outputs = model.inference_images_with_options(&images, task, &generate_options)?;
    for (input, output) in cli.input.iter().zip(&outputs) {
        if cli.input.len() > 1 {
            println!("== {} ==", input.display());
        }
        println!("{}", output.text);
    }

    if let Some(path) = &cli.json_output {
        std::fs::write(
            path,
            serde_json::to_string_pretty(&OutputEnvelope { outputs })?,
        )?;
    }

    Ok(())
}

fn evaluate_dataset(
    model: &mut PaddleOcrVl,
    dataset_root: &Path,
    cli: &Cli,
    task: PaddleOcrVlTask,
    generate_options: &PaddleOcrVlGenerateOptions,
) -> Result<EvaluationSummary> {
    let started = Instant::now();
    let split_name = dataset_root
        .file_name()
        .and_then(|name| name.to_str())
        .context("dataset root must end with a split directory like `train`")?;
    let ground_truth = cli
        .ground_truth
        .clone()
        .unwrap_or_else(|| dataset_root.with_file_name(format!("rec_gt_{split_name}.txt")));
    let dataset_base = dataset_root
        .parent()
        .and_then(Path::parent)
        .context("dataset root must look like `<base>/rec/<split>`")?;

    let records = read_ground_truth(&ground_truth)?;
    let selected = records
        .into_iter()
        .skip(cli.offset)
        .take(cli.limit.unwrap_or(usize::MAX))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        bail!("no evaluation records selected");
    }

    let mut exact_matches = 0usize;
    let mut total_reference_chars = 0usize;
    let mut total_char_distance = 0usize;
    let mut sample_errors = Vec::new();

    for (index, record) in selected.iter().enumerate() {
        let image_path = dataset_base.join(&record.relative_path);
        let image = image::open(&image_path)
            .with_context(|| format!("failed to open `{}`", image_path.display()))?;
        let output = model.inference_with_options(&image, task, generate_options)?;
        let predicted = output.text;
        let expected = record.text.as_str();
        let char_distance = levenshtein_chars(expected, &predicted);
        let reference_chars = expected.chars().count();

        total_reference_chars += reference_chars;
        total_char_distance += char_distance;
        if predicted == expected {
            exact_matches += 1;
        } else if sample_errors.len() < cli.sample_errors {
            sample_errors.push(EvaluationError {
                image: image_path.display().to_string(),
                expected: expected.to_string(),
                predicted,
                char_distance,
            });
        }

        if (index + 1) % 50 == 0 || index + 1 == selected.len() {
            let cer = ratio(total_char_distance, total_reference_chars);
            let exact_match_rate = ratio(exact_matches, index + 1);
            eprintln!(
                "evaluated {}/{}  exact={:.4}  cer={:.4}",
                index + 1,
                selected.len(),
                exact_match_rate,
                cer
            );
        }
    }

    Ok(EvaluationSummary {
        dataset_root: dataset_root.display().to_string(),
        ground_truth: ground_truth.display().to_string(),
        evaluated_examples: selected.len(),
        exact_matches,
        exact_match_rate: ratio(exact_matches, selected.len()),
        total_reference_chars,
        total_char_distance,
        cer: ratio(total_char_distance, total_reference_chars),
        elapsed_seconds: started.elapsed().as_secs_f64(),
        sample_errors,
    })
}

fn read_ground_truth(path: &Path) -> Result<Vec<GroundTruthRecord>> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read ground-truth file `{}`", path.display()))?;
    let (text, _, had_errors) = encoding_rs::UTF_8.decode(&bytes);
    if had_errors {
        bail!("ground-truth file `{}` is not valid UTF-8", path.display());
    }

    let mut records = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if let Some((relative_path, value)) = line.split_once('\t') {
            records.push(GroundTruthRecord {
                relative_path: PathBuf::from(relative_path),
                text: value.to_string(),
            });
            continue;
        }

        let previous = records.last_mut().with_context(|| {
            format!("invalid ground-truth line without a preceding record: `{line}`")
        })?;
        previous.text.push('\n');
        previous.text.push_str(line);
    }

    Ok(records)
}

fn levenshtein_chars(expected: &str, predicted: &str) -> usize {
    let expected = expected.chars().collect::<Vec<_>>();
    let predicted = predicted.chars().collect::<Vec<_>>();

    if expected.is_empty() {
        return predicted.len();
    }
    if predicted.is_empty() {
        return expected.len();
    }

    let mut prev = (0..=predicted.len()).collect::<Vec<_>>();
    let mut curr = vec![0usize; predicted.len() + 1];

    for (i, expected_char) in expected.iter().enumerate() {
        curr[0] = i + 1;
        for (j, predicted_char) in predicted.iter().enumerate() {
            let substitution_cost = usize::from(expected_char != predicted_char);
            curr[j + 1] = (prev[j + 1] + 1)
                .min(curr[j] + 1)
                .min(prev[j] + substitution_cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[predicted.len()]
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}
