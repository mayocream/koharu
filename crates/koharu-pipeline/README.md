# koharu-pipeline

`koharu-pipeline` coordinates detection, OCR, translation, and inpainting over a
`koharu_scene` project. It owns model lifetime and scheduling; the caller owns
durability through the `Committer` trait.

The execution unit is one stage on one page. A stage result is committed as
soon as it finishes, so the application can refresh that page immediately and
retain completed work if the user stops the run or a later model fails.

## Execution

```rust,ignore
let report = pipeline
    .execute(
        session.snapshot(),
        Request {
            operation: Operation::Full,
            scope: Scope::Project,
            stop: stop.clone(),
            progress: Some(progress),
        },
        &mut committer,
    )
    .await?;

match report.status {
    RunStatus::Completed => println!("all page stages finished"),
    RunStatus::Stopped => println!("completed commits were kept"),
}
```

`Committer::commit` receives a `StageOutput` and returns the committed
`SceneSnapshot`. Pipeline outputs are optimistically rebased onto the latest
snapshot before the callback. Stage patches observe the exact hierarchy,
components, and assets they consume. OCR, translation, and inpainting branches
on one page therefore compose, while a changed input or overlapping write fails
conflict validation instead of publishing stale derived output.

The application records all revisions from one invocation as one undo group.

## Fixed workflow

The workflow is small and explicit:

```text
detection -> OCR -> translation
         \-> inpainting
```

There is no runtime graph or graph library. `Operation` selects a subset of
this workflow:

- `Full`
- `Through(Stage)`
- `Only(Stage)`

For each page, a stage starts only after its selected prerequisite commits.
Pages enter in project order through one execution lane per model. The
scheduler fills every idle lane with the oldest ready page; there is no global
wave barrier or fixed task-count limit. After page 1 detection, page 1 OCR and
inpainting can run alongside page 2 detection. When page 1 OCR finishes, page 1
translation can join those active lanes without waiting for page 1 inpainting
or page 2 detection.

```text
time          1           2           3           4
detection     page 1      page 2      page 3
OCR                       page 1      page 2
translation                           page 1      page 2
inpainting                page 1      page 2      page 3
```

Page priority prevents an upstream model from racing arbitrarily far ahead,
while independent page branches use otherwise idle models. The number of
active pages is bounded by the selected stage count, so the scheduler keeps a
small rolling window instead of filling an unbounded upstream queue.

Stages return an empty patch when their page has no applicable input. A page
with no detected text skips OCR and translation work; an empty removal mask
skips inpainting inference. These are normal completed work items, not
preflight failures.

## Stop semantics

`StopToken` is cooperative scheduling control, not transactional cancellation.

- `stop()` prevents new work from starting.
- A native inference already in progress reaches its safe return boundary.
- Its result is discarded if stop was requested before commit.
- Every earlier commit remains in the project.
- `execute` returns `Ok(Report { status: RunStatus::Stopped, .. })`.

Errors are reserved for invalid input/output, model load or inference failure,
and commit failure. An error also leaves earlier page-stage commits intact.

## Model residency

Models are loaded lazily and remain resident for reuse. The resource monitor
samples the selected accelerator twice per second. A model's first measured run
is exclusive so the residency manager can learn its resident and peak workspace
footprint. Later runs reserve bytes from that profile before admission, so the
number of concurrent models emerges from the current VRAM budget rather than a
configured model count. Admission relies on observed memory rather than a
stage-specific device-memory declaration.

Loaded models reserve only their incremental workspace. A model that must be
loaded reserves its measured peak footprint. The budget also retains a safety
margin for driver and non-pipeline allocations. Idle models are evicted in
least-recently-used order when a ready lane does not fit. Missing accelerator
telemetry falls back to one GPU model at a time. A detected out-of-memory
failure raises the learned estimate, performs exclusive cleanup, and retries
once.

`ModelCell` serializes access to one model and prevents an active model from
being unloaded.

## Stages

`stages/mod.rs` defines `StageProcessor`. Each stage implements the same small
contract:

- identify its model;
- load lazily;
- process exactly one page and produce a semantic `ScenePatch`.

`StageInput` carries one page ID plus optional entity and region filters for
that page. A stage cannot receive a project or page collection. Stage
implementations own their preprocessing, inference, and scene mapping. There
is no shared `common.rs`, stage registry indexing, or catch-all processor enum.
`Stages` has named fields and one exhaustive dispatch point.

## Progress

Progress events carry both page and stage:

- `Started { pages, stages }`
- `Loading { page, stage, model }`
- `Running { page, stage, model }`
- `Finished { page, stage, model, elapsed }`
- `Skipped { page, stage }`

`Finished` is emitted after the stage output commits. Progress callbacks are
isolated from panics and cannot crash execution.

## Source map

```text
src/
  config.rs       model selection and serialized settings
  error.rs        typed execution failures
  images.rs       decoded scene-asset reuse within each active page
  model_cell.rs   lazy model ownership and per-model serialization
  pipeline.rs     execution lifecycle, stage attempts, and incremental commits
  progress.rs     request-local progress events
  report.rs       run outcome, stage output, and Committer
  request.rs      operation, scope, and StopToken
  residency.rs    VRAM admission, unloading, and OOM retry policy
  resources.rs    accelerator and host-memory telemetry
  scheduler.rs    page window, stage readiness, and lane scheduling
  scope.rs        validated project/page/region/entity scope
  stage.rs        stable stage identifiers
  stage_runner.rs model execution, VRAM admission, and OOM recovery
  stages/         detection, OCR, translation, and inpainting processors
```

## Validation

```powershell
cargo test -p koharu-pipeline --all-targets
cargo clippy -p koharu-pipeline --all-targets --no-deps -- -D warnings
```
