# koharu-pipeline

This document is the authoritative design contract for the replacement
`koharu-pipeline`. It describes the target architecture, not the current
implementation.

This is an intentionally incompatible, one-off rewrite. Remove obsolete APIs,
modules, configuration fields, executables, and worker infrastructure instead
of adapting the new design around them.

## Purpose

`koharu-pipeline` runs Koharu's model and translation stages over an immutable
`koharu_scene::SceneSnapshot`. It selects a dependency closure, loads configured
models in the application process, schedules ready stages concurrently, and
returns one validated `koharu_scene::ScenePatch`.

The pipeline does not own a project session and does not persist revisions. The
application commits the returned patch exactly once after the asynchronous run
has finished.

## Decisions

| Concern | Decision |
| --- | --- |
| Process model | All runtimes, model weights, preprocessing, inference, and postprocessing run in the Koharu application process. |
| Runtime startup | The application opens directly into an initialization page and automatically retries the idempotent `koharu_ml::init()` barrier until it succeeds or the app exits. |
| ML surface | `koharu-ml` exposes `init()` and the process-wide `LLAMA_BACKEND` handle. LibTorch and stable-diffusion.cpp runtime handles remain private. |
| Runtime count | Initialize LibTorch, llama.cpp, and stable-diffusion.cpp once during application bootstrap. |
| Model lifecycle | Loading resolves missing assets into the package cache before constructing a model. Loaded models are recycled automatically under memory pressure while cached files remain available. |
| Live configuration | The app may atomically replace pipeline and translation configuration. Existing runs keep their captured generation; new runs use the new one. |
| Scheduling | Use a readiness-driven DAG scheduler. A stage starts when its own prerequisites and resource claims are satisfied. |
| Parallelism | Independent stages run concurrently when current CPU, RAM, and VRAM pressure permits. There is no run-wide lock and no accelerator-wide mutex. |
| Graph | Use `petgraph::graphmap::DiGraphMap<Stage, Dependency>` only for topology, validation, selection, and traversal. |
| Scene input | Every stage receives an owned, cheap-to-clone `SceneSnapshot` previewing exactly its ancestors. |
| Scene output | Every stage returns a `ScenePatch` plus optional run-local artifacts. Sibling patches merge through `koharu-scene`. |
| Persistence | A successful run returns one merged patch based on the input revision. The application owns the single commit. |
| Determinism | Completion timing can affect progress events, never scene visibility, merge order, reports, or durable output. |
| Resource policy | Ready stages overlap automatically; pressure recycling and one-shot OOM recovery are private. No public memory or concurrency limits are exposed. |
| UI observability | The app owns runtime-initialization status and can observe model residency and system-resource telemetry without polling model internals. |
| Worker removal | Delete `koharu-worker`, worker mode, RPC, MessagePack frames, shared-memory arenas, child-process management, and worker lifecycle events. |
| Compatibility | No old pipeline API, serialized configuration, or worker protocol is preserved. |

## Crate boundaries

```text
koharu-ml / koharu-translator
    runtimes, model architecture, weights, intrinsic preprocessing,
    inference, intrinsic postprocessing, provider clients
                         |
                         v
koharu-pipeline
    stage configuration, DAG selection, model reuse, scheduling,
    run-local image/artifact caches, progress, scene patch production
                         |
                         v
koharu-scene
    typed components, hierarchy, relations, generated ownership,
    immutable snapshots, preview, patch merge, semantic validation
                         |
                         v
koharu-storage
    opaque records, blobs, SQLite, revisions, atomic commit, history
```

`koharu-pipeline` depends on `koharu-scene`, not directly on
`koharu-storage`. It must not bypass scene validation or inspect raw storage
operations.

The pipeline owns:

- selection of configured stages and their prerequisite closure;
- the built-in dependency graph and its declared data reasons;
- lazy, reusable in-process model slots;
- live configuration generations and stage-slot reuse;
- automatic readiness and pressure-aware scheduling;
- scope normalization and deterministic input ordering;
- run-local encoded-blob, decoded-image, crop, and ephemeral-artifact caches;
- conversion from model/provider results into typed scene edits;
- node timing, progress, cancellation propagation, and run reports;
- model residency status for application indicators;
- resource telemetry shared with the future application UI; and
- final patch merge and preview validation.

It does not own:

- runtime package implementation or model checkpoint loading logic;
- scene schemas, component codecs, hierarchy, relations, revisions, or SQLite;
- application job queues, initialization-page presentation, UI state, or
  project commit policy;
- glyph shaping, page compositing, renderer caches, or export formats; or
- a generic user-extensible workflow engine.

The headless `run` binary may depend on `koharu-renderer` for export, but the
pipeline library does not render pages as part of model execution.

## Application bootstrap and runtime initialization

The app window and frontend shell start before native ML initialization. The
only available route is an initialization page. Immediately after that page can
render, the app starts its initialization loop in an async background service.
The loop owns attempt, error, and retry-delay state; runtime-package byte
progress continues to come from `koharu-runtime`'s existing download stream.

The public ML boundary is:

```rust
pub async fn init() -> anyhow::Result<()>;

pub fn llama_backend() -> Option<&'static koharu_llama::llama_backend::LlamaBackend>;
```

There is no public `Runtimes` type and no Torch or stable-diffusion runtime
handle. Those backends are process-global implementation details inside
`koharu-ml`. The initialized llama.cpp backend is the only exposed native
runtime handle because local LLM construction requires it.

`koharu-ml` exposes no initialization subscription. The app knows when it starts
an attempt, receives its result directly from the `init()` future, and therefore
does not need a second status channel. The initialization page shows that
app-owned state together with `koharu-runtime` download progress.

`koharu_ml::init()` completes all of the following before returning:

1. Resolve and preload LibTorch.
2. Resolve llama.cpp, load its backends, install tracing, and initialize the one
   public `LLAMA_BACKEND` handle.
3. Resolve stable-diffusion.cpp, load its GGML backends, and install tracing.

The three backend states use private once-cells. One private aggregate
initialization state coalesces concurrent `init()` calls and becomes ready only
after all three succeed. A failed call returns its backend context to every
waiter. A later call starts another attempt while reusing already successful
private backend initialization. Partial state is never treated as ready.

`init()` performs one attempt and never sleeps or retries internally. This keeps
its behavior deterministic for tests and non-app callers. The application owns
automatic retry: call immediately, record and display a failure, wait with
capped exponential backoff and jitter, then call again. Start at one second,
double to a maximum of thirty seconds, and retry until initialization succeeds
or application shutdown cancels the loop. There is no manual Retry action. A
successful call clears the last error and retry delay before the workspace is
opened.

Package resolution may run concurrently where native loaders permit it.
Backend registration with an ordering requirement remains sequenced inside
private helpers. The former public `init_torch`, `init_llama`, and
`init_diffusion` functions are removed, and model adapters never call a runtime
initializer.

Model loading checks the private aggregate-ready state and returns a clear
`runtime is not initialized` error if a non-app caller violates this order. It
does not initialize a missing backend on demand.

Application bootstrap is:

```text
create window and frontend shell
        |
        v
show Initializing route
        |
        +--> observe koharu-runtime download progress
        `--> start app-owned initialization loop
                    |
               call init()
                    |
             +------+------+
             |             |
          success        failure
             |             |
   construct pipeline    show error and
   and open workspace    retry countdown
                           |
                           `----> wait with capped backoff
                                      and call init() again
```

The app cannot enqueue pipeline jobs or open the normal workspace before the
initialization state is ready. Headless application entry points use the same
retry loop and report attempts through logs instead of a page. Runtime
initialization loads native libraries and backend registries, not model
checkpoints.

Removing process isolation intentionally means an unrecoverable native crash
also terminates the application. Rust errors, model load errors, and ordinary
native error returns remain recoverable; a worker crash protocol is not
retained.

## Pipeline construction and live configuration

A `Pipeline` owns an atomically replaceable configuration generation:

```rust
pub struct Pipeline {
    current: ArcSwap<ConfigurationGeneration>,
    graph: PipelineGraph,
    device: koharu_ml::Device,
    model_status: Arc<ModelStatusHub>,
    resources: Arc<ResourceMonitor>,
    events: Arc<EventHub>,
}

struct ConfigurationGeneration {
    revision: ConfigRevision,
    pipeline: Arc<PipelineConfig>,
    translation: Arc<koharu_translator::TranslationConfig>,
    processors: BTreeMap<Stage, Arc<dyn Processor>>,
    usage: BTreeMap<Stage, Arc<tokio::sync::Mutex<()>>>,
}

impl Pipeline {
    pub fn new(
        config: PipelineConfig,
        translation: koharu_translator::TranslationConfig,
    ) -> Result<Self>;

    pub fn reconfigure(
        &self,
        config: PipelineConfig,
        translation: koharu_translator::TranslationConfig,
    ) -> Result<ConfigChange>;
}
```

Construction validates configuration and topology but performs no model load
and allocates no model tensor. The app calls `reconfigure` after a settings
edit. Validation and processor construction complete before one atomic
generation swap; invalid settings leave the previous generation active.

Every run captures one `Arc<ConfigurationGeneration>` before planning and uses
it through completion. New runs see the new generation immediately, while
already-running work remains internally consistent. Unchanged stage
configuration reuses the same processor and model slot by direct typed equality.
A changed stage receives a new slot; its old slot remains valid for captured
runs and becomes recyclable when idle. Translation configuration follows the
same rule. Configuration identity is never derived from serialized bytes.

One pipeline instance is shared by application jobs. Separate runs may progress
concurrently and configuration may change between them. There is no `run_lock`.

## Public run API

Long-running inference is separated from the synchronous scene commit:

```rust
let snapshot = session.snapshot();

let report = pipeline
    .run(snapshot)
    .scope(Scope::Pages(vec![page]))
    .target(Target::All)
    .cancellation(cancellation)
    .execute()
    .await?;

let commit = session.commit(report.patch)?;
renderer.prepare(&commit.snapshot, request)?;
```

The primary public types are:

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Stage {
    Detection,
    Ocr,
    Translation,
    Inpainting,
}

pub enum Target {
    All,
    Stage(Stage),
    Stages(BTreeSet<Stage>),
    Exact(BTreeSet<Stage>),
}

pub enum Scope {
    Project,
    Pages(Vec<koharu_scene::EntityId>),
    Region {
        page: koharu_scene::EntityId,
        bounds: Bounds,
    },
    Entities(Vec<koharu_scene::EntityId>),
}

pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub struct RunReport {
    pub run: RunId,
    pub base: koharu_scene::Revision,
    pub patch: koharu_scene::ScenePatch,
    pub preview: koharu_scene::SceneSnapshot,
    pub nodes: Vec<NodeReport>,
    pub elapsed: Duration,
}
```

`Target::Stage` and `Target::Stages` select the requested stages plus every
transitive prerequisite. `Target::Exact` runs only the named stages and
requires their omitted inputs to be valid in the base scene; it exists for
focused reruns such as translating user-corrected OCR. An empty explicit stage
set is invalid.

Scope is normalized once against the base snapshot into stable page and entity
order. Duplicate IDs are removed, every entity must belong to the selected
project, region coordinates must be finite and non-empty, and unsupported
stage/scope combinations fail before any model loads. `Project` means all pages
in scene order. `Entities` is intended for an exact OCR or translation rerun
over existing text entities; detection and page-level inpainting reject it.

`Pipeline::run` owns the cheap `SceneSnapshot` clone. It never borrows a
`SceneSession` across an `await`. `RunReport::preview` is the already validated
result of applying `patch` to the base and may be used for a non-durable preview.
The durable commit may still fail if the session head advanced during the run.

## Configuration

There is exactly one configured implementation for each stage:

```rust
pub struct PipelineConfig {
    pub detection: DetectionModel,
    pub ocr: OcrModel,
    pub inpainting: InpaintingModel,
}
```

Translation provider configuration and credentials remain in
`koharu-translator`; its immutable `TranslationConfig` is passed separately and
the translation adapter constructs the selected local or remote implementation.
Model enums own only model-specific settings. Compatibility aliases, legacy
processor arrays, ignored legacy fields, and worker settings are removed.
Unknown fields are rejected rather than silently ignored.

All configured numeric values are validated for range and finiteness during
`Pipeline::new` and `Pipeline::reconfigure`. The app may persist new pipeline
and translation settings at any time, then applies both to the pipeline as one
configuration generation. A run captures that generation and never reads live
settings again.

Hardware selection, load/inference concurrency, memory headroom, and recycling
thresholds are not public configuration. Ready stages overlap by dependency;
the internal resource policy reacts primarily to sampled accelerator-memory
pressure, uses RAM as a secondary signal, and treats allocation failures as the
final authority without creating a production tuning API.

## Stage contracts and scene ownership

The built-in graph has four stable responsibilities. Typography inference is a
fixed internal part of detection rather than a configurable stage:

| Stage | Reads | Writes | Stable producer |
| --- | --- | --- | --- |
| Detection | page source asset and scope | generated region/text entities, `Geometry`, `Region`, `DetectionAnalysis`, text-to-bubble relations, and default renderer-independent `Typography` intent | `dev.koharu.pipeline.detection`; internal typography uses `dev.koharu.pipeline.typography` |
| OCR | source asset and detected text entities | `SourceText` and `OcrAnalysis` on detected text entities | `dev.koharu.pipeline.ocr` |
| Translation | `SourceText`, source language, target locale, and instructions | locale-slotted `Translation` | `dev.koharu.pipeline.translation` |
| Inpainting | source asset and detected text mask | page `Asset` in the configured clean-image role | `dev.koharu.pipeline.inpainting` |

Detection emits the semantic kinds already consumed by the renderer, including
`dev.koharu.region.bubble` and
`dev.koharu.relation.text-region`. These stable identifiers belong to the scene
contract shared by the producer and renderer; adapters do not invent private
aliases.

The detection adapter applies class-aware bounding-box NMS before it creates
scene entities. It then derives deterministic manga reading order from the
detected layout: panels are ordered top-to-bottom and right-to-left, bubbles
are ordered within their containing panel, and text is ordered within its
smallest containing bubble. A region is associated with a container when at
least half of its area overlaps it, so slightly imperfect detector boxes do not
break the hierarchy. Text outside a bubble or panel falls back to the same
spatial order.

The detection slot loads and recycles the layout and font models as one unit. It
runs layout inference first, previews that patch, runs the font detector with
its fixed default behavior over the new text entities, and merges both patches
into one detection result. There is no public typography target, configuration
field, model-status row, or independent scheduling node.

Every processor creates its patch with
`SceneSnapshot::edit_as(Generation)`. The producer is the stable responsibility
above, while `Generation::model` records the selected implementation/checkpoint.
Changing a model does not change ownership.

Reruns may replace components and lifecycle objects still owned by the same
producer. They do not overwrite user-owned components, delete user-promoted
entities or relations, or reinterpret renderer details as scene semantics.
Generated results are sorted by normalized scope order and model-independent
spatial tie breakers before IDs and edit operations are created.

Each stage reconciles generated output only inside the normalized run scope.
Results on unselected pages and entities remain untouched.

## Clean petgraph model

The dependency topology uses stage identity directly:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dependency {
    DetectedTextRegions,
    RecognizedSourceText,
    DetectedTextMask,
}

struct PipelineGraph {
    dependencies: petgraph::graphmap::DiGraphMap<Stage, Dependency>,
    canonical_order: Vec<Stage>,
}
```

The graph is:

```text
Source snapshot --> Detection ----+--> OCR --> Translation
                                  `--> Inpainting
```

| Prerequisite | Dependent | Edge value |
| --- | --- | --- |
| Detection | OCR | `DetectedTextRegions` |
| OCR | Translation | `RecognizedSourceText` |
| Detection | Inpainting | `DetectedTextMask` |

Edges always point from prerequisite to dependent. One helper makes that
direction explicit:

```rust
fn depends_on(
    graph: &mut DiGraphMap<Stage, Dependency>,
    dependent: Stage,
    prerequisite: Stage,
    reason: Dependency,
) {
    graph.add_edge(prerequisite, dependent, reason);
}
```

`petgraph::algo::toposort` validates graph acyclicity. Reverse traversal from
ordinary targets selects prerequisite closure. `Target::Exact` deliberately
skips that traversal and runs preflight validators for every omitted incoming
contract. Selection is a `BTreeSet<Stage>` over the original graph; it does not
retain/remove nodes, remap indices, or build an induced graph.

Canonical topological order is computed by Kahn traversal with a
`BTreeSet<Stage>` ready queue. `toposort` is not treated as a stable tie-order
guarantee. The same canonical order controls deterministic reports, ancestor
preview order, final patch merge, and DOT output.

Petgraph owns topology, neighbor traversal, cycle detection, and reachability.
The scheduler owns transient states such as remaining prerequisites, ready,
loading, running, completed, failed, and cancelled. Resource availability is
not represented as a graph edge.

## Readiness-driven scheduler

The scheduler does not precompute topological waves. Waves introduce a false
barrier: translation would wait for unrelated inpainting even after OCR had
completed.

For the selected stage set, execution proceeds as follows:

1. Compute each selected stage's selected incoming-edge count.
2. Insert every zero-count stage into a `BTreeSet<Stage>` ready queue.
3. Start every ready stage in canonical key order.
4. Track running futures in `FuturesUnordered`.
5. When a stage succeeds, retain its immutable output and decrement only its
   selected outgoing neighbors.
6. Make a neighbor ready immediately when its count reaches zero.
7. Continue until all selected stages succeed, cancellation wins, or one stage
   fails.

For a full run, OCR and inpainting become ready together after detection.
Translation becomes ready as soon as OCR completes, regardless of whether
inpainting has completed.

Ready ordering is deterministic, while completion order is allowed to vary.
Per-model usage gates serialize only callers of the same configured model slot;
they do not block unrelated stages.

## Automatic resource management

The production API exposes no concurrency limit, memory budget, prefetch flag,
or cache-size knob. The current best-effort policy samples process/system RAM,
CPU, and accelerator memory once per second. Low selected-device headroom is the
primary signal before a local model load. Critically low RAM is a secondary
signal, and becomes the conservative fallback when accelerator metrics are
unavailable. Pressure triggers a non-blocking pass over other model slots. A
slot is recycled only when its usage gate can be acquired immediately, so
active loading or inference is never force-unloaded.

Allocation errors are the fallback signal when GPU telemetry is unavailable.
If loading or inference reports an out-of-memory error, the scheduler recycles
idle model residency and retries that operation once. A final inference failure
also unloads its model so poisoned or partially failed native state is not
reused. Resolved checkpoints remain in the package cache throughout.

There is no process-wide `accelerator: Mutex<()>`. Distinct Torch, llama,
diffusion, CPU, and remote-provider stages overlap whenever their graph
dependencies are ready. A backend-wide exclusive gate may be introduced only
for demonstrated native thread safety and must not serialize unrelated
backends.

One loaded model instance is exclusive because some wrappers or native contexts
have mutable inference state. Concurrent runs requesting the same slot await it
without preventing other slots from progressing.

Synchronous model inference and substantial CPU image work run through
`tokio::task::spawn_blocking`. Remote translation remains async. Stage adapters
batch pages or crops when the model supports it; results return to canonical
entity order before scene edits are built.

## Resource telemetry and future UI

The pipeline owns one cross-platform `ResourceMonitor` so the scheduler and UI
use the same measurements. It publishes a coalesced watch stream:

```rust
pub struct ResourceSnapshot {
    pub process_memory_bytes: u64,
    pub system_memory_total_bytes: u64,
    pub system_memory_used_bytes: u64,
    pub available_system_memory_bytes: u64,
    pub process_cpu_percent: f32,
    pub system_cpu_percent: f32,
    pub devices: Vec<DeviceResources>,
    pub loaded_models: Vec<LoadedModelResources>,
}

impl Pipeline {
    pub fn subscribe_resources(&self) -> watch::Receiver<ResourceSnapshot>;
}
```

Each `DeviceResources` identifies whether it represents the configured device
and exposes optional memory budget, used, available, and utilization values.
Windows uses DXGI's per-process video-memory budget and current usage. Linux
uses `nvml-wrapper` for NVIDIA and driver DRM sysfs for AMD and Intel. Apple
Silicon reports unified system memory for Metal because it has no separate VRAM
pool. CPU devices publish no accelerator entry.

The application may later subscribe and render CPU, RAM, VRAM, and loaded-model
usage in the UI. That visualization is later work; the scheduler-facing monitor
and stable subscription boundary belong in this redesign. Sampling is bounded
and independent of UI frame rate. Unsupported GPU metrics are represented as
unknown, never as zero; the scheduler then relies on allocation-error recovery.

## Deterministic scene branches

Concurrent stages never observe whichever sibling happened to finish first.
Before a ready stage starts, the scheduler constructs its input from:

- the immutable run base `SceneSnapshot`; and
- exactly that stage's transitive ancestor patches, in canonical topological
  order.

It calls `SceneSnapshot::preview` with those patches. The resulting snapshot
retains the original durable revision and carries patch ancestry through
`koharu-storage` internally.

Consequently:

- OCR and inpainting see detection output, including its typography intent;
- translation sees detection and OCR output; and
- translation never sees inpainting output.

Every processor returns a `ScenePatch` created from its provided preview. The
patch contains only that processor's edits while retaining its ancestor
preconditions. After all selected stages succeed, the scheduler passes patches
to `ScenePatch::merge` in canonical topological order, previews the merged patch
on the original base, and returns both in `RunReport`.

Component kind plus slot is the concurrency boundary. OCR and translation can
edit the same entity because they write distinct components or locale slots.
Two siblings writing the same component key fail with a scene/storage patch
conflict. The pipeline does not implement JSON-field merging or inspect raw
operation footprints.

These rules guarantee:

- one successful run produces one undoable scene commit when the caller commits;
- failure or cancellation produces no intermediate durable revision;
- node completion order cannot change the merged patch;
- descendants can refer to records created by ancestors;
- a stale application revision cannot be overwritten; and
- scene semantic validation runs before the report is returned and again at
  durable commit.

## Processor boundary

The scheduler is model-agnostic:

```rust
#[async_trait]
trait Processor: Send + Sync {
    fn spec(&self) -> &ProcessorSpec;
    async fn ensure_loaded(&self, context: &LoadContext) -> Result<()>;
    async fn run(&self, input: NodeInput) -> Result<NodeOutput>;
}

struct NodeInput {
    run: RunId,
    scene: koharu_scene::SceneSnapshot,
    scope: Arc<NormalizedScope>,
    options: Arc<RunOptions>,
    cache: Arc<RunCache>,
    artifacts: AncestorArtifacts,
    cancellation: CancellationToken,
}

struct NodeOutput {
    patch: koharu_scene::ScenePatch,
    artifacts: ArtifactSet,
    measurements: NodeMeasurements,
}
```

Processors do not receive `SceneSession`, petgraph indices, dependency counters,
other sibling outputs, renderer state, or persistence callbacks. Built-in
processors use typed component reads and `SceneEdit`; they never construct raw
storage patches.

`ensure_loaded` resolves every checkpoint/config/tokenizer asset through the
durable package cache, then constructs the configured model on the selected
device. Concurrent calls coalesce through the model slot. Remote providers and
stages without local model residency report loading as not required.

Every `koharu-ml` public model exposes `pub async fn load(device: Device)`.
`load()` resolves all assets it uses, downloading only artifacts missing from
the package cache.

Processor-specific preprocessing and postprocessing that define model semantics
remain in `koharu-ml` under the project model-interface rules. Pipeline adapters
own only page/entity selection, shared image access, batching, semantic result
mapping, and generation metadata.

## Run-local artifacts and image cache

Scene patches are the durable semantic truth. Large data needed only during one
run remains run-local. Decoded blob bytes and images are the current built-in
examples. Detection masks are durable scene assets because a later exact
inpainting rerun must be able to consume them after detection has ended.

The processor boundary also permits optional `Arc`-backed artifacts. The
scheduler exposes only transitive-ancestor artifacts to a stage and drops the
entire store after the run. Current built-ins communicate through scene patches,
so their artifact sets are empty.

`RunCache` provides coalesced concurrent loading:

```rust
struct RunCache {
    blobs: Mutex<HashMap<BlobId, SharedCell<[u8]>>>,
    images: Mutex<HashMap<BlobId, SharedCell<DynamicImage>>>,
}
```

Each `SharedCell` is an `Arc<OnceLock<Result<Arc<T>, String>>>`. Blob bytes come
through `SceneSnapshot::read_blob`; each encoded blob and decoded image is
materialized at most once per run even when sibling stages request it
concurrently. Failures are shared with concurrent waiters. The entire cache is
dropped with the run, so there is no global cache limit or retained tensor
state.

## Model load and status

Process residency is exposed as a single state machine:

```rust
pub enum LoadState {
    Unloaded,
    WaitingForMemory,
    Loading,
    Loaded,
    InUse { runs: usize },
    Unloading,
    Failed { message: String },
    NotRequired,
}

pub struct ModelStatus {
    pub generation: ConfigRevision,
    pub stage: Stage,
    pub model: String,
    pub active_configuration: bool,
    pub load: LoadState,
}

impl Pipeline {
    pub fn model_status(&self) -> Arc<[ModelStatus]>;
    pub fn subscribe_model_status(&self)
        -> watch::Receiver<Arc<[ModelStatus]>>;
}
```

The app uses this snapshot/stream for model residency indicators. A recycled
model transitions from loaded to unloaded while its resolved assets remain in
the package cache. Remote stages with no local residency use `NotRequired`.

Reconfiguration publishes status for newly selected models immediately and
reuses load state when the configured processor is unchanged. The watch
snapshot is bounded to the active generation; an older captured run may finish,
but cannot overwrite the new generation's status. Package byte progress remains
owned by `koharu-runtime` while a model is loading.

Each configured local model owns one slot:

```rust
struct BuiltinProcessor {
    spec: ProcessorSpec,
    loaded: tokio::sync::Mutex<Option<Loaded>>,
}

// Stored beside the processor in a configuration generation.
type UsageGate = Arc<tokio::sync::Mutex<()>>;
```

The slot contract is:

- the usage gate serializes load and inference for one slot;
- resolved assets remain in the package cache across unloads, config changes,
  pipeline recreation, and application restarts;
- a failed load leaves the slot retryable;
- successful weights are reused across pages and runs while resident;
- the usage gate is acquired before loading and held through inference;
- recycling uses `try_lock`, so it unloads only an actually idle slot and never
  waits behind native work;
- a later request transparently reloads a recycled slot from cached files;
  and
- dropping the pipeline eventually drops all resident models after in-flight
  runs release their `Arc`s.

Private LibTorch and stable-diffusion runtime state and the public
`LLAMA_BACKEND` outlive every slot. There is no shutdown RPC. Recycling is
coordinated by the scheduler and resource monitor, never by UI limits.

## Cancellation, failure, and panic handling

Cancellation is cooperative and has one terminal outcome:

1. Stop admitting new model loads and stages.
2. Cancel async provider requests and cancellable preprocessing.
3. Invoke backend cancellation hooks where they are supported.
4. Safely drain native blocking calls that cannot be interrupted.
5. Drop all node patches and artifacts and return `RunError::Cancelled`.

A native call is never detached while it owns model or run state. Cancellation
latency for a non-interruptible native call is reported in the node measurement.

On the first stage failure, no descendants are scheduled. The scheduler signals
cancellation to independent running stages, drains them, discards every patch,
and returns an error containing run ID, stage, configured model/provider,
optional page/entity context, and the original source chain.

`spawn_blocking` join failure becomes a stage panic/join error; poisoned model
state is not reused. Load and insufficient-memory errors remain distinct and do
not poison unrelated slots.

Patch conflict, invalid preview, and stale durable commit remain distinct error
classes. The first two are pipeline execution failures; the last is returned by
`SceneSession::commit` after a successful run.

## Events and measurements

Progress describes stages and models, never processes:

```rust
pub enum PipelineEvent {
    ConfigurationChanged { generation: ConfigRevision, changed: Vec<Stage> },
    RunStarted { run: RunId, base: Revision, stages: Vec<Stage> },
    ModelLoadStarted { run: RunId, stage: Stage, model: String },
    ModelLoadFinished { run: RunId, stage: Stage, model: String, elapsed: Duration },
    ModelUnloaded { stage: Stage, model: String, reason: UnloadReason },
    StageStarted { run: RunId, stage: Stage },
    StageProgress { run: RunId, stage: Stage, completed: u64, total: Option<u64> },
    StageFinished { run: RunId, stage: Stage, elapsed: Duration },
    RunFinished { run: RunId, elapsed: Duration },
    RunCancelled { run: RunId },
    RunFailed { run: RunId, stage: Option<Stage>, message: String },
}
```

The pipeline exposes a bounded broadcast subscription for temporal events and
separate watch subscriptions for current model/resource state. Emission never
waits on a slow UI subscriber; lag is observable through the receiver's normal
lag error. Events may reflect real completion order. `NodeReport` values are
returned in canonical stage order.

`NodeMeasurements` separates time waiting for the slot usage gate, model load,
and processor execution; `NodeReport::elapsed` is total stage time. Runtime and
checkpoint byte progress continues through `koharu-runtime`'s download
subscription. There are no worker-generation, process-startup, RPC, arena,
shared-memory, or round-trip measurements.

## Renderer integration

Rendering is not a pipeline stage and adds no DAG edge. `koharu-renderer`
consumes a `SceneSnapshot` independently:

- use `RunReport::preview` for a transient pre-commit preview;
- use `SceneCommit::snapshot` after the durable commit;
- use `SceneChangeSet` and renderer dependencies for invalidation; and
- report renderer text-fit diagnostics without writing them back into the scene
  unless a separate explicit application action chooses to persist new intent.

Detection must emit the region and relation semantics expected by the renderer.
OCR must retain `LanguageTag`; translation writes the target locale slot; and
detection's internal font model writes only renderer-independent typography
intent. Glyph runs, fallback-family resolution, punctuation shaping, balloon
line profiles, and rendered images do not become pipeline artifacts or scene
components.

## Target module layout

```text
src/
  lib.rs              public facade and re-exports
  config.rs           typed configuration validation and generation swaps
  graph.rs            Stage, Dependency, graph construction and target closure
  scope.rs            public scope and deterministic normalization
  scheduler.rs        readiness, resource admission, ancestry, and completion
  run.rs              run builder, report, errors, and final merge/preview
  processor.rs        Processor, NodeInput, NodeOutput, and ProcessorSpec
  resources.rs        CPU/RAM/accelerator sampling and pressure recovery
  resources/          platform accelerator-memory providers and sample mapping
  status.rs           model-status snapshots and watch publication
  cache.rs            coalesced run-local blob and image cache
  events.rs           cancellation, temporal events, and measurements
  builtin/
    detection.rs
    ocr.rs
    translation.rs
    typography.rs       hidden default font inference owned by detection
    inpainting.rs
  bin/run.rs          headless caller of the same in-process API
```

Model-specific variants may remain in submodules below their stage adapter, but
the scheduler never matches on a model enum. `graph.rs` does not load models or
touch scene patches. The slot implementation in `builtin/mod.rs` knows nothing
about petgraph. Built-in adapters do not select targets, merge patches, or
commit sessions.

There is no `worker.rs`, `worker/`, server, wire protocol, worker factory,
shared-byte variant, hidden worker branch, or compatibility feature.

## Dependency and executable cleanup

The rewrite performs all of the following:

- remove `koharu-worker` from the workspace and every `Cargo.toml`;
- remove `koharu_pipeline::serve_worker` and `koharu_app::serve_worker`;
- remove `Pipeline::with_worker_executable`;
- remove hidden `--worker` arguments from desktop and headless executables;
- remove RPC request/response/event types and MessagePack framing;
- remove child-process discovery, spawning, generations, shutdown, and restart;
- remove mapped arenas, shared slices, temporary arena directories, and
  transfer-size policy;
- remove `tempfile` from `koharu-pipeline` if no headless use remains;
- remove public execution-limit, memory-budget, prefetch, and cache-size options
  from pipeline config and CLI surfaces;
- add `koharu-scene` and `petgraph` as direct pipeline dependencies;
- keep `koharu-storage` transitive through `koharu-scene`;
- place platform CPU/RAM/VRAM sampling behind one internal resource-monitor
  abstraction shared by the governor and app subscription; and
- use `tokio-util` cancellation primitives or an equally race-safe single
  implementation rather than retaining a worker-era token.

The old path is deleted, not hidden behind a feature flag.

## Efficiency invariants

- The app shows its initialization page while all three native runtimes
  initialize once per process.
- Model loads resolve assets into the durable package cache before constructing
  RAM/VRAM residency.
- A configured model loads at most once successfully per residency epoch;
  concurrent requests never duplicate a load.
- The governor may overlap useful model loads with prerequisite execution when
  current pressure makes that beneficial.
- Idle model residency is recycled automatically before memory pressure becomes
  an avoidable allocation failure.
- Each run reads a blob and decodes a given representation at most once.
- Large immutable values cross stage boundaries through `Arc`, not serialization.
- No scene snapshot clone copies project records, indexes, or blob bytes.
- Independent ready stages can execute concurrently on distinct model slots.
- Translation starts after OCR, not after an unrelated global wave.
- Adaptive resource throttling does not become a data dependency or a global
  accelerator mutex.
- No synchronous inference or large image loop blocks a Tokio worker thread.
- Node completion timing cannot change scene inputs or final merge order.
- No intermediate preview writes SQLite.
- A successful application action performs one scene transaction.

## Reliability invariants

- The app cannot leave initialization mode or enqueue a pipeline run before
  `koharu_ml::init()` succeeds.
- An initialization failure remains on that page and schedules an automatic
  retry; no initialization subscription or manual Retry action exists.
- `koharu-ml` exposes no aggregate runtime handle; only `LLAMA_BACKEND` is
  externally accessible.
- Every run uses one captured configuration generation, base project, base
  revision, target set, and normalized scope.
- A failed reconfiguration leaves the active generation unchanged.
- A node sees its ancestors and never its independent siblings.
- Every generated scene edit carries a stable producer and configured model.
- User-owned scene content is not overwritten by an ordinary rerun.
- Every node patch is scene-valid on its input preview.
- The merged patch is scene-valid on the original base before it is returned.
- Failure and cancellation return no committable partial patch.
- The scene session remains the only durable writer and rejects stale revisions.
- Broadcast progress backpressure cannot stall inference; a per-run callback is
  expected to enqueue briefly and is isolated from callback panics.
- Model load status always describes the selected configuration; captured older
  runs cannot overwrite it.
- No public resource limit permanently serializes otherwise independent model
  slots.

## Verification contract

The implementation is complete only when automated tests prove:

1. The desktop renders its initialization page before a controlled
   `koharu_ml::init()` barrier resolves, observes runtime-package download
   progress, and cannot enqueue a run until initialization succeeds.
2. Concurrent `koharu_ml::init()` calls initialize LibTorch, llama.cpp, and
   stable-diffusion.cpp exactly once and all return `()` on success.
3. No public aggregate runtime, Torch handle, stable-diffusion handle, or
   backend-specific `init_*` function exists; `LLAMA_BACKEND` is the only
   exposed runtime handle, no initialization subscription exists, and adapters
   never initialize runtimes.
4. Controlled initialization failures keep the app on its initialization page,
   use capped exponential retry delays, eventually open the workspace after a
   success, and do not repeat backend initialization that already succeeded.
5. A valid live reconfiguration swaps pipeline and translation configuration
   atomically; an invalid one changes nothing; old runs retain the old
   generation and new runs use the new generation.
6. Reconfiguration reuses unchanged typed model slots, preserves their active
   load state, and cannot regress revisions under concurrent calls.
7. Model loading resolves missing assets through the package cache before
   construction, and a failed load remains retryable.
8. One slot's usage gate spans load through inference and coalesces concurrent
   requests for the same configured model.
9. Model-status snapshots correctly distinguish unloaded, loading, loaded,
   in-use, failed, and not-required states.
10. Memory pressure recycling acquires only idle usage gates, preserves cached
    files, and reloads a recycled model transparently when needed.
11. An active model is never unloaded; an out-of-memory load or inference
    recycles idle models and retries once rather than retrying forever.
12. Resource snapshots distinguish unknown GPU metrics from zero, coalesce for
    slow subscribers, and expose the same process telemetry used by pressure
    recovery to the future UI.
13. The production configuration and public API contain no concurrency, RAM,
    VRAM, prefetch, cache-size, or translation-request limits.
14. The built-in graph has exactly the declared nodes, labeled edges, and no
    cycle; closure targets select the correct ancestors, exact targets select no
    implicit stage, and missing exact-target inputs fail before model load.
15. Canonical graph order is stable across construction runs and independent of
    hash-map iteration.
16. After detection, OCR and inpainting overlap when the fake resource monitor
    reports sufficient headroom; typography is already part of detection.
17. Translation starts immediately after OCR while a slower independent sibling
    is still running, and a pressured GPU stage does not block ready network or
    light CPU work.
18. Distinct model slots overlap, while concurrent use of one exclusive slot is
    serialized without blocking unrelated stages.
19. A node preview contains all and only its transitive ancestor patches and
    artifacts.
20. Given fixed processor patches, randomized completion delays produce the
    same merged patch fingerprint and canonically ordered `NodeReport` values.
21. OCR and translation patches on one entity merge because their component
    keys differ; deliberate sibling writes to one key conflict.
22. Detection-created entity IDs remain usable by descendant patches and final
    merge.
23. A successful full run previews as a semantically valid scene and commits as
    one revision; an advanced session head rejects the same report as stale.
24. Failure and cancellation never mutate `SceneSession`, return a partial
    patch, detach a model task, or unload an active native context.
25. Generated ownership permits same-producer reruns while preserving user-owned
    and user-promoted content.
26. Concurrent requests for one blob/decode key perform one read and decode, and
    cache eviction never invalidates active `Arc`s.
27. Renderer planning succeeds on the final preview with detected bubble
    relations, language-tagged OCR, locale translations, typography intent, and
    clean assets.
28. The desktop and headless executables have no worker mode, worker process,
    RPC transport, shared arena, or `koharu-worker` dependency.

Concurrency tests use controlled barriers rather than timing-only sleeps.
Failure tests inject initialization, load, out-of-memory, inference, provider,
decode, patch-conflict, cancellation, and blocking-task-join errors.

Performance validation uses the actual target device. CUDA benchmarks
synchronize immediately before and after measured inference, keep model loading
and warm-up outside the measured interval, and include representative page sets
plus the checked-in 4K LaMa fixture. Report device, input size, sequential
baseline, concurrent result, peak memory, and structured-output equivalence.

## Rewrite order

1. Add the app initialization route and automatic retry loop around the single
   `koharu_ml::init()`, keep runtime state private except `LLAMA_BACKEND`, expose
   no initialization subscription, and migrate every binary/test.
2. Delete `koharu-worker` integration and all worker-only CLI/application paths.
3. Replace the current pipeline facade with the immutable snapshot-in/patch-out
   run API plus atomic live configuration generations.
4. Replace stable graph indices and waves with `DiGraphMap`, target closure, and
   the readiness scheduler.
5. Port built-in adapters to `SceneSnapshot`, `SceneEdit::edit_as`, typed
   components, stable producer IDs, and run-local artifacts.
6. Add reusable local model slots and load-status streams; each model load
   resolves its own package assets.
7. Add reusable model slots, automatic CPU/RAM/VRAM monitoring, adaptive
   admission, idle recycling, OOM recovery, and the coalesced image cache.
8. Integrate application settings updates, scene commit, and renderer against
   `Pipeline::reconfigure`, `RunReport`, and `SceneCommit`.
9. Remove obsolete dependencies and complete the verification contract.

Do not keep transitional adapters to the old `Session`/`Commands`, worker,
wave, immutable-config, or user-tuned resource-limit designs. There is one
pipeline architecture when the rewrite is complete.
