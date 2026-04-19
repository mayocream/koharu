//! One-shot pipeline CLI.
//!
//! Runs the full engine chain (or a custom subset) on a single image and
//! dumps every intermediate artifact to an output directory. Reuses the
//! production `pipeline::run` driver — same code path the HTTP server
//! takes — so renderer / engine regressions surface identically here.
//!
//! ## Quick-start
//!
//! ```text
//! cargo run --features bin -p koharu-app --bin pipeline -- \
//!     --input sample.png \
//!     --output-dir out/
//! ```
//!
//! By default the LLM translate step is skipped (it would need a local
//! model loaded). When translate is skipped we copy OCR text into the
//! translation slot so the renderer still has something to rasterise.
//!
//! To run the translate step end-to-end, preload a local model:
//! `--with-translate --llm <modelId> --target-lang en`.
//!
//! ## Output files
//!
//! For every role-keyed image/mask that lands on the page:
//!
//! - `source.png`, `inpainted.png`, `rendered.png`
//! - `segment.png`, `bubble.png` (only if the engine produced them)
//! - `brush.png` (if the user painted anything — unusual from CLI)
//! - `scene.json` — the final Scene snapshot (useful for diffing).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, anyhow};
use camino::Utf8PathBuf;
use clap::Parser;
use image::{DynamicImage, GenericImageView};
use koharu_app::{App, AppConfig};
use koharu_core::{
    ImageData, ImageRole, MaskRole, Node, NodeId, NodeKind, Op, Page, PageId, Transform,
};
use koharu_runtime::{ComputePolicy, RuntimeHttpConfig, RuntimeManager};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Parser, Debug)]
#[command(version, about = "Run the Koharu pipeline against a single image")]
struct Cli {
    /// Source image (png / jpg / webp).
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    /// Directory to write intermediate + final artifacts into. Created if missing.
    #[arg(short, long, value_name = "DIR")]
    output_dir: PathBuf,

    /// Optional TOML override for the runtime config. Defaults to the
    /// built-in `AppConfig::default()`.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Override the pipeline step list (comma-separated engine ids).
    /// When omitted we run the engines named in `config.pipeline.*`,
    /// skipping translate unless `--with-translate` is passed.
    #[arg(long, value_name = "IDS", value_delimiter = ',')]
    steps: Option<Vec<String>>,

    /// Target language for the translator engine (ignored when translate is skipped).
    #[arg(long, default_value = "en")]
    target_lang: String,

    /// Custom system prompt for the translator.
    #[arg(long)]
    system_prompt: Option<String>,

    /// Default font family to apply when a block has no detected font.
    #[arg(long)]
    default_font: Option<String>,

    /// Include the llm-translate step. Requires `--llm <id>` to pre-load a
    /// local model, or for the currently-registered translator to accept
    /// provider-backed requests.
    #[arg(long)]
    with_translate: bool,

    /// Pre-load a local LLM before the pipeline runs (e.g. `lfm2.5-1.2b-instruct`).
    #[arg(long, value_name = "MODEL_ID")]
    llm: Option<String>,

    /// Force CPU-only compute.
    #[arg(long)]
    cpu: bool,
}

fn main() -> Result<()> {
    init_tracing();

    // A generous stack keeps ONNX + large image decoders happy on Windows.
    std::thread::Builder::new()
        .name("koharu-pipeline".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(run())
        })?
        .join()
        .map_err(|_| anyhow!("pipeline worker thread panicked"))?
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::Level::INFO.into())
        .from_env_lossy();
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    std::fs::create_dir_all(&cli.output_dir)
        .with_context(|| format!("create output dir {}", cli.output_dir.display()))?;

    let cfg = load_config(cli.config.as_deref())?;

    // Stage the project + runtime under a fresh tempdir so repeat runs
    // never collide. TempDir cleans up automatically when the CLI exits.
    let temp_root = env!("CARGO_MANIFEST_DIR")
        .parse::<Utf8PathBuf>()
        .expect("manifest dir not UTF-8")
        .join(".cache");

    let mut cfg = cfg;
    cfg.data.path = temp_root.join("data");
    std::fs::create_dir_all(cfg.data.path.as_std_path()).context("create data dir")?;

    let http = RuntimeHttpConfig {
        connect_timeout_secs: cfg.http.connect_timeout.max(1),
        read_timeout_secs: cfg.http.read_timeout.max(1),
        max_retries: cfg.http.max_retries,
    };
    let compute = if cli.cpu {
        ComputePolicy::CpuOnly
    } else {
        ComputePolicy::PreferGpu
    };
    let runtime = RuntimeManager::new_with_http(cfg.data.path.as_std_path(), compute, http)?;
    runtime
        .prepare()
        .await
        .context("prepare runtime (downloads llama.cpp if missing)")?;

    let app = Arc::new(App::new(cfg, Arc::new(runtime), cli.cpu, "cli")?);
    app.spawn_download_forwarder();
    app.spawn_llm_forwarder();

    // Optional LLM preload so the translate step can reach the model.
    if let Some(model_id) = cli.llm.as_deref() {
        eprintln!("=> loading LLM `{model_id}`");
        app.llm
            .load_from_request(
                koharu_core::LlmLoadRequest {
                    target: koharu_core::LlmTarget {
                        kind: koharu_core::LlmTargetKind::Local,
                        model_id: model_id.to_string(),
                        provider_id: None,
                    },
                    options: None,
                },
                None,
            )
            .await
            .with_context(|| format!("load local llm `{model_id}`"))?;
        // `load_local` is fire-and-forget; poll until it reports Ready.
        wait_for_llm_ready(&app).await?;
    } else if cli.with_translate {
        anyhow::bail!("--with-translate requires --llm <modelId>");
    }

    // Project session + source image.
    let project_dir = tempfile::tempdir()
        .context("create temp project dir")?
        .path()
        .to_string_lossy()
        .parse::<Utf8PathBuf>()
        .context("temp project dir not UTF-8")?;
    let session = app
        .open_project(project_dir, Some("cli".to_string()))
        .await
        .context("open cli project")?;

    let page_id = import_page(&app, &cli.input).context("import source image")?;

    // Pick the step chain.
    let steps = resolve_steps(&cli, &app.config.load())?;
    if steps.is_empty() {
        anyhow::bail!("no steps to run; check --steps or config.pipeline.*");
    }
    eprintln!("=> running steps: {}", steps.join(" → "));

    // Progress sink — one JSON line per tick to stdout. Useful when a step
    // hangs and you want to see which one.
    let progress_sink: koharu_app::pipeline::ProgressSink =
        Arc::new(|tick: koharu_app::pipeline::ProgressTick| {
            let line = serde_json::json!({
                "step_id": tick.step_id,
                "step_index": tick.step_index,
                "total_steps": tick.total_steps,
                "page_index": tick.page_index,
                "total_pages": tick.total_pages,
                "percent": tick.overall_percent,
            });
            println!("{line}");
        });

    let spec = koharu_app::pipeline::PipelineSpec {
        scope: koharu_app::pipeline::Scope::Pages(vec![page_id]),
        steps,
        options: koharu_app::PipelineRunOptions {
            target_language: Some(cli.target_lang.clone()),
            system_prompt: cli.system_prompt.clone(),
            default_font: cli.default_font.clone(),
            region: None,
        },
    };

    // When translate is skipped, copy OCR text into the translation slot so
    // the renderer has something to rasterise.
    let ensure_translation_fallback = !cli.with_translate;

    let cancel = Arc::new(AtomicBool::new(false));
    let result = koharu_app::pipeline::run(
        session.clone(),
        app.registry.clone(),
        app.runtime.clone(),
        app.cpu_only(),
        app.llm.clone(),
        app.renderer.clone(),
        spec,
        cancel,
        Some(progress_sink),
    )
    .await;

    match &result {
        Ok(()) => eprintln!("=> pipeline succeeded"),
        Err(e) => eprintln!("=> pipeline failed: {e:#}"),
    }

    if ensure_translation_fallback && let Err(e) = synthesize_translations(&app, page_id).await {
        eprintln!("warn: failed to synthesize translations: {e:#}");
    }

    dump_artifacts(&session, page_id, &cli.output_dir)
        .with_context(|| format!("dump artifacts to {}", cli.output_dir.display()))?;

    app.close_project().await.ok();
    result
}

/// Load `AppConfig` from TOML at `path` or default.
fn load_config(path: Option<&std::path::Path>) -> Result<AppConfig> {
    match path {
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .with_context(|| format!("read config {}", p.display()))?;
            Ok(toml::from_str(&text)?)
        }
        None => Ok(AppConfig::default()),
    }
}

/// Poll the LLM state every 200 ms until it's ready or fails. Local GGUF
/// loads are seconds to minutes depending on size — this avoids racing the
/// pipeline against a still-loading model.
async fn wait_for_llm_ready(app: &App) -> Result<()> {
    loop {
        let snap = app.llm.snapshot().await;
        match snap.status {
            koharu_core::LlmStateStatus::Ready => return Ok(()),
            koharu_core::LlmStateStatus::Failed => {
                anyhow::bail!("llm load failed");
            }
            _ => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
        }
    }
}

/// Import the source image as a new page + `Image { Source }` node. Mirrors
/// the `POST /pages` handler, minus the multipart plumbing.
fn import_page(app: &App, input: &std::path::Path) -> Result<PageId> {
    let bytes =
        std::fs::read(input).with_context(|| format!("read input image {}", input.display()))?;
    let decoded =
        image::load_from_memory(&bytes).with_context(|| format!("decode {}", input.display()))?;
    let (w, h) = decoded.dimensions();
    let filename = input
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input")
        .to_string();

    let session = app
        .current_session()
        .ok_or_else(|| anyhow!("no session open"))?;
    let blob = session.blobs.put_bytes(&bytes)?;
    let mut page = Page::new(&filename, w, h);
    let page_id = page.id;
    let source_node_id = NodeId::new();
    page.nodes.insert(
        source_node_id,
        Node {
            id: source_node_id,
            transform: Transform::default(),
            visible: true,
            kind: NodeKind::Image(ImageData {
                role: ImageRole::Source,
                blob,
                opacity: 1.0,
                natural_width: w,
                natural_height: h,
                name: Some(filename),
            }),
        },
    );

    app.apply(Op::AddPage { page, at: 0 })?;
    Ok(page_id)
}

/// Compose the step list. Order preference:
/// 1. `--steps a,b,c` — literal, in user-supplied order.
/// 2. Else: engines named in `config.pipeline.*` in the canonical order,
///    with `translator` included only when `--with-translate`.
fn resolve_steps(cli: &Cli, cfg: &AppConfig) -> Result<Vec<String>> {
    if let Some(s) = cli.steps.clone() {
        return Ok(s.into_iter().filter(|s| !s.is_empty()).collect());
    }
    let p = &cfg.pipeline;
    let mut steps: Vec<String> = Vec::new();
    let push = |v: &mut Vec<String>, s: &str| {
        if !s.is_empty() {
            v.push(s.to_string());
        }
    };
    push(&mut steps, &p.detector);
    push(&mut steps, &p.segmenter);
    push(&mut steps, &p.bubble_segmenter);
    push(&mut steps, &p.font_detector);
    push(&mut steps, &p.ocr);
    if cli.with_translate {
        push(&mut steps, &p.translator);
    }
    push(&mut steps, &p.inpainter);
    push(&mut steps, &p.renderer);
    Ok(steps)
}

/// Populate `translation` with `raw` on every text node that's missing
/// one. Lets the renderer produce output when the translate step is
/// skipped — we want to exercise layout + expansion, not the LLM.
async fn synthesize_translations(app: &App, page: PageId) -> Result<()> {
    let session = app
        .current_session()
        .ok_or_else(|| anyhow!("no session open"))?;
    let mut ops = Vec::new();
    {
        let scene = session.scene.read();
        let Some(page_data) = scene.pages.get(&page) else {
            return Ok(());
        };
        for (id, node) in &page_data.nodes {
            if let NodeKind::Text(t) = &node.kind
                && t.translation.is_none()
                && let Some(raw) = t.text.as_ref().filter(|s| !s.is_empty())
            {
                let patch = koharu_core::NodeDataPatch::Text(koharu_core::TextDataPatch {
                    translation: Some(Some(raw.clone())),
                    ..Default::default()
                });
                ops.push(Op::UpdateNode {
                    page,
                    id: *id,
                    patch: koharu_core::NodePatch {
                        data: Some(patch),
                        transform: None,
                        visible: None,
                    },
                    prev: koharu_core::NodePatch::default(),
                });
            }
        }
    }
    if ops.is_empty() {
        return Ok(());
    }
    app.apply(Op::Batch {
        ops,
        label: "synthesize translations".into(),
    })?;
    Ok(())
}

/// Walk the final scene and dump every role-keyed image/mask to disk.
fn dump_artifacts(
    session: &koharu_app::ProjectSession,
    page: PageId,
    out_dir: &std::path::Path,
) -> Result<()> {
    let scene = session.scene.read();
    let page_data = scene
        .pages
        .get(&page)
        .ok_or_else(|| anyhow!("page disappeared from scene"))?;

    for node in page_data.nodes.values() {
        match &node.kind {
            NodeKind::Image(img) => {
                let name = match img.role {
                    ImageRole::Source => "source.png",
                    ImageRole::Inpainted => "inpainted.png",
                    ImageRole::Rendered => "rendered.png",
                    ImageRole::Custom => continue,
                };
                save_blob_image(session, &img.blob, &out_dir.join(name))?;
            }
            NodeKind::Mask(m) => {
                let name = match m.role {
                    MaskRole::Segment => "segment.png",
                    MaskRole::Bubble => "bubble.png",
                    MaskRole::BrushInpaint => "brush.png",
                };
                save_blob_image(session, &m.blob, &out_dir.join(name))?;
            }
            NodeKind::Text(_) => {}
        }
    }

    // Dump the full scene JSON for diffing / inspection.
    let scene_json = serde_json::to_string_pretty(&*scene)?;
    std::fs::write(out_dir.join("scene.json"), scene_json)?;

    eprintln!("=> wrote artifacts to {}", out_dir.display());
    Ok(())
}

fn save_blob_image(
    session: &koharu_app::ProjectSession,
    blob: &koharu_core::BlobRef,
    path: &std::path::Path,
) -> Result<()> {
    let img: DynamicImage = session.blobs.load_image(blob)?;
    img.save(path)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
