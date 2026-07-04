# Agent Harness Memory Platform Implementation Plan

> **For agentic workers:** REQUIRED MODEL: use `chatgpt-5.5` with reasoning effort `xhigh`. REQUIRED SUB-SKILL: use `superpowers:subagent-driven-development` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Every task requires task-start analysis, task-completion analysis, and an independent read-only subagent audit before the task can be marked complete.

**Goal:** Replace Jyowo's current partial memory stack with a first-class Agent Harness memory platform: scoped, durable, searchable, inspectable, auditable, secure, and subordinate to runtime policy and workspace instructions.

**Architecture:** Rust owns memory policy, storage, recall, write authorization, extraction, consolidation, trace, and audit. React only displays state and sends typed requests through Tauri commands. Long-term memory is split into durable structured storage, visible file-backed memdir context, generated candidate inbox, and run-scoped transient context; no layer can override system instructions, runtime policy, permissions, or workspace instructions.

**Tech Stack:** Rust 1.96, Tokio, serde, schemars, rusqlite with SQLite FTS5, blake3, uuid/newtype IDs already used in contracts, Tauri 2, React 19, TypeScript 6, Zod, TanStack Query, Vitest, Testing Library, cargo test, pnpm gates.

---

## Required Execution Mode

Implementation must happen in an isolated git worktree created from `main`. Do not implement product code in the original `main` workspace.

This plan file must land on `main` before implementation starts. The implementation agent must stop if the plan is not visible in the isolated worktree.

Use branch prefix `goya`.

```bash
cd /Users/goya/Repo/Git/Jyowo
git status --short --branch
git rev-parse --verify main
git ls-tree -r --name-only main -- docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md | grep -Fx docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md
git show main:docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md >/dev/null
git worktree add ../Jyowo-memory-platform -b goya/memory-platform main
cd ../Jyowo-memory-platform
test -f docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md
git status --short --branch
```

Expected:

```text
git rev-parse exits 0
git ls-tree grep exits 0
git show exits 0
test command exits 0
## goya/memory-platform
```

If the branch or directory already exists:

```bash
cd /Users/goya/Repo/Git/Jyowo
git worktree add ../Jyowo-memory-platform-2 -b goya/memory-platform-2 main
cd ../Jyowo-memory-platform-2
test -f docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md
```

All implementation commits must be created from the isolated worktree path.

The original checkout may contain unrelated uncommitted files. Do not revert, stash, or include those files. The isolated implementation worktree must be clean before Task 0 starts; if `git status --short` inside the worktree shows tracked modifications, stop and recreate the worktree from `main`.

After every task, audit, and gate passes, merge the implementation branch into `main` from the original repository path. Do not leave the completed implementation only on a feature branch.

## Mandatory Reading

Before Task 0, read these files inside the isolated worktree:

```text
AGENTS.md
docs/testing/testing-strategy.md
docs/frontend/agent-harness-frontend-development-guidelines.md
docs/frontend/frontend-product-ux.md
docs/frontend/frontend-engineering.md
docs/frontend/frontend-quality.md
docs/design/DESIGN.md
docs/backend/agent-harness-backend-development-guidelines.md
docs/backend/backend-runtime.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md
```

Before each task, re-read that task section in this plan.

For any task touching frontend code, reread all frontend and design docs above.

For any task touching Rust backend code, reread all backend docs above.

For any task touching tests, gates, fixtures, or test helpers, reread `docs/testing/testing-strategy.md`.

If any mandatory file is missing, stop before implementation. Do not substitute another file without revising this plan.

## Mandatory Per-Task Protocol

Each task must follow this exact order:

1. Task-start analysis.
2. Write or update failing tests for every task that changes Rust behavior, frontend behavior, IPC contracts, prompt assembly, storage, or security policy. For docs-only tasks, identify the exact docs gate and diff checks instead.
3. Run the targeted failing test and confirm it fails for the intended reason. For docs-only tasks, record why no behavioral red phase applies and continue to the docs gate.
4. Implement the task.
5. Run targeted verification.
6. Task-completion analysis.
7. Run an independent read-only subagent audit for this task.
8. Fix every audit finding.
9. Re-run targeted verification.
10. Commit the task.

Task-start analysis must be written in the conversation before edits:

```text
Task N start analysis:
- Objective:
- Files read:
- Files to modify:
- Current code facts:
- Invariants from this plan:
- Forbidden shortcuts:
- Targeted failing tests:
- Targeted verification:
```

Task-completion analysis must be written before subagent audit:

```text
Task N completion analysis:
- Implemented requirements:
- Files changed:
- Tests run:
- Runtime/security boundaries preserved:
- Removed obsolete paths:
- Remaining risk:
```

The read-only subagent audit prompt must be used after each task:

```text
Audit Task N of docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md.

Use read-only inspection only.
Read the task section, current diff, changed files, and test output.
Verify:
- The task objective is fully implemented.
- The implementation follows the exact design in the plan.
- The task-start analysis was completed before edits and matches the actual implementation.
- No mock product data, fake provider, hardcoded success, placeholder implementation, compatibility shim, or unused legacy path was introduced.
- Runtime policy remains the authority for memory, permission, tool, MCP, filesystem, network, journal, redaction, replay, and audit.
- Memory stays below system, runtime policy, workspace instructions, and explicit user request priority.
- Tests prove the behavior and fail for the intended reason before implementation for every behavior-changing task.
- Required gates for the task were run.

Return PASS or FAIL.
If FAIL, include exact file paths and line references.
```

If subagent tools are not available, stop and report blocked. Do not self-audit in place of the required subagent audit.

## Non-Negotiable Rules

- Do not use mock product data.
- Do not ship fake memory providers, fake extraction, fake recall traces, fake UI state, hardcoded success, or placeholder persistence.
- Test fixtures may use temporary directories, temporary SQLite databases, deterministic test-only embedding vectors, and scripted model providers only when the test asserts real product behavior. Test doubles must live under test modules or test-support paths and must never be used by production code.
- Do not keep compatibility shims for the old partial memory behavior. If a public contract changes, update the contract, schema export, frontend Zod schema, tests, and docs in the same task.
- Do not make React the authority for memory scope, recall, write permission, extraction, deletion, export, redaction, or security.
- Do not let memory override system, runtime policy, workspace instructions, or user-provided current-turn instruction.
- Do not write secrets into prompt, memory, journal, trace, log, screenshot, frontend state, test snapshot, or docs examples.
- Do not use memory as a fact source for current external facts.
- Do not bypass `PermissionBroker`, `Redactor`, sandbox, workspace scope, MCP origin checks, or tenant scope.
- Do not add unused dependencies, imports, variables, files, feature flags, IPC commands, or docs.
- Keep `unsafe_code = "forbid"` untouched.
- Do not call a feature available until backend runtime, IPC, frontend UI, tests, trace, and recovery semantics exist.

## Audit Classification And Required Fixes

These issues were found during architecture review of this plan. The classification is part of the implementation contract; do not treat design gaps as wording-only edits.

| # | Classification | Required fix in this plan |
|---|---|---|
| 1 | Delivery state issue | The plan must be committed on `main` before worktree creation; the execution command verifies tracked presence with `git ls-tree` and `git show main:...` without switching or modifying the original checkout. |
| 2 | Writing issue | The mandatory design reading file is `docs/design/DESIGN.md`; obsolete design doc paths must not be referenced. |
| 3 | Writing issue | The final gate must not call nonexistent docs scripts; new gate scripts must be added to `package.json` with tests before use. |
| 4 | Writing issue | Tauri Rust tests must use package `jyowo-desktop-shell`, matching `apps/desktop/src-tauri/Cargo.toml`. |
| 5 | Design gap | Desktop runtime wiring must replace `InMemoryMemoryProvider::new("desktop-memory")` with the local provider default and test that production runtime never selects in-memory memory. |
| 6 | Design gap | The feature graph must retire `external-slot` / `memory-external-slot` as runtime architecture, or rename it to registry semantics with compatibility removed. |
| 7 | Design gap | Public contracts must define every trace, settings, provider, candidate, tool, IPC, and error type referenced by later tasks. |
| 8 | Design gap | Embedding and ranking must define provider behavior, fallback behavior, vector dimensions, score normalization, and a deterministic ranking formula. |
| 9 | Design gap | Extraction/consolidation must be a durable worker with queue, lease, idempotency, retry, crash recovery, quota, and typed model output schema. |
| 10 | Design/gate baseline gap | Task 0 must establish a clean executable baseline and fix `docs/testing/test-inventory.md` if it differs from `pnpm audit:tests`. |

## External Reference Principles

These references inform product shape. They do not override Jyowo security rules.

- OpenAI Codex memories: https://developers.openai.com/codex/codex-manual.md
- Claude Code memory: https://code.claude.com/docs/en/memory
- Claude memory tool: https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool

Reference principles to keep:

- Memories are auxiliary context, not required policy.
- Required team guidance belongs in checked-in instructions or docs, not hidden memory.
- Memory use and memory generation must be separately controllable.
- Thread-level choices must not mutate global memory settings.
- Memory generation must be delayed until work is idle enough to avoid summarizing active work.
- Memory storage must be visible, inspectable, deletable, and exportable.
- Memory content must be treated as untrusted data and fenced before injection.
- Secret redaction and deletion must be fail-closed.

## Current Code Facts

Use these facts as the baseline. Do not invent a different starting state.

- `crates/jyowo-harness-sdk/src/system_prompt.rs` defines base prompt priority: system, runtime policy, workspace instructions, memory, user request, external content.
- `crates/jyowo-harness-sdk/src/harness/memory.rs` wires builtin memory into system prompt and external memory into session context.
- `crates/jyowo-harness-context/src/engine.rs` injects memory recall as transient context patches prepended to the user message.
- `crates/jyowo-harness-memory/src/external.rs` owns `MemoryManager`, recall policy, external provider slot, write/update/delete/export, lifecycle hooks, threat scanning, metrics, and events.
- `crates/jyowo-harness-memory/src/store.rs` defines `MemoryStore`.
- `crates/jyowo-harness-memory/src/lifecycle.rs` defines `MemoryProvider = MemoryStore + MemoryLifecycle`.
- `crates/jyowo-harness-memory/src/in_memory.rs` is a test-oriented in-memory provider. It does not perform semantic search, TTL filtering, ranking, dedupe, durable persistence, or production storage.
- `crates/jyowo-harness-memory/src/memdir/mod.rs` implements `MEMORY.md`, `USER.md`, and `DREAMS.md` file storage with atomic writes, locks, limits, snapshots, write events, and threat scanning.
- `crates/jyowo-harness-memory/src/memdir/file.rs` maps `DREAMS.md`, but builtin prompt rendering in `crates/jyowo-harness-sdk/src/system_prompt.rs` only renders `MEMORY.md` and `USER.md`.
- `crates/jyowo-harness-contracts/src/enums.rs` defines `MemoryKind`, `MemoryVisibility`, `MemoryWriteAction`, and `MemorySource`.
- `crates/jyowo-harness-contracts/src/events/memory.rs` defines memory events that avoid raw content.
- `apps/desktop/src-tauri/src/commands/memory.rs` exposes list/get/update/delete/export memory commands.
- `apps/desktop/src/features/memory/MemoryBrowser.tsx` displays memory items and supports inspect, edit, delete, and export.
- `apps/desktop/src/features/conversation/Composer.tsx` can add a memory context reference candidate.
- `crates/jyowo-harness-sdk/src/harness/conversation.rs` currently renders a memory context reference as only `- memory: label (id)`. It does not hydrate the memory content.
- `crates/jyowo-harness-tool/src/builtin/todo.rs` is the only built-in tool under `ToolGroup::Memory`. It is a run todo tool, not a long-term memory tool.
- `crates/jyowo-harness-team/src/lib.rs` contains `SharedMemory`, write policies, and journal-backed team memory writes. This is not a unified long-term memory system.
- `crates/jyowo-harness-subagent/src/lib.rs` contains `SubagentMemoryScope`, including inherit, empty, and subset behavior.
- `crates/jyowo-harness-engine/src/engine.rs` rejects unresolved subset memory scope for subagents.
- `apps/desktop/src-tauri/src/commands/runtime.rs` currently wires `.with_memory_provider(InMemoryMemoryProvider::new("desktop-memory"))`; this must be removed from production runtime.
- `crates/jyowo-harness-sdk/Cargo.toml`, `crates/jyowo-harness-context/Cargo.toml`, `crates/jyowo-harness-engine/Cargo.toml`, and `apps/desktop/src-tauri/Cargo.toml` currently carry `memory-external-slot` / `external-slot` feature wiring; this must be replaced with provider registry semantics.

## Problem Analysis And Target Design

### Problem 1: No production-grade default provider

Cause:

- The existing design exposes `MemoryProvider`, but ships only an in-memory provider for tests and plugin slots.
- Recall quality depends entirely on external provider behavior.
- `MemoryQuery::text`, `min_similarity`, `ttl`, `recall_score`, and `confidence` are not enforced by the in-memory provider.

Target design:

- Add one built-in production provider: `LocalMemoryProvider`.
- Store records in SQLite under `.jyowo/runtime/memory/memory.sqlite3`.
- Use SQLite FTS5 for lexical retrieval.
- Add an embedding table and a provider-independent vector column format for semantic retrieval.
- Use hybrid ranking: lexical score, vector similarity, recency, confidence, access history, source trust, visibility scope, and explicit user selection boost.
- Enforce TTL, tenant, visibility, tombstones, deletion, and source trust inside provider/manager, not UI.

Implementation decision:

- The local provider is the default when memory is enabled.
- Desktop runtime replaces `InMemoryMemoryProvider::new("desktop-memory")` in `apps/desktop/src-tauri/src/commands/runtime.rs` with `LocalMemoryProvider` opened at `.jyowo/runtime/memory/memory.sqlite3`.
- Plugin providers become additional providers in a registry, not a replacement singleton.
- The old single external provider slot is removed as a runtime design. Builder APIs are updated to register providers into the registry.
- The in-memory provider remains available only for tests or explicit `testing` feature exports. Production builders and desktop runtime must not import or construct it.

Embedding and ranking decision:

- Add a `MemoryEmbeddingProvider` trait in `crates/jyowo-harness-memory/src/local/embedding.rs`.
- The local provider must not call network APIs. If no local embedding provider is configured, records store `embedding_state = "missing"` and semantic score is absent, not faked with zero vectors.
- Test-only deterministic embeddings are allowed only under test support modules.
- Embedding vectors use `f32` values serialized as little-endian bytes with an explicit `dimension` column. Query vectors with a mismatched dimension are rejected with a typed error.
- Lexical score is normalized from SQLite FTS5 rank into `[0.0, 1.0]`.
- Vector score is cosine similarity normalized into `[0.0, 1.0]` when both query and record vectors exist.
- Final ranking must use this formula unless a new plan revision changes it:

```text
final_score =
  0.45 * lexical_score
  + 0.30 * vector_score_or_0
  + 0.10 * confidence_score
  + 0.05 * recency_score
  + 0.05 * source_trust_score
  + 0.03 * explicit_selection_boost
  + 0.02 * access_score
```

If no vector score exists, do not renormalize weights. This makes lexical-only recall deterministic and prevents hidden semantic behavior from being implied.

SQLite schema decision:

- `schema_version(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)` records applied migrations.
- `memory_records` stores durable records with these columns:
  - `id TEXT PRIMARY KEY`
  - `tenant_id TEXT NOT NULL`
  - `kind TEXT NOT NULL`
  - `visibility TEXT NOT NULL`
  - `content TEXT NOT NULL`
  - `metadata_json TEXT NOT NULL`
  - `content_hash TEXT NOT NULL`
  - `source_kind TEXT NOT NULL`
  - `evidence_json TEXT NOT NULL`
  - `confidence REAL NOT NULL DEFAULT 1.0`
  - `access_count INTEGER NOT NULL DEFAULT 0`
  - `last_accessed_at TEXT`
  - `created_at TEXT NOT NULL`
  - `updated_at TEXT NOT NULL`
  - `expires_at TEXT`
  - `deleted_at TEXT`
- `memory_embeddings` stores provider-independent vectors with these columns:
  - `memory_id TEXT PRIMARY KEY REFERENCES memory_records(id) ON DELETE CASCADE`
  - `embedding_state TEXT NOT NULL CHECK (embedding_state IN ('missing', 'ready', 'failed', 'disabled'))`
  - `dimension INTEGER`
  - `vector_le_f32 BLOB`
  - `model_id TEXT`
  - `updated_at TEXT NOT NULL`
  - `error_kind TEXT`
- `memory_tombstones` stores deletion barriers with `id`, `tenant_id`, `memory_id`, `content_hash`, `reason`, `evidence_json`, and `created_at`.
- Add indexes for `(tenant_id, visibility)`, `(tenant_id, content_hash)`, `(tenant_id, expires_at)`, `(tenant_id, deleted_at)`, and `(tenant_id, last_accessed_at)`.
- Add an FTS5 table `memory_records_fts` using `unicode61 remove_diacritics 2`, with `content`, `metadata_text`, `memory_id UNINDEXED`, and `tenant_id UNINDEXED`.
- Add insert/update/delete triggers so FTS rows stay synchronized with `memory_records`. Tests must prove update and tombstone/delete paths remove stale searchable text.
- Open SQLite with `PRAGMA foreign_keys=ON`, `PRAGMA journal_mode=WAL`, and a nonzero `busy_timeout`.
- Every write path must use an explicit transaction. Recall must read from one connection snapshot and must not update access counters until final provider-level filters pass.

### Problem 2: No controlled long-term memory write tool

Cause:

- `MemoryManager::upsert` exists, but the model has no general `remember`, `forget`, or `update memory` tool.
- Current `ToolGroup::Memory` only contains `Todo`.
- User/UI edits and provider APIs exist, but there is no runtime-authorized model-visible memory write path.

Target design:

- Add a real built-in `MemoryTool`.
- Actions: `search`, `read`, `create`, `update`, `delete`, `list`, `propose`.
- All write actions use typed action plans, permission checks, threat scanning, redaction, journal/audit events, and provider registry.
- Model-derived writes default to pending candidate unless explicit user instruction or policy allows immediate write.
- Team/tenant visibility writes require stronger authorization than private/user writes.

Implementation decision:

- `TodoTool` stays a run-scoped planning tool but moves out of long-term memory semantics in product copy and capability docs.
- `MemoryTool` is the only built-in tool allowed to mutate long-term memory.

### Problem 3: Builtin memdir is a session-start snapshot

Cause:

- Builtin memory is read by `Harness::builtin_system_prompt(...)` when the session is created.
- Default `BuiltinMemory::write_takes_effect` is `TakesEffect::NextSession`.
- This is safe, but users can mistake it for live memory.

Target design:

- Keep a file-backed memory layer, but make it an index and user-editable knowledge surface, not the entire memory system.
- `MEMORY.md` is a short project memory index.
- `USER.md` is a short user preference index.
- Detailed entries live in topic files under `topics/`.
- Current-session writes become visible through context patches/tool results, not by mutating already-rendered system prompt.

Implementation decision:

- Replace the current monolithic memdir rendering with `MemdirIndex`.
- Prompt rendering includes bounded index summaries only.
- Full memory content is read through `MemoryTool::read` or explicit context reference hydration.

### Problem 4: `DREAMS.md` is undefined product state

Cause:

- `DREAMS.md` exists in file mapping.
- `ConsolidationOutcome` has `draft_dreams`.
- The prompt never renders dreams, and the UI has no lifecycle for them.

Target design:

- Remove `DREAMS.md` from runtime semantics.
- Replace it with a structured `MemoryInbox`.
- Generated candidates live in SQLite and can optionally be mirrored into a human-readable `INBOX.md`.
- Candidate states: `proposed`, `approved`, `rejected`, `promoted`, `merged`, `expired`.

Implementation decision:

- Migration converts existing `DREAMS.md` content into inbox candidates with source `Imported`.
- No unapproved candidate enters model context.

### Problem 5: User control is incomplete

Cause:

- Current UI supports browsing/editing existing memory items.
- There are no first-class settings for use vs generate, per-thread behavior, external-context exclusion, retention, or generation quota.

Target design:

- Add global settings:
  - `use_memories`
  - `generate_memories`
  - `disable_generation_when_external_context_used`
  - `retention_days`
  - `max_memory_bytes`
  - `max_recall_records_per_turn`
  - `max_recall_chars_per_turn`
- Add per-thread settings:
  - `use_memories`
  - `generate_memories`
  - `memory_mode`: `off | read_only | read_write | candidate_only`
- Global settings define defaults. Thread settings override only the current thread.

Implementation decision:

- Rust stores and validates settings.
- Frontend can display toggles and send requests, but it cannot decide availability.

### Problem 6: Memory references are not hydrated

Cause:

- `ConversationContextReference::Memory` currently carries only id and label.
- SDK renders the label and id into user-visible context text.
- The selected memory content is never resolved into actual model context.

Target design:

- Add `ContextReferenceResolver`.
- Memory references are hydrated before `TurnInput` assembly.
- Resolved memory content becomes a fenced transient context patch.
- Hydration failure blocks run start with a typed error.

Implementation decision:

- Do not silently fall back to label-only references.
- Explicit user-selected references get a recall score boost but still pass threat scanning and budget limits.

### Problem 7: Recall explainability is weak

Cause:

- `MemoryRecalledEvent` has counts and hashes, but not candidate IDs, scores, filter reasons, provider latency, budget drops, or redaction outcomes.
- `returned_count` and `kept_count` currently collapse after filtering.

Target design:

- Add `MemoryRecallTrace`.
- Trace stores no raw content.
- Trace captures raw provider counts, candidate IDs, score components, filter/drop reasons, redaction counts, injected IDs, budget usage, provider latency, and policy decisions.

Implementation decision:

- Timeline and memory UI show per-turn "memory used" details from trace.
- Audit/replay can reconstruct which memory IDs influenced a turn without exposing full content.

### Problem 8: TTL exists but is not enforced

Cause:

- `MemoryMetadata.ttl` exists as data.
- Current provider implementations do not uniformly filter or expire records.

Target design:

- TTL is enforced at provider query boundaries.
- Expired items are hidden from recall/list/get by default.
- Cleanup writes tombstones and audit events.
- Export can include expired/tombstoned records only when explicitly requested.

Implementation decision:

- TTL enforcement is not a UI concern.
- TTL fields use absolute `expires_at` in storage and public contracts. Duration-only TTL is converted on write.

### Problem 9: Single external provider slot is too narrow

Cause:

- `MemoryManager` owns one external provider.
- Plugin registry exposes one registered provider.
- There is no built-in fanout, dedupe, rerank, or provider policy.

Target design:

- Add `MemoryProviderRegistry`.
- Providers have id, priority, trust level, read/write capability, visibility limits, timeout, and budget.
- Recall fans out across eligible providers, normalizes candidates, dedupes by content hash/source/evidence, reranks, applies budget, scans, and injects.

Implementation decision:

- Builtin local provider is provider id `local`.
- File-backed memdir index is provider id `memdir`.
- Team shared memory is provider id `team`.
- Plugin providers register as additional providers with explicit trust and capability metadata.
- Replace the old `external-slot` / `memory-external-slot` runtime feature semantics with registry semantics. Rename feature flags to `memory-provider-registry` / `provider-registry`, update every Cargo consumer, and delete the single-slot code path. Do not keep a runtime compatibility alias.

### Problem 10: Team and subagent memory are not unified

Cause:

- Team shared memory is an internal provider-like object.
- Subagent memory scope exists, but not as a unified long-term memory policy.

Target design:

- Subagents choose memory scope explicitly: `inherit`, `empty`, `subset`, `read_only`, `read_write`, `candidate_only`.
- Child writes default to candidate inbox unless parent/coordinator policy allows direct write.
- Team memory promotion requires coordinator or role-gated authorization.
- All child-derived memory records store child session, parent session, run id, agent profile, source evidence, and scope.

Implementation decision:

- Shared team memory participates in provider registry.
- Subagent scope resolution produces typed memory grants, not raw messages.

### Problem 11: Security boundaries need stricter policy

Cause:

- Threat scanner exists, but write tool, extraction, external-context mixing, deletion, and file hydration do not yet share a complete memory security policy.

Target design:

- Memory contents are untrusted data.
- Memory write is fail-closed.
- Memory tool can access only memory provider APIs, not arbitrary filesystem paths.
- External content, MCP output, tool output, web content, and plugin content produce candidates by default.
- Secret detection blocks or redacts before storage, prompt injection, journal, trace, and export.

Implementation decision:

- Add `MemoryPolicyEngine`.
- Add source trust policy and visibility escalation policy.
- Add tombstone protections so deleted memory is not regenerated from old transcript evidence.

### Problem 12: Context panel does not show final model request

Cause:

- Frontend context snapshot is not the final `ModelRequest`.
- SDK/context engine can add system prompt sections, builtin memory, recall patches, pending patches, tool snapshots, and hook-added context.

Target design:

- Rename current panel semantics to `Context Sources`.
- Add backend-generated `Model Request Preview` with redacted sections and trace metadata.
- Preview shows section headers, token estimate, memory ids, provider ids, trace ids, tool names, and policy decisions without exposing secrets.

Implementation decision:

- Do not expose full raw system prompt.
- Preview is generated by Rust using the same redactor as journal/replay.

## Target File Map

Create:

```text
crates/jyowo-harness-memory/src/local/mod.rs
crates/jyowo-harness-memory/src/local/schema.rs
crates/jyowo-harness-memory/src/local/provider.rs
crates/jyowo-harness-memory/src/local/ranking.rs
crates/jyowo-harness-memory/src/local/migrations.rs
crates/jyowo-harness-memory/src/local/embedding.rs
crates/jyowo-harness-memory/src/registry.rs
crates/jyowo-harness-memory/src/policy.rs
crates/jyowo-harness-memory/src/trace.rs
crates/jyowo-harness-memory/src/inbox.rs
crates/jyowo-harness-memory/src/extraction/mod.rs
crates/jyowo-harness-memory/src/extraction/job.rs
crates/jyowo-harness-memory/src/extraction/worker.rs
crates/jyowo-harness-memory/src/extraction/schema.rs
crates/jyowo-harness-memory/src/reference.rs
crates/jyowo-harness-tool/src/builtin/memory.rs
crates/jyowo-harness-sdk/src/harness/memory_preview.rs
apps/desktop/src-tauri/src/commands/memory_settings.rs
apps/desktop/src-tauri/src/commands/memory_traces.rs
apps/desktop/src/features/memory/MemorySettings.tsx
apps/desktop/src/features/memory/MemoryInbox.tsx
apps/desktop/src/features/memory/MemoryRecallTracePanel.tsx
apps/desktop/src/features/context/ModelRequestPreview.tsx
```

Modify:

```text
crates/jyowo-harness-memory/src/lib.rs
crates/jyowo-harness-memory/src/types.rs
crates/jyowo-harness-memory/src/store.rs
crates/jyowo-harness-memory/src/lifecycle.rs
crates/jyowo-harness-memory/src/external.rs
crates/jyowo-harness-memory/src/memdir/mod.rs
crates/jyowo-harness-memory/src/memdir/file.rs
crates/jyowo-harness-memory/src/memdir/fence.rs
crates/jyowo-harness-memory/Cargo.toml
crates/jyowo-harness-contracts/src/enums.rs
crates/jyowo-harness-contracts/src/events/memory.rs
crates/jyowo-harness-contracts/src/events/mod.rs
crates/jyowo-harness-contracts/src/events/types.rs
crates/jyowo-harness-contracts/src/messages.rs
crates/jyowo-harness-contracts/src/capability.rs
crates/jyowo-harness-contracts/src/schema_export.rs
crates/jyowo-harness-context/src/engine.rs
crates/jyowo-harness-context/src/buffer.rs
crates/jyowo-harness-engine/src/turn.rs
crates/jyowo-harness-engine/src/engine.rs
crates/jyowo-harness-sdk/src/builder.rs
crates/jyowo-harness-sdk/src/harness/memory.rs
crates/jyowo-harness-sdk/src/harness/conversation.rs
crates/jyowo-harness-sdk/src/harness/session_runtime.rs
crates/jyowo-harness-sdk/src/system_prompt.rs
crates/jyowo-harness-sdk/Cargo.toml
crates/jyowo-harness-context/Cargo.toml
crates/jyowo-harness-engine/Cargo.toml
crates/jyowo-harness-plugin/src/registry.rs
crates/jyowo-harness-tool/src/builtin/mod.rs
crates/jyowo-harness-tool/src/builder.rs
crates/jyowo-harness-team/src/lib.rs
crates/jyowo-harness-subagent/src/lib.rs
apps/desktop/src-tauri/src/commands/contracts.rs
apps/desktop/src-tauri/src/commands/conversations.rs
apps/desktop/src-tauri/src/commands/memory.rs
apps/desktop/src-tauri/src/commands/mod.rs
apps/desktop/src-tauri/src/commands/runtime.rs
apps/desktop/src-tauri/src/lib.rs
apps/desktop/src-tauri/Cargo.toml
apps/desktop/src/shared/tauri/commands.ts
apps/desktop/src/features/memory/MemoryBrowser.tsx
apps/desktop/src/features/conversation/Composer.tsx
apps/desktop/src/features/context/ContextPanel.tsx
apps/desktop/src/routes/memory.lazy.tsx
```

Delete or fully retire:

```text
crates/jyowo-harness-memory/src/in_memory.rs as production surface
runtime semantics for DREAMS.md
single external memory provider slot as runtime architecture
external-slot / memory-external-slot feature names and cfg paths
label-only memory context reference rendering
```

`InMemoryMemoryProvider` may remain test-only under `#[cfg(any(test, feature = "testing"))]` if existing test utilities still need it. It must not be selected by production builder defaults.

## Target Contracts

These names and shapes are required. Adjust module paths to match existing crate layout, but do not change field semantics without updating this plan through a new plan revision.

```rust
define_scopes! {
    MemoryTraceScope => MemoryTraceId,
    MemoryCandidateScope => MemoryCandidateId,
}

pub type MemoryProviderId = String;

pub type MemoryOriginName = String;
pub type MemoryOriginLabel = String;
pub type MemoryPageCursor = String;

pub struct MemoryRecord {
    pub id: MemoryId,
    pub tenant_id: TenantId,
    pub kind: MemoryKind,
    pub visibility: MemoryVisibility,
    pub content: String,
    pub metadata: MemoryMetadata,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
}

pub enum MemorySource {
    UserInput,
    AgentDerived,
    SubagentDerived,
    ToolOutput,
    McpToolOutput,
    PluginOutput,
    WebRetrieval,
    WorkspaceFile,
    ExternalRetrieval,
    Imported,
    Consolidated,
}

pub struct MemoryEvidence {
    pub source: MemorySource,
    pub origin: MemoryEvidenceOrigin,
    pub content_hash: ContentHash,
    pub session_id: Option<SessionId>,
    pub run_id: Option<RunId>,
    pub message_id: Option<MessageId>,
    pub tool_use_id: Option<ToolUseId>,
}

pub enum MemoryEvidenceOrigin {
    UserMessage {
        session_id: SessionId,
        run_id: RunId,
        message_id: MessageId,
    },
    AssistantMessage {
        session_id: SessionId,
        run_id: RunId,
        message_id: MessageId,
    },
    SubagentOutput {
        parent_session_id: SessionId,
        child_session_id: SessionId,
        run_id: RunId,
        agent_id: Option<AgentId>,
    },
    BuiltinToolOutput {
        tool_name: MemoryOriginName,
        tool_use_id: ToolUseId,
    },
    McpToolOutput {
        server_id: String,
        tool_name: MemoryOriginName,
        tool_use_id: ToolUseId,
    },
    PluginOutput {
        plugin_id: String,
        tool_name: Option<MemoryOriginName>,
        tool_use_id: Option<ToolUseId>,
    },
    WebRetrieval {
        url_hash: ContentHash,
        fetch_tool_use_id: Option<ToolUseId>,
    },
    WorkspaceFile {
        workspace_id: WorkspaceId,
        path_hash: ContentHash,
        snapshot_id: Option<SnapshotId>,
    },
    Imported {
        importer: MemoryOriginName,
        import_id: String,
    },
    Consolidated {
        from: Vec<MemoryId>,
    },
}

pub struct MemoryCandidate {
    pub id: MemoryCandidateId,
    pub tenant_id: TenantId,
    pub state: MemoryCandidateState,
    pub proposed_record: MemoryRecordDraft,
    pub evidence: MemoryEvidence,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

pub struct MemoryRecordDraft {
    pub kind: MemoryKind,
    pub visibility: MemoryVisibility,
    pub content: String,
    pub metadata: MemoryMetadata,
    pub expires_at: Option<DateTime<Utc>>,
}

pub struct MemoryRecallTrace {
    pub trace_id: MemoryTraceId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub turn: u32,
    pub query_text_hash: ContentHash,
    pub provider_results: Vec<MemoryProviderTrace>,
    pub candidates: Vec<MemoryCandidateTrace>,
    pub injected: Vec<MemoryInjectedTrace>,
    pub dropped: Vec<MemoryDroppedTrace>,
    pub redacted_count: u32,
    pub injected_chars: u32,
    pub deadline_used_ms: u32,
    pub at: DateTime<Utc>,
}

pub struct MemoryProviderTrace {
    pub provider_id: MemoryProviderId,
    pub trust_level: MemoryProviderTrust,
    pub readable: bool,
    pub writable: bool,
    pub requested_count: u32,
    pub returned_count: u32,
    pub timed_out: bool,
    pub error_kind: Option<String>,
    pub latency_ms: u32,
}

pub struct MemoryScoreBreakdown {
    pub lexical_score: f32,
    pub vector_score: Option<f32>,
    pub confidence_score: f32,
    pub recency_score: f32,
    pub access_score: f32,
    pub source_trust_score: f32,
    pub explicit_selection_boost: f32,
    pub final_score: f32,
}

pub struct MemoryCandidateTrace {
    pub memory_id: MemoryId,
    pub provider_id: MemoryProviderId,
    pub content_hash: ContentHash,
    pub score: MemoryScoreBreakdown,
    pub policy_decision: MemoryPolicyDecision,
}

pub struct MemoryInjectedTrace {
    pub memory_id: MemoryId,
    pub provider_id: MemoryProviderId,
    pub content_hash: ContentHash,
    pub injected_chars: u32,
    pub fence_id: String,
}

pub struct MemoryDroppedTrace {
    pub memory_id: Option<MemoryId>,
    pub provider_id: Option<MemoryProviderId>,
    pub content_hash: Option<ContentHash>,
    pub reason: MemoryDropReason,
}

pub enum MemoryDropReason {
    Expired,
    Deleted,
    VisibilityDenied,
    PolicyDenied,
    ThreatBlocked,
    BudgetExceeded,
    Duplicate,
    ProviderTimeout,
    ProviderError,
    ScoreBelowThreshold,
}

pub enum MemoryPolicyDecision {
    Allow,
    Deny { reason: MemoryPolicyDenyReason },
    CandidateOnly { reason: MemoryPolicyDenyReason },
}

pub enum MemoryPolicyDenyReason {
    GlobalUseDisabled,
    ThreadUseDisabled,
    GlobalGenerationDisabled,
    ThreadGenerationDisabled,
    ExternalContextGenerationDisabled,
    MissingPolicy,
    VisibilityEscalationDenied,
    ProviderNotWritable,
    TenantMismatch,
    TombstoneMatched,
    PermissionRequired,
    ThreatBlocked,
}

pub enum MemoryCandidateState {
    Proposed,
    Approved,
    Rejected,
    Promoted,
    Merged,
    Expired,
}

pub enum MemoryThreadMode {
    Off,
    ReadOnly,
    ReadWrite,
    CandidateOnly,
}

pub struct MemoryGlobalSettings {
    pub use_memories: bool,
    pub generate_memories: bool,
    pub disable_generation_when_external_context_used: bool,
    pub retention_days: Option<u32>,
    pub max_memory_bytes: u64,
    pub max_recall_records_per_turn: u32,
    pub max_recall_chars_per_turn: u32,
}

pub struct MemoryThreadSettings {
    pub session_id: SessionId,
    pub use_memories: Option<bool>,
    pub generate_memories: Option<bool>,
    pub memory_mode: MemoryThreadMode,
}

pub enum MemoryActor {
    User { user_label: Option<MemoryOriginLabel> },
    Model,
    System,
    Subagent { child_session_id: SessionId, agent_id: Option<AgentId> },
}

pub struct MemoryPermissionContext {
    pub explicit_user_instruction: bool,
    pub action_plan_id: Option<ActionPlanId>,
    pub authorization_ticket_id: Option<AuthorizationTicketId>,
    pub non_interactive_policy_grant: bool,
}

pub struct MemoryToolArgs {
    pub action: MemoryToolAction,
}

pub struct MemoryToolRuntimeContext {
    pub actor: MemoryActor,
    pub permission_context: MemoryPermissionContext,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub provider_policy: MemoryProviderSelectionPolicy,
}

pub struct MemoryToolRequest {
    pub args: MemoryToolArgs,
    pub runtime: MemoryToolRuntimeContext,
}

pub enum MemoryProviderSelectionPolicy {
    PolicySelected,
    RequireProvider { provider_id: MemoryProviderId },
    DenyModelSelectedProvider,
}

pub enum MemoryToolAction {
    Search(MemorySearchRequest),
    Read(MemoryReadRequest),
    Create(MemoryToolCreateArgs),
    Update(MemoryToolUpdateArgs),
    Delete(MemoryDeleteRequest),
    List(MemoryListRequest),
    Propose(MemoryToolProposeArgs),
}

pub struct MemorySearchRequest {
    pub query: String,
    pub max_records: u32,
    pub visibility: Option<MemoryVisibility>,
    pub cursor: Option<MemoryPageCursor>,
}

pub struct MemoryReadRequest {
    pub memory_id: MemoryId,
}

pub struct MemoryToolCreateArgs {
    pub draft: MemoryRecordDraft,
}

pub struct MemoryToolUpdateArgs {
    pub memory_id: MemoryId,
    pub draft: MemoryRecordDraft,
}

pub struct MemoryDeleteRequest {
    pub memory_id: MemoryId,
    pub reason: String,
}

pub struct MemoryListRequest {
    pub visibility: Option<MemoryVisibility>,
    pub include_expired: bool,
    pub include_deleted: bool,
    pub limit: u32,
    pub cursor: Option<MemoryPageCursor>,
}

pub struct MemoryToolProposeArgs {
    pub draft: MemoryRecordDraft,
}

pub struct MemoryToolResponse {
    pub action: String,
    pub state: MemoryToolState,
    pub memory_ids: Vec<MemoryId>,
    pub candidate_ids: Vec<MemoryCandidateId>,
    pub records: Vec<MemoryToolRecordView>,
    pub next_cursor: Option<MemoryPageCursor>,
    pub action_plan_id: Option<ActionPlanId>,
    pub denial: Option<MemoryToolDenial>,
    pub redaction: MemoryRedactionSummary,
    pub trace_id: Option<MemoryTraceId>,
    pub takes_effect: MemoryTakesEffect,
}

pub struct MemoryToolRecordView {
    pub memory_id: MemoryId,
    pub provider_id: MemoryProviderId,
    pub kind: MemoryKind,
    pub visibility: MemoryVisibility,
    pub redacted_content: Option<String>,
    pub content_hash: ContentHash,
    pub score: Option<MemoryScoreBreakdown>,
}

pub struct MemoryToolDenial {
    pub reason: MemoryPolicyDenyReason,
    pub safe_message: String,
    pub action_plan_id: Option<ActionPlanId>,
}

pub struct MemoryRedactionSummary {
    pub redacted_count: u32,
    pub dropped_count: u32,
}

pub enum MemoryToolState {
    Completed,
    CandidateCreated,
    PermissionRequired { action_plan_id: ActionPlanId },
    Denied { reason: MemoryPolicyDenyReason },
}

pub enum MemoryTakesEffect {
    CurrentTurn,
    NextTurn,
    NextSession,
    Never,
}
```

Required provider registry abstractions:

```rust
pub enum MemoryProviderTrust {
    BuiltIn,
    Workspace,
    Team,
    Plugin,
    External,
}

pub enum MemoryVisibilityClass {
    Private,
    User,
    Team,
    Tenant,
}

pub struct MemoryProviderDescriptor {
    pub provider_id: MemoryProviderId,
    pub priority: i32,
    pub trust_level: MemoryProviderTrust,
    pub readable: bool,
    pub writable: bool,
    pub allowed_visibility: Vec<MemoryVisibilityClass>,
    pub timeout_ms: u32,
    pub max_records_per_recall: u32,
    pub max_chars_per_recall: u32,
    pub max_bytes_per_record: u64,
}

pub trait MemoryProvider: MemoryStore + MemoryLifecycle {
    fn descriptor(&self) -> MemoryProviderDescriptor;
}
```

Required tool actions:

```text
memory.search
memory.read
memory.create
memory.update
memory.delete
memory.list
memory.propose
```

Only `MemoryToolArgs` is prompt-visible. `MemoryToolRuntimeContext` is injected by the harness after tool-call parsing and must never be accepted from model JSON. A model-selected provider id is not trusted input; provider choice is resolved by `MemoryPolicyEngine` and `MemoryProviderSelectionPolicy`. Evidence for create, update, delete, and propose actions is constructed by the runtime from the current turn, actor, tool call id, and redacted source context; it is never accepted from model-provided arguments.

Required IPC surfaces:

```text
get_memory_settings
update_memory_settings
get_thread_memory_settings
update_thread_memory_settings
list_memory_candidates
approve_memory_candidate
reject_memory_candidate
merge_memory_candidate
list_memory_recall_traces
get_memory_recall_trace
get_model_request_preview
```

All IPC request and response payloads must be backed by Rust serde contracts and mirrored by frontend Zod schemas. Do not define frontend-only memory payloads.

Required IPC payload contracts:

```rust
pub struct GetMemorySettingsRequest {
    pub tenant_id: TenantId,
}

pub struct GetMemorySettingsResponse {
    pub settings: MemoryGlobalSettings,
}

pub struct UpdateMemorySettingsRequest {
    pub tenant_id: TenantId,
    pub settings: MemoryGlobalSettings,
}

pub struct UpdateMemorySettingsResponse {
    pub settings: MemoryGlobalSettings,
}

pub struct GetThreadMemorySettingsRequest {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
}

pub struct GetThreadMemorySettingsResponse {
    pub settings: MemoryThreadSettings,
}

pub struct UpdateThreadMemorySettingsRequest {
    pub tenant_id: TenantId,
    pub settings: MemoryThreadSettings,
}

pub struct UpdateThreadMemorySettingsResponse {
    pub settings: MemoryThreadSettings,
}

pub struct ListMemoryCandidatesRequest {
    pub tenant_id: TenantId,
    pub session_id: Option<SessionId>,
    pub state: Option<MemoryCandidateState>,
    pub limit: u32,
    pub cursor: Option<MemoryPageCursor>,
}

pub struct ListMemoryCandidatesResponse {
    pub candidates: Vec<MemoryCandidateListItem>,
    pub next_cursor: Option<MemoryPageCursor>,
}

pub struct MemoryCandidateListItem {
    pub id: MemoryCandidateId,
    pub state: MemoryCandidateState,
    pub proposed_record: MemoryRecordDraft,
    pub evidence: MemoryEvidence,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

pub struct ApproveMemoryCandidateRequest {
    pub tenant_id: TenantId,
    pub candidate_id: MemoryCandidateId,
    pub action_plan_id: Option<ActionPlanId>,
}

pub struct ApproveMemoryCandidateResponse {
    pub candidate: MemoryCandidate,
    pub memory_id: MemoryId,
}

pub struct RejectMemoryCandidateRequest {
    pub tenant_id: TenantId,
    pub candidate_id: MemoryCandidateId,
    pub reason: String,
}

pub struct RejectMemoryCandidateResponse {
    pub candidate: MemoryCandidate,
}

pub struct MergeMemoryCandidateRequest {
    pub tenant_id: TenantId,
    pub candidate_ids: Vec<MemoryCandidateId>,
    pub merged_record: MemoryRecordDraft,
    pub evidence: MemoryEvidence,
    pub action_plan_id: Option<ActionPlanId>,
}

pub struct MergeMemoryCandidateResponse {
    pub candidate_ids: Vec<MemoryCandidateId>,
    pub memory_id: MemoryId,
}

pub struct ListMemoryRecallTracesRequest {
    pub tenant_id: TenantId,
    pub session_id: Option<SessionId>,
    pub run_id: Option<RunId>,
    pub limit: u32,
    pub cursor: Option<MemoryPageCursor>,
}

pub struct ListMemoryRecallTracesResponse {
    pub traces: Vec<MemoryRecallTraceSummary>,
    pub next_cursor: Option<MemoryPageCursor>,
}

pub struct MemoryRecallTraceSummary {
    pub trace_id: MemoryTraceId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub injected_count: u32,
    pub dropped_count: u32,
    pub redacted_count: u32,
    pub at: DateTime<Utc>,
}

pub struct GetMemoryRecallTraceRequest {
    pub tenant_id: TenantId,
    pub trace_id: MemoryTraceId,
}

pub struct GetMemoryRecallTraceResponse {
    pub trace: MemoryRecallTrace,
}

pub struct GetModelRequestPreviewRequest {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub trace_id: Option<MemoryTraceId>,
}

pub struct GetModelRequestPreviewResponse {
    pub preview: MemoryModelRequestPreview,
}

pub struct MemoryModelRequestPreview {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub sections: Vec<MemoryModelRequestPreviewSection>,
    pub redacted_count: u32,
    pub content_hash: ContentHash,
}

pub struct MemoryModelRequestPreviewSection {
    pub source: MemorySource,
    pub provider_id: Option<MemoryProviderId>,
    pub memory_ids: Vec<MemoryId>,
    pub redacted_content: String,
}
```

## Task 0: Baseline Repository And Gate Sanity

**Purpose:** Ensure the plan is executable from `main` and remove known gate drift before product implementation starts.

**Files:**

- Modify if needed: `docs/testing/test-inventory.md`
- Modify only if a new gate is intentionally added: `package.json`
- Test: docs and gate commands listed below

**Design requirements:**

- This plan file must be tracked on `main`.
- Every mandatory reading file must exist.
- Every final gate command in this plan must exist in `package.json` or be a direct `cargo` command.
- `docs/testing/test-inventory.md` must match `pnpm audit:tests`.
- Do not add a nonexistent design docs gate; this repository currently has `docs/design/DESIGN.md`.
- Task 0 must run inside the isolated worktree. Dirty files in the original checkout are not evidence of plan failure and must not be reverted, stashed, staged, or committed by this plan.

- [x] Verify plan tracking and worktree entry preconditions:

```bash
git rev-parse --verify main
git ls-tree -r --name-only main -- docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md | grep -Fx docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md
git show main:docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md >/dev/null
git status --short
```

Expected: every command exits 0, and `git status --short` inside the isolated worktree prints no tracked modifications.

- [x] Verify mandatory reading files exist:

```bash
for file in \
  AGENTS.md \
  docs/testing/testing-strategy.md \
  docs/frontend/agent-harness-frontend-development-guidelines.md \
  docs/frontend/frontend-product-ux.md \
  docs/frontend/frontend-engineering.md \
  docs/frontend/frontend-quality.md \
  docs/design/DESIGN.md \
  docs/backend/agent-harness-backend-development-guidelines.md \
  docs/backend/backend-runtime.md \
  docs/backend/backend-engineering.md \
  docs/backend/backend-quality.md \
  docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md
do
  test -f "$file" || { echo "missing $file"; exit 1; }
done
```

Expected: exits 0 with no missing file output.

- [x] Verify final pnpm gates exist:

```bash
node -e 'const pkg = require("./package.json"); for (const name of ["check:docs","check:agent-docs","check:frontend-docs","check:backend-docs","check:desktop","check:rust","audit:tests","check:test-architecture","check:agent-orchestration-no-fakes","check:agent-supervisor-sidecar","check:quick","check:frontend:fast","check:rust:fast","check"]) { if (!pkg.scripts[name]) { throw new Error(`missing script ${name}`); } }'
```

Expected: exits 0.

- [x] Regenerate testing inventory only if it differs:

```bash
pnpm audit:tests > /tmp/jyowo-test-inventory.current
diff -u docs/testing/test-inventory.md /tmp/jyowo-test-inventory.current || cp /tmp/jyowo-test-inventory.current docs/testing/test-inventory.md
```

Expected: if `diff` fails, `docs/testing/test-inventory.md` is updated to the audited output.

- [x] Run baseline verification:

```bash
pnpm check:testing-docs
pnpm check:docs
```

Expected: every command exits 0.

- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 0.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit if files changed: no product files changed; test inventory already in sync.

```bash
git add docs/testing/test-inventory.md package.json
git diff --cached --quiet || git commit -m "chore(testing): refresh baseline docs gate"
```

## Task 1: Contracts, Schema, And Docs Baseline

**Purpose:** Lock the public memory model before touching runtime code.

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-contracts/src/ids.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/memory.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/types.rs`
- Modify: `crates/jyowo-harness-contracts/src/messages.rs`
- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Test: `crates/jyowo-harness-contracts/tests/memory_platform_contracts.rs`

- [x] Write contract tests for every type in "Target Contracts": ids, records, drafts, evidence origin, candidates, trace child structures, score breakdown, drop reasons, policy decisions, global/thread settings, prompt-visible tool args, runtime tool context, tool request/response, provider descriptor budgets, provider trust, visibility classes, IPC request/response payloads, and typed denial reasons.
- [x] Run targeted red test: confirmed 226 errors before implementation.
- [x] Add/modify contracts exactly as defined in "Target Contracts"; do not leave any referenced type undefined for later tasks.
- [x] Export schemas in `schema_export.rs`.
- [x] Update backend docs to state memory is auxiliary context, not policy or fact authority.
- [x] Update frontend docs to state React can display memory settings and traces but cannot decide memory policy.
- [x] Run targeted verification:

```bash
cargo test -p jyowo-harness-contracts memory_platform_contracts -- --nocapture
pnpm check:backend-docs
pnpm check:frontend-docs
```

- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 1.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

```bash
git add crates/jyowo-harness-contracts docs/backend docs/frontend
git commit -m "feat(memory): define platform memory contracts"
```

## Task 2: Local SQLite Memory Provider

**Purpose:** Add the production default provider. This replaces the current test-only provider as the real storage path.

**Files:**

- Create: `crates/jyowo-harness-memory/src/local/mod.rs`
- Create: `crates/jyowo-harness-memory/src/local/schema.rs`
- Create: `crates/jyowo-harness-memory/src/local/provider.rs`
- Create: `crates/jyowo-harness-memory/src/local/ranking.rs`
- Create: `crates/jyowo-harness-memory/src/local/migrations.rs`
- Create: `crates/jyowo-harness-memory/src/local/embedding.rs`
- Modify: `crates/jyowo-harness-memory/src/lib.rs`
- Modify: `crates/jyowo-harness-memory/src/types.rs`
- Modify: `crates/jyowo-harness-memory/src/store.rs`
- Modify: `crates/jyowo-harness-memory/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Test: `crates/jyowo-harness-memory/tests/local_provider.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs` or `apps/desktop/src-tauri/tests/commands/memory.rs`, following existing test structure

**Design requirements:**

- Use SQLite under caller-provided path.
- Use FTS5 for lexical search.
- Implement the exact schema, indexes, FTS triggers, WAL settings, and transaction rules from "SQLite schema decision". Do not replace them with ad hoc tables.
- Store embeddings as typed binary vectors or JSON arrays with explicit dimension.
- Store embedding state with the explicit enum values `missing`, `ready`, `failed`, and `disabled`; do not fake semantic recall with empty or zero vectors.
- Enforce tenant, visibility, `expires_at`, and `deleted_at` in SQL queries.
- Increment `access_count` and `last_accessed_at` only after a record survives provider-level visibility and expiry filters.
- Do not depend on network APIs.
- Do not select the in-memory provider by default.
- Desktop runtime must open `LocalMemoryProvider` at `.jyowo/runtime/memory/memory.sqlite3`.
- Desktop runtime must remove `.with_memory_provider(InMemoryMemoryProvider::new("desktop-memory"))`.
- Any production import of `InMemoryMemoryProvider` in `apps/desktop/src-tauri/src` is a task failure.
- Add `rusqlite` / `refinery` dependencies through workspace dependencies only.

- [x] Write failing tests: 12 tests covering all required behaviors.
- [x] Run targeted red test: confirmed compilation failure before module existed.
- [x] Implement migrations with explicit schema version table.
- [x] Implement `LocalMemoryProvider::open(path, tenant_id, options)`.
- [x] Implement durable CRUD, list, recall, export, TTL filtering, tombstone filtering, and ranking.
- [x] Implement desktop runtime provider path resolution using the existing workspace runtime directory pattern.
- [x] Move production access away from `InMemoryMemoryProvider`. Production clean; remains available under `#[cfg(feature = "external-slot")]` for existing test support.
- [x] Run targeted verification: 12/12 local_provider tests pass; all memory crate tests pass; desktop shell source clean of InMemoryMemoryProvider.
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 2.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

## Task 3: Provider Registry And Manager Refactor

**Purpose:** Replace the single external provider slot with a registry and fanout pipeline.

**Files:**

- Create: `crates/jyowo-harness-memory/src/registry.rs`
- Modify: `crates/jyowo-harness-memory/src/external.rs`
- Modify: `crates/jyowo-harness-memory/src/lifecycle.rs`
- Modify: `crates/jyowo-harness-memory/Cargo.toml`
- Modify: `crates/jyowo-harness-context/Cargo.toml`
- Modify: `crates/jyowo-harness-engine/Cargo.toml`
- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory.rs`
- Modify: `crates/jyowo-harness-sdk/Cargo.toml`
- Modify: `crates/jyowo-harness-plugin/src/registry.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Test: `crates/jyowo-harness-memory/tests/provider_registry.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly_memory.rs`
- Test: `crates/jyowo-harness-plugin/tests/registry.rs`

**Design requirements:**

- `MemoryManager` owns `MemoryProviderRegistry`.
- Registry supports multiple providers.
- Provider descriptors define read/write capability and visibility limits.
- Provider descriptors define per-provider recall budgets: max records, max chars, and max bytes per record.
- Recall fans out with provider-specific timeout.
- Fanout failures degrade only that provider unless policy requires fail-closed.
- Dedupe occurs by content hash, source evidence, and record id.
- Reranking happens after fanout and before budget.
- Builder APIs register providers. They do not overwrite previous providers.
- Rename `external-slot` and `memory-external-slot` features to registry feature names and update all Cargo consumers.
- Remove or rewrite every `#[cfg(feature = "memory-external-slot")]` and `#[cfg(feature = "external-slot")]` branch so no single-slot runtime path remains.
- Add a feature matrix check for `provider-registry`, `memory-provider-registry`, and all crates that still expose `recall-memory`. Default workspace checks alone are not sufficient for this task.

- [x] Write failing tests for two providers returning overlapping records and verify dedupe/rerank.
- [x] Write failing test that plugin provider and local provider both participate in recall.
- [x] Write failing test that provider-level record, char, and byte budgets are enforced before global recall budget.
- [x] Write failing test that write targets choose the correct writable provider by policy.
- [x] Write failing architecture/gate test that rejects single-slot fields such as `external: RwLock<Option<Arc<dyn MemoryProvider>>>`.
- [x] Run targeted red tests:

```bash
cargo test -p jyowo-harness-memory --test provider_registry -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly_memory -- --nocapture
cargo test -p jyowo-harness-plugin registry -- --nocapture
rg -n '"(external-slot|memory-external-slot)"|feature = "(external-slot|memory-external-slot)"|with_external_memory_provider|external: RwLock<Option<Arc<dyn MemoryProvider>>>' Cargo.toml crates apps && exit 1 || true
```

Expected: behavior tests fail because registry behavior is not implemented; the grep gate may fail while old single-slot feature names still exist. Do not treat missing new feature names as a valid red-test signal.

- [x] Implement registry and update `MemoryManager`.
- [x] Remove runtime dependency on single `external` provider slot.
- [x] Rename memory feature flags and update every Cargo consumer.
- [x] Update plugin registration to append providers into registry with descriptors.
- [x] Update metrics to include provider id per registry participant.
- [x] Run targeted verification:

```bash
cargo test -p jyowo-harness-memory --test provider_registry -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly_memory -- --nocapture
cargo test -p jyowo-harness-plugin registry -- --nocapture
cargo check -p jyowo-harness-memory --no-default-features --features provider-registry
cargo test -p jyowo-harness-memory --no-default-features --features provider-registry --test provider_registry -- --nocapture
cargo check -p jyowo-harness-context --no-default-features --features recall-memory
cargo check -p jyowo-harness-engine --no-default-features --features recall-memory
cargo check -p jyowo-harness-sdk --no-default-features --features memory-provider-registry,stream-permission,rule-engine-permission,integrity,jsonl-store,sqlite-store,blob-file,local-sandbox,mcp-http,mcp-stdio
pnpm check:rust-deps
rg -n '"(external-slot|memory-external-slot)"|feature = "(external-slot|memory-external-slot)"|with_external_memory_provider|external: RwLock<Option<Arc<dyn MemoryProvider>>>' Cargo.toml crates apps && exit 1 || true
```
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 3.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

```bash
git add crates/jyowo-harness-memory crates/jyowo-harness-context crates/jyowo-harness-engine crates/jyowo-harness-sdk crates/jyowo-harness-plugin apps/desktop/src-tauri/Cargo.toml scripts
git commit -m "refactor(memory): use provider registry"
```

## Task 4: Memory Policy Engine

**Purpose:** Centralize memory use/generation/write policy and source trust.

**Files:**

- Create: `crates/jyowo-harness-memory/src/policy.rs`
- Modify: `crates/jyowo-harness-memory/src/external.rs`
- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/memory.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory.rs`
- Test: `crates/jyowo-harness-memory/tests/policy.rs`

**Design requirements:**

- Memory policy resolves global settings, thread settings, provider trust, actor, source, visibility, requested action, and external-context state.
- Source trust decisions must use `MemoryEvidenceOrigin`, not only `MemorySource`. MCP server id, plugin id, builtin tool name, workspace file hash, and web retrieval hash must remain available to policy and audit.
- Writes fail closed when policy is missing.
- External content and tool/MCP/plugin output default to candidate-only.
- Team/tenant visibility writes require explicit policy allowance.
- Deleted/tombstoned content cannot be regenerated from old transcript evidence.

- [x] Write failing tests:
  - global off prevents recall and generation.
  - thread off overrides global on for that thread only.
  - external-context thread blocks generation when configured.
  - user explicit remember can write user/private memory when policy allows.
  - model-derived external fact becomes candidate.
  - team visibility write is denied without role/coordinator policy.
  - tombstoned content cannot be recreated from same evidence hash.
- [x] Run targeted red test:

```bash
cargo test -p jyowo-harness-memory --test policy -- --nocapture
```

- [x] Implement `MemoryPolicyEngine`.
- [x] Wire policy checks into recall, write, delete, candidate promotion, extraction, and export.
- [x] Add typed denial reasons in contracts.
- [x] Run targeted verification:

```bash
cargo test -p jyowo-harness-memory --test policy -- --nocapture
cargo test -p jyowo-harness-memory --test store_audit -- --nocapture
```

- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 4.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

```bash
git add crates/jyowo-harness-memory crates/jyowo-harness-contracts crates/jyowo-harness-sdk
git commit -m "feat(memory): add policy engine"
```

## Task 5: Recall Trace And Explainability

**Purpose:** Make every memory injection auditable without storing raw memory content in events.

**Files:**

- Create: `crates/jyowo-harness-memory/src/trace.rs`
- Modify: `crates/jyowo-harness-memory/src/external.rs`
- Modify: `crates/jyowo-harness-context/src/engine.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/memory.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory.rs`
- Create: `apps/desktop/src-tauri/src/commands/memory_traces.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Test: `crates/jyowo-harness-memory/tests/recall_trace.rs`
- Test: `crates/jyowo-harness-context/tests/memory_recall.rs`
- Test: `apps/desktop/src-tauri/tests/commands/memory.rs`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`

**Design requirements:**

- Trace stores provider counts, candidate IDs, score components, drop reasons, redaction count, injected IDs, budget usage, latency, and policy decisions.
- Trace does not store raw content.
- `MemoryRecalledEvent` links to trace id.
- IPC can list and fetch traces by session/turn.

- [ ] Write failing tests that assert trace contains candidate ids and score components but not raw content.
- [ ] Write failing tests that budget-dropped and threat-blocked records appear as dropped reasons.
- [ ] Write failing IPC/Zod tests for trace commands.
- [ ] Run targeted red tests:

```bash
cargo test -p jyowo-harness-memory --test recall_trace -- --nocapture
cargo test -p jyowo-harness-context --test memory_recall -- --nocapture
pnpm --dir apps/desktop test src/shared/tauri/commands.test.ts
```

- [ ] Implement trace structures, storage through event/journal-compatible payloads, and IPC.
- [ ] Update context engine to attach trace ids to memory recall patches.
- [ ] Run targeted verification with the same commands.
- [ ] Complete task-completion analysis.
- [ ] Run read-only subagent audit for Task 5.
- [ ] Fix audit findings and re-run targeted verification.
- [ ] Commit:

```bash
git add crates apps/desktop
git commit -m "feat(memory): add recall traces"
```

## Task 6: Reference Hydration And Model Request Preview

**Purpose:** Replace label-only memory references and show the real redacted model context shape.

**Files:**

- Create: `crates/jyowo-harness-memory/src/reference.rs`
- Create: `crates/jyowo-harness-sdk/src/harness/memory_preview.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/conversation.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Create: `apps/desktop/src/features/context/ModelRequestPreview.tsx`
- Modify: `apps/desktop/src/features/context/ContextPanel.tsx`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly_context.rs`
- Test: `apps/desktop/src-tauri/tests/commands/runs.rs`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`
- Test: `apps/desktop/src/features/context/ContextPanel.test.tsx`

**Design requirements:**

- `ConversationContextReference::Memory` resolves to actual memory content before model request assembly.
- Hydrated memory content is fenced as untrusted context.
- Missing, unauthorized, expired, deleted, or threat-blocked memory references fail run start with typed errors.
- Model request preview is generated in Rust and redacted.
- Preview shows section metadata, memory ids, trace ids, provider ids, tool names, and token estimate; it does not expose full raw system prompt.

- [ ] Write failing SDK test proving memory reference content appears in the model request and label-only text is removed.
- [ ] Write failing SDK test proving unauthorized/expired memory reference blocks run start.
- [ ] Write failing IPC/Zod tests for `get_model_request_preview`.
- [ ] Write failing frontend test showing `Context Sources` and `Model Request Preview` are distinct.
- [ ] Run targeted red tests:

```bash
cargo test -p jyowo-harness-sdk runtime_assembly_context -- --nocapture
cargo test -p jyowo-desktop-shell --test commands runs -- --nocapture
pnpm --dir apps/desktop test src/shared/tauri/commands.test.ts
pnpm --dir apps/desktop test src/features/context/ContextPanel.test.tsx
```

- [ ] Implement reference resolver and preview facade.
- [ ] Remove label-only memory rendering path.
- [ ] Wire preview command through Tauri and frontend.
- [ ] Run targeted verification with the same commands.
- [ ] Complete task-completion analysis.
- [ ] Run read-only subagent audit for Task 6.
- [ ] Fix audit findings and re-run targeted verification.
- [ ] Commit:

```bash
git add crates apps/desktop
git commit -m "feat(memory): hydrate references and preview context"
```

## Task 7: Memory Tool

**Purpose:** Add the model-visible controlled long-term memory tool.

**Files:**

- Create: `crates/jyowo-harness-tool/src/builtin/memory.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/mod.rs`
- Modify: `crates/jyowo-harness-tool/src/builder.rs`
- Modify: `crates/jyowo-harness-tool/src/pool.rs`
- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Test: `crates/jyowo-harness-tool/tests/memory_tool.rs`
- Test: `crates/jyowo-harness-engine/tests/main_loop.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs`

**Design requirements:**

- Tool actions: `search`, `read`, `create`, `update`, `delete`, `list`, `propose`.
- Tool descriptors expose only `MemoryToolArgs` to the model. Tool handlers must build `MemoryToolRequest` internally by combining parsed args with harness-injected `MemoryToolRuntimeContext`.
- The model must not be able to provide or override actor, tenant id, session id, run id, permission context, provider policy, action plan id, authorization ticket id, or any non-interactive policy grant.
- Write/delete/update actions produce action plans and require permission unless policy explicitly grants non-interactive write.
- All writes pass policy, threat scan, redaction, event emission, provider registry, and tombstone checks.
- `propose` creates inbox candidate only.
- Tool result returns structured ids, state, redacted record views, denial detail, and takes-effect metadata. It does not echo full secret-bearing content.
- `TodoTool` remains run-scoped and is not marketed as long-term memory.

- [ ] Write failing tool tests for every action.
- [ ] Write failing engine test that a model memory tool call mutates real local memory only after authorization.
- [ ] Write failing test that delete writes tombstone and blocks recall.
- [ ] Run targeted red tests:

```bash
cargo test -p jyowo-harness-tool --test memory_tool -- --nocapture
cargo test -p jyowo-harness-engine --test main_loop memory -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly_tools -- --nocapture
```

- [ ] Implement memory tool descriptors, model-argument validation, runtime context injection, action planning, and authorized execution.
- [ ] Wire memory capabilities into tool context.
- [ ] Update prompt-visible tool descriptors so the model sees exact actions and trust boundary.
- [ ] Run targeted verification with the same commands.
- [ ] Complete task-completion analysis.
- [ ] Run read-only subagent audit for Task 7.
- [ ] Fix audit findings and re-run targeted verification.
- [ ] Commit:

```bash
git add crates/jyowo-harness-tool crates/jyowo-harness-engine crates/jyowo-harness-sdk crates/jyowo-harness-contracts
git commit -m "feat(memory): add controlled memory tool"
```

## Task 8: Memdir Redesign And DREAMS Migration

**Purpose:** Convert file-backed memory into a bounded visible index and replace `DREAMS.md` with candidate inbox semantics.

**Files:**

- Modify: `crates/jyowo-harness-memory/src/memdir/mod.rs`
- Modify: `crates/jyowo-harness-memory/src/memdir/file.rs`
- Modify: `crates/jyowo-harness-memory/src/memdir/fence.rs`
- Modify: `crates/jyowo-harness-sdk/src/system_prompt.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory.rs`
- Create: `crates/jyowo-harness-memory/src/inbox.rs`
- Test: `crates/jyowo-harness-memory/tests/memdir.rs`
- Test: `crates/jyowo-harness-memory/tests/inbox.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly_memory.rs`

**Design requirements:**

- `MEMORY.md` and `USER.md` are bounded indexes.
- Topic files are stored under tenant-scoped `topics/`.
- Prompt rendering includes indexes only.
- Current-session full content access uses memory tool or hydrated references.
- Existing `DREAMS.md` is migrated to inbox candidates with source `Imported`.
- No unapproved inbox candidate enters prompt or recall.

- [ ] Write failing tests:
  - system prompt renders only bounded indexes.
  - topic file content is not included unless explicitly hydrated.
  - `DREAMS.md` content migrates into inbox candidate records.
  - unapproved candidate is not recalled.
  - approved candidate can be promoted to local memory.
- [ ] Run targeted red tests:

```bash
cargo test -p jyowo-harness-memory --test memdir -- --nocapture
cargo test -p jyowo-harness-memory --test inbox -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly_memory -- --nocapture
```

- [ ] Implement memdir index/topic layout.
- [ ] Remove runtime use of `DREAMS.md`.
- [ ] Implement migration and candidate inbox.
- [ ] Run targeted verification with the same commands.
- [ ] Complete task-completion analysis.
- [ ] Run read-only subagent audit for Task 8.
- [ ] Fix audit findings and re-run targeted verification.
- [ ] Commit:

```bash
git add crates/jyowo-harness-memory crates/jyowo-harness-sdk
git commit -m "refactor(memory): replace dreams with inbox"
```

## Task 9: Extraction And Consolidation Worker

**Purpose:** Implement delayed memory generation and consolidation as real runtime behavior, not a hook-only placeholder.

**Files:**

- Create: `crates/jyowo-harness-memory/src/extraction/mod.rs`
- Create: `crates/jyowo-harness-memory/src/extraction/job.rs`
- Create: `crates/jyowo-harness-memory/src/extraction/worker.rs`
- Create: `crates/jyowo-harness-memory/src/extraction/schema.rs`
- Modify: `crates/jyowo-harness-memory/src/inbox.rs`
- Modify: `crates/jyowo-harness-memory/src/lifecycle.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Test: `crates/jyowo-harness-memory/tests/extraction.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly_memory.rs`
- Test: `crates/jyowo-harness-engine/tests/memory_lifecycle.rs`

**Design requirements:**

- Extraction runs only after session is ended or idle long enough.
- Active sessions are not summarized.
- Short-lived sessions are skipped by policy.
- Threads using external context are skipped when policy says so.
- Low quota or unavailable extraction model skips generation with event/metric.
- Extractor creates candidates, not direct long-term records, unless policy explicitly allows direct user/private memory.
- Consolidation merges duplicates, demotes stale entries, expires low-confidence candidates, and writes trace/audit events.
- Extraction jobs are stored durably in SQLite with `job_id`, `tenant_id`, `session_id`, `run_id`, `evidence_hash`, `state`, `attempt_count`, `lease_owner`, `lease_expires_at`, `next_attempt_at`, `created_at`, and `updated_at`.
- Job idempotency key is `(tenant_id, session_id, run_id, evidence_hash, job_kind)`.
- Worker states are `queued`, `leased`, `completed`, `skipped`, `failed_retryable`, `failed_permanent`.
- Leases expire and are recoverable after process crash.
- Retry uses bounded exponential backoff and stops at a configured maximum attempt count.
- Model output must parse into a typed schema from `extraction/schema.rs`; unparsable output is a retryable failure until attempts are exhausted.
- The worker never writes long-term memory directly unless `MemoryPolicyEngine` returns an explicit direct-write allowance for user/private visibility.

- [ ] Write failing tests for idle gating, active-session skip, external-context skip, candidate creation, promotion, merge, demotion, audit events, durable job persistence, lease expiry recovery, idempotency, retry backoff, quota skip, unavailable model skip, and invalid model output.
- [ ] Run targeted red tests:

```bash
cargo test -p jyowo-harness-memory --test extraction -- --nocapture
cargo test -p jyowo-harness-engine --test memory_lifecycle -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly_memory -- --nocapture
```

- [ ] Implement extractor and consolidator interfaces.
- [ ] Implement durable extraction job queue and worker lease handling.
- [ ] Implement typed extraction model output parsing in `extraction/schema.rs`.
- [ ] Use real configured model provider path for extraction. If no model is configured, skip with typed event; do not fake extraction.
- [ ] Persist candidates and consolidation outcomes.
- [ ] Run targeted verification with the same commands.
- [ ] Complete task-completion analysis.
- [ ] Run read-only subagent audit for Task 9.
- [ ] Fix audit findings and re-run targeted verification.
- [ ] Commit:

```bash
git add crates/jyowo-harness-memory crates/jyowo-harness-sdk crates/jyowo-harness-engine
git commit -m "feat(memory): add extraction and consolidation"
```

## Task 10: Settings, IPC, And Thread Controls

**Purpose:** Add global and per-thread memory settings with Rust-owned validation.

**Files:**

- Create: `apps/desktop/src-tauri/src/commands/memory_settings.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory.rs`
- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Test: `apps/desktop/src-tauri/tests/commands/memory.rs`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly_memory.rs`

**Design requirements:**

- Global settings and thread settings are separate.
- Thread settings do not mutate global defaults.
- Settings persist in `.jyowo/runtime` using existing store patterns.
- Rust rejects invalid limits and unavailable states.
- Frontend Zod schemas match Rust contract exactly.

- [ ] Write failing IPC tests for global and thread settings.
- [ ] Write failing SDK tests for settings affecting recall/generation.
- [ ] Run targeted red tests:

```bash
cargo test -p jyowo-desktop-shell --test commands memory -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly_memory -- --nocapture
pnpm --dir apps/desktop test src/shared/tauri/commands.test.ts
```

- [ ] Implement settings commands and persistence.
- [ ] Wire settings into `MemoryPolicyEngine`.
- [ ] Update Zod schemas and command client.
- [ ] Run targeted verification with the same commands.
- [ ] Complete task-completion analysis.
- [ ] Run read-only subagent audit for Task 10.
- [ ] Fix audit findings and re-run targeted verification.
- [ ] Commit:

```bash
git add apps/desktop crates/jyowo-harness-sdk crates/jyowo-harness-contracts
git commit -m "feat(memory): add settings and thread controls"
```

## Task 11: Frontend Memory Product Surface

**Purpose:** Make memory visible and controllable without making frontend the policy authority.

**Files:**

- Modify: `apps/desktop/src/features/memory/MemoryBrowser.tsx`
- Create: `apps/desktop/src/features/memory/MemorySettings.tsx`
- Create: `apps/desktop/src/features/memory/MemoryInbox.tsx`
- Create: `apps/desktop/src/features/memory/MemoryRecallTracePanel.tsx`
- Modify: `apps/desktop/src/routes/memory.lazy.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.tsx`
- Modify: `apps/desktop/src/features/context/ContextPanel.tsx`
- Test: `apps/desktop/src/features/memory/MemoryBrowser.test.tsx`
- Test: `apps/desktop/src/features/conversation/Composer.test.tsx`
- Test: `apps/desktop/src/features/context/ContextPanel.test.tsx`

**Design requirements:**

- Memory route has tabs or segmented views: Items, Inbox, Recall Traces, Settings.
- Browser shows source, visibility, provider, confidence, expiry, deletion state, and last access.
- Inbox supports approve, reject, and merge.
- Recall trace panel shows per-turn memory use from trace metadata.
- Composer exposes thread memory mode and selected memory references.
- Context panel separates `Context Sources` from `Model Request Preview`.
- UI never marks a memory setting as successful until backend command returns success.

- [ ] Write failing frontend tests for settings, inbox candidate actions, recall trace rendering, and distinct preview panel.
- [ ] Run targeted red tests:

```bash
pnpm --dir apps/desktop test src/features/memory/MemoryBrowser.test.tsx
pnpm --dir apps/desktop test src/features/conversation/Composer.test.tsx
pnpm --dir apps/desktop test src/features/context/ContextPanel.test.tsx
```

- [ ] Implement UI using existing shared UI components and TanStack Query patterns.
- [ ] Keep cards at existing radius and avoid nested cards.
- [ ] Cover loading, empty, error, and ready states for every new panel.
- [ ] Run targeted verification with the same commands.
- [ ] Complete task-completion analysis.
- [ ] Run read-only subagent audit for Task 11.
- [ ] Fix audit findings and re-run targeted verification.
- [ ] Commit:

```bash
git add apps/desktop/src/features apps/desktop/src/routes
git commit -m "feat(memory): add memory management UI"
```

## Task 12: Team And Subagent Memory Integration

**Purpose:** Unify team/shared memory and subagent memory scope with the new provider registry and policy engine.

**Files:**

- Modify: `crates/jyowo-harness-team/src/lib.rs`
- Modify: `crates/jyowo-harness-subagent/src/lib.rs`
- Modify: `crates/jyowo-harness-engine/src/engine.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/team_runtime.rs`
- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Test: `crates/jyowo-harness-team/tests/shared_memory.rs`
- Test: `crates/jyowo-harness-subagent/tests/default_runner.rs`
- Test: `crates/jyowo-harness-engine/tests/subagent_tool_feature.rs`
- Test: `crates/jyowo-harness-sdk/tests/agents_team.rs`

**Design requirements:**

- Team shared memory registers as provider id `team`.
- Subagent memory scope resolves to typed memory grants.
- `empty` scope disables memory recall and tool access for that child.
- `subset` scope uses selector-resolved memory ids and fails closed if resolver is missing.
- Child writes default to candidate inbox.
- Coordinator or role-gated policy is required to promote child/team memory.
- All child-derived records include source evidence.

- [ ] Write failing tests for inherit, empty, subset, read-only, read-write, and candidate-only scopes.
- [ ] Write failing tests for role-gated team promotion.
- [ ] Run targeted red tests:

```bash
cargo test -p jyowo-harness-team --test shared_memory -- --nocapture
cargo test -p jyowo-harness-subagent --test default_runner -- --nocapture
cargo test -p jyowo-harness-engine --test subagent_tool_feature -- --nocapture
cargo test -p jyowo-harness-sdk --test agents_team -- --nocapture
```

- [ ] Integrate team shared memory with provider registry.
- [ ] Refactor subagent memory scope into typed grants.
- [ ] Wire child writes into policy and inbox.
- [ ] Run targeted verification with the same commands.
- [ ] Complete task-completion analysis.
- [ ] Run read-only subagent audit for Task 12.
- [ ] Fix audit findings and re-run targeted verification.
- [ ] Commit:

```bash
git add crates/jyowo-harness-team crates/jyowo-harness-subagent crates/jyowo-harness-engine crates/jyowo-harness-sdk crates/jyowo-harness-contracts
git commit -m "feat(memory): unify team and subagent memory"
```

## Task 13: Security, Redaction, And Export Hardening

**Purpose:** Close memory security gaps across write, recall, extraction, trace, preview, export, and deletion.

**Files:**

- Modify: `crates/jyowo-harness-memory/src/scanner.rs`
- Modify: `crates/jyowo-harness-memory/src/external.rs`
- Modify: `crates/jyowo-harness-memory/src/policy.rs`
- Modify: `crates/jyowo-harness-memory/src/trace.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory.rs`
- Modify: `apps/desktop/src-tauri/src/commands/memory.rs`
- Test: `crates/jyowo-harness-memory/tests/scanner.rs`
- Test: `crates/jyowo-harness-memory/tests/store_audit.rs`
- Test: `crates/jyowo-harness-sdk/tests/event_stream_redaction.rs`
- Test: `apps/desktop/src-tauri/tests/commands/memory.rs`

**Design requirements:**

- Secret/prompt-injection scanner runs on write, recall, extraction candidate, promotion, preview, trace, and export.
- Raw content never appears in trace, audit event, metric attribute, frontend error, or test snapshot.
- Delete writes tombstone and audit event.
- Export requires explicit request and includes hashes/metadata; raw content export must be user-initiated.
- Memory tool cannot read filesystem paths.
- Threat-blocked memory fails closed for write and is dropped for recall with trace reason.

- [ ] Write failing tests for redaction across write, recall, preview, trace, export, and error paths.
- [ ] Write failing tests for tombstone preventing regeneration from same evidence hash.
- [ ] Run targeted red tests:

```bash
cargo test -p jyowo-harness-memory --test scanner -- --nocapture
cargo test -p jyowo-harness-memory --test store_audit -- --nocapture
cargo test -p jyowo-harness-sdk event_stream_redaction -- --nocapture
cargo test -p jyowo-desktop-shell --test commands memory -- --nocapture
```

- [ ] Implement security hardening.
- [ ] Verify every new event/trace/metric avoids raw content.
- [ ] Run targeted verification with the same commands.
- [ ] Complete task-completion analysis.
- [ ] Run read-only subagent audit for Task 13.
- [ ] Fix audit findings and re-run targeted verification.
- [ ] Commit:

```bash
git add crates apps/desktop
git commit -m "fix(memory): harden redaction and export"
```

## Task 14: Documentation, Architecture Gates, And Cleanup

**Purpose:** Remove obsolete design text and make docs/gates enforce the new architecture.

**Files:**

- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-product-ux.md`
- Modify: `docs/testing/testing-strategy.md`
- Create: `scripts/memory-architecture-policy.mjs`
- Create: `scripts/memory-architecture-policy.test.mjs`
- Modify: `package.json` to run `scripts/memory-architecture-policy.mjs` from the existing `check:backend-docs` script. Do not add a new final gate command name.
- Test: docs gates and architecture gates.

**Design requirements:**

- Docs describe implemented behavior only.
- Docs say memory is auxiliary context and lower priority than runtime policy and workspace instructions.
- Docs describe provider registry, local provider, inbox, tool, trace, settings, subagent/team integration, and redaction rules.
- Architecture gates prevent reintroducing:
  - label-only memory references,
  - production use of in-memory provider,
  - single external provider runtime slot,
  - raw memory content in events/traces/metrics,
  - `DREAMS.md` runtime semantics.
- `scripts/memory-architecture-policy.mjs` must fail on these concrete patterns outside explicit test-only allowlists:
  - `memory-external-slot` or `external-slot` in `Cargo.toml`, `crates`, or `apps`;
  - `#[cfg(feature = "memory-external-slot")]` or `#[cfg(feature = "external-slot")]`;
  - `external: RwLock<Option<Arc<dyn MemoryProvider>>>`;
  - `with_external_memory_provider`;
  - `InMemoryMemoryProvider::new(` in production runtime, SDK, engine, context, tool, or Tauri command paths;
  - `"- memory: {} ({})"` and any `ConversationContextReference::Memory` rendering path that formats only `id` and `label` instead of hydrated redacted content;
  - `DREAMS.md` in prompt rendering, recall, provider registry, context assembly, memory tool, settings, export defaults, frontend UI, or any runtime semantic path after migration;
  - `MemoryRecallTrace`, `MemoryProviderTrace`, `MemoryInjectedTrace`, or `MemoryDroppedTrace` fields named `content`, `raw_content`, `prompt`, or `message_text`; `content_hash`, `redacted_content`, and redaction counts are allowed.
- The only allowed `DREAMS.md` references are one-shot migration code, migration tests, and historical documentation explaining the migration. The gate must require those references to live under explicit migration modules/tests and must fail if they are used by prompt rendering, recall, or normal runtime reads.
- The policy test file must include one failing fixture for each forbidden pattern and one allowed fixture for each documented exception, including the migration-only `DREAMS.md` allowlist.

- [ ] Write or update docs-gate tests for forbidden patterns in `scripts/memory-architecture-policy.test.mjs`.
- [ ] Run targeted red gate if a new rule is added:

```bash
pnpm check:docs
pnpm check:agent-docs
pnpm check:backend-docs
pnpm check:frontend-docs
pnpm check:test-architecture
```

Expected before implementation: new gate fails if it asserts behavior not yet reflected in docs/code.

- [ ] Update docs and gate scripts.
- [ ] Remove obsolete docs wording and plan-derived temporary notes from normative docs.
- [ ] Run targeted verification with the same commands.
- [ ] Complete task-completion analysis.
- [ ] Run read-only subagent audit for Task 14.
- [ ] Fix audit findings and re-run targeted verification.
- [ ] Commit:

```bash
git add docs package.json scripts
git commit -m "docs(memory): document memory platform architecture"
```

## Final Integration Gates

Run these after Task 14 and after all audit findings are fixed:

```bash
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
cargo check -p jyowo-harness-memory --no-default-features --features provider-registry
cargo test -p jyowo-harness-memory --no-default-features --features provider-registry --test provider_registry -- --nocapture
cargo check -p jyowo-harness-context --no-default-features --features recall-memory
cargo check -p jyowo-harness-engine --no-default-features --features recall-memory
cargo check -p jyowo-harness-sdk --no-default-features --features memory-provider-registry,stream-permission,rule-engine-permission,integrity,jsonl-store,sqlite-store,blob-file,local-sandbox,mcp-http,mcp-stdio
pnpm check:docs
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:desktop
pnpm check:rust
pnpm audit:tests
pnpm check:test-architecture
pnpm check:agent-orchestration-no-fakes
pnpm check:agent-supervisor-sidecar
pnpm check:quick
pnpm check:frontend:fast
pnpm check:rust:fast
pnpm check
```

Expected: every command exits 0.

If a command fails:

1. Keep the implementation branch.
2. Diagnose one failing command at a time.
3. Add or update tests that prove the fix.
4. Fix the issue.
5. Re-run the failed command.
6. Re-run the relevant broader gate.
7. Run a final read-only subagent audit.

Final audit prompt:

```text
Audit the completed implementation of docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md.

Use read-only inspection only.
Verify every task is implemented, every obsolete path is removed or test-only, every required gate passed, and no fake memory behavior or compatibility shim remains.

Focus on:
- provider registry replacing single external slot,
- local SQLite provider as production default,
- MemoryTool authorization,
- memory reference hydration,
- recall traces,
- inbox replacing DREAMS runtime semantics,
- settings and thread controls,
- extraction/consolidation,
- team/subagent memory grants,
- security/redaction/export behavior,
- frontend UI matching backend authority.

Return PASS or FAIL.
If FAIL, include exact file paths and line references.
```

## Final Merge To Main

After all gates and final audit pass:

```bash
cd /Users/goya/Repo/Git/Jyowo
git switch main
git status --short --branch
test -z "$(git status --porcelain)" || { echo "original main checkout has uncommitted changes; stop before merge"; exit 1; }
git merge --ff-only goya/memory-platform
git status --short --branch
```

If the original checkout has uncommitted changes, stop and report that merge requires a clean `main` checkout. Do not stash, revert, delete, or commit unrelated user changes. If fast-forward merge is not possible, stop and inspect. Do not force merge.

Run a final smoke gate on `main`:

```bash
pnpm check:quick
pnpm check:rust:fast
pnpm check:frontend:fast
```

Expected: every command exits 0.

## Completion Criteria

The implementation is complete only when all of these are true:

- The plan file exists on `main`.
- Implementation happened in an isolated worktree.
- Every task has task-start analysis, task-completion analysis, read-only subagent audit, and targeted verification. Every task that changes files has a commit.
- `LocalMemoryProvider` is the production default memory provider when memory is enabled.
- Provider registry supports local, memdir, team, and plugin providers.
- In-memory provider is test-only or removed from production selection.
- Memory references hydrate real content or fail; label-only memory references are gone.
- MemoryTool exists and all writes are policy-authorized.
- Recall trace is visible through backend IPC and frontend UI.
- `DREAMS.md` has no runtime semantics; candidate inbox owns generated memory candidates.
- Settings distinguish use/generate and global/thread scope.
- Extraction and consolidation create candidates and respect idle/external-context/quota policy.
- Team and subagent memory scopes use typed grants and policy.
- Redaction covers write, recall, extraction, preview, trace, export, journal, metrics, and frontend error paths.
- Context UI distinguishes sources from final redacted model request preview.
- All final gates exit 0.
