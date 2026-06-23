# Storage Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Jyowo storage explicitly event-sourced: journal as the only business fact source, projection/index stores as rebuildable acceleration, keyring for secrets, and Rust backend as the persistence boundary.

**Architecture:** Keep `EventStore` as the source of truth. Add durable manifest and projection cache contracts in `jyowo-harness-journal`, wire them through `jyowo-harness-sdk`, then assemble desktop paths in the Tauri shell. Add file-backed memory and signed permission persistence as independent later stages because they change restart behavior.

**Tech Stack:** Rust 1.96, Tauri 2, `jyowo-harness-journal`, `jyowo-harness-sdk`, `jyowo-harness-memory`, `jyowo-harness-permission`, `serde`, `serde_json`, `rusqlite` later for optional indexes, `keyring`, `cargo test`, `pnpm check:rust`.

---

## Preconditions

- Read `AGENTS.md`.
- Read backend docs in order:
  - `docs/backend/agent-harness-backend-development-guidelines.md`
  - `docs/backend/backend-runtime.md`
  - `docs/backend/backend-engineering.md`
  - `docs/backend/backend-quality.md`
- Keep dependency direction:
  - desktop shell -> `jyowo-harness-sdk` -> lower crates
  - no lower crate depends on desktop shell
- Do not move raw secrets into journal, projection, index, logs, exports, tests, or frontend state.
- Do not make SQLite the main fact source in this plan.

## Target Runtime Layout

```text
<workspace>/.jyowo/runtime/
  events/
    _manifest.json
    <tenant>/<session>.<offset>.jsonl
    <tenant>/snapshots/<session>.json
    <tenant>/_compaction_lineage.jsonl

  projections/
    _manifest.json
    session/<tenant>/<session>.json

  indexes/
    search.db

  memory/
    records/<tenant>.json

  permissions/
    decisions/<tenant>.json

  exports/
    ...

  provider-settings.json
  mcp-servers.json
```

## File Map

Create:

- `crates/jyowo-harness-journal/src/manifest.rs`  
  Owns persisted manifest types for journal/projection/index stores.
- `crates/jyowo-harness-journal/src/projection_cache.rs`  
  Owns projection cache metadata, trait, and JSON file implementation.
- `crates/jyowo-harness-journal/tests/manifest.rs`  
  Covers manifest creation, parse errors, and no-secret shape.
- `crates/jyowo-harness-journal/tests/projection_cache.rs`  
  Covers cache save/load/invalidation behavior.
- `crates/jyowo-harness-memory/src/file.rs`  
  File-backed `MemoryProvider` for structured `MemoryRecord` facts.
- `crates/jyowo-harness-memory/tests/file_store.rs`  
  Covers memory restart persistence and visibility filtering.
- `apps/desktop/src-tauri/src/storage.rs`  
  Owns desktop runtime storage paths.

Modify:

- `crates/jyowo-harness-journal/src/lib.rs`
- `crates/jyowo-harness-journal/src/jsonl.rs`
- `crates/jyowo-harness-journal/src/sqlite.rs`
- `crates/jyowo-harness-journal/src/projection.rs`
- `crates/jyowo-harness-journal/Cargo.toml`
- `crates/jyowo-harness-sdk/src/builder.rs`
- `crates/jyowo-harness-sdk/src/builtin.rs`
- `crates/jyowo-harness-sdk/src/harness.rs`
- `crates/jyowo-harness-sdk/Cargo.toml`
- `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- `crates/jyowo-harness-sdk/tests/sdk_session_flow.rs`
- `crates/jyowo-harness-permission/src/stream.rs`
- `crates/jyowo-harness-permission/Cargo.toml`
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/src-tauri/src/commands.rs`
- `apps/desktop/src-tauri/Cargo.toml`
- `apps/desktop/src-tauri/tests/commands.rs`
- `docs/backend/backend-engineering.md`
- `docs/backend/backend-quality.md`

Do not touch frontend UI files unless a command payload changes. This plan should not change IPC response shapes.

---

## Milestone 1: Journal Manifest

### Task 1: Add Manifest Contract

**Files:**

- Create: `crates/jyowo-harness-journal/src/manifest.rs`
- Modify: `crates/jyowo-harness-journal/src/lib.rs`
- Test: `crates/jyowo-harness-journal/tests/manifest.rs`

- [ ] **Step 1: Write failing manifest tests**

Add tests that assert:

- `JournalManifest::new(StoreKind::Jsonl)` serializes with `schemaVersion`, `storeKind`, and `eventSchemaVersion`.
- manifest JSON does not contain `api_key`, `token`, `secret`, or `password`.
- malformed `schemaVersion` fails to parse.

Use this expected shape:

```json
{
  "schemaVersion": 1,
  "storeKind": "jsonl",
  "eventSchemaVersion": 1
}
```

Run:

```bash
cargo test -p jyowo-harness-journal manifest -- --nocapture
```

Expected: FAIL because the manifest module does not exist.

- [ ] **Step 2: Implement manifest types**

Create:

```rust
#[derive(Debug, Clone, Copy, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreKind {
    Jsonl,
    Sqlite,
    ProjectionCache,
    Index,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalManifest {
    pub schema_version: u32,
    pub store_kind: StoreKind,
    pub event_schema_version: u32,
}

impl JournalManifest {
    pub const CURRENT_SCHEMA_VERSION: u32 = 1;
    pub const CURRENT_EVENT_SCHEMA_VERSION: u32 = 1;

    #[must_use]
    pub const fn new(store_kind: StoreKind) -> Self {
        Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            store_kind,
            event_schema_version: Self::CURRENT_EVENT_SCHEMA_VERSION,
        }
    }
}
```

Export it from `lib.rs`.

- [ ] **Step 3: Verify**

Run:

```bash
cargo test -p jyowo-harness-journal manifest -- --nocapture
```

Expected: PASS.

### Task 2: Persist Manifest From Event Stores

**Files:**

- Modify: `crates/jyowo-harness-journal/src/jsonl.rs`
- Modify: `crates/jyowo-harness-journal/src/sqlite.rs`
- Test: `crates/jyowo-harness-journal/tests/manifest.rs`

- [ ] **Step 1: Write failing store tests**

Add tests that:

- opening `JsonlEventStore` creates `<root>/_manifest.json`.
- opening `SqliteEventStore` records manifest metadata in `kv_meta`.
- reopening with the same manifest succeeds.
- replacing `storeKind` with a different kind fails closed.

Run:

```bash
cargo test -p jyowo-harness-journal --features jsonl,sqlite manifest -- --nocapture
```

Expected: FAIL because stores do not persist manifests.

- [ ] **Step 2: Implement JSONL manifest write**

In `JsonlEventStore::open_with_options`, after `create_dir_all`, call a helper:

```rust
ensure_manifest(&root.join("_manifest.json"), JournalManifest::new(StoreKind::Jsonl))?;
```

The helper must:

- create the file atomically when missing.
- parse and validate when present.
- reject mismatched `storeKind`.
- reject unsupported `schemaVersion`.

- [ ] **Step 3: Implement SQLite manifest metadata**

In `SqliteEventStore::open`, after `kv_meta` exists:

- read `storage.manifest`.
- if missing, write `serde_json::to_string(&JournalManifest::new(StoreKind::Sqlite))`.
- if present, parse and validate.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p jyowo-harness-journal --features jsonl,sqlite manifest -- --nocapture
cargo test -p jyowo-harness-journal --features jsonl,sqlite l1b_stores -- --nocapture
```

Expected: PASS.

---

## Milestone 2: Projection Cache

### Task 3: Add Projection Cache Store

**Files:**

- Create: `crates/jyowo-harness-journal/src/projection_cache.rs`
- Modify: `crates/jyowo-harness-journal/src/lib.rs`
- Modify: `crates/jyowo-harness-journal/src/projection.rs`
- Test: `crates/jyowo-harness-journal/tests/projection_cache.rs`

- [ ] **Step 1: Write failing cache tests**

Add tests that:

- saving a session projection writes `projections/session/<tenant>/<session>.json`.
- loading with the same `schemaVersion` and `sourceOffset` returns the value.
- loading with a lower `sourceOffset` returns stale/missing.
- corrupt JSON returns an error and does not silently rebuild inside the store.

Run:

```bash
cargo test -p jyowo-harness-journal projection_cache -- --nocapture
```

Expected: FAIL because the cache store does not exist.

- [ ] **Step 2: Make `SessionProjection` serializable**

Add derives:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionProjection {
    pub messages: Vec<Message>,
    pub usage: UsageSnapshot,
    pub end_reason: Option<EndReason>,
    pub last_offset: JournalOffset,
}
```

- [ ] **Step 3: Implement cache metadata**

Use this record shape:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionCacheRecord<T> {
    pub schema_version: u32,
    pub projection_kind: ProjectionKind,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub source_offset: JournalOffset,
    pub value: T,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionKind {
    Session,
}
```

Use a trait that stores typed JSON without making callers know file paths:

```rust
#[async_trait::async_trait]
pub trait ProjectionCacheStore: Send + Sync + 'static {
    async fn load_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<ProjectionCacheRecord<SessionProjection>>, JournalError>;

    async fn save_session(
        &self,
        record: ProjectionCacheRecord<SessionProjection>,
    ) -> Result<(), JournalError>;

    async fn invalidate_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<(), JournalError>;
}
```

- [ ] **Step 4: Implement file cache**

Add:

```rust
pub struct FileProjectionCacheStore {
    root: PathBuf,
}
```

Rules:

- path: `<root>/session/<tenant>/<session>.json`
- write through temp file + `sync_all` + rename.
- create `<root>/_manifest.json` using `StoreKind::ProjectionCache`.
- no fallback to stale data.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p jyowo-harness-journal projection_cache -- --nocapture
cargo test -p jyowo-harness-journal --features jsonl,sqlite -- --nocapture
```

Expected: PASS.

### Task 4: Add In-Memory Projection Cache For Tests

**Files:**

- Modify: `crates/jyowo-harness-journal/src/projection_cache.rs`
- Test: `crates/jyowo-harness-journal/tests/projection_cache.rs`

- [ ] **Step 1: Write failing test**

Add a test that uses `InMemoryProjectionCacheStore` to save and load two different sessions.

- [ ] **Step 2: Implement test cache**

Implement:

```rust
#[derive(Default)]
pub struct InMemoryProjectionCacheStore {
    sessions: tokio::sync::Mutex<HashMap<(TenantId, SessionId), ProjectionCacheRecord<SessionProjection>>>,
}
```

Gate it with:

```rust
#[cfg(any(test, feature = "in-memory", feature = "mock"))]
```

- [ ] **Step 3: Verify**

Run:

```bash
cargo test -p jyowo-harness-journal projection_cache -- --nocapture
```

Expected: PASS.

---

## Milestone 3: SDK Integration

### Task 5: Wire Projection Cache Through Harness Builder

**Files:**

- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `crates/jyowo-harness-sdk/src/builtin.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`

- [ ] **Step 1: Write failing builder test**

Add a test proving a harness built with `with_projection_cache_store_arc` exposes runtime feature `"projection-cache"` from `runtime_feature_snapshot`.

Run:

```bash
cargo test -p jyowo-harness-sdk runtime_assembly -- --nocapture
```

Expected: FAIL because the builder method does not exist.

- [ ] **Step 2: Add builder field**

Add to builder extras:

```rust
pub(crate) projection_cache_store: Option<Arc<dyn ProjectionCacheStore>>,
```

Add public builder methods:

```rust
#[must_use]
pub fn with_projection_cache_store<S>(self, store: S) -> Self
where
    S: ProjectionCacheStore,
{
    self.with_projection_cache_store_arc(Arc::new(store))
}

#[must_use]
pub fn with_projection_cache_store_arc(
    mut self,
    store: Arc<dyn ProjectionCacheStore>,
) -> Self {
    self.extras.projection_cache_store = Some(store);
    self
}
```

- [ ] **Step 3: Add harness field**

Add to `HarnessInner`:

```rust
projection_cache_store: Option<Arc<dyn ProjectionCacheStore>>,
```

Move builder extras into it during `build`.

- [ ] **Step 4: Re-export builtins**

In `builtin.rs`, re-export:

```rust
pub use harness_journal::{FileProjectionCacheStore, InMemoryProjectionCacheStore};
```

Apply the same cfg gates used in journal.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p jyowo-harness-sdk runtime_assembly -- --nocapture
```

Expected: PASS.

### Task 6: Cache Session Projection Rebuilds

**Files:**

- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Test: `crates/jyowo-harness-sdk/tests/sdk_session_flow.rs`

- [ ] **Step 1: Write failing session cache tests**

Add tests that:

- first read of a session writes a cache record at the latest event offset.
- second read with unchanged journal uses the cached projection value.
- appending another event makes the old cache stale and rewrites it.

Run:

```bash
cargo test -p jyowo-harness-sdk sdk_session_flow -- --nocapture
```

Expected: FAIL because session reads always replay directly.

- [ ] **Step 2: Add helper**

Add a helper in `Harness`:

```rust
async fn project_sdk_session(
    &self,
    options: &SessionOptions,
    envelopes: &[EventEnvelope],
) -> Result<SessionProjection, HarnessError>
```

Rules:

- If no projection cache store is configured, replay from envelopes.
- If envelopes are empty, return `SessionProjection::default()`.
- Latest source offset is `envelopes.last().offset`.
- Load cache.
- Use cache only when `schemaVersion` is current and `sourceOffset == latest`.
- On stale/missing cache, replay and save.
- On corrupt cache load error, fail closed. Do not silently ignore corruption.

- [ ] **Step 3: Replace direct replay**

In `read_sdk_session_state`, replace:

```rust
SessionProjection::replay(envelopes.clone())
```

with:

```rust
self.project_sdk_session(options, &envelopes).await
```

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p jyowo-harness-sdk sdk_session_flow -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly -- --nocapture
```

Expected: PASS.

---

## Milestone 4: Desktop Runtime Layout

### Task 7: Centralize Desktop Storage Paths

**Files:**

- Create: `apps/desktop/src-tauri/src/storage.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`

- [ ] **Step 1: Write failing path tests**

Add tests that:

- `DesktopStorageLayout::new(workspace).events_dir()` returns `.jyowo/runtime/events`.
- provider settings path is `.jyowo/runtime/provider-settings.json`.
- MCP settings path is `.jyowo/runtime/mcp-servers.json`.
- exports path is `.jyowo/runtime/exports`.
- paths stay under workspace even when workspace contains symlink-free normal components.

Run:

```bash
cargo test -p jyowo-desktop-shell storage -- --nocapture
```

Expected: FAIL because `storage.rs` does not exist.

- [ ] **Step 2: Implement layout**

Create:

```rust
#[derive(Debug, Clone)]
pub struct DesktopStorageLayout {
    workspace_root: PathBuf,
}

impl DesktopStorageLayout {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self;
    pub fn runtime_dir(&self) -> PathBuf;
    pub fn events_dir(&self) -> PathBuf;
    pub fn projections_dir(&self) -> PathBuf;
    pub fn indexes_dir(&self) -> PathBuf;
    pub fn memory_records_dir(&self) -> PathBuf;
    pub fn permission_decisions_dir(&self) -> PathBuf;
    pub fn exports_dir(&self) -> PathBuf;
    pub fn provider_settings_path(&self) -> PathBuf;
    pub fn mcp_servers_path(&self) -> PathBuf;
}
```

- [ ] **Step 3: Replace duplicated path construction**

Replace direct `.join(".jyowo").join("runtime")...` in `commands.rs` for:

- event store root
- provider settings
- MCP settings
- memory export
- support bundle export

Do not change returned relative export paths.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p jyowo-desktop-shell storage -- --nocapture
cargo test -p jyowo-desktop-shell commands -- --nocapture
```

Expected: PASS.

### Task 8: Wire File Projection Cache Into Desktop Harness

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Test: `apps/desktop/src-tauri/tests/commands.rs`

- [ ] **Step 1: Write failing restart test**

Add a test that:

- creates a runtime under a temp workspace.
- starts a conversation turn or appends a minimal session through existing helpers.
- reads the conversation once.
- asserts `.jyowo/runtime/projections/session/...` exists.
- rebuilds runtime state for the same workspace.
- reads the conversation again and gets the same messages.

Run:

```bash
cargo test -p jyowo-desktop-shell projection -- --nocapture
```

Expected: FAIL because desktop does not wire a projection cache.

- [ ] **Step 2: Add feature exposure if needed**

If `FileProjectionCacheStore` is behind a feature, expose it through `jyowo-harness-sdk` and enable that feature in `apps/desktop/src-tauri/Cargo.toml`.

- [ ] **Step 3: Wire cache**

In `build_desktop_harness`, add:

```rust
let storage = DesktopStorageLayout::new(workspace_root);
let projection_cache = FileProjectionCacheStore::open(storage.projections_dir()).await?;
```

Then add to builder:

```rust
.with_projection_cache_store(projection_cache)
```

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p jyowo-desktop-shell projection -- --nocapture
cargo test -p jyowo-desktop-shell commands -- --nocapture
```

Expected: PASS.

---

## Milestone 5: File-Backed Memory Facts

### Task 9: Add Structured File Memory Provider

**Files:**

- Create: `crates/jyowo-harness-memory/src/file.rs`
- Modify: `crates/jyowo-harness-memory/src/lib.rs`
- Modify: `crates/jyowo-harness-memory/Cargo.toml`
- Test: `crates/jyowo-harness-memory/tests/file_store.rs`

- [ ] **Step 1: Write failing memory persistence tests**

Add tests that:

- `upsert` writes a `MemoryRecord`.
- reopening the provider returns the record from `get`.
- `list(ForActor(...))` applies current visibility filtering.
- `forget` removes the record after restart.
- corrupt JSON fails closed.

Run:

```bash
cargo test -p jyowo-harness-memory file_store -- --nocapture
```

Expected: FAIL because there is no file-backed provider.

- [ ] **Step 2: Add feature**

Add:

```toml
file-store = []
```

Use cfg:

```rust
#[cfg(feature = "file-store")]
pub mod file;
#[cfg(feature = "file-store")]
pub use file::*;
```

- [ ] **Step 3: Implement provider**

Create:

```rust
pub struct FileMemoryProvider {
    provider_id: String,
    root: PathBuf,
}
```

Rules:

- path: `<root>/records/<tenant>.json`
- file shape: `Vec<MemoryRecord>`
- all writes are temp file + `sync_all` + rename.
- reuse the same filtering semantics as `MockMemoryProvider`.
- no raw provider secrets are stored.
- corrupt JSON returns `MemoryError::Io` or `MemoryError::Message`; it must not reset data silently.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p jyowo-harness-memory file_store -- --nocapture
cargo test -p jyowo-harness-memory --features file-store -- --nocapture
```

Expected: PASS.

### Task 10: Use File Memory In Desktop

**Files:**

- Modify: `crates/jyowo-harness-sdk/Cargo.toml`
- Modify: `crates/jyowo-harness-sdk/src/builtin.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`

- [ ] **Step 1: Write failing desktop memory restart test**

Add a test that:

- creates a memory item through the SDK facade.
- rebuilds `runtime_state_for_workspace`.
- lists memory items.
- sees the same item.

Run:

```bash
cargo test -p jyowo-desktop-shell memory -- --nocapture
```

Expected: FAIL because desktop currently uses process-local memory.

- [ ] **Step 2: Expose SDK feature**

Add SDK feature:

```toml
memory-file-store = ["jyowo-harness-memory/file-store"]
```

Re-export:

```rust
#[cfg(feature = "memory-file-store")]
pub use harness_memory::FileMemoryProvider;
```

- [ ] **Step 3: Enable desktop feature**

In desktop Cargo, replace process-local memory use with:

```toml
"memory-file-store"
```

Keep `memory-external-slot` if the SDK still needs the external memory manager path.

- [ ] **Step 4: Wire provider**

Replace:

```rust
.with_memory_provider(InMemoryMemoryProvider::new("desktop-memory"))
```

with:

```rust
.with_memory_provider(FileMemoryProvider::open(
    "desktop-memory",
    storage.memory_records_dir(),
)?)
```

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p jyowo-desktop-shell memory -- --nocapture
pnpm check:rust
```

Expected: PASS.

---

## Milestone 6: Signed Permission Persistence

### Task 11: Allow Stream Permission Runtime Persistence

**Files:**

- Modify: `crates/jyowo-harness-permission/Cargo.toml`
- Modify: `crates/jyowo-harness-permission/src/stream.rs`
- Modify: `crates/jyowo-harness-sdk/Cargo.toml`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Test: `crates/jyowo-harness-sdk/tests/facade.rs`

- [ ] **Step 1: Write failing SDK persistence test**

Add a test proving `StreamPermissionRuntime` can be constructed with a trusted `DecisionPersistence` and that resolved decisions are delegated to persistence.

Run:

```bash
cargo test -p jyowo-harness-sdk facade -- --nocapture
```

Expected: FAIL because `StreamPermissionRuntime::default()` always uses `NoopDecisionPersistence`.

- [ ] **Step 2: Expose feature**

Add SDK feature:

```toml
permission-integrity = ["jyowo-harness-permission/integrity"]
```

- [ ] **Step 3: Add constructor**

Add to SDK `StreamPermissionRuntime`:

```rust
pub fn new_with_persistence(
    config: StreamBrokerConfig,
    persistence: Arc<dyn DecisionPersistence>,
) -> Self
```

It must call:

```rust
let (broker, mut receiver, resolver) = StreamBasedBroker::new(config);
let broker = broker.with_persistence(persistence);
```

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p jyowo-harness-sdk facade -- --nocapture
```

Expected: PASS.

### Task 12: Wire Signed Permission File In Desktop

**Files:**

- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`

- [ ] **Step 1: Write failing tamper test**

Add a desktop test that:

- creates permission persistence under `.jyowo/runtime/permissions/decisions/single.json`.
- writes a decision.
- corrupts the JSON.
- rebuilds runtime.
- receives fail-closed runtime initialization error or a permission tamper event.

Run:

```bash
cargo test -p jyowo-desktop-shell permission -- --nocapture
```

Expected: FAIL because desktop does not persist permission decisions.

- [ ] **Step 2: Create signer source**

Use keyring for signing key material:

```text
service: jyowo.permission.integrity
account: <workspace-root-hash>
```

Rules:

- generate random key once if missing.
- never write the raw key to JSON, journal, logs, or tests.
- tests use `StaticSignerStore::from_key`.

- [ ] **Step 3: Create persistence**

Use:

```rust
FileDecisionPersistence::new(
    TenantId::SINGLE,
    storage.permission_decisions_dir().join("single.json"),
    signer,
)
```

Pass it to `StreamPermissionRuntime::new_with_persistence`.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p jyowo-desktop-shell permission -- --nocapture
pnpm check:rust
```

Expected: PASS.

---

## Milestone 7: Optional SQLite Index

### Task 13: Add Derived Runtime Index Contract

**Files:**

- Create: `crates/jyowo-harness-journal/src/index.rs`
- Modify: `crates/jyowo-harness-journal/src/lib.rs`
- Test: `crates/jyowo-harness-journal/tests/index.rs`

- [ ] **Step 1: Write failing index tests**

Add tests that:

- indexing two envelopes makes session ids searchable by text.
- deleting/rebuilding the index from journal produces identical query results.
- redacted event payloads are the only indexed payloads.

Run:

```bash
cargo test -p jyowo-harness-journal --features sqlite index -- --nocapture
```

Expected: FAIL because index contract does not exist.

- [ ] **Step 2: Implement minimal trait**

Add:

```rust
#[async_trait::async_trait]
pub trait RuntimeIndex: Send + Sync + 'static {
    async fn upsert_envelopes(
        &self,
        tenant: TenantId,
        envelopes: &[EventEnvelope],
    ) -> Result<(), JournalError>;

    async fn search_events(
        &self,
        tenant: TenantId,
        query: &str,
        limit: u32,
    ) -> Result<Vec<EventEnvelope>, JournalError>;

    async fn clear_tenant(&self, tenant: TenantId) -> Result<(), JournalError>;
}
```

- [ ] **Step 3: Stop here unless search UX is scheduled**

Do not wire this into desktop until there is a user-facing search/index requirement. The contract can exist, but the first desktop storage milestones do not need SQLite index.

---

## Milestone 8: Documentation And Gates

### Task 14: Update Backend Docs

**Files:**

- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`

- [ ] **Step 1: Update persistence section**

Document:

```text
Journal events are business facts.
Projection stores are rebuildable caches.
Runtime indexes are derived acceleration.
Secrets remain in keyring.
File-backed memory stores structured MemoryRecord facts.
Permission decisions require integrity-backed persistence.
```

- [ ] **Step 2: Update quality section**

Add required coverage for:

- manifest validation.
- projection cache source offset invalidation.
- file memory restart behavior.
- signed permission tamper behavior.
- index rebuild equivalence when SQLite index is introduced.

- [ ] **Step 3: Verify docs**

Run:

```bash
pnpm check:backend-docs
pnpm check:docs
```

Expected: PASS.

### Task 15: Final Verification

**Files:**

- All modified files.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt --all --check
```

Expected: PASS.

- [ ] **Step 2: Rust gate**

Run:

```bash
pnpm check:rust
```

Expected: PASS.

- [ ] **Step 3: Docs gate**

Run:

```bash
pnpm check:docs
```

Expected: PASS.

- [ ] **Step 4: Full gate**

Run:

```bash
pnpm check
```

Expected: PASS.

## Commit Boundaries

Use small commits:

```text
feat(journal): add runtime storage manifests
feat(journal): add file projection cache
feat(sdk): cache session projections by journal offset
feat(desktop): centralize runtime storage layout
feat(memory): add file-backed memory provider
feat(desktop): persist memory records
feat(permission): support signed stream permission persistence
feat(desktop): persist signed permission decisions
docs(backend): document runtime storage boundaries
```

## Self-Review

- Spec coverage: covers journal facts, projections, configs/secrets, exports, memory, permission, and optional SQLite index.
- Placeholder scan: no task relies on unfinished placeholder behavior; each stage has files, tests, and commands.
- Scope check: SQLite search/index is intentionally last and not wired to desktop without a product search requirement.
- Safety check: raw secrets stay in keyring; corrupt projection/memory/permission files fail closed instead of silently resetting durable state.
