# koharu-pipeline design

This document describes the architecture of `koharu-pipeline` and acts as its
design contract.

`koharu-pipeline` coordinates model-backed work against a
`koharu_scene::Session`. It decides what must run, captures immutable scene
snapshots, executes reusable workers, and commits their commands.

The crate must remain an orchestrator. Model architecture and preprocessing
belong in `koharu-ml`, translation providers belong in `koharu-translator`, and
scene mutation belongs in `koharu-scene`.

## Goals

- Expose a small configuration with one model choice per configurable phase.
- Make the execution graph fixed and easy to inspect.
- Keep translation inside the pipeline while leaving provider ownership in
  `koharu-translator`.
- Always configure font detection as the sole model for the required
  typography phase.
- Represent the fixed processor dependencies directly instead of deriving them
  from input/output declarations.
- Load models lazily and reuse them across runs.
- Commit each execution wave as one scene transaction.

## Public phases

The pipeline has five user-addressable phases:

```rust
#[derive(strum::Display, strum::EnumIter, strum::EnumString)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum Phase {
    Detection,
    Ocr,
    Translation,
    Typography,
    Inpainting,
}
```

Font detection is the only processor in the typography phase. It is an
explicit, required configuration slot and typography can be targeted like any
other phase.

Phases are run targets and progress labels. The fixed processor topology
defines dependencies.

`strum` derives iteration, display, and parsing from this enum. The crate must
not maintain a separate `Phase::ALL` array or hand-written `Display` and
`FromStr` match trees.

## Configuration boundary

Detection, OCR, typography, and inpainting are required model-selection slots:

```rust
pub struct PipelineConfig {
    pub detection: DetectionModel,
    pub ocr: OcrModel,
    pub typography: TypographyModel,
    pub inpainting: InpaintingModel,
}
```

Detection has exactly one supported processor:

```rust
pub enum DetectionModel {
    KoharuLayoutRFDetrSeg2XL(KoharuLayoutRFDetrSeg2XLConfig),
}
```

Typography also has exactly one supported processor:

```rust
pub enum TypographyModel {
    FontDetector(FontDetectorConfig),
}
```

Translation configuration stays in `koharu-translator`. `Pipeline` receives
both live configuration handles because it owns translation scheduling and
calls the translator:

```rust
pub fn new(
    pipeline: Config<PipelineConfig>,
    translation: Config<koharu_translator::TranslationConfig>,
) -> Pipeline;
```

```toml
[pipeline.detection]
model = "koharu-layout-rfdetr-seg-2xl"
# Optional per-class overrides; omitted values use the checkpoint recommendations.
text_threshold = 0.25
onomatopoeia_threshold = 0.35

[pipeline.ocr]
model = "paddleocr-vl-1.6"

[pipeline.typography]
model = "font-detector"
top_k = 3

[pipeline.inpainting]
model = "lama"
```

Model configuration follows the backend's actual inference controls. RF-DETR
supports optional per-class confidence thresholds. LaMa exposes its HD
strategy, AOT exposes its input-size limit, and prompt-driven FLUX.2 Klein and
RORem expose prompts and sampling controls. OCR models remain empty configs
because their current inference APIs have no caller-selectable options.

All four slots are required in the runtime type. `PipelineConfig::default`
selects Koharu Layout RF-DETR Seg 2XL, PaddleOCR-VL, FontDetector, and LaMa.
Missing tables use those defaults so configurations from the previous pipeline
schema can still load; unknown legacy fields are ignored. Translation remains
required through its separate `koharu-translator::TranslationConfig` handle.

## Fixed topology

The graph is assembled from five required node roles. Pipeline configuration
selects each model implementation without adding or removing graph nodes.

```text
                                  +--> OCR --> Translation
                                  |
Source image --> Detection -------+--> Font detector (Typography)
                  |               |
                  +---------------+--> Inpainting
```

The topology has four explicit dependency edges:

| Dependency | Reason |
| --- | --- |
| Detection -> OCR | OCR operates on detected text blocks. |
| OCR -> Translation | Translation operates on recognized text. |
| Detection -> Typography | Font detection operates on detected text blocks. |
| Detection -> Inpainting | Inpainting uses masks produced by detection. |

Processors read the scene data they need directly from `Context` and return
scene commands. The planner does not model those values as input or output
ports.

`KoharuLayoutRFDetrSeg2XL` replaces the former collection of layout, text,
speech-bubble, onomatopoeia, and mask-fusion processors. Its single inference
produces all detection regions and final masks. The following processors are
not part of this design:

- `comic_layout_yolo26s`
- `comic_onomatopoeia`
- `comic_text_detector`
- `manga_text_mask`
- `mask_fusion`
- `pp_doclayout_v3`
- `speech_bubble_yolo11n`
- `speech_bubble_yolov8m`

## Typography and font detection

The required typography slot always adds a font detector to the constructed
plan. FontDetector is the sole processor for `Phase::Typography`, so the slot
cannot be disabled or switched to another implementation.

`RunTarget::All` includes font detection automatically. Targeting the
typography phase selects it and its detection ancestor.

Font detection can run in the same topological wave as OCR and inpainting after
detection. Accelerator access still serializes GPU-backed nodes when the target
device requires it.

## Internal model representation

The crate should have one internal enum representing executable nodes:

```rust
enum ConfiguredNode {
    Detection(DetectionModel),
    Ocr(OcrModel),
    Translation(koharu_translator::Providers),
    Typography(TypographyModel),
    Inpainting(InpaintingModel),
}
```

Each variant maps once to an immutable descriptor:

```rust
struct NodeSpec {
    id: ProcessorId,
    name: &'static str,
    phase: Phase,
    runtime: ModelRuntime,
    supports_element_scope: bool,
}
```

`TypographyModel::FontDetector` uses `phase: Phase::Typography`. Every
`PipelineConfig` slot contributes exactly one node, and the translation config
contributes the fifth.

The descriptor is the single source of truth for planning, scheduling,
progress reporting, and worker runtime selection. Processor adapters are
responsible for loading a backend and turning a `Context` into scene commands.

## Graph representation

Planning uses `petgraph::stable_graph::StableDiGraph<PlanNode, ()>`. Nodes hold
configured processors and unit edges represent the four fixed dependencies.
Stable node indices make execution reports and worker lookups deterministic
while a selected plan is pruned.

After all nodes and fixed edges are present, `petgraph::algo::toposort`
validates acyclicity. Topological waves are computed from that order by
assigning each node one plus the maximum depth of its incoming neighbors.

Selection uses graph traversal instead of parallel vectors for dependencies,
bindings, targets, and remapped indices. Starting from target node indices, a
reverse traversal retains every ancestor. The selected plan is an induced
subgraph with the original stable ordering preserved.

## Planning

`Plan::build` performs four small operations:

1. Convert live settings into `ConfiguredNode` values.
2. Add the translation node from `TranslationConfig`.
3. Add the fixed dependency edges between processor roles.
4. Compute topological waves.

Target selection marks the requested phase or processor and retains its graph
ancestors. `RunTarget::All` includes all five required nodes. Each processor
reports missing scene data when it reads its `Context`.

The planner does not contain model-specific match trees beyond construction of
`ConfiguredNode`. It does not model data ports, ordered writers, multi-phase
nodes, or artifact targets.

## Supporting crates

The redesign adds two focused dependencies:

```toml
petgraph = { workspace = true }
strum = { workspace = true, features = ["derive"] }
```

- `strum` owns `Phase` iteration, display, and parsing.
- `petgraph` owns graph storage, traversal, cycle detection, and topological
  ordering.

These dependencies replace bespoke enum match trees and custom graph/remapping
code; they must not become generic abstraction layers around model execution.

## Execution

Execution proceeds in topological waves:

1. Read both live configurations once at the start of the run.
2. Build and select an immutable plan.
3. Capture one immutable scene snapshot and one shared-memory arena per wave.
4. Lazily create or reuse each selected worker.
5. Run independent nodes concurrently; guard only accelerator-backed work.
6. Merge command batches in stable plan order.
7. Apply the merged batch as one `koharu-scene` revision per wave.

Remote translation has no accelerator runtime and may overlap independent
model work. Local translation uses the llama runtime. Torch and diffusion
workers share the device guard unless a backend is proven safe to run
concurrently on the target device.

Workers are keyed by stable `ProcessorId` plus their serialized configuration.
A configuration change unloads only the affected worker. Unchanged workers are
reused across page and phase runs.

## Scene command boundary

Workers receive an immutable `Context` and return `koharu_scene::Commands`.
They never mutate `Session` directly.

`koharu-pipeline` does not validate command ownership. Built-in processors are
trusted components. The executor merges their batches in stable plan order and
delegates structural command checking and atomic application to
`koharu-scene`.

If a worker fails or `koharu-scene` rejects the merged batch, that wave is not
committed. Revisions from earlier completed waves remain available through
normal scene history.

## Module layout

The implementation should be split by responsibility:

```text
src/
  lib.rs          public types and the Pipeline facade
  config.rs       public phase model configuration
  node.rs         ConfiguredNode and NodeSpec
  plan.rs         petgraph construction, target selection, and waves
  execute.rs      scheduling, worker reuse, and scene commits
  context.rs      immutable worker scene snapshots
  run.rs          run builder and reports
  events.rs       progress events
  worker/         process protocol and worker lifecycle
  builtin/        thin model/translator adapters
```

`lib.rs` should not contain model match trees. `plan.rs` should not load models
or inspect scene commands. `builtin` adapters should not know how target
selection works.

## Invariants

The redesign is complete only when these invariants hold:

- The public phase set is detection, OCR, translation, typography, and
  inpainting.
- `PipelineConfig` contains required detection, OCR, typography, and inpainting
  model slots.
- Koharu Layout RF-DETR Seg 2XL is the only detection model.
- Translation is executed by `koharu-pipeline` through `koharu-translator`.
- Font detection is the only supported typography model and cannot be
  disabled.
- `strum` is the single source for phase iteration, display, and parsing.
- `petgraph` is the graph representation used for planning and traversal.
- Node metadata is declared once.
- Nodes do not declare input or output artifacts.
- Dependencies are the four explicit edges in the fixed graph.
- Workers cannot mutate a session directly.
- A wave is merged deterministically and committed atomically by
  `koharu-scene`; the pipeline performs no command validation.
