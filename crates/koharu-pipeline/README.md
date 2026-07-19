# koharu-pipeline

`koharu-pipeline` runs Koharu processors against a `koharu_scene::Session`. It
constructs a typed data-flow graph, executes the requested portion, and commits
processor results as scene commands.

## Boundary

- `koharu-scene` owns projects, elements, masks, commands, history, blobs, and SQLite.
- `koharu-ml` and `koharu-translator` provide inference implementations.
- `koharu-config` owns live configuration handles.
- `koharu-renderer` and the canvas render the scene.

The central invariant is:

> A processor reads one immutable scene revision and returns
> `koharu_scene::Commands`. Only the executor applies commands.

## Phases and artifacts

A phase is presentation metadata used by the UI, progress events, and phase
run buttons. It does not define execution order.

```rust
pub enum Phase {
    Detection,
    Segmentation,
    Ocr,
    Translation,
    Typography,
    Inpainting,
}
```

Dependencies come exclusively from typed artifacts:

```rust
pub enum Artifact {
    SourceImage,
    PanelRegion,
    BubbleRegion,
    TextRegion,
    CooRegion,
    TextMaskCandidate,
    LayoutTextMask,
    TextMask,
    CooMask,
    BrushMask,
    BubbleMask,
    SourceText,
    CooText,
    Translation,
    Typography,
    CleanImage,
}
```

`ComicLayoutYolo26s`, for example, emits panel, bubble, and text regions plus
layout and bubble masks. `ComicOnomatopoeia` separately detects the COO branch
and recognizes its text. `MaskFusion` then separates the pixel-level
candidate into `TextMask` and `CooMask`. One inference remains one DAG node even
when it contributes artifacts to multiple phases.

Configuration is an ordered list of named processors. Multiple processors may
contribute to one phase, and one processor may contribute to multiple phases.
Processors that write the same artifact form an ordered writer chain, and
consumers bind to the latest configured producer. Write ordering is not treated
as an input dependency.

## Configuration

The processor list is explicit and does not use one mutually exclusive model
slot per phase:

```rust
pub struct PipelineConfig {
    pub processors: Vec<ProcessorConfig>,
}
```

```toml
[[pipeline.processors]]
model = "pp_doclayout_v3"
confidence = 0.25

[[pipeline.processors]]
model = "comic_layout_yolo26s"

[[pipeline.processors]]
model = "manga_text_mask"

[[pipeline.processors]]
model = "comic_onomatopoeia"

[[pipeline.processors]]
model = "mask_fusion"

[[pipeline.processors]]
model = "paddleocr_vl_1.6"

[[pipeline.processors]]
model = "rorem_mixed"
resolution = 1024
mask_dilation = 20
```

To have the dedicated YOLO11n segmenter produce the final bubble mask, add it
after `comic_layout_yolo26s`:

```toml
[[pipeline.processors]]
model = "speech_bubble_yolo11n"
# confidence = 0.25
# nms_iou = 0.7
```

Translation configuration remains in `koharu-translator` because it also owns
target language, instructions, providers, and credential lookup.

## Processor contract

Every configured processor declares the same contract used by the planner and
the command validator:

```rust
#[async_trait::async_trait]
pub trait Processor: Send {
    fn name(&self) -> &'static str;
    fn inputs(&self) -> &'static [Artifact];
    fn outputs(&self) -> &'static [Artifact];
    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands>;
}
```

`Context` is immutable. It provides the revision, scope, selected scene data,
encoded and lazily decoded images, cancellation, and events. Each model runs in
an isolated reusable worker process. The parent validates the declared
contract, merges one topological wave, and performs the scene commit.

## Run targets

Runs target the whole graph, one phase, processor IDs, or requested artifacts:

```rust
pub enum RunTarget {
    All,
    Phase { phase: Phase },
    Processors { processors: Vec<ProcessorId> },
    Artifacts { artifacts: Vec<Artifact> },
}

pub enum Force {
    None,
    Targets,
    All,
}
```

Selecting a phase means “run processors displayed in this phase and ensure
their prerequisites.” It does not mean “run every numerically earlier phase.”

```rust
let report = pipeline
    .run(&mut session)
    .pages([page_1, page_2])
    .phase(Phase::Translation)
    .execute()
    .await?;
```

`Force::Targets` is the interactive default: clicking OCR reruns OCR while
fresh detection dependencies are skipped. `Force::None` ensures cached output,
and `Force::All` recomputes the complete dependency closure.

## Freshness

Output existence alone is not a cache hit. The pipeline records, per project,
scope, processor, and output port:

- processor implementation and typed configuration;
- translation target and instructions where applicable;
- fingerprints of declared input artifacts;
- fingerprints of each emitted output artifact.

Changing an input or manually editing an output makes the corresponding node
stale. Output ports are checked independently, so replacing a candidate mask
does not invalidate its producer's still-current text regions. The cache is
conservative and process-local: reopening the application safely recomputes
outputs instead of trusting provenance it cannot verify.

## Execution

Execution proceeds in topological waves:

1. Resolve the requested targets and their dependency closure.
2. Prune fresh processors according to `Force`.
3. Capture scoped inputs into a shared-memory arena.
4. Run ready workers concurrently where device constraints allow it.
5. Validate and merge command batches in stable plan order.
6. Commit one scene revision and fingerprint the emitted output ports.

Scope is part of freshness. Project, page, region, and element runs therefore
cannot accidentally reuse results computed for a different scope. Earlier
committed waves remain available for undo if a later wave fails.

## Adding a processor

Add its typed configuration and adapter under `src/builtin/`, assign a stable
`ProcessorId`, declare phase/input/output contracts in both the configured
model descriptor and processor, and wire its factory arm. Startup validation
rejects contract drift before inference runs.
