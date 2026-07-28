# koharu-scene

This document is the authoritative design contract for `koharu-scene`.
The implementation is a typed scene layer over `koharu-storage`, with no
transitional re-export of the old monolithic scene API.

`koharu-scene` is rebuilt on `koharu-storage`. Existing scene APIs, domain
types, serialized values, and project files are not compatibility constraints.

## Purpose

`koharu-scene` is Koharu's typed project model and ergonomic editing layer. It
defines the semantic components used by the application, renderer, and model
pipeline, then stores them as opaque component records in `koharu-storage`.

Storage owns bytes, records, references, patches, SQLite, and revisions. Scene
owns what those records mean.

The scene model is component-based rather than a closed enum of page elements.
One entity can acquire new independent capabilities without changing a central
`ElementContent` type or migrating unrelated data.

## Decisions

| Concern | Decision |
| --- | --- |
| Foundation | Wrap `koharu-storage`; never duplicate its state, blobs, history, patches, or revision counter. |
| Domain model | Scene entities are storage records carrying independently versioned semantic components. |
| Hierarchy | One scene-owned ordered `Children` component is the authority; parent indexes are derived. |
| Relations | Relation records carry a typed relation component with declared endpoint references. |
| Public API | Typed `SceneSession`, `SceneSnapshot`, `SceneEdit`, and `ScenePatch` newtypes. |
| Compatibility | Each scene component evolves independently with `revision`; unknown extension components are preserved. |
| Pipeline concurrency | Pipeline concerns write separate component keys and merge through storage patch ancestry. |
| Provenance | Authorship belongs to each replaceable component; entity lifecycle ownership is separate. |
| Assets | Scene components describe blob meaning and metadata; storage owns only encoded bytes. |
| Rendering detail | Persist user intent, not decoded images, glyph runs, render caches, tensors, or temporary layout results. |

## Dependency boundary

```text
koharu-storage
    records, opaque components, refs, snapshots, patches
    blobs, SQLite, revision history, atomic commit
                 ^
                 |
koharu-scene
    component schemas and codecs
    hierarchy, relations, domain indexes and validation
    typed reads and ergonomic edits
                 ^
                 |
pipeline / renderer / canvas / application
```

Normal Koharu consumers depend on `koharu-scene`, not directly on
`koharu-storage`. This prevents raw record edits from bypassing scene
validation.

`koharu-scene` owns:

- typed scene identities and views;
- built-in component kinds, payload revisions, and codecs;
- project/page/entity hierarchy and ordering;
- semantic relations and adjacency views;
- page, geometry, text, translation, asset, typography-intent, and provenance
  components;
- cross-component validation and derived scene indexes;
- ergonomic user and pipeline edit helpers;
- scene patch preview, merge, and commit wrappers;
- conversion to application protocol DTOs where a dedicated adapter is not
  more appropriate.

It does not own:

- SQLite connections, tables, transactions, checkpoints, or history encoding;
- structural component storage, patch ancestry, blob caching, or garbage
  collection;
- model loading, inference, scheduling, cancellation, or runtime selection;
- image decoding, tensor conversion, shaping, compositing, or export;
- renderer caches, glyph runs, line breaking results, or GPU resources;
- UI selection, hover, viewport, gesture, or job state.

There is one durable state: `koharu_storage::State`. Scene wrappers hold storage
handles and derived read indexes only.

## Wrapper types

```rust
pub struct SceneSession {
    storage: koharu_storage::Session,
    current: SceneSnapshot,
}

#[derive(Clone)]
pub struct SceneSnapshot {
    storage: koharu_storage::Snapshot,
    index: Arc<SceneIndex>,
}

pub struct SceneEdit {
    storage: koharu_storage::Edit,
    index: SceneIndex,
    generation: Option<Generation>,
}

#[derive(Clone)]
pub struct ScenePatch {
    storage: koharu_storage::Patch,
    result_index: Option<Arc<SceneIndex>>,
}

pub struct SceneCommit {
    pub revision: Revision,
    pub changes: SceneChangeSet,
    pub snapshot: SceneSnapshot,
}
```

These are real newtypes, not public type aliases. They enforce domain
validation and keep normal callers from mixing arbitrary storage records into
a scene.

There is no `pub use koharu_storage::*` compatibility facade. Raw storage
extraction, if ever needed for repair tooling, is an
explicit feature-gated operation rather than part of ordinary editing.

`SceneSnapshot` and `ScenePatch` are `Send + Sync`. `SceneSession` remains a
synchronous single-writer handle because it owns the storage session.

## Identity model

```rust
pub struct ProjectId(koharu_storage::DocumentId);
pub struct EntityId(koharu_storage::RecordId);
pub struct RelationId(koharu_storage::RecordId);
pub use koharu_storage::{BlobBatch, BlobId, PatchId, Revision};
```

The storage root record is the project anchor. It is not exposed as a normal
entity ID. Project-level components and the ordered page list live on it.

Entity and relation IDs wrap the same underlying record identity but remain
distinct in the typed API. A valid scene record is exactly one of:

- the permanent project root;
- a hierarchy entity;
- a relation record.

A relation record cannot appear in `Children`. An entity record cannot carry
the required relation marker. Validation rejects ambiguous records.

IDs never encode page membership, entity kind, model ownership, or order.

## Component schema

Scene components implement a codec owned by this crate:

```rust
pub trait SceneComponent: Clone + Send + Sync + Sized + 'static {
    const KIND: &'static str;
    const CURRENT_SCHEMA: u32;

    fn encode(&self) -> Result<EncodedSceneComponent>;
    fn decode(schema: u32, payload: &[u8]) -> Result<Self>;
    fn record_refs(&self) -> Vec<EntityId>;
    fn blob_refs(&self) -> Vec<BlobId>;
    fn origin(&self) -> Option<&Origin>;
    fn set_origin(&mut self, origin: Origin) -> bool;
    fn validate(&self, context: &ValidationContext<'_>) -> Result<()>;
}
```

Encoding produces a validated `koharu_storage::ComponentRecord`. Decoding
checks the key, payload revision, declared references, and component-specific
invariants.

Koharu-owned payloads use `revision`. Each component kind evolves independently
and owns golden old-version fixtures. Adding a field to translation does not
change page, geometry, storage checkpoints, or the SQLite schema.

Component kinds use stable reverse-DNS names under `dev.koharu.scene.*`.
Slots distinguish multiple values of one kind. Component kind and slot are the
atomic storage conflict boundary.

Unknown extension kinds remain in storage records and are copied unchanged.
Scene does not expose them through typed APIs, but unrelated edits preserve
them. Unknown schemas of required structural components prevent opening a
mutable `SceneSession`; callers may still use lower-level recovery tooling.

## Modeling rule: split by ownership and concurrency

One large element payload would recreate the previous design's coupling.
Instead, values that evolve or are written independently use separate
components.

For example, a text entity can contain:

```text
dev.koharu.scene.geometry          slot default
dev.koharu.scene.text.source       slot default
dev.koharu.scene.text.translation  slot en
dev.koharu.scene.text.translation  slot ja
dev.koharu.scene.typography        slot default
dev.koharu.scene.provenance        slots matching owned components
```

OCR, translation, and typography can therefore write the same entity
concurrently without merging inside an opaque payload.

A component should contain multiple fields when they form one invariant and
have one writer. It should be split when fields have different owners,
lifecycles, revision cadence, or concurrent writers.

The scene does not implement field-path or JSON merging. Component boundaries
are the stable concurrency contract.

## Ordered hierarchy

Hierarchy is a scene component, not storage structure:

```rust
#[revisioned(revision = 1)]
pub struct Children {
    ordered: Vec<EntityId>,
}
```

`Children` declares every child as a storage record reference. The project
root's `Children` component is the ordered page list. A page or group entity
may also carry `Children`.

There is no persisted `parent` field. `SceneIndex` derives:

```rust
struct SceneIndex {
    pages: Arc<[EntityId]>,
    parents: HashMap<EntityId, Parent>,
    children: HashMap<EntityId, Arc<[EntityId]>>,
    relations: HashMap<RelationId, Relation>,
    outgoing_relations: HashMap<EntityId, Arc<[RelationId]>>,
    incoming_relations: HashMap<EntityId, Arc<[RelationId]>>,
}
```

The hierarchy guarantees:

- every hierarchy entity occurs exactly once under the root or another entity;
- relation records never occur in hierarchy;
- no duplicate siblings;
- no cycles;
- no missing referenced entities;
- deterministic sibling order;
- bounded depth when opening untrusted projects.

The index is never persisted. It is rebuilt from storage components when a
snapshot is wrapped and updated incrementally by `SceneEdit`. Debug builds and
tests independently rebuild and compare it after previews and commits.

Hierarchy editing is ergonomic:

```rust
pub enum At {
    Start,
    End,
    Before(EntityId),
    After(EntityId),
}

pub enum RemovePolicy {
    RejectNonEmpty,
    Cascade,
    PromoteChildren,
}
```

`SceneEdit` updates the authoritative `Children` components before removing
records, satisfying storage reference integrity. A sibling list is one atomic
component, so concurrent edits to the same list conflict deterministically.

## Relations

A relation is a storage record carrying one required relation component:

```rust
#[revisioned(revision = 1)]
pub struct Relation {
    kind: RelationKind,
    source: EntityId,
    target: EntityId,
}
```

Its encoded component declares both endpoints as record references. Additional
relation metadata uses ordinary independent components on the relation record.

`RelationKind` is a validated namespaced string rather than a closed enum, so
new associations do not change the scene core. Built-in constants cover known
Koharu relationships.

Relations do not imply containment, deletion, rendering, or pipeline
dependency. Scene derives incoming and outgoing adjacency indexes. Removing an
entity requires an explicit policy for incident relation records.

This representation avoids a second storage mechanism for edges and lets
relations use the same component versioning, patching, history, and references
as entities.

## Initial semantic components

The first implementation defines a small set of components aligned with actual
Koharu ownership boundaries. The following are schemas, not fields in the
storage engine.

### Project and page

- `ProjectSettings`: locale and project-level authored settings that genuinely
  need persistence.
- `Children` on the project root: page order.
- `Page`: page label and intrinsic canvas dimensions.
- `Asset` with slots such as `source`, `clean`, `rendered`, or namespaced model
  outputs.

Asset roles are validated strings, not a permanently closed `PageAsset` enum.
New pipeline outputs can add slots without changing storage or a giant page
record.

### Spatial components

- `Geometry`: persistent scene-space bounds or polygon data required to edit
  and reproduce an object.
- `Visibility`: user-authored visibility and opacity when those values must
  survive reload and participate in undo.
- `Children`: ordered containment for pages and groups.

Geometry contains stable document coordinates. Viewport transforms, selection
handles, cached tessellation, damage regions, and GPU buffers do not belong in
scene.

### Text components

- `SourceText`: recognized or user-entered source content and language intent.
- `OcrAnalysis`: durable recognition direction, confidence, and semantic line
  boundaries consumed by later stages; it is separate from source content.
- `Translation`: translated content, stored in BCP-47 locale slots.
- `TextRole`: semantic role needed by editing or pipeline behavior.
- `ReadingOrder`: producer-owned semantic order used by layout and translation.
- `Typography`: persistent user-facing typography intent.

Typography may contain alignment, writing mode, preferred font, size, or other
values if users can edit them and expect exact reload/undo behavior. It must
not contain shaped glyphs, line boxes, fallback decisions, rasterized effects,
or renderer cache keys. Those are derived by `koharu-renderer`.

The distinction is persistence intent, not whether a value is visually
specific: authored alignment belongs in scene; a computed line break does not.

### Region and analysis components

- `Region`: a semantic detected or authored region using a namespaced kind.
- `DetectionAnalysis`: durable model output that downstream pipeline nodes
  actually consume or users inspect.
- Temporary logits, tensors, crops, feature maps, and intermediate masks stay
  in pipeline run memory unless explicitly promoted to an asset component.

### Asset components

```rust
#[revisioned(revision = 1)]
pub struct Asset {
    origin: Origin,
    blob: BlobId,
    media_type: String,
    metadata: AssetMetadata,
}
```

Scene owns metadata needed to interpret the encoded blob, such as raster size
or color intent. Storage owns only the bytes and hash. Scene does not decode
the bytes during open, preview, checkpoint, or metadata-only commit.

Asset construction accepts metadata from the decoder or encoder already used
by import, pipeline, or rendering. Scene checks structural metadata and blob
existence but does not independently prove arbitrary bytes decode correctly.

## Authorship and generated values

Pipeline reruns must replace their own values without overwriting user edits.
Authorship therefore lives with each replaceable component rather than on one
large entity:

```rust
#[revisioned(revision = 1)]
pub enum Origin {
    User,
    Generated(Generation),
}

#[revisioned(revision = 1)]
pub struct Generation {
    producer: ProducerId,
    model: Option<String>,
    confidence: Option<f32>,
}

#[revisioned(revision = 1)]
pub struct Authored<T> {
    value: T,
    origin: Origin,
}
```

Every component that a pipeline may replace or remove exposes ownership
through `SceneComponent::origin` and accepts automatic stamping through
`set_origin`. This includes text, geometry, roles, regions, analyses, assets,
typography, visibility, reading order, and relations. Ordinary scene setters
stamp `User`. Pipeline editors are created with a `Generation` context and
stamp generated values automatically. Pipeline edits cannot write or remove
unmanaged component kinds.

Entity lifecycle ownership is a separate `EntityOrigin` component. Editing one
generated component does not necessarily claim the entire entity. Operations
that semantically turn a detected entity into a manual entity explicitly
promote lifecycle ownership.

`add_page` and `add_entity` always create the required lifecycle component:
ordinary edits stamp `User`, while `edit_as` stamps that generation. A
pipeline edit may cascade-remove only entities still owned by its producer;
user-owned or other-producer entities are rejected before patch construction.

A stable producer identifies a pipeline responsibility, not a selected model
checkpoint. Ownership is decoded from the affected built-in component when a
pipeline edit checks replacement policy; it is never stored in a parallel
`koharu-storage` index.

Rerun policy is enforced by scene helpers:

- a producer may replace a component still owned by that producer;
- it may not replace a user-owned component without an explicit force action;
- it may remove an entity only while lifecycle ownership remains compatible;
- component removal and relation removal apply the same ownership checks as
  replacement;
- `promote_entity_to_user` and `promote_relation_to_user` protect manually
  adopted generated structure from later producer deletion;
- it reconciles nearby user-owned entities instead of silently deleting them.

## Typed read API

```rust
impl SceneSession {
    pub fn create(path: impl AsRef<Path>) -> Result<Self>;
    pub fn open(path: impl AsRef<Path>) -> Result<Self>;
    pub fn memory() -> Result<Self>;
    pub fn snapshot(&self) -> SceneSnapshot;
    pub fn commit(&mut self, patch: ScenePatch) -> Result<SceneCommit>;
}

impl SceneSnapshot {
    pub fn project_id(&self) -> ProjectId;
    pub fn revision(&self) -> Revision;

    pub fn pages(&self) -> impl ExactSizeIterator<Item = PageRef<'_>>;
    pub fn entities(&self) -> impl ExactSizeIterator<Item = EntityRef<'_>>;
    pub fn subtree(&self, root: EntityId)
        -> Result<impl Iterator<Item = EntityRef<'_>> + '_>;
    pub fn descendants(&self, root: EntityId)
        -> Result<impl Iterator<Item = EntityRef<'_>> + '_>;
    pub fn entities_with<T: SceneComponent>(
        &self,
        slot: impl Into<ComponentSlot>,
    ) -> Result<impl ExactSizeIterator<Item = EntityRef<'_>>>;
    pub fn page(&self, id: EntityId) -> Result<PageRef<'_>>;
    pub fn entity(&self, id: EntityId) -> Result<EntityRef<'_>>;
    pub fn parent(&self, id: EntityId) -> Result<Option<EntityId>>;
    pub fn children(&self, id: EntityId)
        -> Result<impl ExactSizeIterator<Item = EntityId> + '_>;

    pub fn component<T: SceneComponent>(
        &self,
        entity: EntityId,
        slot: impl Into<ComponentSlot>,
    ) -> Result<Option<T>>;

    pub fn relations_from(
        &self,
        entity: EntityId,
        kind: Option<&RelationKind>,
    ) -> impl Iterator<Item = RelationRef<'_>>;

    pub fn read_blob(&self, id: BlobId) -> Result<Arc<[u8]>>;
    pub fn read_blobs(
        &self,
        ids: impl IntoIterator<Item = BlobId>,
    ) -> Result<BlobBatch>;
}
```

`PageRef`, `EntityRef`, and `RelationRef` borrow the snapshot and decode only
requested components. There is no public `ElementContent` match required to
reach text or region data. Capability queries inspect component presence:

```rust
if let Some(text) = entity.component::<SourceText>("default")? {
    // This entity has source text capability.
}
```

Frequently used built-in components may have a snapshot-local decoded cache.
It is bounded, immutable from the caller's perspective, and never durable.

## Ergonomic editing

```rust
impl SceneSnapshot {
    pub fn edit(&self) -> SceneEdit;
    pub fn edit_as(&self, generation: Generation) -> SceneEdit;
    pub fn patch(
        &self,
        f: impl FnOnce(&mut SceneEdit) -> Result<()>,
    ) -> Result<ScenePatch>;
}

impl SceneEdit {
    pub fn add_page(&mut self, page: PageDraft, at: At) -> Result<EntityId>;
    pub fn add_entity(
        &mut self,
        parent: EntityId,
        at: At,
    ) -> Result<EntityId>;
    pub fn move_entity(
        &mut self,
        entity: EntityId,
        parent: Option<EntityId>,
        at: At,
    ) -> Result<()>;
    pub fn remove_entity(
        &mut self,
        entity: EntityId,
        policy: RemovePolicy,
    ) -> Result<()>;
    pub fn promote_entity_to_user(&mut self, entity: EntityId) -> Result<()>;

    pub fn set<T: SceneComponent>(
        &mut self,
        entity: EntityId,
        slot: impl Into<ComponentSlot>,
        value: &T,
    ) -> Result<()>;
    pub fn remove<T: SceneComponent>(
        &mut self,
        entity: EntityId,
        slot: impl Into<ComponentSlot>,
    ) -> Result<()>;

    pub fn set_translation(
        &mut self,
        entity: EntityId,
        locale: &LanguageTag,
        value: Translation,
    ) -> Result<()>;
    pub fn set_asset(
        &mut self,
        entity: EntityId,
        role: &AssetRole,
        value: AssetInput,
    ) -> Result<()>;
    pub fn promote_relation_to_user(&mut self, relation: RelationId)
        -> Result<()>;

    pub fn finish(self) -> Result<ScenePatch>;
}
```

Domain convenience methods call the same generic typed `set` and storage edit
path. They do not build a second command representation.

`SceneEdit` updates a private storage edit and its working scene index together.
Errors occur at the operation that violates hierarchy, authorship, relation,
or component rules. Later edits can use newly inserted entities.

Setters accept semantic values, clear optional values with named removal
methods, and return IDs directly. Callers do not repeatedly pass a page ID when
the parent index already knows it.

## Scene patches and pipeline branches

`ScenePatch` wraps one `koharu_storage::Patch`. It adds no parallel operation
list or attachment map.

```rust
impl SceneSnapshot {
    pub fn preview<'a>(
        &self,
        patches: impl IntoIterator<Item = &'a ScenePatch>,
    ) -> Result<SceneSnapshot>;
}

impl ScenePatch {
    pub fn merge<'a>(
        patches: impl IntoIterator<Item = &'a ScenePatch>,
    ) -> Result<ScenePatch>;

    pub fn project_id(&self) -> ProjectId;
    pub fn base_revision(&self) -> Revision;
    pub fn fingerprint(&self) -> PatchId;
}
```

Preview delegates ancestry, preconditions, structural references, and opaque
component conflicts to storage. A patch created by `SceneEdit` retains its
validated result index. Exact ancestor previews reuse that index. Component-
only combinations validate affected components and update the private
persistent component-membership index; record lifecycle or structural changes
fall back to a full authoritative index rebuild.

Merge delegates ancestry and component-level conflicts to storage. Semantic
validation needs a base snapshot, so it occurs when the merged patch is
previewed or committed; invalid hierarchy, relation, authorship, and
cross-component results are rejected before durable mutation.

For the pipeline graph:

```text
Detection --> OCR --> Translation
    |--------> Typography
    `--------> Inpainting
```

the intended component ownership is:

- detection creates entities, geometry, region, and lifecycle-origin
  components;
- OCR adds or replaces source-text components;
- translation adds locale-slotted translation components;
- typography adds typography-intent components;
- inpainting adds a page asset in its own role slot.

Dependent nodes run on snapshots previewing exactly their ancestor patches.
Independent nodes write different component keys and can run concurrently.
Attempting to replace the same component from sibling branches conflicts.

The pipeline merges in canonical `petgraph` topological order and commits one
`ScenePatch`. No model receives a `SceneSession`, and intermediate previews do
not touch SQLite.

## Scene commit validation

`SceneSession::commit` does not blindly forward a patch:

1. Confirm the patch belongs to the wrapped storage document and base revision.
2. Preview through storage without durable mutation.
3. Decode and validate affected known components.
4. Validate the affected hierarchy, relation endpoints, asset references, and
   authorship rules.
5. Rebuild affected scene indexes and compare them in debug/test builds.
6. Delegate the unchanged storage patch to `koharu_storage::Session::commit`.
7. Wrap the committed storage snapshot and produce a scene change summary.

Storage repeats its own structural and head-revision checks inside the atomic
commit. Scene validation never weakens storage validation.

SQLite failure or stale revision leaves both storage and scene state unchanged.

## Validation

Scene validation covers semantics absent from storage:

- required project root components and supported structural schemas;
- page ordering and page component presence;
- one-parent containment, no cycles, bounded depth, and no relation records in
  hierarchy;
- relation marker uniqueness and valid typed endpoints;
- finite geometry and valid coordinate invariants;
- locale, text, asset-role, media metadata, confidence, and producer syntax;
- component-specific record/blob reference extraction;
- generated ownership and rerun permissions;
- cross-component requirements, such as a translation requiring a compatible
  text-capable entity.

Validation is scoped during owned edits and component-only preview/commit
paths. A translation edit does not decode asset bytes on unrelated pages.
Open, refresh from an external writer, undo, and structural patches perform a
full semantic validation and authoritative index rebuild. Debug/test builds
also retain full storage consistency reconstruction. Exact patch ancestry and
component-only branches reuse or persistently update validated indexes.
Storage independently validates every replayed structural operation.

Unknown extension components are structurally preserved but semantically
ignored. Extensions that need cross-component enforcement must provide a
separate trusted domain wrapper; `koharu-scene` does not run arbitrary plugin
code while opening a project.

## Compatibility and evolution

The compatibility contract is:

1. Storage checkpoint and commit evolution remains the responsibility of
   `koharu-storage`.
2. Every built-in scene component has an independent `revision` history.
3. New scene versions must decode all released older versions of each built-in
   component.
4. Adding an optional field changes only that component payload revision.
5. Adding a component kind or slot requires no storage or SQLite migration.
6. Unknown optional components remain byte-identical across unrelated edits.
7. Old scene versions need not understand newer required structural schemas.
8. A semantic replacement gets a new component kind; stable keys are never
   silently reused for incompatible meaning.
9. Component encodings are deterministic and reference extraction is tested.

CI keeps golden bytes and semantic fixtures for every released component
revision. Fixtures test added fields, defaults, references, validation, and
round-trip preservation through storage history.

## Change summaries

```rust
pub struct SceneChangeSet {
    pub from: Revision,
    pub to: Revision,
    pub entities: Vec<EntityChange>,
    pub components: Vec<ComponentChange>,
    pub relations: Vec<RelationChange>,
    pub pages_changed: bool,
}
```

Scene derives this from the storage change set and affected typed components.
It contains IDs and semantic change kinds, not copied component payloads.
Consumers read new values from `SceneCommit::snapshot`.

Entries are deterministic: pages and hierarchy entities follow semantic order;
components sort by key; relations sort by ID. Renderer and application adapters
can derive damage or protocol events without diffing whole snapshots.

## Blob handling

Scene delegates bytes, attachment deduplication, lazy reads, snapshot pins, and
GC to storage.

`AssetInput` combines encoded bytes with scene metadata. `SceneEdit::set_asset`
attaches bytes through storage, receives a `BlobId`, encodes the asset component
with that blob reference, and writes the component in one edit.

The scene crate has no `image` dependency. Import, pipeline, and renderer own
decoding. A pipeline run-local cache may share decoded `Arc<DynamicImage>`
values while the scene snapshot shares encoded `Arc<[u8]>` values.

No scene validation, preview, checkpoint, traversal, or component query reads
blob payload bytes unless the caller explicitly requests them.

## Concurrency contract

- `SceneSession` is synchronous, single-writer, and not `Sync`.
- `SceneSnapshot`, `ScenePatch`, component values, and read views are
  `Send + Sync` where their payload types allow it.
- Snapshot clone is O(1) plus an `Arc` clone of the derived index.
- Repeated `SceneSession::snapshot` calls clone the cached current snapshot;
  they do not rebuild the scene index.
- Metadata reads and hierarchy queries require no mutex.
- Preview and patch construction are entirely in memory.
- Multiple pipeline branches can construct patches concurrently.
- Component decoding caches never hold locks during caller work.
- One storage head comparison arbitrates concurrent durable writers.
- Old scene snapshots remain immutable and retain storage blob pins.

The scene does not add a global run lock or model lock.

## Performance contract

| Operation | Target cost |
| --- | --- |
| Clone scene snapshot or patch | O(1) |
| Entity lookup | Storage record lookup plus typed view construction |
| Component lookup | Storage component lookup plus lazy decode |
| Parent or child query | Derived parent lookup or decoded ordered child component |
| Add/replace one component | Storage and scene persistent-map path updates |
| Build scene preview | Storage preview plus validation/index work for affected scope |
| Merge pipeline patches | Storage ancestry/conflict work plus scoped scene validation |
| Commit | Scene validation plus one storage transaction and one revision |
| Read asset bytes | Explicit lazy storage blob read |

Required invariants:

- scene wrappers never clone the complete storage document;
- one component change does not decode or serialize sibling components;
- derived indexes are shared between snapshot clones;
- unknown components are not decoded;
- hierarchy and relation indexes are rebuilt from authoritative components on
  structural boundaries and shared across component-only previews; they are
  never checkpointed separately;
- pipeline nodes with separate component keys do not conflict;
- task completion order cannot change a canonical merged result;
- a successful pipeline run creates one storage revision;
- metadata-only work performs no blob reads or image decoding.

The `scene` Criterion benchmark uses a multi-thousand-entity project and covers
snapshot clone, component-indexed query, component patch construction, and
component-only preview. Larger fixture benchmarks additionally report branch
merge and full structural index-rebuild costs.

## Module layout

```text
src/
  lib.rs             public scene facade
  id.rs              ProjectId, EntityId, RelationId, ProducerId
  component.rs       SceneComponent codec boundary and component keys
  components.rs      independently versioned built-in scene components
  index.rs           derived pages, parents, children, and relation adjacency
  snapshot.rs        typed immutable reads and scene validation boundary
  edit.rs            typed generic and ergonomic editing
  patch.rs           ScenePatch preview/merge wrapper
  session.rs         SceneSession open/commit/refresh/undo facade
  change.rs          semantic change summaries
  error.rs           scene errors and storage error mapping
```

There is no SQLite module, blob cache, patch-operation duplicate, graph arena,
image decoder, renderer cache, or public closed element enum.

## Dependency shape

Direct dependencies are limited to:

- `koharu-storage` for all durable state and patch mechanics;
- `imbl` for private persistent derived indexes shared by scene snapshots;
- `revision` for built-in component evolution;
- `serde` and `specta` only for public component/protocol integration that
  genuinely needs them;
- small language-tag or value-validation dependencies when justified.

Do not add `rusqlite`, `blake3`, a second blob cache, `petgraph`, `image`, an
async runtime, model crates, renderer crates, or frontend application crates.

`koharu-storage` must never depend on `koharu-scene`.

## Removed rather than migrated

The scene rewrite does not preserve:

- the current re-export facade;
- the former `Page` containing an embedded element map;
- `ElementContent` or any other closed union of scene entity kinds;
- fixed page asset fields;
- whole-element writes for text, translation, geometry, or typography changes;
- duplicated persistence, history, blob, revision, or patch code;
- raw storage operations in the normal public API;
- worker/shared-memory transfer types;
- current Serde/Specta shapes or old scene project files;
- decoded images, shaped text, or render intermediates.

Useful domain names may be reused only where their new component semantics
match this contract. There is no compatibility adapter requirement.

## Verification

The redesign is complete when tests and benchmarks prove:

1. `SceneSession`, `SceneSnapshot`, `SceneEdit`, and `ScenePatch` contain one
   storage representation rather than mirrored scene state.
2. Adding or replacing one typed component changes exactly one storage
   component address plus any explicitly affected hierarchy component.
3. Unknown extension components survive typed scene edits, preview, commit,
   checkpoint, replay, undo, and backup byte-for-byte.
4. Golden fixtures prove new component revisions read every released older
   revision, including newly added optional fields and defaults.
5. Full scene index reconstruction equals incrementally maintained hierarchy
   and relation indexes after generated edit sequences.
6. Property tests never produce multiple parents, cycles, duplicate children,
   hierarchy relations, dangling endpoints, or invalid root page order.
7. Typed component codecs declare exactly the record and blob references in
   their logical values.
8. Independent sibling pipeline patches write different components on one
   entity successfully; same-component and same-children conflicts fail.
9. An ancestor patch can insert an entity and a descendant patch can add
   components to it, while missing ancestry is rejected by storage.
10. Every permutation of pipeline task completion yields the same scene when
    patches are merged in canonical dependency order.
11. User-owned component values survive producer reruns; producer-owned values
    can be replaced only by the compatible producer policy.
12. Scene validation failure, cancellation, merge conflict, stale revision,
    and SQLite failure leave the durable storage document unchanged.
13. One successful full pipeline patch creates exactly one revision and one
    undo unit.
14. Asset previews are readable before commit, while metadata paths never load
    encoded bytes.
15. Old snapshots remain valid through later commits and storage GC.
16. Large-project benchmarks confirm O(1) snapshot clones, component-local
    edits, scoped validation, shared derived indexes, and bounded lazy decode
    caches.
