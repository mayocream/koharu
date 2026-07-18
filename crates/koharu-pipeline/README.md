# koharu-pipeline

`koharu-pipeline` runs Koharu's models against a `koharu_scene::Session`. It
selects configured models, schedules their dependencies, and commits their
results as scene commands.

This is a greenfield design. The existing implementation is not an API or data
compatibility constraint.

## Boundary

- `koharu-scene` owns projects, text, images, masks, commands, history, blobs,
  and SQLite.
- `koharu-ml` and `koharu-translator` provide inference implementations.
- `koharu-config` owns live configuration and secrets.
- `koharu-renderer` and the canvas render the scene. Rendering is not a
  pipeline stage.

The pipeline owns only model adapters, scheduling, model lifetime, progress,
and cancellation. It has no document model, storage layer, renderer, generic
artifact system, or second mutation language.

The central invariant is:

> A processor reads one immutable scene revision and returns
> `koharu_scene::Commands`. Only the executor applies commands.

## Stages and scheduling

Stages are fixed UI, configuration, and reporting names:

```rust
pub enum Stage {
    Detection,
    Segmentation,
    Ocr,
    Translation,
    Typography,
    Inpainting,
}
```

| Stage | Durable scene result |
| --- | --- |
| Detection | text elements and frames |
| Segmentation | text or bubble page masks |
| OCR | `TextBlock::source` |
| Translation | `TextBlock::translation` |
| Typography | `TextStyle` and `TextLayout` |
| Inpainting | clean page image |

Koharu derives a small DAG directly from the outputs and dependencies declared
for each configured model. There is no user-defined graph and no general graph
library. A typical plan is:

```text
source -> detection -> OCR -> translation -> typography
              |                              ^
              +-> text segmentation --------+
                         +-> inpainting
source -> bubble segmentation ---------------+
```

Users select one model for each stage; they never define step IDs, edges,
`after`, inputs, or outputs. The planner maps each result to its single
configured producer, walks dependencies for `through(stage)`, and groups nodes
by dependency depth. Two selected models that write the same scene result are
rejected before weights are loaded.

## Configuration

Each stage has one typed model enum and a built-in default selection.
The default pipeline uses PP-DocLayoutV3, Manga Text Segmentation,
PaddleOCR-VL 1.6, the `lfm2.5-1.2b-instruct` local translator, Font Detector, and
LaMa.

```rust
pub struct PipelineConfig {
    pub detection: DetectionModel,
    pub segmentation: SegmentationModel,
    pub ocr: OcrModel,
    pub translation: TranslationModel,
    pub typography: TypographyModel,
    pub inpainting: InpaintingModel,
}

#[serde(tag = "model", rename_all = "snake_case")]
pub enum DetectionModel {
    ComicTextDetector(ComicTextDetectorConfig),
    PPDocLayoutV3(PPDocLayoutV3Config),
}

pub struct PPDocLayoutV3Config {
    pub confidence: f32,
}
```

This keeps model-only options beside that model and makes invalid stage/model
combinations unrepresentable. Device and concurrency choices are automatic.

```toml
[pipeline.detection]
model = "pp_doclayout_v3"
confidence = 0.35

[pipeline.ocr]
model = "paddleocr_vl_1.6"

[pipeline.translation]
model = "openai"
remote_model = "gpt-4.1-mini"

[pipeline.inpainting]
model = "lama"
```

`Pipeline` owns the live `Config<PipelineConfig>` returned by
`koharu_config::load("pipeline")`. A run snapshots the latest value, so a
configuration change affects the next run but cannot change an active run.
Secrets come from `koharu_config::secrets()` and are not fields in this config.

Translation supports every backend in `koharu-translator`: every catalogued
local GGUF model, OpenAI, Gemini, Claude, DeepSeek, OpenAI-compatible servers,
DeepL, Google Cloud Translation, and Caiyun. For example:

```toml
[pipeline.translation]
model = "local"
local_model = "lfm2.5-1.2b-instruct"
```

Or select one hosted backend instead:

```toml
[pipeline.translation]
model = "openai_compatible"
base_url = "http://localhost:1234/v1"
remote_model = "local-model"
```

Only one translation model may be active because translation is one durable
scene result. Remote processors reuse their HTTP client but read the current
credential before every run. Local translators follow the normal lazy
load/unload lifecycle.

## Processor contract

Every selectable model has one adapter in this crate:

```rust
#[async_trait::async_trait]
pub trait Processor: Send {
    fn name(&self) -> &'static str;
    fn stage(&self) -> Stage;
    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands>;
}
```

`Context` is immutable and has private fields. It provides the revision,
scope, selected scene data, required image bytes, a run-local lazy decode
cache, and cancellation. `context.commands()` creates a command batch at the
correct base revision.

An adapter owns its loaded model and converts model-specific results into scene
values. The executor adds cross-stage invalidations before committing: for
example, new OCR text clears stale translation, and a new text mask clears the
stale clean image. A processor never receives a `Session`, accesses SQLite, or
commits its own output.

## Execution API

```rust
pub enum Scope {
    Project,
    Pages(Vec<PageId>),
    Region { page: PageId, frame: Frame },
    Elements(Vec<ElementId>),
}
```

The executor validates that inputs and emitted commands stay within the scope.
Regions use page coordinates.

```rust
let report = pipeline
    .run(&mut session)
    .pages([page_1, page_2])
    .through(Stage::Translation)
    .target_language("en")
    .execute()
    .await?;
```

The default is the whole project and every configured stage.
`through(stage)` includes its required ancestors; `only(stage)` uses existing
scene inputs. The fluent API only builds a `RunRequest`.

Execution proceeds in topological waves:

1. Capture the revision and scoped inputs.
2. Run all ready processors concurrently.
3. Sort their batches by stable plan order and merge them.
4. Apply one scene transaction.
5. Read the committed scene for the next wave.

Completion order never determines output order. A processor error, cancellation,
or `Commands::merge` conflict discards the current wave. A concurrent scene
edit produces `RevisionConflict`; the pipeline never silently rebases stale
model output. Earlier waves remain committed and their revisions are returned
for undo.

## Model lifetime and performance

Processors load lazily and are reused while their typed configuration is
unchanged. Changed or removed processors are dropped before the next run.

```rust
pipeline.load(Stage::Detection).await?;   // optional warm-up
pipeline.unload(Stage::Detection).await?;
pipeline.unload_all().await?;
```

Dropping a processor releases native and GPU resources through RAII. One model
instance is not used concurrently unless its implementation supports it;
different ready models run in parallel when their resources allow it. GPU
inference is serialized by default to avoid oversubscription. Synchronous
native inference runs outside async runtime workers.

Performance requirements:

- capture scoped metadata once per wave and share it with `Arc`;
- read each blob once per run and cache decoded images by `BlobId`;
- never hold a SQLite transaction during inference;
- batch pages, crops, OCR, or translation when a model benefits;
- choose devices and safe concurrency from available hardware;
- keep command order, errors, and progress deterministic.

There is no global registry, `inventory`, LRU, model lease, or cross-run output
cache. Add those only if profiling demonstrates a need.

## Adding and testing a model

Each adapter is one self-contained file under `src/builtin/`;
`builtin/mod.rs` contains only factory wiring. Adding a model requires one
stage-enum variant, one typed config, one processor file, and one
factory/dependency match arm. Tests inject fake processors and use
`koharu_scene::Session::memory`.
