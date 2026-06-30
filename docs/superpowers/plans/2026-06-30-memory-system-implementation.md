# Memory System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn Jyowo memory from an in-process external slot into a durable, configurable, auditable memory system with real retrieval, provenance, generation controls, and desktop management.

**Architecture:** Keep Rust runtime as the policy authority. Keep memory below workspace instructions and policy; memory is recall context, not a rule source. Implement durable storage as a `MemoryProvider` in `jyowo-harness-memory`, wire it through `jyowo-harness-sdk`, expose only validated Tauri commands, and keep React as a state/rendering layer.

**Tech Stack:** Rust 1.96, Tauri 2, React, TypeScript, Zod, `jyowo-harness-memory`, `jyowo-harness-contracts`, `jyowo-harness-sdk`, `rusqlite` with SQLite FTS5, existing redaction/threat scanner/event store, `cargo test`, `pnpm check:rust`, `pnpm check:desktop`, `pnpm check`.

---

## Source Design Inputs

Use these external behaviors as design constraints, not as copied implementation:

- Codex memory: default-off or explicitly configurable, local storage, per-thread use/generate controls, background generation after idle, secret redaction, external-context exclusion, and separate `AGENTS.md` for durable rules. Source: <https://developers.openai.com/codex/memories>
- Claude memory: chat search and memory are separate; memory can be paused/reset/viewed/edited; project memory is isolated; citations are shown when prior chats are used. Source: <https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context>
- Claude Code memory: `CLAUDE.md` and auto memory are separate context sources; auto memory is project-local, user-controllable, and bounded in prompt size. Source: <https://code.claude.com/docs/en/memory>
- OpenClaw memory: long-term memory is explicit file/state, daily memory and dreams are separate, search uses SQLite with embedding/BM25 hybrid options, active memory is optional and injected as hidden untrusted context. Sources: <https://docs.openclaw.ai/concepts/memory>, <https://docs.openclaw.ai/concepts/memory-search>, <https://docs.openclaw.ai/concepts/active-memory>
- Cascade memory: auto memories are workspace-local; durable reusable rules belong in Rules or `AGENTS.md`, not memory. Source: <https://docs.devin.ai/desktop/cascade/memories>

## Current Jyowo Facts

- Desktop currently wires `InMemoryMemoryProvider::new("desktop-memory")`, so default memory does not persist across restart: `apps/desktop/src-tauri/src/commands.rs`.
- `InMemoryMemoryProvider::recall` filters by tenant/kind/visibility and returns first N; it does not rank semantically: `crates/jyowo-harness-memory/src/in_memory.rs`.
- `MemoryStore::get` defaults to `NotFound`; browser flows require providers to implement it: `crates/jyowo-harness-memory/src/store.rs`.
- `MemoryManager` already has lifecycle, audit sink, metrics, threat scanner, recall policy, external slot, optional builtin memory, and optional consolidation hook: `crates/jyowo-harness-memory/src/external.rs`.
- `MemoryRecord` already has kind, visibility, metadata, TTL, confidence, access count, and redaction counters: `crates/jyowo-harness-memory/src/types.rs`.
- `MemorySource` already distinguishes user input, agent-derived, subagent-derived, external retrieval, imported, and consolidated sources: `crates/jyowo-harness-contracts/src/enums.rs`.
- The base system prompt already says memory is auxiliary context and not a fact source: `crates/jyowo-harness-sdk/src/system_prompt.rs`.
- Desktop UI can list, inspect, edit, delete, and export memory, but cannot create a memory or review generated candidates: `apps/desktop/src/features/memory/MemoryBrowser.tsx`.
- Tool-result memory recall currently depends on the hardcoded Chinese phrase `需要查阅历史`: `crates/jyowo-harness-context/src/engine.rs`.
- `crates/jyowo-harness-memory/README.md` points to a missing architecture doc.

## Non-Negotiable Rules

- Implement in an isolated git worktree. Do not implement in `/Users/goya/Repo/Git/Jyowo`.
- No production mock, fake provider, hardcoded result, heuristic extraction pretending to be model extraction, or UI-only implementation.
- Tests must exercise real code paths. The durable provider tests must use the real SQLite provider and real temp files.
- Memory is not a policy authority. It cannot override system, runtime policy, workspace instructions, permissions, redaction, or security gates.
- External content is untrusted. MCP/web/tool output must not become durable memory unless an explicit generation policy allows it and provenance records the source.
- Redaction runs before memory storage, candidate storage, journal event emission, export, trace, log, and frontend payload.
- Public IPC payloads must have Rust serde types, TS Zod schemas, and tests.
- No `#[ignore]`, no skipped tests, no compatibility shim that keeps the broken desktop in-memory default as the real path.
- Breaking refactor is allowed when it removes technical debt and simplifies the memory flow. Do not keep dual legacy paths unless a test proves both are required.
- Every task must end with a fresh subagent audit. A task is not complete until the audit returns PASS or all findings are fixed and re-audited.

## Required Worktree Setup

Execution must start here.

The plan file itself must already be committed on the source branch before creating the implementation worktree. If this file is untracked or modified, stop and commit the plan first; otherwise the implementation worktree will not contain the audited plan.

```bash
cd /Users/goya/Repo/Git/Jyowo
git status --short docs/superpowers/plans/2026-06-30-memory-system-implementation.md
git status --short
git branch --list goya/memory-system-runtime
git worktree add ../Jyowo-memory-system-runtime -b goya/memory-system-runtime
cd ../Jyowo-memory-system-runtime
```

Expected:

- `git status --short docs/superpowers/plans/2026-06-30-memory-system-implementation.md` must print nothing.
- `git status --short` may show user changes in the original tree. Do not modify or revert them.
- `git branch --list goya/memory-system-runtime` must print nothing. If it prints a branch, stop and ask before reusing it.
- All implementation commands after this point run in `/Users/goya/Repo/Git/Jyowo-memory-system-runtime`.

## Per-Task Required Ritual

Before editing files in each task, write a short task analysis in the agent response:

```text
Task N analysis:
- Objective:
- Existing code facts:
- Files to touch:
- Tests that must fail before implementation:
- Security constraints:
- What will not be changed:
```

Before marking each task complete:

1. Run the task-specific tests.
2. Run the task-specific gate.
3. Run `git diff --check`.
4. Spawn a fresh review subagent with this exact audit prompt:

```text
Audit Task N in docs/superpowers/plans/2026-06-30-memory-system-implementation.md.

Review only the diff for this task.
Check:
- The stated Task N objective is fully implemented.
- No production mock, fake provider, hardcoded memory result, or UI-only implementation was added.
- The implementation follows Jyowo layer boundaries and Rust remains the policy authority.
- Memory cannot override system/runtime/workspace instructions.
- External content is not persisted as durable memory without explicit policy and provenance.
- Redaction happens before storage, events, export, traces, logs, and frontend payloads.
- Tests cover the new behavior and fail without the implementation.
- Public payloads have Rust serde, TS Zod, and tests when IPC changed.
- No unrelated refactor or compatibility dead code remains.

Return PASS or FAIL.
For FAIL, include file path and line-level findings.
```

If audit returns FAIL, fix the findings, rerun tests/gates, and run the same audit again. Do not commit until audit returns PASS.

## Target Architecture

```text
React Memory UI
  -> shared/tauri/commands.ts Zod validated IPC
  -> apps/desktop/src-tauri/src/commands.rs
  -> jyowo-harness-sdk facade
  -> MemoryManager
  -> SqliteMemoryProvider
  -> <workspace>/.jyowo/runtime/memory/memory.sqlite

Session lifecycle
  -> MemoryRuntimeConfig
  -> recall policy + external-context taint
  -> retrieval context injection as untrusted memory context
  -> session idle generation worker
  -> candidate queue
  -> reviewed/accepted durable memory
```

Design units:

- `MemoryStore` remains the storage/retrieval trait.
- `SqliteMemoryProvider` implements durable provider behavior.
- `MemoryRecallRanker` owns lexical scoring, recency, confidence, TTL, and deterministic ordering.
- `MemoryRuntimeConfig` owns use/generate/session controls.
- `MemoryGenerationWorker` owns session-end idle generation and candidate creation.
- `MemoryCandidateStore` is a real trait boundary. `MemoryManager` must use this trait, not downcast to `SqliteMemoryProvider`.
- `MemoryCandidateStore` must remain separate from `MemoryProvider`; do not extend `MemoryProvider` with candidate methods because plugin memory providers already implement the existing provider trait.
- `MemorySettingsStore` is a real trait boundary read before `MemoryManager` is created. Disabling memory disables recall/generation, not settings reads/writes.
- `MemoryManager` must own separate optional trait-object slots for provider and candidate operations:
  - `external: RwLock<Option<Arc<dyn MemoryProvider>>>`
  - `candidate_store: RwLock<Option<Arc<dyn MemoryCandidateStore>>>`
  - `set_external(...)` configures recall/CRUD provider behavior.
  - `set_candidate_store(...)` configures candidate list/accept/reject/create behavior.
- `jyowo-harness-sdk` must mirror those slots in `BuilderExtras` and `HarnessInner`. It must provide `with_memory_candidate_store(...)` / `with_memory_candidate_store_arc(...)` and `with_memory_settings_store(...)` / `with_memory_settings_store_arc(...)`.
- The same concrete `Arc<SqliteMemoryProvider>` may be coerced into `Arc<dyn MemoryProvider>`, `Arc<dyn MemoryCandidateStore>`, and `Arc<dyn MemorySettingsStore>`, but each trait object must be passed explicitly. Do not recover candidate/settings behavior by downcasting `Arc<dyn MemoryProvider>`.
- `MemoryContextTaint` records whether a session used external content.
- Desktop commands are IPC adapters only.
- React renders and sends commands only; it does not decide memory policy.

Storage path:

```text
<workspace>/.jyowo/runtime/
  memory/
    memory.sqlite
  exports/
    memory-<timestamp>.json
```

The SQLite database lives under `.jyowo/runtime/memory`. Memory exports keep the existing desktop/frontend contract and write under `.jyowo/runtime/exports`, not under `.jyowo/runtime/memory/exports`.

SQLite tables:

```sql
CREATE TABLE memory_records (
  rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  tenant_id TEXT NOT NULL,
  kind_json TEXT NOT NULL,
  visibility_json TEXT NOT NULL,
  content TEXT NOT NULL,
  tags_json TEXT NOT NULL,
  source_json TEXT NOT NULL,
  confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
  access_count INTEGER NOT NULL CHECK (access_count >= 0),
  recall_score REAL NOT NULL,
  ttl_ms INTEGER,
  redacted_segments INTEGER NOT NULL CHECK (redacted_segments >= 0),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_accessed_at TEXT
);

CREATE VIRTUAL TABLE memory_records_fts USING fts5(
  content,
  tags,
  content='memory_records',
  content_rowid='rowid'
);

CREATE TABLE memory_evidence (
  rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  memory_id TEXT NOT NULL,
  evidence_kind TEXT NOT NULL,
  content_hash BLOB,
  event_id TEXT,
  source_ref_json TEXT,
  captured_at TEXT NOT NULL,
  trust_level TEXT NOT NULL,
  FOREIGN KEY(memory_id) REFERENCES memory_records(id) ON DELETE CASCADE
);

CREATE TABLE memory_candidates (
  rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  tenant_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  generation_key TEXT NOT NULL,
  proposed_kind_json TEXT NOT NULL,
  proposed_visibility_json TEXT NOT NULL,
  proposed_tags_json TEXT NOT NULL,
  proposed_confidence REAL NOT NULL CHECK (proposed_confidence >= 0.0 AND proposed_confidence <= 1.0),
  content TEXT NOT NULL,
  source_json TEXT NOT NULL,
  evidence_json TEXT NOT NULL,
  redacted_segments INTEGER NOT NULL CHECK (redacted_segments >= 0),
  status TEXT NOT NULL CHECK (status IN ('pending','accepted','rejected')),
  rejection_reason TEXT,
  created_at TEXT NOT NULL,
  reviewed_at TEXT,
  UNIQUE(tenant_id, generation_key)
);

CREATE TABLE memory_settings (
  scope TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  config_json TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(scope, scope_id)
);
```

`memory_settings` lives in the same SQLite file for operational simplicity, but it must be readable through `MemorySettingsStore` before any `MemoryManager` exists. Do not make runtime settings depend on an already-enabled memory manager.

SQLite schema versioning:

```text
Set PRAGMA user_version = 1 after schema initialization.
On open, read PRAGMA user_version before creating or mutating tables.
Empty database or user_version = 0 -> initialize schema transactionally and set user_version = 1.
user_version = 1 -> verify required tables/indexes/triggers exist.
user_version > 1 -> fail closed with MemoryError::UnsupportedSchemaVersion.
```

Tests must cover empty database initialization, reopen with `user_version = 1`, and fail-closed behavior for a higher unsupported version. Do not silently drop or rebuild an existing non-empty database.

FTS maintenance must use SQLite triggers or explicit updates inside the same transaction. Choose one and test insert/update/delete behavior.

Retrieval scoring for this plan:

```text
base = lexical_score_from_fts_or_substring
freshness = bounded recency boost from updated_at and last_accessed_at
confidence = metadata.confidence
usage = small bounded boost from access_count
ttl = expired records are excluded
final = base * 0.70 + freshness * 0.10 + confidence * 0.15 + usage * 0.05
```

Do not add embeddings in this plan. Add the schema so embeddings can be introduced without rewriting the provider:

```sql
CREATE TABLE memory_embeddings (
  memory_id TEXT PRIMARY KEY,
  model TEXT NOT NULL,
  dims INTEGER NOT NULL,
  vector BLOB NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(memory_id) REFERENCES memory_records(id) ON DELETE CASCADE
);
```

The embedding table remains unused in this plan and must not affect retrieval results.

## File Map

Create:

- `crates/jyowo-harness-memory/src/sqlite.rs`
  Durable SQLite `MemoryProvider`, schema initialization, transactions, CRUD, candidate store, FTS maintenance.
- `crates/jyowo-harness-memory/src/ranking.rs`
  Deterministic lexical scoring and recall ranking.
- `crates/jyowo-harness-memory/src/config.rs`
  `MemoryRuntimeConfig`, `MemoryGenerationConfig`, scope merge helpers, validation.
- `crates/jyowo-harness-memory/src/generation.rs`
  Generation candidate types, extractor interface, real model-driven worker integration boundaries.
- `crates/jyowo-harness-memory/src/taint.rs`
  `MemoryContextTaint` and persistence eligibility decisions.
- `crates/jyowo-harness-memory/tests/sqlite_store.rs`
- `crates/jyowo-harness-memory/tests/sqlite_recall.rs`
- `crates/jyowo-harness-memory/tests/config.rs`
- `crates/jyowo-harness-memory/tests/generation_candidates.rs`
- `crates/jyowo-harness-memory/tests/settings_store.rs`
- `crates/jyowo-harness-sdk/tests/memory_runtime_config.rs`
- `crates/jyowo-harness-sdk/tests/memory_generation_worker.rs`
- `crates/jyowo-harness-sdk/tests/memory_management.rs`
- `docs/architecture/harness/crates/harness-memory.md`

Modify:

- `crates/jyowo-harness-memory/Cargo.toml`
- `crates/jyowo-harness-memory/src/lib.rs`
- `crates/jyowo-harness-memory/src/types.rs`
- `crates/jyowo-harness-memory/src/external.rs`
- `crates/jyowo-harness-memory/src/lifecycle.rs`
- `crates/jyowo-harness-contracts/src/enums.rs`
- `crates/jyowo-harness-contracts/src/events/memory.rs`
- `crates/jyowo-harness-contracts/src/events/types.rs`
- `crates/jyowo-harness-contracts/src/lib.rs`
- `crates/jyowo-harness-sdk/Cargo.toml`
- `crates/jyowo-harness-sdk/src/builder.rs`
- `crates/jyowo-harness-sdk/src/harness.rs`
- `crates/jyowo-harness-sdk/src/options.rs`
- `crates/jyowo-harness-sdk/src/system_prompt.rs`
- `crates/jyowo-harness-context/src/engine.rs`
- `apps/desktop/src-tauri/Cargo.toml`
- `apps/desktop/src-tauri/src/commands.rs`
- `apps/desktop/src-tauri/tests/commands.rs`
- `apps/desktop/src/shared/tauri/commands.ts`
- `apps/desktop/src/shared/tauri/commands.test.ts`
- `apps/desktop/src/testing/command-client.ts`
- `apps/desktop/src/features/memory/MemoryBrowser.tsx`
- `apps/desktop/src/features/memory/MemoryBrowser.test.tsx`
- `apps/desktop/src/features/memory/MemoryItemCard.tsx`
- `crates/jyowo-harness-memory/README.md`
- `scripts/check-backend-docs.mjs`

## Task 0: Worktree And Baseline

**Files:**

- Read: `AGENTS.md`
- Read: `docs/backend/agent-harness-backend-development-guidelines.md`
- Read: `docs/backend/backend-runtime.md`
- Read: `docs/backend/backend-engineering.md`
- Read: `docs/backend/backend-quality.md`
- Read: `docs/frontend/agent-harness-frontend-development-guidelines.md`
- Read: `docs/frontend/frontend-product-ux.md`
- Read: `docs/frontend/frontend-engineering.md`
- Read: `docs/frontend/frontend-quality.md`

- [ ] Create the required worktree with the commands from `Required Worktree Setup`.
- [ ] Read all required docs listed above.
- [ ] Run baseline gates:

```bash
pnpm check:agent-docs
pnpm check:backend-docs
pnpm check:frontend-docs
cargo test -p jyowo-harness-memory
cargo test -p jyowo-harness-memory --no-default-features --features external-slot
pnpm check:desktop
```

Expected: all commands exit 0 before feature work starts. If an existing failure appears, record the command and failure, then stop before editing.

- [ ] Run the required subagent audit for Task 0.
- [ ] Commit:

```bash
git status --short
git commit --allow-empty -m "chore: establish memory system worktree baseline"
```

## Task 1: Public Memory Contracts And Provenance

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/memory.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/types.rs`
- Modify: `crates/jyowo-harness-contracts/src/lib.rs`
- Modify: `crates/jyowo-harness-memory/src/types.rs`
- Test: `crates/jyowo-harness-memory/tests/api_contract.rs`
- Test: `crates/jyowo-harness-memory/tests/contract.rs`

- [ ] Write failing contract tests for evidence and candidate serialization.

Required shapes:

```rust
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEvidenceKind {
    UserMessage,
    AssistantMessage,
    ToolResult,
    SessionSummary,
    ImportedFile,
    ManualEntry,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTrustLevel {
    UserProvided,
    RuntimeDerived,
    ExternalUntrusted,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemorySourceRef {
    Event { event_id: EventId },
    ContentHash { content_hash: ContentHash },
    ExternalHost { host: String },
    RedactedLabel { label: String },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryEvidence {
    pub kind: MemoryEvidenceKind,
    pub content_hash: Option<ContentHash>,
    pub event_id: Option<EventId>,
    pub source_ref: Option<MemorySourceRef>,
    pub captured_at: DateTime<Utc>,
    pub trust_level: MemoryTrustLevel,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateStatus {
    Pending,
    Accepted,
    Rejected,
}
```

`MemoryRecord` must gain:

```rust
pub evidence: Vec<MemoryEvidence>,
```

`MemorySummary` must gain:

```rust
pub evidence_count: u32,
```

- [ ] Run failing tests:

```bash
cargo test -p jyowo-harness-memory memory_types --test api_contract -- --nocapture
cargo test -p jyowo-harness-memory memory_contract --test contract -- --nocapture
```

Expected: fail because new fields/types do not exist.

- [ ] Implement the contract additions exactly in contracts first, then memory types.
- [ ] Do not add any raw URI or raw absolute path field to memory evidence. Use `MemorySourceRef::ExternalHost` for web/MCP origin host and `MemorySourceRef::RedactedLabel` for user-visible labels that have already passed the redactor.
- [ ] Update all existing test builders for `MemoryRecord` and `MemorySummary` with real evidence values. Do not use empty evidence for records whose source is external or derived.
- [ ] Ensure serde names are snake_case and stable.
- [ ] Run:

```bash
cargo test -p jyowo-harness-memory
cargo test -p jyowo-harness-contracts
pnpm check:backend-docs
git diff --check
```

- [ ] Run the required subagent audit for Task 1.
- [ ] Commit:

```bash
git add crates/jyowo-harness-contracts crates/jyowo-harness-memory
git commit -m "feat(memory): add provenance contracts"
```

## Task 2: Durable SQLite Memory Provider

**Files:**

- Create: `crates/jyowo-harness-memory/src/sqlite.rs`
- Modify: `crates/jyowo-harness-memory/Cargo.toml`
- Modify: `crates/jyowo-harness-memory/src/lib.rs`
- Test: `crates/jyowo-harness-memory/tests/sqlite_store.rs`

- [ ] Add feature and dependency wiring:

```toml
[features]
sqlite = ["dep:rusqlite"]

[dependencies]
rusqlite = { workspace = true, optional = true }
```

- [ ] Write failing SQLite persistence tests using `tempfile::tempdir`.

Required test cases:

```text
sqlite_provider_persists_records_across_reopen
sqlite_provider_persists_tenant_kind_visibility_and_evidence_fields
memory_manager_enforces_tenant_and_visibility_for_sqlite_provider
sqlite_provider_updates_content_without_changing_id_or_created_at
sqlite_provider_delete_removes_record_fts_and_evidence
sqlite_provider_rejects_expired_ttl_records_on_recall
sqlite_provider_records_access_count_and_last_accessed_at_after_recall
sqlite_provider_initializes_user_version_and_reopens_supported_schema
sqlite_provider_rejects_unsupported_future_schema_version
```

These tests must instantiate `SqliteMemoryProvider::open(path)` and reopen the same path. They must not use `InMemoryMemoryProvider`.

- [ ] Run failing tests:

```bash
cargo test -p jyowo-harness-memory --features sqlite sqlite_provider --test sqlite_store -- --nocapture
```

Expected: fail because `SqliteMemoryProvider` is missing.

- [ ] Implement:

```rust
pub struct SqliteMemoryProvider {
    provider_id: String,
    path: PathBuf,
}
```

This plan uses the per-call `spawn_blocking` approach. Do not implement a dedicated SQLite worker thread in this plan.

Do not put `rusqlite::Connection` behind `tokio::sync::Mutex` on async paths. `rusqlite` is synchronous. Each public async provider, candidate-store, and settings-store method must:

- call `tokio::task::spawn_blocking`,
- open/use the SQLite connection inside the blocking closure,
- enable per-connection pragmas before queries, and
- return only owned, redacted data from the blocking closure.

Tests must prove concurrent recall/upsert does not panic and does not hold an async executor thread while blocking on a SQLite mutex.

Required methods:

```rust
impl SqliteMemoryProvider {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, MemoryError>;
    pub fn open_with_provider_id(path: impl Into<PathBuf>, provider_id: impl Into<String>) -> Result<Self, MemoryError>;
}
```

Provider id for desktop default: `sqlite-memory`.

- [ ] Initialize schema inside a transaction. Enable:

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
```

`PRAGMA foreign_keys = ON` is per connection. Every connection opened inside a `spawn_blocking` call must enable `foreign_keys` before running queries.

- [ ] Implement `MemoryStore` fully:
  - `recall`
  - `get`
  - `upsert`
  - `forget`
  - `list`

- [ ] Keep actor visibility enforcement in `MemoryManager`, not in provider `get(id)`. `SqliteMemoryProvider::get` only loads by id. `MemoryManager::get_for_actor`, `list_for_actor`, and `export_for_actor` must recheck tenant and visibility.
- [ ] Implement `MemoryLifecycle` without side effects in `initialize`. It may verify schema and tenant access, but must not create duplicate records.
- [ ] Keep `SqliteMemoryProvider::shutdown` safe for a provider shared by desktop runtime and browser operations. Because this plan uses per-call `spawn_blocking`, `shutdown` must be a no-op for `SqliteMemoryProvider`; session end must not make the shared provider unusable.
- [ ] Add conversion helpers for `MemoryKind`, `MemoryVisibility`, and `MemorySource` as JSON strings. Do not invent lossy string formats.
- [ ] Ensure all writes are transactional.
- [ ] Ensure `forget` is idempotent for missing records only if existing `MemoryManager` behavior expects it; otherwise preserve existing `MemoryError::NotFound` semantics.
- [ ] Run:

```bash
cargo test -p jyowo-harness-memory --features sqlite sqlite_provider --test sqlite_store -- --nocapture
cargo test -p jyowo-harness-memory --features sqlite
git diff --check
```

- [ ] Run the required subagent audit for Task 2.
- [ ] Commit:

```bash
git add crates/jyowo-harness-memory
git commit -m "feat(memory): add sqlite provider"
```

## Task 3: Real Retrieval And Ranking

**Files:**

- Create: `crates/jyowo-harness-memory/src/ranking.rs`
- Modify: `crates/jyowo-harness-memory/src/sqlite.rs`
- Modify: `crates/jyowo-harness-memory/src/lib.rs`
- Test: `crates/jyowo-harness-memory/tests/sqlite_recall.rs`
- Test: `crates/jyowo-harness-memory/tests/recall.rs`

- [ ] Write failing recall tests.

Required test cases:

```text
recall_prefers_lexically_relevant_records_over_insert_order
recall_applies_visibility_filter_before_scoring
recall_excludes_expired_records
recall_is_stable_when_scores_tie_by_updated_at_then_id
recall_respects_max_records_at_provider_layer
memory_manager_limits_injected_chars_by_recall_policy
recall_updates_access_metrics_inside_same_provider
```

Use records with real content:

```text
"project uses pnpm and cargo gates"
"user prefers concise Chinese answers"
"database migration failed because FTS trigger was missing"
```

- [ ] Run failing tests:

```bash
cargo test -p jyowo-harness-memory --features sqlite recall_ --test sqlite_recall -- --nocapture
```

Expected: fail because recall still lacks ranking.

- [ ] Implement `MemoryRecallRanker`.

Required public API:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemoryRecallScore {
    pub lexical: f32,
    pub freshness: f32,
    pub confidence: f32,
    pub usage: f32,
    pub final_score: f32,
}

pub struct MemoryRecallRanker;

impl MemoryRecallRanker {
    pub fn score(query: &str, record: &MemoryRecord, now: DateTime<Utc>) -> MemoryRecallScore;
}
```

- [ ] Use SQLite FTS5 when query text has searchable tokens. If FTS produces no rows, use deterministic substring/token overlap fallback. Do not return insertion order unless all scores are equal.
- [ ] Apply tenant/kind/visibility/TTL filters before scoring.
- [ ] Store `metadata.recall_score` from final score on returned cloned records. Persist `access_count` and `last_accessed_at`.
- [ ] Keep `min_similarity` meaningful: exclude records where `final_score < query.min_similarity`, except when `min_similarity <= 0.0`.
- [ ] Do not implement char-budget trimming inside `SqliteMemoryProvider`. Provider recall owns record filtering/ranking and `max_records`; `MemoryManager` or context injection owns `RecallPolicy.max_chars_per_turn`.
- [ ] Run:

```bash
cargo test -p jyowo-harness-memory --features sqlite recall_ --test sqlite_recall -- --nocapture
cargo test -p jyowo-harness-memory --features sqlite recall --test recall -- --nocapture
cargo test -p jyowo-harness-memory --features sqlite
git diff --check
```

- [ ] Run the required subagent audit for Task 3.
- [ ] Commit:

```bash
git add crates/jyowo-harness-memory
git commit -m "feat(memory): rank sqlite recall results"
```

## Task 4: Runtime Configuration And Session Controls

**Files:**

- Create: `crates/jyowo-harness-memory/src/config.rs`
- Modify: `crates/jyowo-harness-memory/src/lib.rs`
- Modify: `crates/jyowo-harness-memory/src/sqlite.rs`
- Modify: `crates/jyowo-harness-sdk/src/options.rs`
- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `crates/jyowo-harness-sdk/src/system_prompt.rs`
- Test: `crates/jyowo-harness-memory/tests/config.rs`
- Test: `crates/jyowo-harness-memory/tests/settings_store.rs`
- Test: `crates/jyowo-harness-sdk/tests/memory_runtime_config.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`

- [ ] Write failing config tests.

Required config shape:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryRuntimeConfig {
    pub use_memory: bool,
    pub generate_memory: bool,
    pub disable_generation_on_external_context: bool,
    pub min_generation_turns: u32,
    pub generation_idle_ms: u64,
    pub min_rate_limit_remaining_percent: u8,
    pub extraction_model_route: Option<String>,
    pub consolidation_model_route: Option<String>,
}
```

Defaults:

```text
use_memory = true
generate_memory = false
disable_generation_on_external_context = true
min_generation_turns = 3
generation_idle_ms = 5000
min_rate_limit_remaining_percent = 10
extraction_model_route = None
consolidation_model_route = None
```

Rationale: Jyowo is local/dev-stage, so recall can be on when provider exists; generation remains off until configured.

- [ ] Run failing tests:

```bash
cargo test -p jyowo-harness-memory --features sqlite memory_runtime_config --test config -- --nocapture
cargo test -p jyowo-harness-sdk --features testing,memory-external-slot,memory-sqlite memory_runtime_config --test memory_runtime_config -- --nocapture
```

Expected: fail because config does not exist.

- [ ] Add `memory-sqlite` feature to SDK:

```toml
memory-sqlite = ["jyowo-harness-memory/sqlite"]
```

- [ ] Add a settings-store trait in `crates/jyowo-harness-memory/src/config.rs`:

```rust
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryConfigScope {
    Global,
    Workspace,
    Project,
    Session,
}

#[async_trait]
pub trait MemorySettingsStore: Send + Sync + 'static {
    async fn load_memory_config(
        &self,
        scope: MemoryConfigScope,
        scope_id: &str,
    ) -> Result<Option<MemoryRuntimeConfig>, MemoryError>;

    async fn save_memory_config(
        &self,
        scope: MemoryConfigScope,
        scope_id: &str,
        config: MemoryRuntimeConfig,
    ) -> Result<(), MemoryError>;
}
```

`SqliteMemoryProvider` must implement `MemorySettingsStore`. SDK must receive this settings store through explicit builder wiring and read persisted settings before deciding whether to construct a `MemoryManager`.

Required SDK wiring:

```rust
pub(crate) struct BuilderExtras {
    pub(crate) memory_settings_store: Option<Arc<dyn MemorySettingsStore>>,
}

struct HarnessInner {
    memory_settings_store: Option<Arc<dyn MemorySettingsStore>>,
}

impl<M, S, SB> HarnessBuilder<M, S, SB> {
    pub fn with_memory_settings_store<T>(self, store: T) -> Self
    where
        T: MemorySettingsStore,
    {
        self.with_memory_settings_store_arc(Arc::new(store))
    }

    pub fn with_memory_settings_store_arc(
        mut self,
        store: Arc<dyn MemorySettingsStore>,
    ) -> Self {
        self.extras.memory_settings_store = Some(store);
        self
    }
}
```

Do not open SQLite ad hoc inside Tauri commands or inside settings facade methods. The desktop runtime creates the concrete `SqliteMemoryProvider` once and passes it into the SDK as a settings store. Tests may construct a real temp-file SQLite provider and pass it through this builder API.

If `get_memory_settings` or `update_memory_settings` is called without a configured `MemorySettingsStore`, return a fail-closed `MemoryError::SettingsStoreNotConfigured` through `HarnessError::Memory`. Do not silently fall back to defaults for writes.

- [ ] Add memory config to `HarnessOptions` and session override to `SessionOptions` if `SessionOptions` is the existing per-session control point. Do not create a frontend-only setting.
- [ ] Implement merge order:

```text
hardcoded safe defaults
  < HarnessOptions.memory
  < workspace/project persisted setting
  < SessionOptions.memory override
```

- [ ] Ensure `MemoryManager` is not created when `use_memory = false`.
- [ ] Ensure `get_memory_settings` and `update_memory_settings` still work when `use_memory = false`; they use `MemorySettingsStore`, not `MemoryManager`, and they must not call `memory_manager_for_session`.
- [ ] Ensure generation worker is not scheduled when `generate_memory = false`.
- [ ] Render runtime context as:

```text
memory_recall: enabled|disabled
memory_generation: enabled|disabled
memory_external_context_generation: disabled|allowed
```

Do not render secrets or model route values.

- [ ] Run:

```bash
cargo test -p jyowo-harness-memory --features sqlite memory_runtime_config --test config -- --nocapture
cargo test -p jyowo-harness-sdk --features testing,memory-external-slot,memory-sqlite memory_runtime_config --test memory_runtime_config -- --nocapture
cargo test -p jyowo-harness-sdk --features testing,memory-external-slot,memory-sqlite runtime_assembly --test runtime_assembly -- --nocapture
git diff --check
```

- [ ] Run the required subagent audit for Task 4.
- [ ] Commit:

```bash
git add crates/jyowo-harness-memory crates/jyowo-harness-sdk
git commit -m "feat(memory): add runtime controls"
```

## Task 5: External Context Taint And Structured Recall Trigger

**Files:**

- Create: `crates/jyowo-harness-memory/src/taint.rs`
- Modify: `crates/jyowo-harness-memory/src/lib.rs`
- Modify: `crates/jyowo-harness-memory/src/external.rs`
- Modify: `crates/jyowo-harness-context/src/engine.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/memory.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Test: `crates/jyowo-harness-context/tests/memory_recall.rs`
- Test: `crates/jyowo-harness-memory/tests/config.rs`
- Test: `crates/jyowo-harness-sdk/tests/memory_runtime_config.rs`

- [ ] Write failing tests proving the hardcoded phrase is no longer required.

Required behavior:

```text
tool result with structured memory.recallRequested = true triggers recall
tool result text "需要查阅历史" alone does not trigger special recall behavior
user message recall still follows RecallTriggerStrategy
external-context taint prevents generation when disable_generation_on_external_context = true
taint source is preserved as ExternalUntrusted evidence if generation is explicitly allowed
SDK session summary state records external taint for later generation decisions
```

Suggested structured tool result field:

```json
{
  "memory": {
    "recallRequested": true,
    "reason": "project preference needed"
  }
}
```

This is the only control schema for tool-result-triggered recall. In Rust structs, use `recall_requested` with serde `rename = "recallRequested"` if a typed helper is introduced.

- [ ] Run failing tests:

```bash
cargo test -p jyowo-harness-context --features recall-memory memory_recall --test memory_recall -- --nocapture
```

Expected: fail because recall still checks a string phrase.

- [ ] Implement `MemoryContextTaint`:

```rust
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct MemoryContextTaint {
    pub has_external_context: bool,
    pub sources: Vec<MemoryExternalSource>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MemoryExternalSource {
    Tool { tool_name: String },
    Mcp { server_id: String, tool_name: String },
    Web { url_host: String },
    Imported { label: String },
}
```

Do not store full URLs with secrets. Store host or redacted label.

- [ ] Replace phrase-based trigger with structured extraction:
  - Parse `ToolResult::Structured`.
  - Parse structured parts inside `ToolResult::Mixed`.
  - Accept only `memory.recallRequested == true`.
  - Ignore unknown fields.
  - Do not treat plain text as a control channel.

- [ ] Add the SDK taint bridge in `crates/jyowo-harness-sdk/src/harness.rs`. Extend the existing memory session summary state with `MemoryContextTaint`, merge sanitized taint from tool/MCP/web/imported context events while events are recorded, and make Task 6 read this stored state. Do not recompute generation eligibility from raw transcript text.
- [ ] Emit memory recall skipped/degraded events without raw query text.
- [ ] Run:

```bash
cargo test -p jyowo-harness-context --features recall-memory memory_recall --test memory_recall -- --nocapture
cargo test -p jyowo-harness-memory --features sqlite config --test config -- --nocapture
cargo test -p jyowo-harness-sdk --features testing,memory-external-slot,memory-sqlite memory_external_taint --test memory_runtime_config -- --nocapture
cargo test -p jyowo-harness-context --features recall-memory
git diff --check
```

- [ ] Run the required subagent audit for Task 5.
- [ ] Commit:

```bash
git add crates/jyowo-harness-context crates/jyowo-harness-memory crates/jyowo-harness-contracts crates/jyowo-harness-sdk
git commit -m "feat(memory): track external taint and structured recall"
```

## Task 6: Generation Candidate Pipeline

**Files:**

- Create: `crates/jyowo-harness-memory/src/generation.rs`
- Modify: `crates/jyowo-harness-memory/src/sqlite.rs`
- Modify: `crates/jyowo-harness-memory/src/external.rs`
- Modify: `crates/jyowo-harness-memory/src/lifecycle.rs`
- Modify: `crates/jyowo-harness-memory/src/in_memory.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Test: `crates/jyowo-harness-memory/tests/generation_candidates.rs`
- Test: `crates/jyowo-harness-sdk/tests/memory_generation_worker.rs`

- [ ] Write failing tests for candidate lifecycle.

Required cases:

```text
candidate_created_from_session_summary_is_pending
candidate_accept_promotes_to_memory_record_with_same_evidence
candidate_reject_does_not_create_memory_record
candidate_content_is_scanned_before_persist
candidate_generation_skips_short_sessions
candidate_generation_skips_external_tainted_sessions_by_default
candidate_generation_requires_real_extraction_model_route
candidate_accept_preserves_proposed_confidence_and_tags
candidate_generation_key_prevents_duplicates_across_repeated_session_end
generation_worker_reads_external_taint_from_sdk_session_summary_state
```

- [ ] Run failing tests:

```bash
cargo test -p jyowo-harness-memory --features sqlite,consolidation,threat-scanner generation_candidate --test generation_candidates -- --nocapture
cargo test -p jyowo-harness-sdk --features testing,memory-external-slot,memory-sqlite,memory-consolidation memory_generation_worker --test memory_generation_worker -- --nocapture
```

Expected: fail because candidates and worker do not exist.

- [ ] Implement candidate types:

```rust
pub struct MemoryCandidate {
    pub id: MemoryId,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub generation_key: String,
    pub proposed_kind: MemoryKind,
    pub proposed_visibility: MemoryVisibility,
    pub proposed_tags: Vec<String>,
    pub proposed_confidence: f32,
    pub content: String,
    pub source: MemorySource,
    pub evidence: Vec<MemoryEvidence>,
    pub redacted_segments: u32,
    pub status: MemoryCandidateStatus,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
}
```

`generation_key` is a deterministic idempotency key, not a display id. Compute it after redaction from trusted runtime fields:

```text
generation_key = blake3(tenant_id || session_id || source_summary_hash || redacted_content_hash || proposed_kind_json || proposed_visibility_json)
```

Repeated session-end handling, runtime restart, or worker retry must not create duplicate pending candidates for the same `tenant_id + generation_key`.

- [ ] Add a candidate-store trait in `crates/jyowo-harness-memory/src/generation.rs` and make `MemoryManager` depend on this trait. Do not downcast `Arc<dyn MemoryProvider>` to `SqliteMemoryProvider`.

```rust
#[async_trait]
pub trait MemoryCandidateStore: Send + Sync + 'static {
    async fn create_candidate(&self, candidate: MemoryCandidate) -> Result<MemoryId, MemoryError>;
    async fn list_candidates(&self, actor: MemoryActor) -> Result<Vec<MemoryCandidate>, MemoryError>;
    async fn accept_candidate(&self, id: MemoryId, actor: MemoryActor) -> Result<MemoryRecord, MemoryError>;
    async fn reject_candidate(
        &self,
        id: MemoryId,
        actor: MemoryActor,
        reason: Option<String>,
    ) -> Result<(), MemoryError>;
}
```

- [ ] Add these `MemoryManager` fields and APIs while keeping the existing event sink, metrics sink, recall policy, threat scanner, builtin, and consolidation fields:

```rust
pub struct MemoryManager {
    external: RwLock<Option<Arc<dyn MemoryProvider>>>,
    candidate_store: RwLock<Option<Arc<dyn MemoryCandidateStore>>>,
}

impl MemoryManager {
    pub fn set_candidate_store(
        &self,
        store: Arc<dyn MemoryCandidateStore>,
    ) -> Result<(), MemoryError>;

    pub fn candidate_store(&self) -> Option<Arc<dyn MemoryCandidateStore>>;
}
```

`set_candidate_store` must return an occupied-slot error if called twice. Candidate list/accept/reject/create methods must return a fail-closed `MemoryError::CandidateStoreNotConfigured` when the slot is empty.

- [ ] Add SDK builder and runtime storage for the same candidate trait object:

```rust
pub(crate) struct BuilderExtras {
    pub(crate) memory_candidate_store: Option<Arc<dyn MemoryCandidateStore>>,
}

struct HarnessInner {
    memory_candidate_store: Option<Arc<dyn MemoryCandidateStore>>,
}

impl<M, S, SB> HarnessBuilder<M, S, SB> {
    pub fn with_memory_candidate_store<T>(self, store: T) -> Self
    where
        T: MemoryCandidateStore,
    {
        self.with_memory_candidate_store_arc(Arc::new(store))
    }

    pub fn with_memory_candidate_store_arc(
        mut self,
        store: Arc<dyn MemoryCandidateStore>,
    ) -> Self {
        self.extras.memory_candidate_store = Some(store);
        self
    }
}
```

`memory_manager_for_session` and `memory_manager_for_browser` must set this store on the manager when configured. They must not infer it from `effective_memory_provider()` by downcast.

- [ ] Implement `MemoryCandidateStore` for `SqliteMemoryProvider`. Do not extend `MemoryProvider` with candidate methods. If tests need an in-memory candidate store, implement `MemoryCandidateStore` for `InMemoryMemoryProvider` with real candidate storage, not stubbed success.
- [ ] Add `MemoryManager` facade methods for candidate list/accept/reject. They must call `MemoryCandidateStore`, not provider-specific inherent methods. Recheck visibility in manager after provider returns data.
- [ ] Wire the desktop default by constructing one concrete SQLite provider and coercing it explicitly into each trait object:

```rust
let sqlite = Arc::new(SqliteMemoryProvider::open(memory_db_path)?);
let memory_provider: Arc<dyn MemoryProvider> = sqlite.clone();
let candidate_store: Arc<dyn MemoryCandidateStore> = sqlite.clone();
let settings_store: Arc<dyn MemorySettingsStore> = sqlite;
```

Pass those three trait objects through the SDK builder. Do not create three separate SQLite providers for the same database path.
- [ ] On `create_candidate`, enforce the `UNIQUE(tenant_id, generation_key)` constraint idempotently: return the existing pending candidate id when the same key already exists; do not create a second candidate.
- [ ] On `accept_candidate`, create the durable `MemoryRecord` with the same evidence, `metadata.tags = proposed_tags`, and `metadata.confidence = proposed_confidence`.
- [ ] Implement generation worker in SDK with these rules:
  - Runs only after session end.
  - Waits `generation_idle_ms`.
  - Skips when `turn_count < min_generation_turns`.
  - Skips when `generate_memory = false`.
  - Skips when route is absent; emit degraded/skipped event. Do not create heuristic candidates.
  - Reads `MemoryContextTaint` from the SDK session summary state produced by Task 5.
  - Skips when external taint exists and disable flag is true.
  - Uses configured real model route through existing model provider path.
  - Parses model output as strict JSON candidate array.
  - Runs redaction/threat scanning before storing candidates.
  - Computes `generation_key` after redaction and trusted visibility mapping.
  - Stores candidates as `Pending`, never directly durable records.

Strict model output schema:

```json
{
  "candidates": [
    {
      "kind": "user_preference",
      "visibilityScope": "user",
      "content": "User prefers concise Chinese answers.",
      "confidence": 0.82,
      "tags": ["communication"]
    }
  ]
}
```

The model outputs only `visibilityScope` with one of `private`, `user`, `team`, or `tenant`. Runtime constructs the real `MemoryVisibility` from trusted `MemorySessionCtx`:

```text
private -> MemoryVisibility::Private { session_id: ctx.session_id }
user -> MemoryVisibility::User { user_id: ctx.user_id } and must be dropped if user_id is None
team -> MemoryVisibility::Team { team_id: ctx.team_id } and must be dropped if team_id is None
tenant -> MemoryVisibility::Tenant
```

If a candidate requests `user` or `team` without the required trusted id, reject that candidate and emit a degraded/skipped event without storing it.

Do not accept arbitrary instruction text from model output. Treat it as data only.

- [ ] Run:

```bash
cargo test -p jyowo-harness-memory --features sqlite,consolidation,threat-scanner generation_candidate --test generation_candidates -- --nocapture
cargo test -p jyowo-harness-sdk --features testing,memory-external-slot,memory-sqlite,memory-consolidation memory_generation_worker --test memory_generation_worker -- --nocapture
cargo test -p jyowo-harness-memory --features sqlite,consolidation,threat-scanner
cargo test -p jyowo-harness-sdk --features testing,memory-external-slot,memory-sqlite,memory-consolidation
git diff --check
```

- [ ] Run the required subagent audit for Task 6.
- [ ] Commit:

```bash
git add crates/jyowo-harness-memory crates/jyowo-harness-sdk
git commit -m "feat(memory): add generation candidates"
```

## Task 7: Desktop Runtime Wiring

**Files:**

- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `crates/jyowo-harness-sdk/Cargo.toml`
- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`

- [ ] Write failing desktop tests.

Required cases:

```text
desktop_uses_sqlite_memory_provider_by_default
desktop_memory_survives_runtime_rebuild
desktop_memory_browser_does_not_reinitialize_provider_side_effects_per_call
desktop_export_redacts_and_writes_to_runtime_exports
desktop_returns_error_when_memory_provider_unavailable
```

- [ ] Run failing tests:

```bash
cargo test -p jyowo-desktop-shell commands::desktop_uses_sqlite_memory_provider_by_default -- --nocapture
```

Expected: fail because desktop still wires `InMemoryMemoryProvider`.

- [ ] Enable SDK feature `memory-sqlite` in desktop Cargo features.
- [ ] Replace:

```rust
.with_memory_provider(InMemoryMemoryProvider::new("desktop-memory"))
```

with a durable provider rooted at:

```text
state.workspace_root/.jyowo/runtime/memory/memory.sqlite
```

Use `SqliteMemoryProvider::open_with_provider_id(path, "desktop-memory")` only if provider id compatibility is required by existing events. Prefer `sqlite-memory` if tests do not depend on the old id.

- [ ] Add a desktop storage helper if necessary. Keep path construction in Tauri shell, not inside lower crates.
- [ ] Reuse one SQLite provider instance per desktop runtime. Pass the same concrete `Arc<SqliteMemoryProvider>` into the SDK as `MemoryProvider`, `MemoryCandidateStore`, and `MemorySettingsStore`. Do not create a new provider per browser command.
- [ ] Ensure exports use redacted payloads and do not include full private absolute paths in frontend response.
- [ ] Run:

```bash
cargo test -p jyowo-desktop-shell desktop_memory --test commands -- --nocapture
cargo test -p jyowo-desktop-shell memory --test commands -- --nocapture
pnpm check:rust
git diff --check
```

- [ ] Run the required subagent audit for Task 7.
- [ ] Commit:

```bash
git add apps/desktop/src-tauri crates/jyowo-harness-sdk
git commit -m "feat(memory): persist desktop memory"
```

## Task 8: IPC Payloads And Frontend Memory Management

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `crates/jyowo-harness-memory/src/types.rs`
- Modify: `crates/jyowo-harness-memory/src/generation.rs`
- Modify: `crates/jyowo-harness-memory/src/config.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `crates/jyowo-harness-sdk/src/ext.rs`
- Test: `crates/jyowo-harness-sdk/tests/memory_management.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `apps/desktop/src/testing/command-client.ts`
- Modify: `apps/desktop/src/features/memory/MemoryBrowser.tsx`
- Modify: `apps/desktop/src/features/memory/MemoryBrowser.test.tsx`
- Modify: `apps/desktop/src/features/memory/MemoryItemCard.tsx`

- [ ] Write failing Rust command tests for:

```text
create_memory_item_rejects_empty_content
create_memory_item_rejects_unredacted_secret
create_memory_item_creates_user_visible_record_with_manual_evidence
list_memory_candidates_returns_pending_only_for_actor
accept_memory_candidate_promotes_candidate
reject_memory_candidate_marks_candidate_rejected
memory_payload_includes_evidence_count_and_candidate_status
```

- [ ] Write failing SDK facade tests for:

```text
sdk_create_memory_item_enforces_actor_visibility_and_redaction
sdk_list_memory_candidates_uses_candidate_store_not_tauri_logic
sdk_accept_memory_candidate_promotes_through_memory_manager
sdk_reject_memory_candidate_updates_candidate_status
sdk_memory_settings_use_settings_store_when_use_memory_false
```

- [ ] Write failing TS/Zod tests for:

```text
createMemoryItem validates request and response
listMemoryCandidates rejects unknown fields
acceptMemoryCandidate parses promoted item
rejectMemoryCandidate parses rejected status
memory payload rejects unredacted secret content
```

- [ ] Write failing UI tests for:

```text
MemoryBrowser can create a manual memory
MemoryBrowser shows pending candidate count
MemoryBrowser can accept a candidate
MemoryBrowser can reject a candidate
MemoryBrowser shows evidence count
MemoryBrowser keeps loading empty error ready states
```

- [ ] Run failing tests:

```bash
cargo test -p jyowo-desktop-shell memory_item --test commands -- --nocapture
cargo test -p jyowo-harness-sdk --features testing,memory-external-slot,memory-sqlite memory_management --test memory_management -- --nocapture
pnpm --dir apps/desktop vitest run src/shared/tauri/commands.test.ts src/features/memory/MemoryBrowser.test.tsx
```

Expected: fail because create/candidate APIs and UI do not exist.

- [ ] Add Rust IPC request/response types with `#[serde(deny_unknown_fields, rename_all = "camelCase")]`.
- [ ] Add SDK facade methods in `crates/jyowo-harness-sdk/src/harness.rs`. Tauri commands must call these methods and must not create, accept, reject, filter, or persist memory directly.

Canonical Rust DTO ownership:

```text
crates/jyowo-harness-memory/src/types.rs
  - MemoryVisibilityScope
  - CreateMemoryItemRequest
  - MemoryItemResponse if a response wrapper is needed

crates/jyowo-harness-memory/src/generation.rs
  - MemoryCandidate
  - MemoryCandidateStatus
  - candidate list/accept/reject response DTOs if wrappers are needed

crates/jyowo-harness-memory/src/config.rs
  - MemoryRuntimeConfig
  - MemoryConfigScope
  - memory settings request/response DTOs if wrappers are needed
```

`crates/jyowo-harness-sdk/src/ext.rs` must re-export these canonical memory DTOs. `apps/desktop/src-tauri` may import them through `jyowo_harness_sdk::ext`, but must not define duplicate Rust structs or enums with the same wire shape.

Required SDK DTO shapes in `jyowo-harness-memory`:

```rust
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVisibilityScope {
    Private,
    User,
    Team,
    Tenant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreateMemoryItemRequest {
    pub kind: MemoryKind,
    pub visibility_scope: MemoryVisibilityScope,
    pub content: String,
    pub tags: Vec<String>,
}
```

Manual create must map `visibility_scope` to trusted `SessionOptions` ids using the same rules as generation candidates. It must create `MemoryEvidenceKind::ManualEntry`, `MemoryTrustLevel::UserProvided`, and `MemorySource::UserInput` in Rust. The frontend must not send raw `MemoryVisibility`, `MemorySource`, or evidence snippets.

Required SDK facade methods:

```rust
pub async fn create_memory_item(&self, options: SessionOptions, request: CreateMemoryItemRequest) -> Result<MemoryRecord, HarnessError>;
pub async fn list_memory_candidates(&self, options: SessionOptions) -> Result<Vec<MemoryCandidate>, HarnessError>;
pub async fn accept_memory_candidate(&self, options: SessionOptions, id: MemoryId) -> Result<MemoryRecord, HarnessError>;
pub async fn reject_memory_candidate(&self, options: SessionOptions, id: MemoryId, reason: Option<String>) -> Result<(), HarnessError>;
pub async fn get_memory_settings(&self, scope: MemoryConfigScope, scope_id: &str) -> Result<MemoryRuntimeConfig, HarnessError>;
pub async fn update_memory_settings(&self, scope: MemoryConfigScope, scope_id: &str, config: MemoryRuntimeConfig) -> Result<MemoryRuntimeConfig, HarnessError>;
```

Tauri command request/response types must use the canonical DTOs or thin wrappers that contain only command transport fields such as `id`; wrappers must not duplicate `MemoryVisibilityScope`, candidate status, settings shape, `MemoryKind`, `MemorySource`, or evidence fields.

Required commands:

```text
create_memory_item
list_memory_candidates
accept_memory_candidate
reject_memory_candidate
get_memory_settings
update_memory_settings
```

- [ ] Add matching `CommandClient` methods and Zod schemas.
- [ ] UI requirements:
  - Add a manual create form.
  - Add a candidate review section.
  - Show kind, source, visibility, evidence count, updated time.
  - Keep existing list/detail/edit/delete/export behavior.
  - Keep loading, empty, error, ready states.
  - Do not expose raw evidence snippets or secret-containing content.
  - React must not decide whether generation is allowed; it only sends settings.

- [ ] Run:

```bash
cargo test -p jyowo-desktop-shell memory --test commands -- --nocapture
cargo test -p jyowo-harness-sdk --features testing,memory-external-slot,memory-sqlite memory_management --test memory_management -- --nocapture
pnpm --dir apps/desktop vitest run src/shared/tauri/commands.test.ts src/features/memory/MemoryBrowser.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Run the required subagent audit for Task 8.
- [ ] Commit:

```bash
git add apps/desktop crates/jyowo-harness-memory crates/jyowo-harness-sdk
git commit -m "feat(memory): add memory management UI"
```

## Task 9: Docs, README, And Gate Integration

**Files:**

- Create: `docs/architecture/harness/crates/harness-memory.md`
- Modify: `crates/jyowo-harness-memory/README.md`
- Modify: `scripts/check-backend-docs.mjs`
- Test: docs gates

- [ ] Write the architecture doc with these sections:

```text
# harness-memory

## Purpose
## Non-Authority Rule
## Storage Providers
## SQLite Provider
## Retrieval
## Runtime Config
## External Context Taint
## Generation Candidates
## Redaction And Threat Scanning
## Audit Events
## Desktop Wiring
## Test Gates
```

Mandatory wording:

```text
Memory is auxiliary recall context. It is never a policy authority and cannot override system, runtime policy, workspace instructions, permissions, redaction, or sandbox decisions.
```

- [ ] Update README `SPEC:` link to the new doc.
- [ ] Update `scripts/check-backend-docs.mjs` so `requiredArchitectureDocs` includes `docs/architecture/harness/crates/harness-memory.md`. Do not add an orphan Markdown file.
- [ ] If `scripts/check-backend-docs.mjs` has already been changed by an earlier task, verify with `rg -n "harness-memory.md" scripts/check-backend-docs.mjs` before running docs gates.
- [ ] Run:

```bash
pnpm check:docs
pnpm check:agent-docs
pnpm check:backend-docs
pnpm check:frontend-docs
git diff --check
```

- [ ] Run the required subagent audit for Task 9.
- [ ] Commit:

```bash
git add docs crates/jyowo-harness-memory/README.md scripts/check-backend-docs.mjs
git commit -m "docs(memory): document memory architecture"
```

## Task 10: Full Integration And Debt Removal

**Files:**

- Modify as needed from previous tasks.
- Do not create new feature surfaces in this task.

- [ ] Remove unused compatibility code introduced during the implementation.
- [ ] Confirm desktop no longer uses `InMemoryMemoryProvider` in production runtime assembly.

Run:

```bash
if rg -n "InMemoryMemoryProvider::new\\(\"desktop-memory\"\\)|需要查阅历史|memory-builtin.*desktop" \
  apps/desktop/src-tauri/src \
  apps/desktop/src-tauri/Cargo.toml \
  crates \
  --glob '!**/tests/**' \
  --glob '!**/*test*.rs' \
  --glob '!**/*.md' \
  --glob '!**/target/**' \
  -S; then
  echo "forbidden production memory compatibility path found"
  exit 1
fi
```

Expected:

- No production desktop use of `InMemoryMemoryProvider::new("desktop-memory")`.
- No hardcoded memory trigger on `需要查阅历史`.
- Any remaining `memory-builtin` references are tests, docs, or existing feature-gated builtin-memory code.
- This command intentionally does not scan `docs`; the plan itself contains historical examples and would otherwise create false positives.

- [ ] Run full gates:

```bash
pnpm check
pnpm check:docs
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:desktop
pnpm check:rust
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
```

- [ ] Run security-focused checks:

```bash
set --
for path in .jyowo/runtime/memory .jyowo/runtime/exports; do
  if test -d "$path"; then
    set -- "$@" "$path"
  fi
done

if test "$#" -eq 0; then
  echo "no runtime memory/export output dirs found"
elif rg -q "Authorization:|Bearer |AKIA[0-9A-Z]{16}|sk-[A-Za-z0-9]{12,}|gh[pousr]_[A-Za-z0-9_]{20,}|xox[baprs]-[0-9A-Za-z-]{20,}" "$@" -S --text; then
  rg -l "Authorization:|Bearer |AKIA[0-9A-Z]{16}|sk-[A-Za-z0-9]{12,}|gh[pousr]_[A-Za-z0-9_]{20,}|xox[baprs]-[0-9A-Za-z-]{20,}" "$@" -S --text
  echo "raw secret-like value found in runtime memory/export output"
  exit 1
fi
```

Expected: either `no runtime memory/export output dirs found` or no matches. On failure it may print filenames only; it must not print matched secret-like content. This command intentionally scans only generated runtime memory/export output, not source docs/tests, because source redactor tests contain intentional secret-pattern examples.

- [ ] Run the required subagent audit for Task 10.
- [ ] Commit:

```bash
git status --short
git add \
  apps/desktop \
  crates/jyowo-harness-context \
  crates/jyowo-harness-contracts \
  crates/jyowo-harness-memory \
  crates/jyowo-harness-sdk \
  docs/architecture/harness/crates/harness-memory.md \
  crates/jyowo-harness-memory/README.md \
  scripts/check-backend-docs.mjs
git commit -m "chore(memory): verify memory system integration"
```

Before `git add`, `git status --short` must contain only files intentionally changed by this plan. If generated runtime output, unrelated files, or user changes appear, stop and narrow the add list.

## Final Acceptance Criteria

- Desktop memory survives app/runtime restart through SQLite.
- Default desktop provider is not in-memory.
- Recall results are ranked by real query relevance and filtered by tenant/kind/visibility/TTL before scoring.
- Browser operations use the same durable provider and do not reinitialize a new provider per operation.
- Manual create/edit/delete/export works through Rust IPC and TS Zod validation.
- Generated memory is candidate-first, redacted, provenance-backed, and disabled unless config allows generation.
- External-context sessions do not generate durable memory by default.
- Hardcoded Chinese recall trigger is gone.
- Memory settings support recall use, generation use, external-context disable, idle delay, minimum turns, and model route selection.
- Memory evidence/provenance is persisted and shown as counts/metadata, not raw sensitive snippets.
- Memory export is redacted and audited.
- `harness-memory` architecture doc exists and README link is valid.
- Full gates pass with exit code 0.

## Requirement Traceability

| Issue | Covered By |
|---|---|
| Desktop default provider is in-memory | Task 2, Task 7, Task 10 |
| No real semantic retrieval/ranking | Task 3 |
| Browser requires provider `get` and stable lifecycle | Task 2, Task 7 |
| Browser operations reinitialize session manager/provider side effects | Task 7 |
| Hardcoded Chinese recall trigger | Task 5, Task 10 |
| README points to missing doc | Task 9 |
| UI has no create memory entry | Task 8 |
| Single-process recall de-dupe only | Task 3, Task 5; persistent generation idempotency is handled by Task 6 `generation_key` and SQLite unique constraint |
| Desktop only enables external slot, not durable memory | Task 4, Task 7 |
| No generation/config/provenance/taint governance | Task 1, Task 4, Task 5, Task 6, Task 8 |

## Final Report Requirements

The implementing agent must report:

- Worktree path and branch.
- Commit list.
- Files changed by task.
- Subagent audit result for every task.
- Gate commands and exit codes.
- Any intentional destructive refactor and why it was necessary.
- Any requirement not completed. If any requirement is incomplete, do not claim the plan is complete.
