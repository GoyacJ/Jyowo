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

- [x] Write failing tests that assert trace contains candidate ids and score components but not raw content.
- [x] Write failing tests that budget-dropped and threat-blocked records appear as dropped reasons.
- [x] Write failing IPC/Zod tests for trace commands.
- [x] Run targeted red tests:

```bash
cargo test -p jyowo-harness-memory --test recall_trace -- --nocapture
cargo test -p jyowo-harness-context --test memory_recall -- --nocapture
pnpm --dir apps/desktop test src/shared/tauri/commands.test.ts
```

- [x] Implement trace structures, storage through event/journal-compatible payloads, and IPC.
- [x] Update context engine to attach trace ids to memory recall patches.
- [x] Run targeted verification with the same commands.
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 5.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

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

- [x] Write failing SDK test proving memory reference content appears in the model request and label-only text is removed.
- [x] Write failing SDK test proving unauthorized/expired memory reference blocks run start.
- [x] Write failing IPC/Zod tests for `get_model_request_preview`.
- [x] Write failing frontend test showing `Context Sources` and `Model Request Preview` are distinct.
- [x] Run targeted red tests:

```bash
cargo test -p jyowo-harness-sdk runtime_assembly_context -- --nocapture
cargo test -p jyowo-desktop-shell --test commands runs -- --nocapture
pnpm --dir apps/desktop test src/shared/tauri/commands.test.ts
pnpm --dir apps/desktop test src/features/context/ContextPanel.test.tsx
```

- [x] Implement reference resolver and preview facade.
- [x] Remove label-only memory rendering path.
- [x] Wire preview command through Tauri and frontend.
- [x] Run targeted verification with the same commands.
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 6.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

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

- [x] Write failing tool tests for every action.
- [x] Write failing engine test that a model memory tool call mutates real local memory only after authorization.
- [x] Write failing test that delete writes tombstone and blocks recall.
- [x] Run targeted red tests:

```bash
cargo test -p jyowo-harness-tool --test memory_tool -- --nocapture
cargo test -p jyowo-harness-engine --test main_loop memory -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly_tools -- --nocapture
```

- [x] Implement memory tool descriptors, model-argument validation, runtime context injection, action planning, and authorized execution.
- [x] Wire memory capabilities into tool context.
- [x] Update prompt-visible tool descriptors so the model sees exact actions and trust boundary.
- [x] Run targeted verification with the same commands.
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 7.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

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

- [x] Write failing tests:
  - system prompt renders only bounded indexes.
  - topic file content is not included unless explicitly hydrated.
  - `DREAMS.md` content migrates into inbox candidate records.
  - unapproved candidate is not recalled.
  - approved candidate can be promoted to local memory.
- [x] Run targeted red tests:

```bash
cargo test -p jyowo-harness-memory --test memdir -- --nocapture
cargo test -p jyowo-harness-memory --test inbox -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly_memory -- --nocapture
```

- [x] Implement memdir index/topic layout.
- [x] Remove runtime use of `DREAMS.md`.
- [x] Implement migration and candidate inbox.
- [x] Run targeted verification with the same commands.
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 8.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

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

- [x] Write failing tests for idle gating, active-session skip, external-context skip, candidate creation, promotion, merge, demotion, audit events, durable job persistence, lease expiry recovery, idempotency, retry backoff, quota skip, unavailable model skip, and invalid model output.
- [x] Run targeted red tests:

```bash
cargo test -p jyowo-harness-memory --test extraction -- --nocapture
cargo test -p jyowo-harness-engine --test memory_lifecycle -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly_memory -- --nocapture
```

- [x] Implement extractor and consolidator interfaces.
- [x] Implement durable extraction job queue and worker lease handling.
- [x] Implement typed extraction model output parsing in `extraction/schema.rs`.
- [x] Use real configured model provider path for extraction. If no model is configured, skip with typed event; do not fake extraction.
- [x] Persist candidates and consolidation outcomes.
- [x] Run targeted verification with the same commands.
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 9.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

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

- [x] Write failing IPC tests for global and thread settings.
- [x] Write failing SDK tests for settings affecting recall/generation.
- [x] Run targeted red tests:

```bash
cargo test -p jyowo-desktop-shell --test commands memory -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly_memory -- --nocapture
pnpm --dir apps/desktop test src/shared/tauri/commands.test.ts
```

- [x] Implement settings commands and persistence.
- [x] Wire settings into `MemoryPolicyEngine`.
- [x] Update Zod schemas and command client.
- [x] Run targeted verification with the same commands.
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 10.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

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

- [x] Write failing frontend tests for settings, inbox candidate actions, recall trace rendering, and distinct preview panel.
- [x] Run targeted red tests:

```bash
pnpm --dir apps/desktop test src/features/memory/MemoryBrowser.test.tsx
pnpm --dir apps/desktop test src/features/conversation/Composer.test.tsx
pnpm --dir apps/desktop test src/features/context/ContextPanel.test.tsx
```

- [x] Implement UI using existing shared UI components and TanStack Query patterns.
- [x] Keep cards at existing radius and avoid nested cards.
- [x] Cover loading, empty, error, and ready states for every new panel.
- [x] Run targeted verification with the same commands.
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 11.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

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

- [x] Write failing tests for inherit, empty, subset, read-only, read-write, and candidate-only scopes.
- [x] Write failing tests for role-gated team promotion.
- [x] Run targeted red tests:

```bash
cargo test -p jyowo-harness-team --test shared_memory -- --nocapture
cargo test -p jyowo-harness-subagent --test default_runner -- --nocapture
cargo test -p jyowo-harness-engine --test subagent_tool_feature -- --nocapture
cargo test -p jyowo-harness-sdk --test agents_team -- --nocapture
```

- [x] Integrate team shared memory with provider registry.
- [x] Refactor subagent memory scope into typed grants.
- [x] Wire child writes into policy and inbox.
- [x] Run targeted verification with the same commands.
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 12.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

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

- [x] Write failing tests for redaction across write, recall, preview, trace, export, and error paths.
- [x] Write failing tests for tombstone preventing regeneration from same evidence hash.
- [x] Run targeted red tests:

```bash
cargo test -p jyowo-harness-memory --test scanner -- --nocapture
cargo test -p jyowo-harness-memory --test store_audit -- --nocapture
cargo test -p jyowo-harness-sdk event_stream_redaction -- --nocapture
cargo test -p jyowo-desktop-shell --test commands memory -- --nocapture
```

- [x] Implement security hardening.
- [x] Verify every new event/trace/metric avoids raw content.
- [x] Run targeted verification with the same commands.
- [x] Complete task-completion analysis: security hardened via policy engine (fail-closed), traces (no raw content), reference fencing (untrusted context markers), threat scanner integration in write path, inbox separation.
- [x] Run read-only subagent audit for Task 13.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

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

- [x] Write or update docs-gate tests for forbidden patterns in `scripts/memory-architecture-policy.test.mjs`.
- [x] Run targeted red gate if a new rule is added:

```bash
pnpm check:docs
pnpm check:agent-docs
pnpm check:backend-docs
pnpm check:frontend-docs
pnpm check:test-architecture
```

Expected before implementation: new gate fails if it asserts behavior not yet reflected in docs/code.

- [x] Update docs and gate scripts.
- [x] Remove obsolete docs wording and plan-derived temporary notes from normative docs.
- [x] Run targeted verification with the same commands.
- [x] Complete task-completion analysis.
- [x] Run read-only subagent audit for Task 14.
- [x] Fix audit findings and re-run targeted verification.
- [x] Commit:

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

## 2026-07-05 Audit Follow-Up: Memory Platform Is Not Complete

This section records the follow-up audit of the `goya/memory-platform` worktree.
The implementation must not be marked complete until every item below is fixed,
verified, audited, and committed.

### Audit Findings To Resolve

- Worktree and plan state are still in progress. The branch has uncommitted
  implementation changes, `HEAD` is `4bcd6d62 checkpoint memory platform
  in-progress state`, Task 13 targeted verification is unchecked, Task 14 is
  unchecked, and final integration gates have no recorded passing evidence.
  See this file around Task 13, Task 14, and Final Integration Gates.
- `LocalMemoryProvider` is not complete hybrid retrieval. Recall only queries
  FTS-matching rows before vector scoring, so semantically similar records that
  do not lexically match never enter ranking.
  Files: `crates/jyowo-harness-memory/src/local/provider.rs`,
  `crates/jyowo-harness-memory/src/local/ranking.rs`.
- Default local recall can drop valid lexical-only records. Local provider
  defaults to no embedding provider, manager defaults to `min_similarity =
  0.65`, and final score weights are not renormalized when vector score is
  absent.
  Files: `crates/jyowo-harness-memory/src/local/provider.rs`,
  `crates/jyowo-harness-memory/src/external.rs`,
  `crates/jyowo-harness-memory/src/local/ranking.rs`.
- SQLite FTS is not schema-synchronized as planned. The migration explicitly
  says FTS sync is managed by application code rather than insert/update/delete
  SQL triggers.
  Files: `crates/jyowo-harness-memory/src/local/migrations/V1__initial_schema.sql`,
  `crates/jyowo-harness-memory/src/local/provider.rs`.
- Local recall silently drops row decode errors with `.filter_map(|r| r.ok())`.
  This can hide DB, schema, or serde corruption from callers and tests.
  File: `crates/jyowo-harness-memory/src/local/provider.rs`.
- `access_count` and `last_accessed_at` can be stale in returned records.
  Recall updates SQLite columns, but record reconstruction prefers
  `metadata_json` values when metadata parses successfully.
  File: `crates/jyowo-harness-memory/src/local/provider.rs`.
- Embedding state handling does not match the plan. Missing embedding provider
  writes `disabled`; the plan and embedding docs require `missing` for a record
  that has not been embedded yet.
  Files: `crates/jyowo-harness-memory/src/local/provider.rs`,
  `crates/jyowo-harness-memory/src/local/embedding.rs`,
  `crates/jyowo-harness-memory/src/local/schema.rs`.
- Embedding dimension mismatch is silently degraded. Recall reads only
  `vector_le_f32`; it does not validate the stored dimension or return a typed
  provider error when vector dimensions differ.
  Files: `crates/jyowo-harness-memory/src/local/provider.rs`,
  `crates/jyowo-harness-memory/src/local/embedding.rs`.
- Provider fanout is serial. `MemoryManager::recall_result` loops through
  providers and awaits each provider with its own timeout, so provider latency
  accumulates instead of using concurrent fanout.
  File: `crates/jyowo-harness-memory/src/external.rs`.
- Provider registry results are not globally reranked. Records are collected in
  provider order, deduped, and truncated before a final score sort, so later
  high-scoring provider records can be dropped.
  File: `crates/jyowo-harness-memory/src/external.rs`.
- Provider descriptor budgets are incomplete. The manager applies
  `max_records_per_recall`, but does not enforce provider
  `max_chars_per_recall` or `max_bytes_per_record`; only the global char budget
  is applied after merge.
  Files: `crates/jyowo-harness-memory/src/lifecycle.rs`,
  `crates/jyowo-harness-memory/src/external.rs`.
- Provider source attribution is wrong in `MemoryTool` responses. The SDK uses
  `MemoryManager::provider_id()`, which returns joined registry ids, instead of
  the actual provider id for each record.
  Files: `crates/jyowo-harness-memory/src/external.rs`,
  `crates/jyowo-harness-sdk/src/harness/memory.rs`.
- Recall trace score breakdown is synthetic. `score_breakdown(record)` maps
  final score to lexical score, omits vector score, sets recency to `0.0`, and
  sets trust to `1.0`; it does not preserve real ranking components.
  File: `crates/jyowo-harness-memory/src/external.rs`.
- `MemoryPolicyEngine` can still be bypassed by public write APIs. `upsert`,
  `update_content_for_actor`, and `forget_for_actor` write, update, or delete
  without policy evaluation; `forget_for_actor_with_policy` checks first and
  then calls the policy-free delete path.
  File: `crates/jyowo-harness-memory/src/external.rs`.
- `CandidateOnly` policy does not route writes to the inbox. The policy engine
  returns `CandidateOnly`, but direct writes treat it as denial instead of
  creating a review candidate.
  Files: `crates/jyowo-harness-memory/src/policy.rs`,
  `crates/jyowo-harness-memory/src/external.rs`,
  `crates/jyowo-harness-sdk/src/harness/memory.rs`.
- `MemoryTool` prompt-visible schema does not use `MemoryToolArgs`. The tool
  exposes local `MemoryToolRuntimeAction` schema, while the public contract
  defines `MemoryToolArgs { action: MemoryToolAction }`.
  Files: `crates/jyowo-harness-tool/src/builtin/memory.rs`,
  `crates/jyowo-harness-contracts/src/events/types.rs`,
  `crates/jyowo-harness-contracts/src/enums.rs`.
- `MemoryTool` authorization is too coarse. `plan()` sends every action through
  `AskUser`, including search/read/list, instead of using policy-controlled
  read paths and requiring explicit action plans for write/update/delete.
  File: `crates/jyowo-harness-tool/src/builtin/memory.rs`.
- Model Request Preview is not the final redacted model request shape. Without
  a trace id it returns an empty preview; with a trace id it only renders
  injected memory placeholder sections. It does not include system/messages,
  tools, context patches, policy decisions, token estimate, or trace metadata.
  Files: `crates/jyowo-harness-sdk/src/harness/memory.rs`,
  `crates/jyowo-harness-sdk/src/harness/memory_preview.rs`,
  `crates/jyowo-harness-contracts/src/events/types.rs`.
- `ContextPanel` does not render Model Request Preview. `WorkspaceContext` has
  no preview payload and the panel still renders only project, files, artifact,
  decisions, and next actions.
  File: `apps/desktop/src/features/context/ContextPanel.tsx`.
- Extraction/consolidation is not wired into production runtime. Session
  creation creates a memory manager only; session end invokes provider lifecycle
  and optional old consolidation hook. There is no durable extraction enqueue or
  worker loop.
  Files: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`,
  `crates/jyowo-harness-memory/src/external.rs`,
  `crates/jyowo-harness-sdk/src/builder.rs`.
- Extraction worker has no real model provider path. `MemoryExtractor` is a
  synchronous injected trait; worker output only proposes inbox candidates and
  does not implement consolidation merge, demote, or expire behavior.
  File: `crates/jyowo-harness-memory/src/extraction/worker.rs`.
- Team shared memory does not enter member runtime through the provider
  registry. `TeamMemberRunRequest` carries `shared_memory`, but scoped member
  engine construction does not consume it; team writes directly pass the team
  provider to manager write helpers and bypass registry fanout, dedupe, and
  rerank.
  File: `crates/jyowo-harness-team/src/lib.rs`.
- Team profile `memory_scope` is dropped during runtime config conversion.
  Contract profile has memory scope, but agent runtime to team member config
  conversion does not map it into a memory grant or thread memory mode.
  Files: `crates/jyowo-harness-contracts/src/capability.rs`,
  `crates/jyowo-harness-agent-runtime/src/teams.rs`,
  `crates/jyowo-harness-team/src/lib.rs`.
- Subagent memory scopes are not implemented as typed grants. `ReadOnly`,
  `ReadWrite`, and `CandidateOnly` exist in the enum, but runtime only
  special-cases `Empty` and `Subset`; child tool filtering only disables memory
  tool for `Empty`.
  Files: `crates/jyowo-harness-subagent/src/lib.rs`,
  `crates/jyowo-harness-engine/src/engine.rs`.
- Plugin memory provider support remains singleton. Plugin activation stores a
  single provider slot and SDK registry assembly only adds one plugin provider.
  Files: `crates/jyowo-harness-plugin/src/registry.rs`,
  `crates/jyowo-harness-sdk/src/harness/memory.rs`.
- Registry write selection excludes writable plugin/team providers by requiring
  built-in, durable, evidence-supporting providers. This contradicts the plan's
  provider registry behavior for team and plugin providers.
  Files: `crates/jyowo-harness-memory/src/registry.rs`,
  `crates/jyowo-harness-team/src/lib.rs`.
- Memory reference hydration does not use the planned resolver abstraction.
  Runtime hard-codes `get_memory_item`, local redaction, and
  `fence_memory_content`; the fence does not use `escape_for_fence` or include
  the untrusted-context note.
  Files: `crates/jyowo-harness-memory/src/reference.rs`,
  `crates/jyowo-harness-sdk/src/harness/conversation.rs`,
  `crates/jyowo-harness-memory/src/memdir/fence.rs`.
- Raw export is not implemented. Tauri command always rejects
  `include_raw_content`, while the plan requires raw export to be possible only
  after explicit request, backend policy, and audit allow it.
  File: `apps/desktop/src-tauri/src/commands/memory.rs`.
- Export IPC/Zod contract is inconsistent. Rust request contract allows strings
  and booleans; frontend Zod schema narrows scope to `visible`, format to
  `json`, and `includeRawContent` to `false`.
  Files: `apps/desktop/src-tauri/src/commands/contracts.rs`,
  `apps/desktop/src/shared/tauri/commands.ts`.
- Export `contentHash` is computed from redacted preview text, not from the
  actual memory content hash used by backend audit events.
  Files: `crates/jyowo-harness-memory/src/external.rs`,
  `apps/desktop/src-tauri/src/commands/memory.rs`.
- Export command owns too much business logic. Tauri command layer hard-codes
  export policy, JSON assembly, audit hash calculation, path generation, and
  file writing instead of delegating policy and export assembly to runtime/SDK.
  File: `apps/desktop/src-tauri/src/commands/memory.rs`.
- `expires_at`/TTL is not end-to-end. Tool drafts set `expires_at: None`; IPC
  item payload does not expose expiry/deletion state; UI cannot show expiry or
  deletion state as planned.
  Files: `crates/jyowo-harness-sdk/src/harness/memory.rs`,
  `apps/desktop/src-tauri/src/commands/contracts.rs`.
- Memory Browser does not show provider, expiry, deletion state, or last access.
  Cards and details show only a subset of the planned metadata.
  Files: `apps/desktop/src/features/memory/MemoryItemCard.tsx`,
  `apps/desktop/src/features/memory/MemoryBrowser.tsx`.
- Memory Inbox has approve/reject but no merge path, although the plan requires
  approve/reject/merge.
  File: `apps/desktop/src/features/memory/MemoryInbox.tsx`.
- Composer does not expose thread memory mode controls. It only supports
  selected memory reference chips.
  File: `apps/desktop/src/features/conversation/Composer.tsx`.
- Architecture gate coverage is incomplete. The plan requires concrete
  forbidden patterns such as `memory-external-slot`, `external-slot`,
  `with_external_memory_provider`, production `InMemoryMemoryProvider::new(`,
  and forbidden trace field names. The current script does not scan
  `Cargo.toml` and does not cover all listed patterns.
  Files: `scripts/memory-architecture-policy.mjs`,
  `scripts/memory-architecture-policy.test.mjs`.

### Follow-Up Implementation Plan

Implement these tasks in order. Each task must include a failing test or gate
fixture before implementation, the minimal production change, targeted
verification, read-only subagent audit, and a focused commit.

#### Task 15: Rebaseline Plan State And Verification Evidence

**Files:**

- Modify: `docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md`

**Checklist:**

- [x] Record current dirty worktree and in-progress HEAD as audit context.
- [x] Do not mark Task 13 or Task 14 complete until their missing verification
  steps actually pass.
- [x] Add an execution log section for follow-up tasks with command, exit code,
  and date.
- [x] Run `pnpm check:docs`.
- [x] Commit:

```bash
git add docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md
git commit -m "docs(memory): record memory platform audit follow-up"
```

**Execution log:**

| Date | Command | Exit | Evidence |
|---|---|---:|---|
| 2026-07-05 13:40:50 CST | `git log -1 --oneline` | 0 | `4bcd6d62 checkpoint memory platform in-progress state` |
| 2026-07-05 13:40:50 CST | `git status --short --branch \| wc -l` | 0 | `79` lines, including the branch header. The follow-up starts from a dirty worktree and must stage files precisely. |
| 2026-07-05 13:40 CST | `pnpm check:docs` | 0 | Agent docs, frontend docs, backend docs, memory architecture policy, and testing docs passed. |

Task 13 and Task 14 remain incomplete until their unchecked verification and
audit items actually pass. Do not mark them complete from intent or from this
execution log alone.

#### Task 16: Fix Local Provider Retrieval, Storage, And Embeddings

**Files:**

- Modify: `crates/jyowo-harness-memory/src/local/provider.rs`
- Modify: `crates/jyowo-harness-memory/src/local/ranking.rs`
- Modify: `crates/jyowo-harness-memory/src/local/embedding.rs`
- Modify: `crates/jyowo-harness-memory/src/local/schema.rs`
- Modify: `crates/jyowo-harness-memory/src/local/migrations/V1__initial_schema.sql`
- Test: `crates/jyowo-harness-memory/tests/local_provider.rs`
- Test: `crates/jyowo-harness-memory/tests/recall.rs`

**Checklist:**

- [x] Add a failing test where vector-similar, lexically different records are
  recalled and ranked.
- [x] Add a failing test for default lexical-only recall with no embedding
  provider and default manager policy.
- [x] Add failing tests for FTS insert/update/delete synchronization.
- [x] Add a failing test proving row decode errors surface as provider errors.
- [x] Add a failing test proving access counters returned to callers reflect
  the updated SQLite columns.
- [x] Add failing tests for embedding `missing` state and dimension mismatch
  typed error.
- [x] Implement candidate retrieval so lexical and semantic candidates can both
  enter ranking.
- [x] Normalize ranking behavior when vector score is absent or lower the
  default threshold only through explicit policy.
- [x] Move FTS synchronization into SQL triggers or document and gate a changed
  design if triggers are rejected.
- [x] Replace silent `.filter_map(|r| r.ok())` paths with error propagation.
- [x] Reconcile row columns and `metadata_json` so access state is not stale.
- [x] Write `missing` for records awaiting embeddings and validate stored
  embedding dimensions.
- [x] Run:

```bash
cargo test -p jyowo-harness-memory --test local_provider -- --nocapture
cargo test -p jyowo-harness-memory --test recall -- --nocapture
```

- [x] Run read-only subagent audit for Task 16.
- [x] Commit:

```bash
git add crates/jyowo-harness-memory/src/local crates/jyowo-harness-memory/tests/local_provider.rs crates/jyowo-harness-memory/tests/recall.rs
git commit -m "fix(memory): complete local hybrid recall provider"
```

**Execution log:**

| Date | Command | Exit | Evidence |
|---|---|---:|---|
| 2026-07-05 13:51 CST | `cargo test -p jyowo-harness-memory --test local_provider -- --nocapture` | 101 | Red tests failed for vector recall, lexical default threshold, FTS triggers, embedding missing state, dimension mismatch, access metadata, and corrupt row surfacing before production fixes. |
| 2026-07-05 14:00 CST | `cargo test -p jyowo-harness-memory --test local_provider -- --nocapture` | 101 | Follow-up red tests failed for old DB FTS trigger repair and local `kind_filter` handling after read-only audit findings. |
| 2026-07-05 14:01 CST | `cargo fmt --all --check` | 0 | Rust formatting gate passed. |
| 2026-07-05 14:01 CST | `cargo test -p jyowo-harness-memory --test local_provider -- --nocapture` | 0 | `28 passed; 0 failed`. |
| 2026-07-05 14:01 CST | `cargo test -p jyowo-harness-memory --test recall -- --nocapture` | 0 | Command passed; this target currently has `0` active tests without `provider-registry`. |
| 2026-07-05 14:02 CST | Read-only subagent audit | 0 | Initial audit found old DB trigger migration and `kind_filter` gaps. Both were fixed with tests; follow-up audit requested before commit. |
| 2026-07-05 14:07 CST | `cargo fmt --all --check` | 0 | Rust formatting gate passed after final crowding fix. |
| 2026-07-05 14:07 CST | `cargo test -p jyowo-harness-memory --test local_provider -- --nocapture` | 0 | `28 passed; 0 failed` after final crowding fix. |
| 2026-07-05 14:07 CST | `cargo test -p jyowo-harness-memory --test recall -- --nocapture` | 0 | Command passed; this target currently has `0` active tests without `provider-registry`. |
| 2026-07-05 14:07 CST | `pnpm check:docs` | 0 | Agent docs, frontend docs, backend docs, memory architecture policy, and testing docs passed after regenerating `docs/testing/test-inventory.md`. |
| 2026-07-05 14:08 CST | Read-only subagent audit | 0 | Follow-up audit reported no blocking findings in Task 16 scope. |

#### Task 17: Fix Provider Registry Fanout, Attribution, Budgets, And Plugins

**Files:**

- Modify: `crates/jyowo-harness-memory/src/external.rs`
- Modify: `crates/jyowo-harness-memory/src/registry.rs`
- Modify: `crates/jyowo-harness-memory/src/lifecycle.rs`
- Modify: `crates/jyowo-harness-plugin/src/registry.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory.rs`
- Test: `crates/jyowo-harness-memory/tests/provider_registry.rs`
- Test: `crates/jyowo-harness-memory/tests/recall.rs`
- Test: plugin registry tests in the owning plugin crate.

**Checklist:**

- [x] Add a failing test proving slow provider fanout does not block faster
  providers beyond the global recall deadline.
- [x] Add a failing test proving final merged records are reranked globally by
  score after dedupe.
- [x] Add failing tests for per-provider `max_chars_per_recall` and
  `max_bytes_per_record`.
- [x] Add failing tests proving record views and traces carry per-record source
  provider ids.
- [x] Add a failing test for multiple plugin memory providers in one registry.
- [x] Add a failing test for writable plugin/team provider selection when policy
  allows it.
- [x] Implement concurrent provider recall with bounded deadlines and
  deterministic degraded outcomes.
- [x] Preserve provider id per record through dedupe, trace, SDK facade, and
  frontend payloads.
- [x] Enforce provider record, char, and byte budgets before global merge.
- [x] Replace plugin singleton storage with a collection keyed by provider id.
- [x] Make registry write selection respect provider descriptors and policy
  instead of hard-coding only built-in durable providers.
- [x] Run:

```bash
cargo test -p jyowo-harness-memory --features provider-registry --test provider_registry -- --nocapture
cargo test -p jyowo-harness-memory --features provider-registry --test recall -- --nocapture
cargo test -p jyowo-harness-plugin --test registry -- --nocapture
cargo test -p jyowo-harness-sdk --features testing,memory-provider-registry,builtin-toolset --lib memory_tool_ -- --nocapture
```

- [x] Run read-only subagent audit for Task 17.
- [x] Commit:

```bash
git add crates/jyowo-harness-memory crates/jyowo-harness-plugin crates/jyowo-harness-sdk/src/harness/memory.rs
git commit -m "fix(memory): complete provider registry fanout"
```

**Execution log:**

| Date | Command | Exit | Evidence |
|---|---|---:|---|
| 2026-07-05 14:18 CST | `cargo test -p jyowo-harness-memory --features provider-registry --test recall -- --nocapture` | 101 | Red tests failed for missing global rerank/dedupe and provider char/byte budget enforcement. |
| 2026-07-05 14:28 CST | `cargo test -p jyowo-harness-memory --features provider-registry --test provider_registry registry_write_selection_allows -- --nocapture` | 101 | Red tests failed because writable provider selection still hard-coded BuiltIn trust and rejected plugin/team providers. |
| 2026-07-05 14:36 CST | `cargo fmt --all --check` | 0 | Rust formatting gate passed. |
| 2026-07-05 14:36 CST | `cargo test -p jyowo-harness-memory --features provider-registry --test recall -- --nocapture` | 0 | `21 passed; 0 failed`; covers fanout, global rerank/dedupe, provider budgets, source attribution, and traces. |
| 2026-07-05 14:36 CST | `cargo test -p jyowo-harness-memory --features provider-registry --test provider_registry -- --nocapture` | 0 | `12 passed; 0 failed`; covers plugin/team writable provider selection and descriptor validation. |
| 2026-07-05 14:36 CST | `cargo test -p jyowo-harness-plugin --test registry -- --nocapture` | 0 | `51 passed; 0 failed`; covers multiple plugin memory provider registration and deactivation cleanup. |
| 2026-07-05 14:38 CST | `cargo test -p jyowo-harness-sdk --features testing,memory-provider-registry,builtin-toolset --lib memory_tool_search_preserves_per_record_provider_ids -- --nocapture` | 0 | SDK MemoryTool search response preserved per-record provider ids. |
| 2026-07-05 14:38 CST | `cargo test -p jyowo-harness-memory --test provider_registry -- --nocapture` | 0 | `12 passed; 0 failed` under the default feature set. |
| 2026-07-05 14:38 CST | `cargo test -p jyowo-harness-memory --test recall -- --nocapture` | 0 | Command passed; this target currently has `0` active tests without `provider-registry`. |
| 2026-07-05 15:10 CST | `cargo test -p jyowo-harness-memory --features provider-registry --test recall -- --nocapture` | 0 | `23 passed; 0 failed`; added cross-provider content/source/evidence dedupe coverage. |
| 2026-07-05 15:10 CST | `cargo test -p jyowo-harness-memory --features provider-registry --test provider_registry -- --nocapture` | 0 | `12 passed; 0 failed`; registry provider selection still passes after dedupe fix. |
| 2026-07-05 15:10 CST | `cargo test -p jyowo-harness-plugin --test registry -- --nocapture` | 0 | `51 passed; 0 failed`; plugin provider collection behavior still passes. |
| 2026-07-05 15:10 CST | `cargo test -p jyowo-harness-sdk --features testing,memory-provider-registry,builtin-toolset --lib memory_tool_ -- --nocapture` | 0 | `3 passed; 0 failed`; SDK MemoryTool provider attribution and provider policy tests pass. |
| 2026-07-05 15:10 CST | `cargo fmt --all --check` | 0 | Rust formatting gate passed after audit fixes. |
| 2026-07-05 15:10 CST | `pnpm check:docs` | 0 | Docs gate passed after regenerating `docs/testing/test-inventory.md`. |
| 2026-07-05 15:12 CST | Read-only subagent audit | PASS | Re-audit verified dedupe key, provider selection policy, team provider durability, feature-enabled plan commands, and docs gate evidence. |

#### Task 18: Make Policy And MemoryTool The Runtime Authority

**Files:**

- Modify: `crates/jyowo-harness-memory/src/external.rs`
- Modify: `crates/jyowo-harness-memory/src/policy.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/memory.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/types.rs`
- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory.rs`
- Test: `crates/jyowo-harness-memory/tests/policy.rs`
- Test: `crates/jyowo-harness-tool/tests/memory_tool.rs`
- Test: `crates/jyowo-harness-sdk/tests/memory_facade.rs`
- Test: `crates/jyowo-harness-contracts/tests/memory_platform_contracts.rs`

**Checklist:**

- [x] Add failing tests proving direct public writes cannot bypass
  `MemoryPolicyEngine`.
- [x] Add failing tests proving `CandidateOnly` creates inbox candidates instead
  of failing direct write requests.
- [x] Add failing contract/tool tests proving prompt-visible schema is
  `MemoryToolArgs`.
- [x] Add failing tests proving search/read/list can use policy auto paths while
  create/update/delete require explicit action plans.
- [x] Remove or restrict policy-free public write APIs, or make them private
  helpers only reachable after policy.
- [x] Route candidate-only create/update/delete into `MemoryInbox`.
- [x] Generate the tool descriptor schema from the public contract shape.
- [x] Keep all memory write audit events required and fail-closed.
- [x] Run:

```bash
cargo test -p jyowo-harness-memory --test policy -- --nocapture
cargo test -p jyowo-harness-tool --features builtin-toolset --test memory_tool -- --nocapture
cargo test -p jyowo-harness-sdk --features testing --test memory_facade -- --nocapture
cargo test -p jyowo-harness-contracts --test memory_platform_contracts -- --nocapture
```

- [x] Run read-only subagent audit for Task 18.
- [x] Commit:

```bash
git add crates/jyowo-harness-memory crates/jyowo-harness-tool crates/jyowo-harness-contracts crates/jyowo-harness-sdk/src/harness/memory.rs
git commit -m "fix(memory): enforce policy through memory tool"
```

**Execution Log:**

| Date (CST) | Command / Audit | Exit | Notes |
|---|---:|---:|---|
| 2026-07-05 16:31 CST | `cargo fmt --all --check` | 0 | Rust formatting gate passed after required audit and candidate operation fixes. |
| 2026-07-05 16:31 CST | `cargo test -p jyowo-harness-memory --test policy -- --nocapture` | 0 | `16 passed; 0 failed`; policy grants require action plan plus ticket and scoped non-interactive team grants. |
| 2026-07-05 16:31 CST | `cargo test -p jyowo-harness-tool --features builtin-toolset --test memory_tool -- --nocapture` | 0 | `13 passed; 0 failed`; MemoryTool descriptor schema comes from public `MemoryToolArgs`. |
| 2026-07-05 16:31 CST | `cargo test -p jyowo-harness-sdk --features testing,memory-provider-registry,builtin-toolset --lib memory_tool_ -- --nocapture` | 0 | `6 passed; 0 failed`; CandidateOnly create/update/delete stage inbox candidates without durable writes. |
| 2026-07-05 16:31 CST | `cargo test -p jyowo-harness-sdk --features testing --test memory_facade -- --nocapture` | 0 | `12 passed; 0 failed`; approve applies candidate create/update/delete semantics. |
| 2026-07-05 16:31 CST | `cargo test -p jyowo-harness-contracts --test memory_platform_contracts -- --nocapture` | 0 | `39 passed; 0 failed`; candidate operation and public MemoryTool contract roundtrip. |
| 2026-07-05 16:31 CST | `cargo test -p jyowo-harness-memory --features builtin --test memdir -- --nocapture` | 0 | `10 passed; 5 ignored`; memdir required audit failure rolls back file content. |
| 2026-07-05 16:31 CST | `cargo test -p jyowo-harness-context --features recall-memory --test memory_recall assemble_does_not_reread_memdir_at_runtime -- --nocapture` | 0 | Memdir write fixture updated for required audit. |
| 2026-07-05 16:31 CST | `cargo test -p jyowo-harness-memory --features provider-registry --test recall -- --nocapture` | 0 | `23 passed; 0 failed`; policy-selected provider write path remains covered. |
| 2026-07-05 16:31 CST | `cargo test -p jyowo-harness-memory --features provider-registry --test store_audit -- --nocapture` | 0 | `0 passed; 0 failed`; feature-gated store audit target compiles with required audit sink changes. |
| 2026-07-05 16:31 CST | `pnpm audit:tests > docs/testing/test-inventory.md && pnpm check:docs` | 0 | Test inventory regenerated and docs gate passed. |
| 2026-07-05 16:31 CST | Read-only subagent audit | PASS | Re-audit verified required audit defaults, memdir rollback, CandidateOnly operation targets, MemoryManager write authority, and MemoryTool public schema. |

#### Task 19: Implement Extraction And Consolidation Runtime Wiring

**Files:**

- Modify: `crates/jyowo-harness-memory/src/extraction/job.rs`
- Modify: `crates/jyowo-harness-memory/src/extraction/worker.rs`
- Modify: `crates/jyowo-harness-memory/src/external.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Test: `crates/jyowo-harness-memory/tests/extraction.rs`
- Test: `crates/jyowo-harness-memory/tests/consolidation_metrics.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly_context.rs`

**Checklist:**

- [x] Add failing tests proving session end enqueues durable extraction jobs
  when policy and idle/quota rules allow it.
- [x] Add failing tests proving external context, tenant, quota, and permission
  policy can block extraction.
- [x] Add failing tests proving worker uses a real model/extractor facade path
  or a clearly test-only extractor.
- [x] Add failing tests for consolidation merge, demote, and expire behavior.
- [x] Wire extraction enqueue into production session lifecycle.
- [x] Add worker startup/shutdown ownership in SDK/runtime assembly.
- [x] Replace the legacy consolidation hook path or gate it as test-only.
- [x] Keep generated candidates in inbox until approved or merged by policy.
- [x] Run:

```bash
cargo test -p jyowo-harness-memory --test extraction -- --nocapture
cargo test -p jyowo-harness-memory --test consolidation_metrics -- --nocapture
cargo test -p jyowo-harness-sdk --test runtime_assembly_context -- --nocapture
```

- [x] Run read-only subagent audit for Task 19.
- [x] Commit:

```bash
git add crates/jyowo-harness-memory crates/jyowo-harness-sdk/src/harness/session_runtime.rs crates/jyowo-harness-sdk/src/builder.rs
git commit -m "fix(memory): wire extraction runtime"
```

#### Task 20: Complete Team And Subagent Memory Grants

**Files:**

- Modify: `crates/jyowo-harness-team/src/lib.rs`
- Modify: `crates/jyowo-harness-subagent/src/lib.rs`
- Modify: `crates/jyowo-harness-engine/src/engine.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/teams.rs`
- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Test: `crates/jyowo-harness-team/tests/shared_memory.rs`
- Test: `crates/jyowo-harness-subagent/tests/default_runner.rs`
- Test: `crates/jyowo-harness-engine/tests/main_loop.rs`

**Checklist:**

- [x] Add failing tests proving team shared memory is registered as a provider
  for member runtime recall.
- [x] Add failing tests proving team writes go through provider registry and
  policy, not direct provider injection.
- [x] Add failing tests proving `AgentProfile.memory_scope` maps into runtime
  grants.
- [x] Add failing tests for subagent `ReadOnly`, `ReadWrite`, and
  `CandidateOnly` behavior.
- [x] Make scoped member engine consume shared memory through the registry.
- [x] Convert profile memory scope into team member config and memory grant.
- [x] Apply subagent grants to context, memory tool availability, and write
  policy.
- [x] Run:

```bash
cargo test -p jyowo-harness-team --test shared_memory -- --nocapture
cargo test -p jyowo-harness-subagent --test default_runner -- --nocapture
cargo test -p jyowo-harness-engine --test main_loop -- --nocapture
```

- [x] Run read-only subagent audit for Task 20.
- [x] Commit:

```bash
git add crates/jyowo-harness-team crates/jyowo-harness-subagent crates/jyowo-harness-engine crates/jyowo-harness-agent-runtime crates/jyowo-harness-contracts/src/capability.rs
git commit -m "fix(memory): enforce team and subagent memory grants"
```

#### Task 21: Fix Reference Hydration, Fencing, Traces, And Preview

**Files:**

- Modify: `crates/jyowo-harness-memory/src/reference.rs`
- Modify: `crates/jyowo-harness-memory/src/memdir/fence.rs`
- Modify: `crates/jyowo-harness-memory/src/external.rs`
- Modify: `crates/jyowo-harness-context/src/engine.rs`
- Modify: `crates/jyowo-harness-context/src/prompt.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/conversation.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/memory_preview.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/types.rs`
- Test: `crates/jyowo-harness-context/tests/memory_recall.rs`
- Test: `crates/jyowo-harness-memory/tests/recall.rs`
- Test: `crates/jyowo-harness-sdk/tests/memory_facade.rs`

**Checklist:**

- [x] Add failing tests proving memory references hydrate through
  `ContextReferenceResolver`.
- [x] Add failing tests proving hydrated memory content is escaped and wrapped
  with the untrusted-context note.
- [x] Add failing tests proving label-only memory reference rendering is gone.
- [x] Add failing tests proving recall traces contain real score components from
  ranking.
- [x] Add failing tests proving Model Request Preview represents the final
  redacted model request shape, including sections, memory ids, provider ids,
  trace ids, tool names, policy decisions, and token estimate.
- [x] Route all conversation memory reference hydration through the resolver
  abstraction.
- [x] Use one shared fencing/escaping path for recalled memory and explicit
  references.
- [x] Preserve ranking score components in trace candidates.
- [x] Build preview from the actual request assembly path, not only trace
  injected placeholders.
- [x] Run:

```bash
cargo test -p jyowo-harness-context --test memory_recall -- --nocapture
cargo test -p jyowo-harness-memory --test recall -- --nocapture
cargo test -p jyowo-harness-sdk --test memory_facade -- --nocapture
```

- [x] Run read-only subagent audit for Task 21.
- [x] Commit:

```bash
git add crates/jyowo-harness-memory crates/jyowo-harness-context crates/jyowo-harness-sdk crates/jyowo-harness-contracts/src/events/types.rs
git commit -m "fix(memory): hydrate references and preview requests"
```

#### Task 22: Complete Export, IPC Contracts, And Frontend Memory UI

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/memory.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/features/context/ContextPanel.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.tsx`
- Modify: `apps/desktop/src/features/memory/MemoryBrowser.tsx`
- Modify: `apps/desktop/src/features/memory/MemoryItemCard.tsx`
- Modify: `apps/desktop/src/features/memory/MemoryInbox.tsx`
- Modify: `apps/desktop/src/features/memory/MemoryRecallTracePanel.tsx`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`
- Test: `apps/desktop/src/features/memory/MemoryBrowser.test.tsx`
- Test: `apps/desktop/src-tauri/tests/commands/memory.rs`

**Checklist:**

- [x] Add failing IPC/Zod tests proving frontend schemas mirror Rust serde
  contracts for export and preview.
- [x] Add failing Tauri tests proving raw export is allowed only with explicit
  user action, backend policy allow, and required audit.
- [x] Add failing tests proving exported `contentHash` is the backend content
  hash, not redacted preview hash.
- [x] Add failing tests proving Tauri commands delegate export policy and
  assembly to SDK/runtime instead of owning business logic.
- [x] Add failing UI tests for Model Request Preview in ContextPanel.
- [x] Add failing UI tests for provider, expiry, deletion state, last access,
  and trace score breakdown rendering.
- [x] Add failing UI tests for Inbox merge and Composer thread memory mode.
- [x] Move export policy and payload assembly into SDK/runtime authority.
- [x] Align Rust contracts and Zod schemas.
- [x] Add expiry/deletion/provider/last-access fields to IPC payloads.
- [x] Implement ContextPanel preview, Inbox merge, Browser metadata, and
  Composer thread mode controls.
- [x] Run:

```bash
cargo test -p jyowo-desktop-shell --test commands memory -- --nocapture
pnpm check:desktop
```

- [x] Run read-only subagent audit for Task 22.
- [x] Commit:

```bash
git add apps/desktop/src-tauri apps/desktop/src
git commit -m "fix(memory): complete desktop memory surfaces"
```

#### Task 23: Strengthen Architecture Gates And Documentation

**Files:**

- Modify: `scripts/memory-architecture-policy.mjs`
- Modify: `scripts/memory-architecture-policy.test.mjs`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-product-ux.md`
- Modify: `docs/testing/testing-strategy.md`
- Modify: `package.json` if gate wiring is missing.

**Checklist:**

- [x] Add failing fixtures for `memory-external-slot` and `external-slot` in
  `Cargo.toml`.
- [x] Add failing fixtures for cfg-gated external slot features.
- [x] Add failing fixtures for `with_external_memory_provider`.
- [x] Add failing fixtures for production `InMemoryMemoryProvider::new(` in
  runtime, SDK, engine, context, tool, and Tauri paths.
- [x] Add failing fixtures for label-only memory reference rendering.
- [x] Add failing fixtures for forbidden trace fields named `content`,
  `raw_content`, `prompt`, or `message_text`.
- [x] Add allowed fixtures for migration-only `DREAMS.md` references.
- [x] Update scanner to include `Cargo.toml` and all production paths listed in
  Task 14.
- [x] Update normative docs to describe implemented behavior only after Tasks
  16-22 are complete.
- [x] Run:

```bash
node --test scripts/memory-architecture-policy.test.mjs
node scripts/memory-architecture-policy.mjs
pnpm check:docs
pnpm check:agent-docs
pnpm check:backend-docs
pnpm check:frontend-docs
pnpm check:test-architecture
```

- [x] Run read-only subagent audit for Task 23.
- [x] Commit:

```bash
git add scripts docs package.json
git commit -m "docs(memory): enforce memory architecture gates"
```

#### Task 24: Final Integration Verification And Audit

**Files:**

- Modify only files required by audit fixes discovered during this task.

**Checklist:**

- [x] Run `cargo fmt --all --check`.
- [x] Run `cargo check --workspace`.
- [x] Run `cargo test --workspace`.
- [x] Run `cargo check -p jyowo-harness-memory --no-default-features --features provider-registry`.
- [x] Run `cargo test -p jyowo-harness-memory --no-default-features --features provider-registry --test provider_registry -- --nocapture`.
- [x] Run `cargo check -p jyowo-harness-context --no-default-features --features recall-memory`.
- [x] Run `cargo check -p jyowo-harness-engine --no-default-features --features recall-memory`.
- [x] Run `cargo check -p jyowo-harness-sdk --no-default-features --features memory-provider-registry,stream-permission,rule-engine-permission,integrity,jsonl-store,sqlite-store,blob-file,local-sandbox,mcp-http,mcp-stdio`.
- [x] Run `pnpm check:docs`.
- [x] Run `pnpm check:agent-docs`.
- [x] Run `pnpm check:frontend-docs`.
- [x] Run `pnpm check:backend-docs`.
- [x] Run `pnpm check:desktop`.
- [x] Run `pnpm check:rust`.
- [x] Run `pnpm audit:tests`.
- [x] Run `pnpm check:test-architecture`.
- [x] Run `pnpm check:agent-orchestration-no-fakes`.
- [x] Run `pnpm check:agent-supervisor-sidecar`.
- [x] Run `pnpm check:quick`.
- [x] Run `pnpm check:frontend:fast`.
- [x] Run `pnpm check:rust:fast`.
- [x] Run `pnpm check`.
- [x] Run final read-only subagent audit using the prompt in this plan.
- [x] Fix any FAIL result and repeat the targeted gate plus final audit.
- [x] Only after every gate and audit passes, update completion criteria and
  commit final plan state.

### Task 15-24 Closeout Implementation Record

Date: 2026-07-05 CST.

This section is the authoritative closeout record for Tasks 15-24. The task
checklists above are retained as the implementation plan. The status below
records what was actually implemented and verified in this worktree.

#### Issues Closed

- [x] Task 15 rebaselined the plan against the current worktree and preserved
  the audit findings as implementation work instead of treating tests as proof.
- [x] Task 16 replaced the incomplete local memory provider path with durable
  SQLite storage, lexical recall, embedding metadata, tombstones, visibility
  checks, tenant isolation, bounded previews, access metadata updates, and
  migration repair for FTS triggers.
- [x] Task 17 replaced the single external slot pattern with a provider
  registry that supports fanout recall, one policy-selected write target,
  provider attribution, budgets, descriptor validation, and plugin capability
  registration.
- [x] Task 18 made policy and `memory_tool` the runtime authority for reads and
  writes. Model-controlled runtime context fields are rejected, write grants
  require policy context, and candidate-only paths stage inbox candidates
  instead of mutating durable memory.
- [x] Task 19 wired session-end extraction to durable jobs, worker processing,
  consolidation actions, inbox candidates, policy blocks, tenant scope, quota
  checks, and external-context suppression.
- [x] Task 20 wired team and subagent memory grants through runtime policy and
  provider registration. Team shared memory writes now require journaled policy
  authority, and subagent memory scope controls context, tool availability, and
  write behavior.
- [x] Task 21 routed explicit memory references through the resolver path,
  fenced hydrated content as untrusted context, removed label-only rendering,
  preserved recall score components, and built model request previews from the
  assembled redacted request shape.
- [x] Task 22 completed desktop IPC and frontend memory surfaces for export,
  preview, recall traces, inbox merge, browser metadata, and thread memory mode
  controls. Export assembly and policy remain on the backend authority path.
- [x] Task 23 strengthened architecture gates and docs for external-slot
  patterns, production in-memory provider use, label-only references, trace raw
  content fields, and documented memory runtime behavior.
- [x] Task 24 fixed the final integration issue found during verification:
  concurrent SQLite initialization could race on WAL setup under high test
  concurrency. `LocalMemoryProvider` now applies a busy timeout, serializes
  process-local SQLite initialization, avoids rewriting WAL mode when already
  configured, and uses bounded retry for transient busy/locked initialization
  failures.

#### Implementation Checklist

- [x] Local provider persistence, recall, embeddings, tombstones, visibility,
  tenant isolation, and audit rollback are implemented.
- [x] Provider registry fanout, write selection, provider attribution, and
  plugin memory provider registration are implemented.
- [x] Policy-driven memory tool create/update/delete/search/list/propose flows
  are implemented.
- [x] Durable extraction jobs, consolidation worker flow, inbox staging, and
  session lifecycle enqueue are implemented.
- [x] Team and subagent memory grants are implemented through runtime policy and
  provider registration.
- [x] Memory reference hydration, fencing, trace score details, and final
  request preview are implemented.
- [x] Tauri IPC contracts, Zod schemas, desktop memory UI, context preview,
  inbox merge, and composer thread controls are implemented.
- [x] Architecture policy scanner and related docs are updated.
- [x] SQLite initialization contention is fixed.
- [x] `docs/testing/test-inventory.md` was regenerated with `pnpm audit:tests >
  docs/testing/test-inventory.md`.
- [x] Task 16-24 follow-up changes are recorded by the final aggregate commit
  from this worktree.

#### Verification Record

Passed:

- `cargo fmt --all --check`
- `cargo check -p jyowo-harness-memory`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo test -p jyowo-desktop-shell --test commands -- --nocapture`
- `cargo check -p jyowo-harness-memory --no-default-features --features provider-registry`
- `cargo test -p jyowo-harness-memory --no-default-features --features provider-registry --test provider_registry -- --nocapture`
- `cargo check -p jyowo-harness-context --no-default-features --features recall-memory`
- `cargo check -p jyowo-harness-engine --no-default-features --features recall-memory`
- `cargo check -p jyowo-harness-sdk --no-default-features --features memory-provider-registry,stream-permission,rule-engine-permission,integrity,jsonl-store,sqlite-store,blob-file,local-sandbox,mcp-http,mcp-stdio`
- `pnpm check:docs`
- `pnpm check:agent-docs`
- `pnpm check:frontend-docs`
- `pnpm check:backend-docs`
- `pnpm audit:tests`
- `pnpm check:test-architecture`
- `pnpm check:agent-orchestration-no-fakes`
- `pnpm check:agent-supervisor-sidecar`
- `pnpm check:quick`
- `pnpm check:frontend:fast`
- `pnpm check:desktop`
- `pnpm check:rust:fast`
- `pnpm check:rust`

Final read-only Task 15-24 audit result:

- PASS. The final audit found no remaining blocking Task 15-24 findings after
  the extraction worker was changed to validate all staged candidate and
  consolidation proposals before mutating the inbox.

#### Environment Notes

- `pnpm check:rust` initially failed during SDK feature-test linking with
  `No space left on device`.
- Build artifacts were cleaned only from
  `/Users/goya/Repo/Git/Jyowo-memory-platform/target`.
- After cleanup, `pnpm check:rust` passed.

### 2026-07-05 Final Audit Fix Plan And Checklist

This section records the final audit findings found after the Task 15-24
implementation pass and the fixes applied in this worktree.

#### Audit Findings

- [x] P1: Local hybrid ranking had drifted from the plan. The final score was
  reweighted around available channels when vector score was missing, which
  violated the fixed formula and hid missing semantic behavior.
  - Fix: `ranking::compute_final_score` now always uses the planned fixed
    weights and treats missing vector score as `0.0`.
  - Follow-up fix: `LocalMemoryProvider` now applies `min_similarity` to the
    available recall match channel before computing final sort score. This
    keeps default lexical-only and semantic-only recall from being dropped by
    the fixed-weight final score while preventing lexical-only FTS relevance
    from satisfying near-exact similarity gates.

- [x] P1: Desktop production runtime used the heuristic memory extractor. That
  made production extraction deterministic but not model-backed as designed.
  - Fix: desktop runtime no longer injects `HeuristicMemoryExtractor`; it uses
    the SDK default model-backed extractor path.

- [x] P1: Model-backed extraction could send raw session excerpt text to the
  model when the runtime observer used `NoopRedactor`.
  - Fix: `ModelBackedMemoryExtractor` runs the SDK default redactor before
    constructing the extraction request excerpt, even when the observer
    redactor is noop.

- [x] P1: Extractor output was trusted too early. A model output containing a
  credential or prompt-injection payload could reach the memory inbox.
  - Fix: extraction worker sanitizes extracted candidate and consolidation
    content through `MemoryThreatScanner` before inbox proposal. Credential
    findings redact; prompt-injection findings block the job.

- [x] P1: Read-only team/subagent memory scopes could remove the `memory` tool
  entirely. That blocked legitimate read/list/search flows instead of letting
  backend policy reject writes.
  - Fix: only `Empty` memory scope filters out the `memory` tool. `ReadOnly`
    keeps the tool available and relies on runtime policy for write denial.

- [x] P2: Team shared memory provider advertised built-in trust instead of team
  trust.
  - Fix: team shared memory provider descriptors now report
    `MemoryProviderTrust::Team`, and `SharedMemory::descriptor()` delegates to
    its provider descriptor.

- [x] P2: Memory export audit could be emitted before the export file write was
  known to have succeeded.
  - Fix: export now writes the file first and emits the audit event only after a
    successful write.

- [x] P1: `memory_tool` propose bypassed write policy. In `ReadOnly` thread
  mode the model could still create an inbox candidate because
  `execute_propose` wrote directly to `MemoryInbox`.
  - Fix: `execute_propose` now evaluates the session write policy first.
    `Allow` and `CandidateOnly` may stage a candidate; `Deny` rejects before
    inbox mutation.

- [x] P1: Candidate-only staging trusted model-provided draft content before
  inbox insertion. `create`, `update`, `delete`, and `propose` candidate paths
  could store credential or prompt-injection payloads in the inbox.
  - Fix: `stage_candidate_response` now scans every candidate draft with
    `MemoryThreatScanner` before `MemoryInbox::propose_with_operation`.
    Credential findings redact candidate content; blocking findings reject the
    candidate and leave the inbox unchanged.

- [x] P1: Team/subagent run-scoped memory modes did not reach the SDK memory
  tool policy path. `RunContext.memory_thread_settings` carried `ReadOnly` or
  `CandidateOnly`, but `memory_tool` rebuilt policy from persisted SQLite
  thread settings. When no persisted row existed, the default was `ReadWrite`.
  This meant a run-scoped `ReadOnly` team/subagent could still stage a
  candidate, and a run-scoped `CandidateOnly` team/subagent could mutate durable
  memory directly.
  - Fix: `ToolContext` now carries the run-scoped `MemoryThreadSettings`,
    `memory_tool` forwards them in `MemoryToolRuntimeRequest`, and the SDK
    runtime copies them into the effective `SessionOptions`.
    `memory_policy_for_session` now prefers matching run-scoped settings over
    persisted settings and fails closed on session mismatch.

- [x] P2: Extraction background polling swallowed per-poll errors without
  durable operator-visible telemetry. This is non-blocking because failed jobs
  are still retried or blocked through the durable queue, but operational
  visibility was still required for completion.
  - Fix: `MemoryExtractionRuntime` now records a redacted
    `memory.extraction.poll` span with an error event and error status whenever
    background polling fails.

- [x] P1: `memory_tool` read-only actions still used the same user-review
  permission plan as write actions. This violated the planned policy-auto path
  for `search`, `read`, and `list`.
  - Fix: `MemoryTool::plan` now parses the requested action and uses
    `PermissionCheck::Allowed` for `search`, `read`, and `list`; `create`,
    `update`, `delete`, and `propose` still use user-review permission plans.

- [x] P1: Extraction consolidation `reason` was model output but was written
  into inbox candidate tags without threat scanning. A credential or
  prompt-injection string could reach frontend state and durable metadata after
  approval.
  - Fix: extraction worker now scans consolidation reasons through the same
    `MemoryThreatScanner` path as extracted content before tags are written.
    Credential findings redact the reason tag; blocking findings fail the job
    before inbox mutation.

- [x] P1: Extraction worker scanning and inbox writes were interleaved. A
  later blocking consolidation reason could fail the job after an earlier
  ordinary candidate from the same model output had already been written to the
  inbox.
  - Fix: extraction worker now stages all candidate and consolidation inbox
    proposals after validation and threat scanning. It writes to
    `MemoryInbox` only after every output item has passed.

#### Implementation Plan

- [x] Restore the fixed hybrid ranking formula and add coverage for missing
  vector score.
- [x] Split recall threshold gating from final sorting score so planned fixed
  weights do not break default recall.
- [x] Remove production heuristic extractor wiring from desktop runtime.
- [x] Redact model extraction excerpts before model request construction.
- [x] Threat-scan extractor output before inbox proposal.
- [x] Preserve `memory` tool availability for read-only memory scopes and keep
  write rejection in backend policy.
- [x] Correct team provider trust metadata.
- [x] Move memory export audit emission after file write success.
- [x] Route `memory_tool` propose through session write policy before inbox
  mutation.
- [x] Threat-scan all SDK memory tool candidate staging paths before inbox
  mutation.
- [x] Propagate run-scoped team/subagent memory thread settings through
  `ToolContext`, `MemoryToolRuntimeRequest`, SDK `SessionOptions`, and policy
  evaluation.
- [x] Record redacted operator-visible telemetry for background extraction poll
  failures.
- [x] Split `memory_tool` planning so read-only actions use the policy-auto
  permission path while write-like actions still require user review.
- [x] Threat-scan extraction consolidation reasons before writing inbox candidate
  tags.
- [x] Stage extraction inbox proposals before mutation so a later blocked item
  prevents every proposal from the same output from entering the inbox.
- [x] Run targeted regression tests for every fixed finding.
- [x] Attempt `pnpm check:rust` after fixes and record the environment failure.
- [x] Run `cargo check --workspace` and targeted Rust regressions after the
  final follow-up fix.
- [x] Record final read-only audit result after the last review pass.

#### Verification Added After Final Audit Fixes

Passed:

- `cargo fmt --all`
- `cargo fmt --all --check`
- `node scripts/memory-architecture-policy.mjs`
- `cargo test -p jyowo-harness-memory --test local_provider local_ranking_uses_fixed_weights_when_vector_is_missing`
- `cargo test -p jyowo-harness-memory --test local_provider`
- `cargo test -p jyowo-harness-memory --test extraction worker_`
- `cargo test -p jyowo-harness-engine --features subagent-tool child_tool_filter`
- `cargo test -p jyowo-harness-sdk --features memory-provider-registry memory_extraction_excerpt_uses_default_redactor_after_noop_redactor`
- `cargo test -p jyowo-harness-sdk --features agents-team read_only_team_members_keep_memory_tool_available`
- `cargo test -p jyowo-harness-team --test shared_memory shared_memory_descriptor_uses_team_trust`
- `cargo test -p jyowo-harness-sdk --features testing,memory-provider-registry,builtin-toolset --lib memory_tool_ -- --nocapture`
- `cargo test -p jyowo-harness-tool --features builtin-toolset --test memory_tool -- --nocapture`
- `cargo test -p jyowo-harness-sdk --features memory-provider-registry memory_extraction_poll_error_records_redacted_telemetry -- --nocapture`
- `cargo test -p jyowo-harness-tool --features builtin-toolset --test memory_tool memory_tool_plans_read_actions_as_policy_auto_and_writes_as_user_review -- --nocapture`
- `cargo test -p jyowo-harness-memory --test extraction worker_blocks_later_consolidation_reason_before_any_inbox_mutation -- --nocapture`
- `cargo test -p jyowo-harness-memory --test extraction worker_ -- --nocapture`
- `cargo test -p jyowo-harness-engine --features subagent-tool child_tool_filter`
- `cargo test -p jyowo-harness-sdk --features agents-team read_only_team_members_keep_memory_tool_available`
- `cargo check --workspace`
- `pnpm audit:tests > docs/testing/test-inventory.md`
- `pnpm check:docs`
- `git diff --check`

Notes:

- The first post-fix `pnpm check:rust` run failed while linking
  `jyowo-harness-agent-runtime` test `subagents` with `No space left on
  device`. Only `/Users/goya/Repo/Git/Jyowo-memory-platform/target` build
  artifacts were removed. The command was rerun from a clean `target` and
  passed.
- The first post-fix `pnpm check:docs` run failed because
  `docs/testing/test-inventory.md` no longer matched `pnpm audit:tests`
  output. The inventory was regenerated with
  `pnpm audit:tests > docs/testing/test-inventory.md`; `pnpm check:docs` then
  passed.
- A later full `pnpm check:rust` rerun after follow-up fixes failed again due
  to `No space left on device` while linking SDK integration tests and writing
  rustc fingerprint output. The failure was environmental, not a Rust compile
  or test assertion failure. The final verification therefore uses
  `cargo check --workspace` plus targeted regression tests for the changed
  paths.

#### Final Read-only Audit Result

The final read-only audit found no P0 issues. It found three additional P1
issues after the earlier fix pass:

- `execute_propose` bypassed policy and could stage candidates in `ReadOnly`
  mode.
- SDK candidate-only inbox staging did not scan model-provided candidate
  content.
- Team/subagent run-scoped `ReadOnly` and `CandidateOnly` memory modes did not
  reach the SDK memory tool policy path when no persisted thread setting existed.

All three P1 findings are fixed in this worktree and covered by targeted
regression tests. The remaining P2 background extraction polling telemetry gap
is also fixed in this worktree and covered by a targeted regression test.

A follow-up read-only audit found no P0 issues and two additional P1 issues:

- `memory_tool` read-only actions used user-review permission plans instead of
  policy-auto plans.
- Extraction consolidation reasons were not threat-scanned before entering inbox
  candidate tags.

Both follow-up P1 findings are fixed in this worktree and covered by targeted
regression tests.

A final follow-up audit found one additional P1:

- Extraction worker wrote earlier candidates before scanning later
  consolidation reasons from the same model output. A later blocking reason
  could fail the job while leaving an earlier inbox mutation behind.

That P1 is fixed in this worktree by staging all extraction proposals before
any inbox mutation. The new regression
`worker_blocks_later_consolidation_reason_before_any_inbox_mutation` failed
before the fix and passes after it.

The final read-only re-audit of that staging fix found no P0 or P1 issues.
