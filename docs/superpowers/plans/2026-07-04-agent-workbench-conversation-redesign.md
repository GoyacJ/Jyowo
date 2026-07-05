# Agent Workbench Conversation Redesign Implementation Plan

> **For agentic workers:** REQUIRED MODEL PROFILE: use ChatGPT 5.5 xhigh. When the tool requires API-style overrides, use `model: gpt-5.5`, `reasoning_effort: xhigh`, and the highest available service tier. Do not downgrade to a smaller model for implementation or audits.
>
> **Required sub-skill:** use `superpowers:subagent-driven-development` task-by-task. Every task must end with a read-only subagent audit. Do not self-certify a task. If multi-agent tools are unavailable, stop.

**Goal:** Replace the current thin chat timeline with a conversation-native agent workbench that exposes typed decisions, evidence, artifacts, diffs, command executions, streaming state, and recovery without compatibility shims or UI-only policy.

**Architecture:** Rust remains the policy authority and the owner of redacted conversation worktree projection. Tauri transports typed payloads only. React renders the projected workbench model, manages local UI selection, and submits user decisions without inventing policy or reconstructing execution state from raw events.

**Tech Stack:** Rust 1.96, Tauri 2, React 19, TypeScript 6, Zod, schemars JsonSchema, TanStack Query, TanStack Virtual, Testing Library, Vitest, Storybook, cargo test, pnpm 11.7, Git worktrees, existing Jyowo docs gates.

---

## Branch And Worktree Rules

This plan file must be tracked on `main` before implementation starts. Implementation must not run in the active checkout.

Start implementation from an isolated worktree:

```bash
SOURCE_CHECKOUT="$(pwd)"
PLAN_PATH="docs/superpowers/plans/2026-07-04-agent-workbench-conversation-redesign.md"
test -f "$PLAN_PATH"
test "$(git branch --show-current)" = "main"
test "$(git ls-files -- "$PLAN_PATH")" = "$PLAN_PATH"
test -z "$(git status --short -- "$PLAN_PATH")"
git status --short

git worktree add -b goya/agent-workbench-conversation-redesign ../Jyowo-agent-workbench-conversation-redesign main
cd ../Jyowo-agent-workbench-conversation-redesign
test -f "$PLAN_PATH"
test "$(git branch --show-current)" = "goya/agent-workbench-conversation-redesign"
git status --short
```

Expected:

- source checkout branch is `main`
- this plan is tracked and clean on `main`
- implementation branch is `goya/agent-workbench-conversation-redesign`
- implementation happens only in `../Jyowo-agent-workbench-conversation-redesign`
- implementation worktree starts from tracked `main` content
- if branch or worktree already exists, stop and ask for a new branch name

Do not stash, revert, or overwrite unrelated user changes. Stage exact files only. Never stage broad directories such as `crates`, `apps`, or `docs`.

## Mandatory Execution Protocol

Every task must follow this exact order.

1. **Task Intent Check**
   - Restate the task objective.
   - List exact in-scope files.
   - List exact out-of-scope files.
   - State the current design weakness being removed.
   - State the target design being implemented.
   - State the invariant that must remain true.
   - State tests and gates for this task.
   - State why this task does not add mock data, fake runtime paths, noop success, placeholder behavior, old compatibility branches, or UI-only policy.

2. **Read Required Context**
   - Read root `AGENTS.md`.
   - Read `docs/testing/testing-strategy.md`.
   - For backend changes, read:
     - `docs/backend/agent-harness-backend-development-guidelines.md`
     - `docs/backend/backend-runtime.md`
     - `docs/backend/backend-engineering.md`
     - `docs/backend/backend-quality.md`
   - For frontend changes, read:
     - `docs/frontend/agent-harness-frontend-development-guidelines.md`
     - `docs/frontend/frontend-product-ux.md`
     - `docs/frontend/frontend-engineering.md`
     - `docs/frontend/frontend-quality.md`
   - Read every file listed by the task before editing it.

3. **Write Failing Tests First**
   - Add or update tests in the owning layer.
   - Run the narrow test and confirm it fails for the intended reason.
   - If a failing test cannot be written first, explain why in the task response and add the nearest executable contract or component test before implementation.

4. **Implement**
   - Make the task-scoped implementation.
   - Destructive refactor is allowed when it removes the old thin chat model, avoids compatibility debt, or clarifies ownership.
   - Do not keep compatibility wrappers for the old projection semantics.
   - Do not keep old components alive behind hidden branches unless a later task in this plan explicitly deletes them in the same commit.

5. **Task Reality Check**
   - Before running final gates, restate the implemented behavior.
   - Compare it against the task objective.
   - Name any remaining gap. If a gap exists, fix it before audit.
   - Confirm no production mock, fake, noop, placeholder, or UI-only policy path was added.

6. **Local Gate**
   - Run the task-specific commands.
   - If the task changes Rust files, run `cargo fmt --all --check`.
   - If the task changes public Rust API, serde shape, JsonSchema, journal event handling, projection shape, SDK facade, Tauri command payload, permission behavior, artifact behavior, or tool evidence, run `cargo check --workspace`.
   - If the task changes frontend code, run the narrow Vitest target and `pnpm check:desktop`.
   - If the task adds, renames, moves, splits, or deletes tests, run `pnpm check:test-architecture`.
   - If the task changes docs, run `pnpm check:docs`.
   - Always run `git diff --check`.
   - Inspect `git diff --name-only`.

7. **Subagent Audit**
   - Spawn a read-only subagent with ChatGPT 5.5 xhigh.
   - The audit must return `PASS` or `FAIL`.
   - `FAIL` must include file and line evidence.
   - Fix failures and rerun the same audit before moving on.

8. **Commit**
   - Commit each task separately.
   - Commit message format: `refactor: <task subject>`.
   - Do not commit unrelated files.

Subagent audit prompt template:

```text
Read-only audit for Task N of docs/superpowers/plans/2026-07-04-agent-workbench-conversation-redesign.md.

Use ChatGPT 5.5 xhigh. If the tool requires API-style overrides, use `model: gpt-5.5`, `reasoning_effort: xhigh`, and the highest available service tier.
Do not edit files.

Task objective:
<copy the Task Intent Check>

Task reality check:
<copy the Task Reality Check>

Evidence:
- owned files changed:
  - <exact files>
- commands run:
  - command: <exact command>
    exit: <exit code>
- git diff --check: exit <exit code>
- git diff --name-only:
  - <exact files>

Verify:
- implementation matches this task and not a different design
- final policy decision remains in Rust
- React renders projected state and never invents permission, sandbox, tool, diff, or artifact policy
- Tauri commands transport typed payloads and do not upgrade authority
- secrets and private paths are redacted before frontend state
- no production mock data, fake runtime path, noop success, placeholder behavior, old compatibility branch, or UI-only policy was introduced
- tests cover the changed boundary
- command evidence includes every required gate with exit code 0
- Rust tasks include `cargo fmt --all --check`
- public API and serde tasks include `cargo check --workspace`
- frontend tasks include `pnpm check:desktop`
- test structure changes include `pnpm check:test-architecture`
- changed files are limited to task-owned files

Return PASS or FAIL.
For FAIL, include exact file and line evidence.
```

## Forbidden Throughout

- Production mock data, fake runtime paths, fake providers, fake artifacts, fake command output, noop success, or placeholder behavior.
- Compatibility schemas that silently accept the old conversation projection shape.
- UI-only permission policy, UI-only sandbox policy, UI-only artifact trust, or UI-only diff trust.
- Reconstructing main conversation state from raw `RunEvent` in React.
- Raw tool input, raw command output, raw private paths, secrets, environment variables, provider credentials, or raw chain-of-thought in frontend state.
- Large command output, full diff patch, full artifact content, or raw tool result embedded directly in `ConversationTurn`.
- `JSON.stringify` of large segments during render.
- Hidden old components kept alive after their replacement task.
- Broad staging or commits containing unrelated files.

Allowed:

- Test fixtures under test-owned locations.
- Deterministic test adapters that exercise production code paths.
- Destructive refactor when the old design would force compatibility debt.

## Current Code Facts

These facts are observed in the current codebase and must guide the implementation.

- The main canvas uses `pageConversationWorktree` and `ConversationTurn[]`.
  - `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
  - `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.ts`
  - `apps/desktop/src/shared/tauri/commands.ts`
- The public worktree projection lives in `crates/jyowo-harness-contracts/src/conversation.rs`.
- The worktree projector lives in `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`.
- Worktree paging currently loads the complete timeline then slices turns.
  - `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- `ToolPermissionState` currently contains only request identity, status, summary, confirmation text, and event refs.
  - `crates/jyowo-harness-contracts/src/conversation.rs`
  - `apps/desktop/src/shared/tauri/commands.ts`
- `PermissionRequestedEvent` already carries stronger backend data: subject, severity, scope hint, presented options, actor source, action plan hash, review, effective mode, sandbox policy.
  - `crates/jyowo-harness-contracts/src/events/permission.rs`
- `ToolAttempt` currently contains tool name, status, permission, failure summary, and event refs.
  - `crates/jyowo-harness-contracts/src/conversation.rs`
- `ToolUseRequestedEvent` carries raw `input: Value`; projection must never expose it directly.
  - `crates/jyowo-harness-contracts/src/events/tool.rs`
- `ProcessStepDetail::Command` currently contains command, optional output, exit code, and duration only.
  - `crates/jyowo-harness-contracts/src/conversation.rs`
- `ProcessStepDetail::Diff` currently contains preview files only.
- `AssistantSegment::Thinking` still exists and is rendered by `ThinkingPanel`.
  - `apps/desktop/src/features/conversation/timeline/assistant-work-view.tsx`
  - `apps/desktop/src/features/conversation/timeline/thinking-panel.tsx`
- The composer uses a one-row textarea and ad hoc popover reference picker.
  - `apps/desktop/src/features/conversation/Composer.tsx`
- Artifact preview is a timeline card, not a workbench pane.
  - `apps/desktop/src/features/conversation/timeline/artifact-segment-view.tsx`
  - `apps/desktop/src/features/artifacts/ArtifactPreview.tsx`
- The app shell has sidebar, context panel, activity rail, and a disabled more-actions button.
  - `apps/desktop/src/app/shell/AppShell.tsx`

## Target Design

The main product object remains `Conversation`. The visible work surface becomes:

```text
Workspace
  -> Conversation
    -> ConversationTurn[]
      -> AssistantWork
        -> user-safe narrative
        -> user-safe process summary
        -> typed evidence summaries
        -> decision requests
        -> artifact revision references
        -> review / clarification requests
```

The workbench layout becomes:

```text
Left: project and conversation navigation
Center: conversation timeline
Right: inspector pane for Context | Decision | Evidence | Diff | Artifact | Terminal
Bottom: composer
Top: workspace, branch/worktree, run state, model, permission mode, command palette
Activity rail: compact event stream and run status
```

The canonical projection rules:

- Rust emits redacted typed projection.
- Tauri transports the projection.
- Zod validates the exact projection.
- React renders it.
- React stores only UI selection and draft state.
- Full output, full patch, and full artifact content are fetched by ref.
- No raw `RunEvent` drives the main canvas.

The canonical evidence types:

```ts
type EvidenceRefId = string

type EvidenceRefSummary = {
  id: EvidenceRefId
  kind: 'commandOutput' | 'diffPatch' | 'artifactContent'
  contentType: string
  byteLength: number
  truncated: boolean
  redactionState: 'clean' | 'redacted' | 'withheld'
  sourceEventRefs: ConversationEventRef[]
}

type DecisionLifetime = 'once' | 'run' | 'session' | 'persisted'

type DecisionMatcherSummary = {
  kind:
    | 'exactCommand'
    | 'exactArgs'
    | 'toolName'
    | 'category'
    | 'pathPrefix'
    | 'globPattern'
    | 'executeCodeScript'
    | 'any'
  label: string
}

type DecisionOption = {
  // Opaque backend-issued option id. React never derives this value.
  id: string
  decision: 'approve' | 'deny'
  label: string
  lifetime: DecisionLifetime
  matcher: DecisionMatcherSummary
  requiresConfirmation: boolean
}

type DecisionRequestState = {
  id: string
  requestId: string
  toolUseId?: string
  status: 'pending' | 'submitting' | 'approved' | 'denied' | 'failed'
  operation: 'read' | 'write' | 'execute' | 'network' | 'mcp' | 'artifact' | 'git' | 'unknown'
  target: {
    kind: 'file' | 'directory' | 'command' | 'url' | 'mcpTool' | 'artifact' | 'gitRef' | 'workspace' | 'unknown'
    label: string
    secondaryLabel?: string
  }
  riskLevel: 'low' | 'medium' | 'high' | 'critical'
  reason: string
  policy: {
    mode: string
    rule?: string
    sandbox?: string
  }
  decisionOptions: DecisionOption[]
  evidenceRefs: ConversationEventRef[]
  dataExposure: {
    sendsWorkspaceData: boolean
    sendsNetworkData: boolean
    touchesPrivatePath: boolean
    secretRisk: 'none' | 'redacted' | 'blocked'
  }
  confirmation?: {
    expectedText: string
    label: string
  }
}

type ToolAttempt = {
  id: string
  order: number
  toolUseId: string
  toolName: string
  origin: 'builtin' | 'mcp' | 'plugin' | 'app' | 'provider' | 'unknown'
  status: 'queued' | 'waitingPermission' | 'running' | 'completed' | 'failed' | 'denied'
  argumentsPreview?: string
  outputSummary?: string
  affectedTargets: string[]
  startedAt?: string
  endedAt?: string
  durationMs?: number
  retryOf?: string
  failurePhase?: 'validation' | 'permission' | 'execution' | 'transport' | 'projection'
  failureSummary?: string
  permission?: DecisionRequestState
  eventRefs: ConversationEventRef[]
}

type CommandExecution = {
  command: string
  cwd?: string
  shell?: string
  sandbox?: string
  approvalRequestId?: string
  exitCode?: number
  durationMs?: number
  stdoutPreview?: string
  stderrPreview?: string
  fullOutputRef?: EvidenceRefId
  truncated: boolean
  redactionState: 'clean' | 'redacted' | 'withheld'
  riskLevel: 'low' | 'medium' | 'high' | 'critical'
}

type ChangeSet = {
  id: string
  summary: string
  files: Array<{
    path: string
    oldPath?: string
    status: 'added' | 'modified' | 'deleted' | 'renamed'
    addedLines: number
    removedLines: number
    preview?: string
    fullPatchRef?: EvidenceRefId
    riskFlags: Array<'delete' | 'chmod' | 'binary' | 'large' | 'generated'>
  }>
}

type ArtifactRevisionSummary = {
  artifactId: string
  revisionId: string
  kind: 'code' | 'document' | 'image' | 'html' | 'data' | 'media' | 'file'
  status: 'pending' | 'running' | 'ready' | 'failed'
  sourceRunId: string
  title: string
  summary?: string
  previewRef?: string
  contentRef?: EvidenceRefId
  media?: ArtifactMediaPreview
}
```

Do not invent alternative field names during implementation. If Rust existing names differ internally, map them to this UI-facing projection.

Rust serde boundary naming:

- TypeScript and Zod use `fullOutputRef`, `fullPatchRef`, and `contentRef`.
- Rust structs use `full_output_ref`, `full_patch_ref`, and `content_ref`.
- Tauri `invoke(...)` arguments use camelCase because command handlers use `#[tauri::command(rename_all = "camelCase")]`: `fullOutputRef`, `fullPatchRef`, `contentRef`, `revisionId`, and `optionId`.
- Rust command handler parameters and Rust request structs remain snake_case internally.

Permission decision ownership:

- Rust projects `decisionOptions` from backend policy state.
- Replace the current event-level `presented_options: Vec<Decision>` shape with backend-authored selectable options. The public event contract must expose `presented_options: Vec<PermissionDecisionOption>`, not bare `Decision` values.
- Add an opaque backend option id type for selectable permission options. Do not reuse `DecisionId`: `PermissionResolvedEvent.decision_id` is the resolved/persisted decision id, not the UI-selectable option id.
- `PermissionDecisionOption` must include `option_id`, `decision`, `scope`, `lifetime`, `matcher_summary`, `label`, `requires_confirmation`, `action_plan_hash`, and optional `fingerprint`.
- The option id must be minted by Rust when the pending request is created. It must bind to the request id, action plan hash, scope, decision, and fingerprint. It must be stable only for that pending request and invalid after timeout, resolution, restart without pending state, or request mismatch.
- The projector maps backend `option_id` to UI `DecisionOption.id`. React never constructs, hashes, indexes, or guesses an option id.
- `DecisionScope` remains a backend matcher concept: exact command, exact args, tool name, category, path prefix, glob, code script, or any.
- UI lifetime labels are display data derived by Rust, not policy rules invented by React.
- React submits only `requestId`, `decision`, backend-issued `optionId`, and optional `confirmationText`.
- React must never submit matcher internals, policy fields, sandbox state, risk level, or data exposure as authority.
- `ResolvePermissionRequest` must include `option_id` at the Rust serde boundary and `optionId` at the TypeScript/Tauri boundary.
- Rust must resolve `(conversation_id, request_id, option_id)` against the still-pending backend-authored decision option. Missing, stale, mismatched, already-resolved, or unauthorized options fail closed.
- Rust must not map frontend `approve` directly to `Decision::AllowOnce` or `deny` directly to `Decision::DenyOnce`. The real `Decision` is derived only from the selected backend option.
- The submitted `decision: approve | deny` is a consistency check against the selected backend option category. It is not authority. If it conflicts with the selected option, fail closed.

Evidence ref ownership:

- `EvidenceRefId` is an opaque id minted only by Rust.
- `crates/jyowo-harness-journal` owns an `EvidenceRefStore` registry that maps refs to kind, conversation id, run id, source event refs, optional artifact id/revision id, redaction state, content type, byte length, content hash, and backing blob or journal source.
- The durable registry storage is a journal-owned read-model table, not `BlobMeta` and not an in-memory map. Implement the table in the same persistence family as `SqliteConversationReadModelStore` and expose a test-only in-memory implementation only from test support.
- The registry row is the authority. `BlobStore` stores bytes and generic blob metadata only. Blob metadata alone must never authorize evidence reads.
- Write order is blob-or-journal-source first, then registry row. If registry write fails after blob write, delete the newly written blob before returning the error. If cleanup fails, do not mint a ref.
- Read order is registry row first, then source validation. Validate conversation ownership, kind, retention, source event refs, redaction provenance, byte length, and content hash before returning content.
- The projector may include only `EvidenceRefSummary` or opaque `EvidenceRefId` values in `ConversationTurn`.
- Full command output, full diff patch, and artifact content are read only through SDK/Tauri commands that validate conversation ownership, ref kind, visibility, redaction state, and retention before returning bytes.
- Evidence refs are retained with their owning conversation artifacts, journal evidence, or blob records. Conversation deletion must make refs unreadable.
- Redaction provenance is part of the registry metadata. A command must fail closed if the registry cannot prove the content was redacted or explicitly safe before exposure.
- GC must treat registry rows as live references for blobs. Deleting or pruning a conversation deletes or invalidates all rows for that conversation before refs can be read again.

## File Map

Backend contracts:

- Modify `crates/jyowo-harness-contracts/src/ids.rs` if a new typed permission option id is introduced there
- Modify `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify `crates/jyowo-harness-contracts/src/events/permission.rs`
- Modify `crates/jyowo-harness-contracts/src/events/artifact.rs`
- Modify `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify `crates/jyowo-harness-contracts/tests/core_contracts.rs`
- Create `crates/jyowo-harness-contracts/tests/conversation_workbench_contract.rs`
- Modify `crates/jyowo-harness-contracts/tests/fixtures/conversation_worktree_page.json`

Backend projection and read model:

- Create `crates/jyowo-harness-journal/src/evidence.rs`
- Modify `crates/jyowo-harness-journal/src/lib.rs`
- Modify `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify `crates/jyowo-harness-journal/src/retention.rs`
- Modify `crates/jyowo-harness-journal/src/store.rs`
- Modify `crates/jyowo-harness-journal/src/sqlite.rs`
- Modify `crates/jyowo-harness-journal/src/jsonl.rs`
- Modify `crates/jyowo-harness-journal/src/memory.rs`
- Create `crates/jyowo-harness-journal/tests/evidence_ref_store.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
- Create `crates/jyowo-harness-journal/tests/conversation_workbench_projection.rs`
- Modify `crates/jyowo-harness-journal/tests/l1b_stores.rs`
- Modify `crates/jyowo-harness-journal/tests/contract.rs`

SDK facade:

- Modify `crates/jyowo-harness-sdk/src/harness/read_model.rs`
- Modify `crates/jyowo-harness-sdk/src/harness/accessors.rs`
- Modify `crates/jyowo-harness-sdk/src/harness.rs`
- Create `crates/jyowo-harness-sdk/tests/evidence_refs.rs`

Tauri boundary:

- Modify `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify `apps/desktop/src-tauri/src/commands/artifacts.rs`
- Modify `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify `apps/desktop/src-tauri/src/lib.rs`
- Modify `apps/desktop/src-tauri/src/commands/tests.rs`
- Modify `apps/desktop/src-tauri/tests/commands.rs`
- Modify `apps/desktop/src-tauri/tests/commands/permissions.rs`
- Create `apps/desktop/src-tauri/tests/commands/artifact_evidence.rs`

Frontend shared boundary:

- Modify `apps/desktop/src/shared/tauri/commands.ts`
- Modify `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify `apps/desktop/src/shared/state/ui-store.ts`
- Reuse `apps/desktop/src/shared/ui/command-menu.tsx` if it already satisfies combobox needs; otherwise create a local feature wrapper under `features/conversation`.

Frontend conversation and workbench:

- Modify `apps/desktop/src/app/shell/AppShell.tsx`
- Modify `apps/desktop/src/app/shell/AppShell.test.tsx`
- Modify `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-store.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-source.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/assistant-work-view.tsx`
- Delete `apps/desktop/src/features/conversation/timeline/thinking-panel.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.render.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.permission.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.artifacts.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.redaction.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-store.test.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-source.test.ts`
- Modify `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.test.tsx`
- Create `apps/desktop/src/features/workbench/workbench-state.ts`
- Create `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
- Create `apps/desktop/src/features/workbench/WorkbenchInspector.test.tsx`
- Create `apps/desktop/src/features/workbench/WorkbenchInspector.stories.tsx`

Frontend evidence components:

- Replace `apps/desktop/src/features/conversation/timeline/permission-inline-panel.tsx` with `apps/desktop/src/features/conversation/evidence/DecisionPanel.tsx`
- Replace command evidence in `apps/desktop/src/features/conversation/timeline/command-evidence-block.tsx`
- Replace diff evidence in `apps/desktop/src/features/conversation/timeline/diff-evidence-block.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/tool-attempt-row.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/tool-group-segment-view.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/tool-evidence-summary.tsx`
- Create `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.tsx`
- Create `apps/desktop/src/features/conversation/evidence/CommandExecutionView.tsx`
- Create `apps/desktop/src/features/conversation/evidence/ChangeSetSummary.tsx`
- Create `apps/desktop/src/features/conversation/evidence/DiffPane.tsx`
- Create `apps/desktop/src/features/conversation/evidence/EvidenceInspector.tsx`

Frontend artifact workspace:

- Modify `apps/desktop/src/features/conversation/timeline/artifact-segment-view.tsx`
- Modify `apps/desktop/src/features/artifacts/ArtifactPreview.tsx`
- Modify `apps/desktop/src/features/artifacts/ArtifactHistory.tsx`
- Create `apps/desktop/src/features/artifacts/ArtifactPane.tsx`
- Create `apps/desktop/src/features/artifacts/ArtifactPane.test.tsx`
- Create `apps/desktop/src/features/artifacts/ArtifactPane.stories.tsx`

Frontend composer:

- Split `apps/desktop/src/features/conversation/Composer.tsx`
- Create `apps/desktop/src/features/conversation/composer/ComposerEditor.tsx`
- Create `apps/desktop/src/features/conversation/composer/ComposerToolbar.tsx`
- Create `apps/desktop/src/features/conversation/composer/ReferenceCombobox.tsx`
- Create `apps/desktop/src/features/conversation/composer/SlashCommandMenu.tsx`
- Create `apps/desktop/src/features/conversation/composer/composer-draft-store.ts`
- Modify `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify `apps/desktop/src/features/conversation/Composer.stories.tsx`

Docs:

- Modify `docs/frontend/frontend-product-ux.md`
- Modify `docs/frontend/frontend-engineering.md`
- Modify `docs/backend/backend-runtime.md`
- Modify `docs/backend/backend-engineering.md`
- Modify `docs/testing/testing-strategy.md` only if the implementation adds new test categories or gates
- Do not create extra docs unless a gate requires it.

## Task 1: Replace The Conversation Projection Contract As A Compile-Safe Vertical Slice

**Files:**

- Modify `crates/jyowo-harness-contracts/src/ids.rs` if the permission option id is introduced in the shared id macro module
- Modify `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify `crates/jyowo-harness-contracts/src/events/permission.rs`
- Modify `crates/jyowo-harness-contracts/src/events/artifact.rs`
- Modify `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify `crates/jyowo-harness-contracts/tests/core_contracts.rs`
- Create `crates/jyowo-harness-contracts/tests/conversation_workbench_contract.rs`
- Modify `crates/jyowo-harness-contracts/tests/fixtures/conversation_worktree_page.json`
- Modify `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
- Modify `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify `apps/desktop/src-tauri/tests/commands/conversation_worktree.rs`
- Modify `apps/desktop/src-tauri/tests/commands/permissions.rs`
- Modify `apps/desktop/src/shared/tauri/commands.ts`
- Modify `apps/desktop/src/shared/tauri/commands.test.ts`

**Design requirement:** Replace the thin timeline projection with typed workbench projection across the public contract, Rust projector, Tauri response path, permission command contract, and frontend Zod boundary in one compiling vertical slice. Do not add a second legacy shape. Do not keep `AssistantSegment::Thinking`. Do not defer downstream compile fixes to a later task. This task defines optional evidence ref fields but does not mint readable refs yet; Task 3 owns real ref creation after the durable registry exists.

Mandatory split:

- **Task 1A contract slice:** change public Rust contracts, serde fixtures, schema export, artifact revision ids, and backend-issued permission option contracts. This slice must compile with contract tests before projector work starts.
- **Task 1B projector/read-model slice:** update `conversation_worktree_projector.rs` and `conversation_read_model.rs` to emit the new contract without React inference.
- **Task 1C Tauri/Zod boundary slice:** update Tauri command contracts, permission resolve payload, TS command client schemas, and frontend boundary tests.

Treat Task 1A, 1B, and 1C as separate audited tasks for the Mandatory Execution Protocol. Each slice needs its own Task Intent Check, failing tests, Reality Check, read-only subagent audit, and commit. Later references to "Task 1" mean all three slices are complete.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing Rust contract tests in `conversation_workbench_contract.rs`.

Required Rust contract tests:

```rust
#[test]
fn conversation_worktree_page_contains_typed_decision_tool_command_diff_and_artifact_shapes() {
    let page: ConversationWorktreePage =
        serde_json::from_str(include_str!("fixtures/conversation_worktree_page.json")).unwrap();
    let assistant = page.turns[0].assistant.as_ref().unwrap();
    assert!(assistant.projection_version > 0);

    let decision = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::ToolGroup(group) => group
                .attempts
                .iter()
                .find_map(|attempt| attempt.decision.as_ref()),
            _ => None,
        })
        .expect("fixture must include a backend-authored decision request");
    assert!(!decision.decision_options.is_empty());
    assert!(decision.decision_options.iter().all(|option| !option.id.as_str().is_empty()));
    assert!(decision
        .decision_options
        .iter()
        .all(|option| option.id.as_str() != "approve" && option.id.as_str() != "deny"));

    let command = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Process(process) => process.steps.iter().find_map(|step| {
                match step.detail.as_ref()? {
                    ProcessStepDetail::Command(command) => Some(command),
                    _ => None,
                }
            }),
            _ => None,
        })
        .expect("fixture must include command execution evidence");
    assert!(command.truncated);
    assert!(command.full_output_ref.is_none(), "Task 1 must not mint refs before EvidenceRefStore exists");

    let change_set = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Process(process) => process.steps.iter().find_map(|step| {
                match step.detail.as_ref()? {
                    ProcessStepDetail::Diff(change_set) => Some(change_set),
                    _ => None,
                }
            }),
            _ => None,
        })
        .expect("fixture must include changeset evidence");
    assert!(change_set.files.iter().all(|file| file.full_patch_ref.is_none()));

    let artifact = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Artifact(artifact) => Some(&artifact.revision),
            _ => None,
        })
        .expect("fixture must include an artifact revision summary");
    assert!(!artifact.revision_id.as_str().is_empty());
}

#[test]
fn conversation_worktree_page_rejects_legacy_thinking_segment() {
    let raw = r#"{
      "turns":[{
        "id":"turn-1","conversationId":"conversation-1","position":1,
        "user":{"id":"user-1","messageId":"message-1","body":"hi","timestamp":"1970-01-01T00:00:00Z"},
        "assistant":{"id":"assistant-1","runId":"run-1","status":"running","projectionVersion":1,"segments":[
          {"kind":"thinking","id":"thinking-1","order":0,"status":"running","summary":{"text":"raw thought"}}
        ]}
      }],
      "pageCursor":null,"eventCursor":null,"hasMoreBefore":false,"hasMoreAfter":false,"gap":false
    }"#;
    assert!(serde_json::from_str::<ConversationWorktreePage>(raw).is_err());
}
```

The rejection test must be JSON-deserialization-only. Do not reference `AssistantSegment::Thinking` in Rust code after removing the enum variant.

- [ ] Step 4: Add failing Zod tests in `apps/desktop/src/shared/tauri/commands.test.ts`.

Required checks:

```ts
it('parses the workbench projection fixture with typed evidence', () => {
  const page = pageConversationWorktreeResponseSchema.parse(workbenchProjectionFixture)
  expect(page.turns[0]?.assistant?.projectionVersion).toBeGreaterThan(0)
})

it('rejects legacy thinking segments in the conversation canvas projection', () => {
  expect(() => pageConversationWorktreeResponseSchema.parse(legacyThinkingFixture)).toThrow()
})
```

The Zod test must also traverse the parsed typed object and assert:

- at least one backend-authored decision has non-empty `decisionOptions`
- at least one command execution exists and its `fullOutputRef` is absent in Task 1
- at least one changeset exists and every `fullPatchRef` is absent in Task 1
- at least one artifact revision exists and has non-empty `revisionId`

- [ ] Step 5: Replace Rust projection structs.

Required Rust-facing changes:

- add `projection_version: u64` and `stream_version: u64` to `AssistantWork`
- remove `AssistantSegment::Thinking`
- replace `ToolPermissionState` with `DecisionRequestState`, `DecisionOption`, `DecisionLifetime`, and `DecisionMatcherSummary`
- replace `PermissionRequestedEvent.presented_options: Vec<Decision>` with `Vec<PermissionDecisionOption>`; do not project selectable options from array index or decision strings
- add an opaque backend permission option id and expose it as `DecisionOption.id`
- keep `PermissionResolvedEvent.decision_id` separate from permission option ids
- expand `ToolAttempt` exactly per target design
- replace command detail fields with `CommandExecution`
- replace diff detail with `ChangeSet`
- expand artifact segment with `ArtifactRevisionSummary`
- add required `revision_id` to artifact created/updated event contracts and ensure artifact projection uses it
- add `EvidenceRefId` and `EvidenceRefSummary` contract types without adding fetch commands yet
- add `UiVisibility` to process steps and force user-safe or withheld rendering
- add `option_id` to `ResolvePermissionRequest`, Tauri command handler arguments, TS request schema, and TS command client request type
- update pending permission state and resolver APIs so resolution uses backend-authored `(conversation_id, request_id, option_id)` state instead of hardcoded approve/deny to `AllowOnce`/`DenyOnce`
- reject missing, stale, already-resolved, request-mismatched, conversation-mismatched, and decision-category-mismatched option ids

Artifact revision migration:

- introduce a required `ArtifactRevisionId`/`revision_id` contract field; do not support both missing and present revision ids in the projector
- when creating or updating artifacts, mint a new opaque revision id in Rust at event creation time
- update every artifact event fixture and contract test to include real revision ids
- if existing local dev journals must be readable, normalize them through the existing event-version migration layer before projection; the projector must only see normalized events with required `revision_id`
- do not synthesize revision ids inside React, Tauri response mapping, or artifact UI code

- [ ] Step 6: Update the Rust projector, read model, and Tauri worktree command tests in this same task.

Required downstream migration:

- `conversation_worktree_projector.rs` must compile against the new contract.
- Existing projected permission data must map to backend-authored `decisionOptions`; React must not invent decision lifetime or matcher options.
- `decisionOptions` must come from `PermissionRequestedEvent.presented_options: Vec<PermissionDecisionOption>` or pending backend state carrying the same option ids. It must not come from `Vec<Decision>` order, option labels, or frontend-derived hashes.
- Existing command and diff projection must populate previews and leave `fullOutputRef`, `fullPatchRef`, and `contentRef` absent until Task 3 creates a readable registry entry.
- Existing artifact projection must emit `ArtifactRevisionSummary` with event-backed `revision_id`.
- Existing thinking projection must become user-safe `ProcessStep` or withheld process state. It must not serialize `kind: "thinking"`.
- `conversation_read_model.rs` must keep complete-turn paging semantics and compile with new cursor and segment shapes.
- Tauri worktree command tests must parse the new serde shape.
- Tauri permission command tests must prove that valid `optionId` resolves the backend-authored decision and missing, invalid, mismatched, stale, already-resolved, and decision-category-conflicting `optionId` fails closed.

- [ ] Step 7: Replace TypeScript Zod schemas in `commands.ts` with the same shape.
- [ ] Step 8: Update the fixture JSON to the new shape.
- [ ] Step 9: Run failing tests and confirm they fail before implementation, then implement until they pass.

Commands:

```bash
cargo test -p jyowo-harness-contracts conversation_workbench --test conversation_workbench_contract
cargo test -p jyowo-harness-journal conversation_worktree_projector --test conversation_worktree_projector
cargo test -p jyowo-harness-journal conversation_read_model --test conversation_read_model
cargo test -p jyowo-desktop-shell conversation_worktree
cargo test -p jyowo-desktop-shell permissions
pnpm -C apps/desktop test -- src/shared/tauri/commands.test.ts
cargo fmt --all --check
cargo check --workspace
pnpm check:desktop
git diff --check
```

- [ ] Step 10: Run Task Reality Check.
- [ ] Step 11: Run read-only subagent audit.
- [ ] Step 12: Commit `refactor: replace conversation projection contract`.

## Task 2: Rebuild Rust Worktree Projection

**Files:**

- Modify `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
- Create `crates/jyowo-harness-journal/tests/conversation_workbench_projection.rs`

**Design requirement:** The projector emits the target workbench model directly. It must not rely on React to infer permission, tool, command, diff, artifact, or reasoning safety from raw event payloads.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing projector tests.

Required behaviors:

- `PermissionRequestedEvent` projects to `DecisionRequestState` with operation, target, risk, reason, policy, backend-issued decision options, data exposure, confirmation, and evidence refs.
- `ToolUseRequestedEvent` projects to `ToolAttempt.argumentsPreview` only after redaction and size limiting.
- `ToolUseCompletedEvent` fills `outputSummary`, `durationMs`, `endedAt`, and affected targets.
- command evidence includes command, cwd when available, sandbox, approval request id, exit code, duration, previews, truncation, redaction state, and risk.
- diff evidence emits `ChangeSet` with file statuses and full patch refs when available.
- artifact lifecycle emits revision summaries.
- raw thinking is not projected. User-safe reasoning appears only as `ProcessStep` with `visibility: userSafe`; withheld reasoning appears as `visibility: withheld`.
- `projection_version` and `stream_version` increase deterministically from event cursor or sequence.

- [ ] Step 4: Implement projection helpers.

Required helper boundaries:

- `project_decision_request(...) -> DecisionRequestState`
- `project_tool_attempt_preview(...) -> ToolAttempt`
- `project_command_execution(...) -> CommandExecution`
- `project_change_set(...) -> ChangeSet`
- `project_artifact_revision(...) -> ArtifactRevisionSummary`
- `project_user_safe_process_step(...) -> ProcessStep`

Helpers may live in `conversation_worktree_projector.rs` first. Split only if the file becomes harder to review.

- [ ] Step 5: Update paging metadata in `conversation_read_model.rs`.

Required:

- `page_worktree` returns complete turns
- `page_cursor` remains turn-based
- `event_cursor` remains event-based
- `gap` is true only when projection cannot guarantee continuity
- no full output or full patch is embedded in turns

- [ ] Step 6: Run gates.

```bash
cargo test -p jyowo-harness-journal conversation_workbench_projection --test conversation_workbench_projection
cargo test -p jyowo-harness-journal conversation_worktree_projector --test conversation_worktree_projector
cargo test -p jyowo-harness-journal conversation_read_model --test conversation_read_model
cargo fmt --all --check
cargo check --workspace
git diff --check
```

- [ ] Step 7: Run Task Reality Check.
- [ ] Step 8: Run read-only subagent audit.
- [ ] Step 9: Commit `refactor: rebuild conversation worktree projector`.

## Task 3: Add Evidence Ref Registry And SDK Access

**Files:**

- Create `crates/jyowo-harness-journal/src/evidence.rs`
- Modify `crates/jyowo-harness-journal/src/lib.rs`
- Modify `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify `crates/jyowo-harness-journal/src/retention.rs`
- Modify `crates/jyowo-harness-journal/src/store.rs`
- Modify `crates/jyowo-harness-journal/src/sqlite.rs`
- Modify `crates/jyowo-harness-journal/src/jsonl.rs`
- Modify `crates/jyowo-harness-journal/src/memory.rs`
- Create `crates/jyowo-harness-journal/tests/evidence_ref_store.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/tests/l1b_stores.rs`
- Modify `crates/jyowo-harness-journal/tests/contract.rs`
- Modify `crates/jyowo-harness-sdk/src/harness/read_model.rs`
- Modify `crates/jyowo-harness-sdk/src/harness/accessors.rs`
- Modify `crates/jyowo-harness-sdk/src/harness.rs`
- Create `crates/jyowo-harness-sdk/tests/evidence_refs.rs`

**Design requirement:** Evidence refs are durable, conversation-scoped backend references. They are minted by Rust only after redaction and ownership metadata are known. React must never construct, mutate, or authorize an evidence ref.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing registry tests.

Required behaviors:

- ref ids are opaque and cannot be derived from path, command text, artifact id, or event id alone
- every registry entry stores `kind`, `conversation_id`, `run_id`, source event refs, content type, byte length, content hash, retention, redaction state, redaction provenance, and source location
- command-output refs include execution event refs and may point to a redacted blob or redacted journal payload
- diff-patch refs include the change set id and file identity
- artifact-content refs include `artifact_id` and `revision_id`
- owner mismatch fails closed
- kind mismatch fails closed
- missing retention or missing redaction provenance fails closed
- conversation deletion or journal/blob deletion makes the ref unreadable
- ref lookup after process restart still succeeds for persisted refs
- orphan blob created during failed registry write is removed before returning the error
- blob GC keeps blobs that are referenced by live registry rows
- journal prune and conversation deletion delete or invalidate registry rows before any ref can be read again
- registry rows do not make deleted conversations readable again

- [ ] Step 4: Implement `EvidenceRefStore`.

Required Rust API shape:

```rust
pub struct EvidenceRefStore {
    registry: Arc<dyn EvidenceRefRegistry>,
    blob_store: Arc<dyn BlobStore>,
    event_store: Arc<dyn EventStore>,
}

#[async_trait::async_trait]
pub trait EvidenceRefRegistry: Send + Sync + 'static {
    async fn insert(&self, tenant: TenantId, record: EvidenceRefRecord) -> Result<(), JournalError>;
    async fn get(&self, tenant: TenantId, id: &EvidenceRefId) -> Result<Option<EvidenceRefRecord>, JournalError>;
    async fn delete_for_conversation(&self, tenant: TenantId, conversation_id: &str) -> Result<(), JournalError>;
    async fn list_live_blob_roots(&self, tenant: TenantId) -> Result<Vec<BlobRef>, JournalError>;
}

pub enum EvidenceRefKind {
    CommandOutput,
    DiffPatch,
    ArtifactContent,
}

pub enum EvidenceRedactionState {
    Clean,
    Redacted,
    Withheld,
}

pub enum EvidenceRefSource {
    Blob { blob_ref: BlobRef },
    JournalPayload { event_ref: ConversationEventRef, json_pointer: String },
}

pub struct EvidenceRefRecord {
    pub id: EvidenceRefId,
    pub kind: EvidenceRefKind,
    pub conversation_id: String,
    pub run_id: String,
    pub source_event_refs: Vec<ConversationEventRef>,
    pub artifact_id: Option<String>,
    pub revision_id: Option<String>,
    pub content_type: String,
    pub byte_length: u64,
    pub content_hash: Vec<u8>,
    pub redaction_state: EvidenceRedactionState,
    pub redaction_provenance: RedactionProvenance,
    pub retention: BlobRetention,
    pub source: EvidenceRefSource,
}
```

Persistence design:

- Store registry records in a journal-owned durable table named `evidence_refs` in the same database lifecycle as the conversation read model.
- Do not extend `BlobMeta` to carry conversation ownership, policy, or redaction authority.
- Do not use an in-memory map for production refs. Test-only in-memory storage may live in test support.
- Primary key is `(tenant_id, evidence_ref_id)`.
- Required indexes: `(tenant_id, conversation_id)`, `(tenant_id, conversation_id, kind)`, and `(tenant_id, artifact_id, revision_id)`.
- Registry writes are idempotent for the same source hash and fail on conflicting metadata for the same ref id.
- Blob-backed writes store bytes in `BlobStore` first, then insert the registry row. If the row insert fails, delete the just-created blob before returning an error.
- Journal-backed refs store only source event refs and JSON pointer metadata. Reads must re-load the journal payload and re-check the hash.
- Conversation deletion, journal prune, or artifact deletion must delete or invalidate matching registry rows before any ref can be read again.
- GC must treat live registry blob refs as roots. A blob referenced by a registry row cannot be collected.
- Wire registry roots into the existing retention path. `RetentionEnforcer::collect_garbage(...)` must receive live blob ids from `EvidenceRefRegistry::list_live_blob_roots(...)` before deleting from `FileBlobStore`.
- Wire registry invalidation into the existing journal prune/delete paths in `store.rs`, `sqlite.rs`, `jsonl.rs`, and `memory.rs`. If a store implementation cannot support durable invalidation, evidence reads for affected refs must fail closed.
- Test the SQLite-backed registry across process restart. Test-only in-memory registry may be used only for non-persistence unit tests.
- Use existing workspace `async-trait`. Do not add a new async abstraction or a second registry trait unless the Task Intent Check proves it removes object-safety risk.

- [ ] Step 5: Add SDK read methods that resolve evidence refs through the registry.

Required:

- SDK verifies conversation ownership before reading bytes
- SDK verifies requested kind matches the command being served
- SDK returns safe typed content, not raw `serde_json::Value`
- SDK errors use existing safe command error mapping
- SDK validates source hash and byte length after reading source bytes
- SDK refuses `EvidenceRedactionState::Withheld` unless the command response shape is explicitly a withheld marker with no bytes

- [ ] Step 6: Wire projector ref minting.

Required:

- `fullOutputRef` is present only when the registry has a readable redacted command output record
- `fullPatchRef` is present only when the registry has a readable redacted patch record
- `contentRef` is present only when the registry has a readable artifact revision record
- previews remain bounded and redacted
- no registry failure causes raw large content to be embedded in `ConversationTurn`

- [ ] Step 7: Run gates.

```bash
cargo test -p jyowo-harness-journal evidence_ref_store --test evidence_ref_store
cargo test -p jyowo-harness-journal conversation_worktree_projector --test conversation_worktree_projector
cargo test -p jyowo-harness-journal retention --test l1b_stores
cargo test -p jyowo-harness-journal prune --test contract
cargo test -p jyowo-harness-sdk evidence_refs --test evidence_refs
cargo fmt --all --check
cargo check --workspace
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: add evidence ref registry`.

## Task 4: Add Evidence Fetch Commands For Large Data

**Files:**

- Modify `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify `crates/jyowo-harness-journal/src/evidence.rs`
- Modify `crates/jyowo-harness-sdk/src/harness/read_model.rs`
- Modify `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify `apps/desktop/src-tauri/src/commands/artifacts.rs`
- Modify `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify `apps/desktop/src-tauri/src/lib.rs`
- Modify `apps/desktop/src/shared/tauri/commands.ts`
- Modify `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify `apps/desktop/src-tauri/tests/commands.rs`
- Modify `apps/desktop/src-tauri/tests/commands/conversation_worktree.rs`
- Create `apps/desktop/src-tauri/tests/commands/artifact_evidence.rs`

**Design requirement:** Large command output, full diff patches, and artifact content are fetched by ref. They are not embedded in `ConversationTurn`.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing tests for three commands:

```text
get_conversation_command_output(conversation_id, full_output_ref)
get_conversation_diff_patch(conversation_id, full_patch_ref)
get_artifact_revision_content(conversation_id, content_ref)
```

Required behavior:

- invalid refs fail closed
- refs must belong to the conversation
- command output Rust request struct uses `full_output_ref`; Tauri invoke args and frontend wrapper use `fullOutputRef`
- diff patch Rust request struct uses `full_patch_ref`; Tauri invoke args and frontend wrapper use `fullPatchRef`
- artifact revision content Rust request struct uses `content_ref`; Tauri invoke args and frontend wrapper use `contentRef`
- artifact content command validates the registry entry has kind `ArtifactContent` and matching conversation ownership before reading bytes
- artifact id and revision id may be returned in the response metadata, but they must not be used as the authority for content reads
- output is redacted before return
- response includes `truncated`, `redactionState`, and content type
- frontend Zod rejects oversized payloads

- [ ] Step 4: Implement Rust command contracts and command handlers.
- [ ] Step 5: Add TypeScript command client methods and Zod schemas.
- [ ] Step 6: Run gates.

```bash
cargo test -p jyowo-desktop-shell commands
cargo test -p jyowo-desktop-shell artifact_evidence
pnpm -C apps/desktop test -- src/shared/tauri/commands.test.ts
cargo fmt --all --check
cargo check --workspace
pnpm check:desktop
git diff --check
```

- [ ] Step 7: Run Task Reality Check.
- [ ] Step 8: Run read-only subagent audit.
- [ ] Step 9: Commit `refactor: add evidence fetch commands`.

## Task 5: Replace Timeline State With Paged Workbench State

**Files:**

- Modify `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-store.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-source.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-selectors.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-scroll-controller.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-store.test.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-source.test.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-selectors.test.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-scroll-controller.test.ts`
- Modify `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx`

**Design requirement:** State is page-aware and stream-version-aware. No optimistic turn is appended to the wrong page. No render path serializes large segments.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing reducer and hook tests.

Required tests:

- loading older pages prepends without losing scroll anchor
- loading newer pages appends without duplicating turns
- optimistic turn reconciles by `clientMessageId` regardless of loaded page
- gap event inserts a gap marker and refresh action
- `stream_version` updates scroll tick without `JSON.stringify`
- large output ref does not cause timeline render to stringify segment data

- [ ] Step 4: Replace state shape.

Required state model:

```ts
type ConversationTimelineState = {
  conversationId: string
  pages: Array<{
    cursor: ConversationTurnCursor | null
    turns: ConversationTurn[]
  }>
  loadedRange: {
    first?: ConversationTurnCursor
    last?: ConversationTurnCursor
  }
  hasMoreBefore: boolean
  hasMoreAfter: boolean
  gapMarkers: Array<{ id: string; afterCursor: ConversationCursor | null }>
  eventCursor: ConversationCursor | null
  optimisticTurnsByClientMessageId: Record<string, ConversationTurn>
  activeRunIds: string[]
  refreshRequests: number
  immediateRefreshRequests: number
}
```

- [ ] Step 5: Add actions:

```ts
hydrateInitialPage
prependPage
appendPage
markGap
retryGap
localSubmit
commandAccepted
commandFailed
permissionSubmitting
permissionSubmitFailed
worktreeRefreshRequested
```

- [ ] Step 6: Update `useConversationTimeline` to expose `loadEarlier`, `loadLater`, `retryGap`, and version-driven scroll tick.
- [ ] Step 7: Run gates.

```bash
pnpm -C apps/desktop test -- src/features/conversation/timeline/conversation-timeline-store.test.ts
pnpm -C apps/desktop test -- src/features/conversation/timeline/use-conversation-timeline.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/timeline/conversation-timeline.large-output.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: replace timeline state with paged workbench state`.

## Task 6: Build Workbench Shell And Inspector State

**Files:**

- Modify `apps/desktop/src/app/shell/AppShell.tsx`
- Modify `apps/desktop/src/app/shell/AppShell.test.tsx`
- Create `apps/desktop/src/shared/state/workbench-selection.ts`
- Modify `apps/desktop/src/shared/state/ui-store.ts`
- Create `apps/desktop/src/features/workbench/workbench-state.ts`
- Create `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
- Create `apps/desktop/src/features/workbench/WorkbenchInspector.test.tsx`
- Create `apps/desktop/src/features/workbench/WorkbenchInspector.stories.tsx`
- Modify `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`

**Design requirement:** The right side becomes a real inspector with Context, Decision, Evidence, Diff, Artifact, and Terminal panes. Timeline cards select evidence; they do not contain every full workflow action.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing component tests.

Required tests:

- clicking a decision evidence card opens `Decision` pane
- clicking command evidence opens `Terminal` pane
- clicking diff summary opens `Diff` pane
- clicking artifact summary opens `Artifact` pane
- inspector state is conversation-scoped
- closed inspector does not lose selected evidence
- `shared/state/ui-store.ts` does not import from `features/workbench`

- [ ] Step 4: Implement `WorkbenchSelection`.

Required shape:

```ts
type WorkbenchSelection =
  | { kind: 'context' }
  | { kind: 'decision'; conversationId: string; requestId: string }
  | { kind: 'tool'; conversationId: string; toolUseId: string }
  | { kind: 'command'; conversationId: string; fullOutputRef?: EvidenceRefId; eventRef?: ConversationEventRef }
  | { kind: 'diff'; conversationId: string; changeSetId: string }
  | { kind: 'artifact'; conversationId: string; artifactId: string; revisionId?: string }
```

- [ ] Step 5: Place `WorkbenchSelection` and related literal pane types in `apps/desktop/src/shared/state/workbench-selection.ts`.
- [ ] Step 6: Keep `shared/state/ui-store.ts` limited to shell layout and local UI selection. It may import `shared/state/workbench-selection.ts`, but it must not import `features/workbench`.
- [ ] Step 7: Use `apps/desktop/src/features/workbench/workbench-state.ts` only for feature hooks/selectors that import shared state. Do not store backend data in Zustand.
- [ ] Step 8: Replace disabled more-actions placeholder with useful layout controls.
- [ ] Step 9: Keep React state local to UI selection. Do not store policy decisions in UI store.
- [ ] Step 10: Run gates.

```bash
pnpm -C apps/desktop test -- src/app/shell/AppShell.test.tsx
pnpm -C apps/desktop test -- src/features/workbench/WorkbenchInspector.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 11: Run Task Reality Check.
- [ ] Step 12: Run read-only subagent audit.
- [ ] Step 13: Commit `refactor: add workbench inspector shell`.

## Task 7: Replace Permission UI With DecisionPanel

**Files:**

- Delete `apps/desktop/src/features/conversation/timeline/permission-inline-panel.tsx`
- Create `apps/desktop/src/features/conversation/evidence/DecisionPanel.tsx`
- Create `apps/desktop/src/features/conversation/evidence/DecisionPanel.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/tool-attempt-row.tsx`
- Modify `apps/desktop/src/features/context/ContextPanel.tsx`
- Modify `apps/desktop/src/features/context/ContextPanel.test.tsx`
- Modify `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
- Modify `apps/desktop/src/features/workbench/WorkbenchInspector.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.permission.test.tsx`

**Design requirement:** Permissions are user-visible decisions with operation, target, risk, reason, policy, evidence, data exposure, and backend-issued decision options. Buttons must name the action. React must not own policy, matcher, or sandbox authority.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing tests.

Required tests:

- pending write request shows target, risk, reason, policy, data exposure, decision lifetime labels, and matcher summaries from backend-issued options
- high-risk request requires exact confirmation text
- approve button text includes operation and backend-provided option label
- selecting an approval option submits exactly that backend-provided `optionId`
- deny button remains available without confirmation
- submitted state disables duplicate actions
- failed submission keeps request visible with retry-safe state
- panel uses `aria-live` for state changes

- [ ] Step 4: Implement `DecisionPanel`.
- [ ] Step 5: Wire `ToolAttemptRow` and `WorkbenchInspector` to the same component.
- [ ] Step 6: Ensure `resolvePermission` receives only request id, decision, backend-issued `optionId`, and confirmation text. React must not send policy fields, matcher internals, sandbox state, risk level, or data exposure.
- [ ] Step 7: Run gates.

```bash
pnpm -C apps/desktop test -- src/features/conversation/evidence/DecisionPanel.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/timeline/conversation-timeline.permission.test.tsx
pnpm -C apps/desktop test -- src/features/context/ContextPanel.test.tsx
pnpm -C apps/desktop test -- src/features/workbench/WorkbenchInspector.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: replace permission UI with decision panel`.

## Task 8: Implement Tool And Command Evidence Views

**Files:**

- Create `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.tsx`
- Create `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.test.tsx`
- Create `apps/desktop/src/features/conversation/evidence/CommandExecutionView.tsx`
- Create `apps/desktop/src/features/conversation/evidence/CommandExecutionView.test.tsx`
- Create `apps/desktop/src/features/conversation/evidence/EvidenceInspector.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/tool-attempt-row.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/tool-group-segment-view.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/tool-evidence-summary.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/command-evidence-block.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/process-step-row.tsx`

**Design requirement:** Timeline shows compact evidence. Inspector shows full audited details. Command output is fetched by ref when needed.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing tests.

Required tests:

- tool card shows origin, status, duration, arguments preview, output summary, affected targets
- failed tool shows failure phase
- command view shows cwd, shell, sandbox, approval request id, risk, exit, duration, redaction state
- copy command copies only the command
- copy visible output copies only visible output
- copy full output calls `getConversationCommandOutput`
- withheld output never appears in DOM
- focus ring is visible on every interactive control

- [ ] Step 4: Implement compact timeline cards.
- [ ] Step 5: Implement inspector detail views.
- [ ] Step 6: Delete any old command evidence behavior that copies only an ambiguous combined block.
- [ ] Step 7: Run gates.

```bash
pnpm -C apps/desktop test -- src/features/conversation/evidence/ToolInvocationCard.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/evidence/CommandExecutionView.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/timeline/conversation-timeline.render.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: implement tool and command evidence views`.

## Task 9: Implement ChangeSet Summary And Diff Pane

**Files:**

- Create `apps/desktop/src/features/conversation/evidence/ChangeSetSummary.tsx`
- Create `apps/desktop/src/features/conversation/evidence/ChangeSetSummary.test.tsx`
- Create `apps/desktop/src/features/conversation/evidence/DiffPane.tsx`
- Create `apps/desktop/src/features/conversation/evidence/DiffPane.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/diff-evidence-block.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/process-step-row.tsx`
- Modify `apps/desktop/src/features/conversation/DiffViewer.tsx`
- Modify `apps/desktop/src/features/conversation/DiffPreview.tsx`

**Design requirement:** Diff is a `ChangeSet`, not visible preview text. Timeline shows summary. Inspector handles full patch fetch, file selection, large diff truncation, and risk flags.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing tests.

Required tests:

- summary shows file count, added/removed totals, and risk flags
- file list shows added, modified, deleted, renamed
- full patch fetch uses `getConversationDiffPatch`
- visible copy copies visible hunk only
- full patch copy fetches full patch ref
- hidden lines have an explicit action
- binary or generated files do not try to syntax-highlight nonexistent text

- [ ] Step 4: Implement `ChangeSetSummary`.
- [ ] Step 5: Implement `DiffPane`.
- [ ] Step 6: Remove visible-lines-only copy as the default action.
- [ ] Step 7: Run gates.

```bash
pnpm -C apps/desktop test -- src/features/conversation/evidence/ChangeSetSummary.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/evidence/DiffPane.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/timeline/conversation-timeline.render.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: implement changeset diff pane`.

## Task 10: Implement Artifact Revision Workspace

**Files:**

- Modify `apps/desktop/src-tauri/src/commands/artifacts.rs`
- Modify `apps/desktop/src-tauri/tests/commands.rs`
- Create `apps/desktop/src-tauri/tests/commands/artifact_revision.rs`
- Modify `apps/desktop/src/shared/tauri/commands.ts`
- Modify `apps/desktop/src/features/conversation/timeline/artifact-segment-view.tsx`
- Modify `apps/desktop/src/features/artifacts/ArtifactPreview.tsx`
- Modify `apps/desktop/src/features/artifacts/ArtifactHistory.tsx`
- Create `apps/desktop/src/features/artifacts/ArtifactPane.tsx`
- Create `apps/desktop/src/features/artifacts/ArtifactPane.test.tsx`
- Create `apps/desktop/src/features/artifacts/ArtifactPane.stories.tsx`

**Design requirement:** Artifact is a versioned workspace entity. Timeline card opens the artifact pane. HTML and code previews are sandboxed. Updates create revisions instead of mutating user-visible history.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing tests.

Required tests:

- Rust command tests in `artifact_revision.rs` must use the `artifact_revision_` prefix so the cargo filter below cannot run zero newly owned tests.
- `apps/desktop/src-tauri/tests/commands.rs` must register `commands/artifact_revision.rs`.
- artifact pane consumes projected `revisionId` values from `ArtifactRevisionSummary`
- artifact pane lists revisions newest-first
- image preview uses media preview ref
- HTML preview uses sandboxed iframe with no same-origin privilege
- code/document/data preview fetches content by ref
- failed artifact shows error state without broken image
- copy/download actions use backend-provided content refs

- [ ] Step 4: Use the artifact revision contract and fetch command created in Tasks 1, 2, and 4. Do not add a second artifact history model.
- [ ] Step 5: Implement artifact revision lookup in the Tauri command layer only for metadata gaps. Artifact content bytes must be fetched through `get_artifact_revision_content(conversationId, contentRef)`.
- [ ] Step 6: Implement `ArtifactPane` and timeline open action.
- [ ] Step 7: Run gates.

```bash
cargo test -p jyowo-harness-contracts artifact --test core_contracts
cargo test -p jyowo-desktop-shell artifact_revision
cargo test -p jyowo-desktop-shell artifact_listing
cargo test -p jyowo-desktop-shell artifact_preview
cargo test -p jyowo-desktop-shell artifact_evidence
pnpm -C apps/desktop test -- src/features/artifacts/ArtifactPane.test.tsx
pnpm -C apps/desktop test -- src/features/artifacts/ArtifactPreview.test.tsx
cargo fmt --all --check
cargo check --workspace
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: add artifact revision workspace`.

## Task 11: Redesign Composer As A Command Input Surface

**Files:**

- Modify `apps/desktop/src/features/conversation/Composer.tsx`
- Create `apps/desktop/src/features/conversation/composer/ComposerEditor.tsx`
- Create `apps/desktop/src/features/conversation/composer/ComposerToolbar.tsx`
- Create `apps/desktop/src/features/conversation/composer/ReferenceCombobox.tsx`
- Create `apps/desktop/src/features/conversation/composer/SlashCommandMenu.tsx`
- Create `apps/desktop/src/features/conversation/composer/composer-draft-store.ts`
- Modify `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify `apps/desktop/src/features/conversation/Composer.stories.tsx`
- Modify `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`

**Design requirement:** Composer is a multi-line command input surface with slash commands, keyboard reference selection, draft persistence, accessible errors, and stable controls.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing tests.

Required tests:

- editor starts at 64px minimum height and grows up to 160px
- `Enter` submits
- `Shift+Enter` inserts newline
- IME composition never submits
- `/` opens slash command menu
- `@` opens reference combobox
- arrow keys and enter select references
- attachments and references render as removable chips
- error region uses `role="alert"` or `aria-live`
- draft persists per conversation and clears after successful submit
- all icon buttons have accessible names and visible focus styles

- [ ] Step 4: Split components along the file map.
- [ ] Step 5: Implement draft persistence without writing secrets to local storage. If draft contains secret-like text or private path, store only `[REDACTED]` or skip persistence.
- [ ] Step 6: Keep permission mode and model selector as secondary toolbar controls.
- [ ] Step 7: Run gates.

```bash
pnpm -C apps/desktop test -- src/features/conversation/Composer.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/ConversationWorkspace.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: redesign composer command input`.

## Task 12: Remove Legacy Timeline Components And Close Accessibility Gaps

**Files:**

- Delete `apps/desktop/src/features/conversation/timeline/thinking-panel.tsx`
- Confirm deleted `apps/desktop/src/features/conversation/timeline/permission-inline-panel.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/assistant-work-view.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/process-status-row.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/tool-evidence-summary.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/user-attachment-strip.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/artifact-segment-view.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`
- Create `apps/desktop/src/features/conversation/evidence/DecisionPanel.stories.tsx`
- Create `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.stories.tsx`
- Create `apps/desktop/src/features/conversation/evidence/CommandExecutionView.stories.tsx`
- Create `apps/desktop/src/features/conversation/evidence/DiffPane.stories.tsx`
- Create `apps/desktop/src/features/conversation/evidence/ChangeSetSummary.stories.tsx`
- Modify `apps/desktop/src/features/artifacts/ArtifactPane.stories.tsx` created by Task 10
- Modify `apps/desktop/src/features/workbench/WorkbenchInspector.stories.tsx` created by Task 6
- Modify `apps/desktop/src/features/conversation/Composer.stories.tsx`

**Design requirement:** Remove old compatibility UI. Finish focus, aria, image dimensions, and story coverage for the new surfaces.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add or update behavior tests. Storybook does not satisfy this step.

Required behavior tests:

```text
ConversationTimeline loaded / empty / error / gap / long history / streaming
DecisionPanel low / high / critical / submitting / failed
ToolInvocationCard running / completed / failed / permission
CommandExecutionView clean / redacted / withheld / failed
DiffPane small / large / binary / generated
ArtifactPane image / html / code / failed / multiple revisions
Composer ready / disabled / submitting / error / attachments / slash / references
WorkbenchInspector every pane
```

- [ ] Step 4: Add Storybook visual state matrices for the exact story files listed above.
- [ ] Step 5: Remove legacy UI paths and assert `apps/desktop/src/features/conversation/timeline/permission-inline-panel.tsx` is absent. Do not recreate it.
- [ ] Step 6: Add visible focus styles through existing shared button primitives or equivalent local classes.
- [ ] Step 7: Add width/height or stable aspect ratio for images and media.
- [ ] Step 8: Confirm semantic tokens are used. Terminal surfaces may use terminal tokens; do not hardcode arbitrary white/black outside that surface.
- [ ] Step 9: Run gates.

```bash
pnpm -C apps/desktop test -- src/features/conversation/timeline/conversation-timeline.render.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/timeline/conversation-timeline.artifacts.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/timeline/conversation-timeline.redaction.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/evidence/DecisionPanel.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/evidence/ToolInvocationCard.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/evidence/CommandExecutionView.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/evidence/ChangeSetSummary.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/evidence/DiffPane.test.tsx
pnpm -C apps/desktop test -- src/features/artifacts/ArtifactPane.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/Composer.test.tsx
pnpm -C apps/desktop test -- src/features/workbench/WorkbenchInspector.test.tsx
pnpm check:desktop
pnpm check:test-architecture
git diff --check
```

- [ ] Step 10: Run Task Reality Check.
- [ ] Step 11: Run read-only subagent audit.
- [ ] Step 12: Commit `refactor: remove legacy timeline UI`.

## Task 13: Update Product And Runtime Docs

**Files:**

- Modify `docs/frontend/frontend-product-ux.md`
- Modify `docs/frontend/frontend-engineering.md`
- Modify `docs/backend/backend-runtime.md`
- Modify `docs/backend/backend-engineering.md`
- Modify `docs/testing/testing-strategy.md` only if implementation adds new test categories, naming rules, or gates; otherwise state in the task response that no testing-strategy change was required.
- Modify `docs/testing/test-inventory.md` only by regenerating it with `pnpm audit:tests > docs/testing/test-inventory.md` when test files were added, removed, renamed, split, or when `pnpm check:testing-docs` reports an inventory mismatch.

**Design requirement:** Docs must match the implemented architecture. Do not create docs that describe future work. Do not write temporary plans into normative docs.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Compare implemented architecture against existing docs and list concrete normative gaps.
- [ ] Step 4: Update the normative docs for these implemented architecture changes:
  - workbench inspector ownership
  - typed evidence projection
  - `EvidenceRefStore` ownership, retention, and redaction provenance
  - evidence fetch command payload names and scope validation
  - backend-issued permission decision options
  - artifact revision workspace
  - no raw thinking in frontend state
  - large output refs instead of embedded payloads
  - frontend `shared` / `features` dependency boundaries for workbench state
- [ ] Step 5: Run gates.

```bash
pnpm audit:tests > docs/testing/test-inventory.md
pnpm check:testing-docs
pnpm check:docs
pnpm check:frontend-docs
pnpm check:backend-docs
git diff --check
```

- [ ] Step 6: Run Task Reality Check.
- [ ] Step 7: Run read-only subagent audit.
- [ ] Step 8: Commit `refactor: document conversation workbench architecture`.

## Task 14: End-To-End Verification And Cleanup

**Files:**

- Modify only files needed to fix verification failures.
- Do not add new feature scope in this task.

**Design requirement:** The final branch must be coherent. No old thin chat implementation remains. No compatibility projection remains. All safety boundaries remain backend-owned.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Search for forbidden leftovers.

```bash
rg -n "ThinkingPanel|AssistantSegment::Thinking|ToolPermissionState|permission-inline-panel|JSON.stringify\\(segment\\)|visibleLines\\.map\\(formatDiffLineForCopy\\)|h-8 w-full resize-none|legacyThinking|legacy.*conversation" \
  crates \
  apps/desktop/src/shared/tauri \
  apps/desktop/src/features/conversation \
  apps/desktop/src/features/workbench \
  apps/desktop/src/features/artifacts \
  -g '!**/*.test.ts' \
  -g '!**/*.test.tsx' \
  -g '!**/*.stories.tsx'
```

Expected:

- no production references to removed compatibility UI or old projection names
- test names may mention legacy only when asserting rejection
- `apps/desktop/src/features/activity/ToolCallCard.tsx` is intentionally outside this grep because it may contain an activity-local permission display type unrelated to the removed conversation projection contract

- [ ] Step 3: Run full gates.

```bash
pnpm audit:tests > docs/testing/test-inventory.md
pnpm audit:tests
pnpm check:testing-docs
pnpm check
pnpm check:docs
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:desktop
pnpm check:rust
pnpm check:testing-docs
pnpm check:test-architecture
pnpm check:agent-orchestration-no-fakes
pnpm check:agent-supervisor-sidecar
pnpm check:quick
pnpm check:frontend:fast
pnpm check:rust:fast
```

- [ ] Step 4: Run final read-only subagent audit with the full diff.

Final audit must verify:

- every task objective is implemented
- every task has a commit
- no compatibility schema or UI remains
- React does not own policy
- Tauri does not upgrade authority
- all large evidence uses refs
- raw thinking is not accepted by frontend projection
- no production mock/fake/noop placeholder is present
- docs match implementation
- all gates passed

- [ ] Step 5: Fix audit failures and rerun failed gates.
- [ ] Step 6: Commit final cleanup as `refactor: verify conversation workbench redesign`.

## Execution Notes

- Prefer small focused files when creating new frontend components.
- Keep `shared` free of dependencies on `app`, `routes`, or `features`.
- Keep `features` free of dependencies on `app` and `routes`.
- Keep Tauri commands as IPC boundaries only.
- Keep public serde contracts in `crates/jyowo-harness-contracts`.
- Keep projection logic in Rust.
- Keep user-facing UI state in React.
- Keep final safety decisions in Rust.
- Keep full outputs and patches behind backend-owned refs.
- Use existing design tokens and UI primitives before creating new primitives.
- Use Storybook for state matrices, not as a substitute for behavioral tests.

## Implementation Handoff

Plan execution has two allowed paths:

1. **Subagent-driven execution, required by this plan**
   Use `superpowers:subagent-driven-development`. Dispatch one implementation subagent per task. Review the diff after each task. Then dispatch the read-only audit subagent required by that task.

2. **Inline execution is not allowed for this plan**
   The scope crosses contracts, Rust projection, Tauri IPC, frontend state, and UI. Inline execution would weaken the required per-task audit boundary.

Do not start implementation until this plan file is tracked cleanly on `main` and the isolated worktree has been created from `main`.

## Post-Implementation Audit Addendum 2026-07-05

This addendum records the audit result for worktree
`goya/agent-workbench-conversation-redesign`.

Audit conclusion:

- The worktree has not completed this plan's full implementation.
- The issues below were found by tracing production code paths.
- Tests are not accepted as completion evidence unless the production path proves the same behavior.
- Fixes must be implemented as follow-up tasks in this same worktree before Task 14 can be treated as complete.

Positive evidence from the audit:

- `AssistantSegment::Thinking` is removed from the production Rust contract.
- Backend-authored permission option ids exist in the Rust permission contract.
- The Tauri permission resolve path validates conversation id, request id, option id, and submitted decision kind against Rust pending state.
- `git diff --check` passed during audit.
- The forbidden leftover grep in Task 14 did not find the main removed production references outside a legacy test name.

Open audit findings:

1. **P0: Workbench inspector permission decisions are not actionable.**
   - Evidence:
     - `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
     - `apps/desktop/src/features/conversation/evidence/DecisionPanel.tsx`
   - Current behavior:
     - `WorkbenchInspector` renders `DecisionPanel` without `onResolve`.
     - `DecisionPanel` only calls `onResolve?.(...)`.
     - Approval and denial buttons in the inspector are visible but do nothing.
   - Required behavior:
     - The inspector Decision pane and Tool pane must resolve permissions through the same backend-owned resolve path used by the timeline.

2. **P1: `ToolAttempt` projection is incomplete.**
   - Evidence:
     - `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
   - Current behavior:
     - `project_tool_requested` sets `origin: Unknown`, `arguments_preview: None`, `output_summary: None`, `affected_targets: []`, `started_at: None`, `ended_at: None`, and `duration_ms: None`.
     - `tool.completed` updates status and event refs, but does not backfill output summary, duration, end time, or affected targets.
   - Required behavior:
     - `ToolUseRequestedEvent` and `ToolUseCompletedEvent` must produce the planned `ToolAttempt` fields from redacted, bounded, Rust-owned projection data.

3. **P1: Evidence ref registry is not the durable journal-owned design required by Task 3.**
   - Evidence:
     - `crates/jyowo-harness-journal/src/evidence.rs`
     - `apps/desktop/src-tauri/src/commands/runtime.rs`
     - `crates/jyowo-harness-journal/src/conversation_read_model.rs`
   - Current behavior:
     - `EvidenceRefStore` has registry and blob store only; it cannot read journal-backed payloads.
     - Journal-backed refs can be registered but reads return `"journal-backed evidence reads are unavailable"`.
     - Desktop runtime opens a separate `.jyowo/runtime/evidence.sqlite`.
     - `SqliteConversationReadModelStore` does not own the `evidence_refs` table lifecycle.
   - Required behavior:
     - Evidence refs must be stored in a journal-owned durable table in the same lifecycle as the read model.
     - Journal-backed refs must reload the event payload and re-check hash and byte length before returning bytes.

4. **P1: Journal prune paths do not invalidate evidence refs.**
   - Evidence:
     - `crates/jyowo-harness-journal/src/store.rs`
     - `crates/jyowo-harness-journal/src/sqlite.rs`
     - `crates/jyowo-harness-journal/src/memory.rs`
     - `crates/jyowo-harness-sdk/src/harness/events.rs`
   - Current behavior:
     - `EventStore::prune` has no evidence store dependency.
     - SQLite and memory prune remove sessions/events without deleting or invalidating evidence rows.
     - SDK event-store wrappers delegate prune without evidence invalidation.
   - Required behavior:
     - Conversation deletion, journal prune, and blob/journal deletion must make matching evidence refs unreadable before any later read.

5. **P1: Large evidence copy behavior is still wrong.**
   - Evidence:
     - `apps/desktop/src/features/conversation/evidence/DiffPane.tsx`
     - `apps/desktop/src/features/conversation/evidence/CommandExecutionView.tsx`
     - `apps/desktop/src/features/conversation/timeline/command-evidence-block.tsx`
   - Current behavior:
     - Full patch copy calls `exportConversationEvidence` and copies the exported file path.
     - Command visible-output copy includes the command line.
     - Timeline command block still has the old ambiguous combined copy behavior.
   - Required behavior:
     - Full patch copy must fetch patch content through `getConversationDiffPatch`.
     - Copy command must copy only the command.
     - Copy visible output must copy only visible output.
     - The old combined timeline copy action must be removed or replaced with explicit actions.

6. **P2: Artifact image preview does not use the projected preview ref.**
   - Evidence:
     - `crates/jyowo-harness-contracts/src/conversation.rs`
     - `apps/desktop/src/features/workbench/artifacts/ArtifactPane.tsx`
     - `apps/desktop/src-tauri/src/commands/artifacts.rs`
   - Current behavior:
     - `ArtifactRevisionSummary.preview_ref` exists in the contract.
     - `ArtifactPane` fetches image preview by `artifactId` and `revisionId`.
     - The Tauri command scans artifact lifecycle events and reads a `blob_ref`.
   - Required behavior:
     - Image preview must be driven by the Rust-projected `previewRef` or another backend-owned evidence ref that carries the same ownership, redaction, retention, and hash validation guarantees.

7. **P2: Right-side Context pane is not part of the workbench inspector design.**
   - Evidence:
     - `apps/desktop/src/app/shell/AppShell.tsx`
     - `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
   - Current behavior:
     - `AppShell` renders either `WorkbenchInspector` or `ContextPanel`.
     - The inspector `context` pane is a placeholder state.
   - Required behavior:
     - The right side must behave as the planned inspector surface for Context, Decision, Evidence, Diff, Artifact, and Terminal panes.
     - Opening the inspector must not hide real context data behind a placeholder.

8. **P2: `ToolInvocationCard` is a no-op button when no click action is provided.**
   - Evidence:
     - `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.tsx`
     - `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
   - Current behavior:
     - The component always renders a focusable `<button>`.
     - The inspector passes no `onClick`.
   - Required behavior:
     - The component must render a non-interactive element when it has no action, or receive a real action.

9. **P3: Planned file map is not fully followed.**
   - Evidence:
     - `apps/desktop/src/features/workbench/artifacts/ArtifactPane.tsx`
     - `apps/desktop/src/features/conversation/Composer.tsx`
   - Current behavior:
     - The planned `apps/desktop/src/features/artifacts/ArtifactPane.tsx` does not exist.
     - `ComposerToolbar` remains inside `Composer.tsx`.
   - Required behavior:
     - Either align the implementation with the plan file map, or update this plan with a justified replacement file map and ensure docs/tests/gates accept the final structure.

10. **P3: The worktree is not in a coherent final state.**
    - Evidence:
      - `git status --short`
    - Current behavior:
      - The worktree still has tracked modified files and untracked files after Task 14-era commits.
    - Required behavior:
      - Final handoff must have all intended source changes committed or intentionally left uncommitted with a written reason.
      - Generated or unrelated noise must not be left in the final worktree.

## Follow-Up Implementation Plan

The following tasks must be completed after the existing Task 14. They are scoped to close the audit findings above. Do not mark Task 14 complete until every follow-up task is complete, audited, and committed.

### Task 15: Wire Inspector Decisions And Context Pane

**Files:**

- Modify `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
- Modify `apps/desktop/src/app/shell/AppShell.tsx`
- Modify `apps/desktop/src/features/context/ContextPanel.tsx` only if the existing panel needs an embeddable inspector mode
- Modify `apps/desktop/src/features/workbench/WorkbenchInspector.test.tsx`
- Modify or create `apps/desktop/src/features/workbench/WorkbenchInspector.stories.tsx`

**Design requirement:** The right side is one workbench inspector. Decision and Tool panes must submit permission decisions through the backend-owned resolve command. Context must show real context data, not a placeholder, when selected.

- [ ] Step 1: Add a failing inspector test proving `DecisionPanel` in the Decision pane calls `resolvePermission` with `conversationId`, `requestId`, `decision`, `optionId`, and optional `confirmationText`.
- [ ] Step 2: Add a failing inspector test proving a Tool pane with `attempt.permission` resolves through the same path.
- [ ] Step 3: Add a failing shell or inspector test proving selecting/opening the Context pane renders real `ContextPanel` data rather than the placeholder state.
- [ ] Step 4: Move permission resolution wiring from timeline-only ownership into a shared workbench path that still calls `CommandClient.resolvePermission`.
- [ ] Step 5: Render the real context content inside the workbench inspector, or compose `ContextPanel` as the inspector Context pane without creating a second policy source.
- [ ] Step 6: Confirm React submits only user intent and backend option ids; it must not derive permission policy, matcher, sandbox, risk, or data exposure.
- [ ] Step 7: Run gates.

```bash
pnpm -C apps/desktop test -- src/features/workbench/WorkbenchInspector.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/evidence/DecisionPanel.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run read-only subagent audit.
- [ ] Step 9: Commit `refactor: wire workbench inspector decisions`.

### Task 16: Complete ToolAttempt Projection

**Files:**

- Modify `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_workbench_projection.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.tsx`
- Modify `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.test.tsx`

**Design requirement:** Tool cards must render data emitted by Rust. React must not infer tool origin, arguments, affected targets, output summary, or timing from raw events.

- [ ] Step 1: Add failing Rust projector tests for a tool request with redacted input. Assert `arguments_preview` is bounded, redacted, and not raw JSON.
- [ ] Step 2: Add failing Rust projector tests for tool completion. Assert `output_summary`, `ended_at`, `duration_ms`, and `affected_targets` are populated when safe payload fields exist.
- [ ] Step 3: Add failing Rust projector tests for tool origin mapping. Assert built-in, MCP, plugin, app, provider, and unknown origins map to `ToolAttemptOrigin`.
- [ ] Step 4: Implement `project_tool_attempt_preview(...) -> ToolAttempt` and completion merge logic in Rust.
- [ ] Step 5: Keep unsafe tool input, private paths, secrets, and raw output out of `ConversationTurn`.
- [ ] Step 6: Fix `ToolInvocationCard` so it renders a non-button element when no action exists.
- [ ] Step 7: Add component coverage for interactive and non-interactive card variants.
- [ ] Step 8: Run gates.

```bash
cargo test -p jyowo-harness-journal conversation_workbench_projection --test conversation_workbench_projection
cargo test -p jyowo-harness-journal conversation_worktree_projector --test conversation_worktree_projector
pnpm -C apps/desktop test -- src/features/conversation/evidence/ToolInvocationCard.test.tsx
cargo fmt --all --check
cargo check --workspace
pnpm check:desktop
git diff --check
```

- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: complete tool attempt projection`.

### Task 17: Finish Evidence Ref Store Persistence, Journal Reads, And Prune Invalidation

**Files:**

- Modify `crates/jyowo-harness-journal/src/evidence.rs`
- Modify `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify `crates/jyowo-harness-journal/src/store.rs`
- Modify `crates/jyowo-harness-journal/src/sqlite.rs`
- Modify `crates/jyowo-harness-journal/src/jsonl.rs`
- Modify `crates/jyowo-harness-journal/src/memory.rs`
- Modify `crates/jyowo-harness-journal/src/retention.rs`
- Modify `crates/jyowo-harness-sdk/src/harness.rs`
- Modify `crates/jyowo-harness-sdk/src/harness/events.rs`
- Modify `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify `crates/jyowo-harness-journal/tests/evidence_ref_store.rs`
- Modify `crates/jyowo-harness-journal/tests/l1b_stores.rs`
- Modify `crates/jyowo-harness-sdk/tests/evidence_refs.rs`

**Design requirement:** Evidence refs are durable, journal-owned, conversation-scoped authority records. Registry rows must be in the read model lifecycle. Reads must validate owner, kind, retention, redaction provenance, byte length, and content hash. Prune/delete must make refs unreadable.

- [ ] Step 1: Add failing SQLite persistence test proving the read model database owns an `evidence_refs` table and refs survive process restart.
- [ ] Step 2: Add failing journal-backed read test proving `JournalPayload` reloads the source event payload, extracts by JSON pointer, and re-checks hash and byte length.
- [ ] Step 3: Add failing owner/kind/redaction/retention tests for `read_evidence` and paged reads.
- [ ] Step 4: Add failing prune tests for SQLite, JSONL, and memory stores. After prune, matching refs must be unreadable.
- [ ] Step 5: Replace the separate desktop `.jyowo/runtime/evidence.sqlite` registry with the journal/read-model-owned registry lifecycle.
- [ ] Step 6: Add the event store dependency needed for journal-backed reads without creating a higher-layer dependency cycle.
- [ ] Step 7: Wire evidence invalidation into conversation deletion and prune paths before event/blob data can be read again.
- [ ] Step 8: Keep GC live roots wired through `EvidenceRefRegistry::list_live_blob_roots`.
- [ ] Step 9: Run gates.

```bash
cargo test -p jyowo-harness-journal evidence_ref_store --test evidence_ref_store
cargo test -p jyowo-harness-journal retention --test l1b_stores
cargo test -p jyowo-harness-journal prune --test contract
cargo test -p jyowo-harness-sdk evidence_refs --test evidence_refs
cargo fmt --all --check
cargo check --workspace
git diff --check
```

- [ ] Step 10: Run read-only subagent audit.
- [ ] Step 11: Commit `refactor: finish evidence ref persistence`.

### Task 18: Fix Evidence Copy And Fetch UI Semantics

**Files:**

- Modify `apps/desktop/src/features/conversation/evidence/DiffPane.tsx`
- Modify `apps/desktop/src/features/conversation/evidence/DiffPane.test.tsx`
- Modify `apps/desktop/src/features/conversation/evidence/CommandExecutionView.tsx`
- Modify `apps/desktop/src/features/conversation/evidence/CommandExecutionView.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/command-evidence-block.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx`

**Design requirement:** Copy and fetch actions must match the planned evidence model. Visible copy copies visible content only. Full copy fetches bytes through the backend evidence commands. Export remains a separate action.

- [ ] Step 1: Add failing `DiffPane` test proving full patch copy calls `getConversationDiffPatch` and writes patch content, not an exported path.
- [ ] Step 2: Add failing `CommandExecutionView` test proving copy command writes only `command.command`.
- [ ] Step 3: Add failing `CommandExecutionView` test proving copy visible output writes only visible stdout/stderr content and not `$ command`, exit code, or duration.
- [ ] Step 4: Add failing timeline test proving the old combined copy action is gone or split into explicit actions.
- [ ] Step 5: Implement the copy changes without embedding full output, full patch, or artifact content in `ConversationTurn`.
- [ ] Step 6: Add user-visible error state for failed evidence fetch/copy where the component already owns the action.
- [ ] Step 7: Run gates.

```bash
pnpm -C apps/desktop test -- src/features/conversation/evidence/DiffPane.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/evidence/CommandExecutionView.test.tsx
pnpm -C apps/desktop test -- src/features/conversation/timeline/conversation-timeline.large-output.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run read-only subagent audit.
- [ ] Step 9: Commit `refactor: fix evidence copy semantics`.

### Task 19: Align Artifact Workspace With Evidence Refs

**Files:**

- Modify `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify `apps/desktop/src-tauri/src/commands/artifacts.rs`
- Modify `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify `apps/desktop/src/features/workbench/artifacts/ArtifactPane.tsx`
- Modify or create `apps/desktop/src/features/workbench/WorkbenchInspector.artifacts.test.tsx`
- Modify or create `apps/desktop/src/features/workbench/artifacts/ArtifactPane.stories.tsx`
- Modify `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
- Modify `apps/desktop/src/shared/artifacts/ArtifactPreview.tsx`

**Design requirement:** Artifact preview and content reads must use backend-owned refs. The accepted file map keeps artifact workspace UI under `features/workbench/artifacts` because it is owned by `WorkbenchInspector`, not a standalone route-level feature.

- [ ] Step 1: Update docs to state that `ArtifactPane` remains in `features/workbench/artifacts` because it is inspector-owned.
- [ ] Step 2: Add failing Rust projector test proving image artifacts expose `preview_ref` as an evidence-backed preview identifier when the backend can validate it.
- [ ] Step 3: Add failing Tauri command test proving image preview reads by backend-owned ref and rejects owner, kind, retention, redaction, hash, missing artifact, non-ready revision, and non-image revision mismatches.
- [ ] Step 4: Add failing `ArtifactPane` test proving image preview uses the projected `previewRef` or evidence-backed preview id, not only `artifactId`/`revisionId` event scanning.
- [ ] Step 5: Update `ArtifactPreview` image rendering with stable width/height or aspect-ratio constraints.
- [ ] Step 6: Ensure HTML preview remains sandboxed with no same-origin privilege.
- [ ] Step 7: Ensure the Tauri preview command validates the selected artifact, revision, status, kind, and content ref through read-model/evidence metadata before reading bytes.
- [ ] Step 8: Update artifact preview command tests so success and rejection cases register or discover real artifact content evidence refs instead of relying on raw lifecycle `blob_ref` scans.
- [ ] Step 9: Run gates.

```bash
cargo test -p jyowo-harness-journal conversation_workbench_projection --test conversation_workbench_projection
cargo test -p jyowo-desktop-shell artifact_preview
cargo test -p jyowo-desktop-shell artifact_evidence
pnpm -C apps/desktop test -- src/features/workbench/WorkbenchInspector.artifacts.test.tsx
cargo fmt --all --check
cargo check --workspace
pnpm check:desktop
pnpm check:test-architecture
git diff --check
```

- [ ] Step 10: Run read-only subagent audit.
- [ ] Step 11: Commit `refactor: align artifact workspace refs`.

### Task 20: Finish Composer Split And Planned File Map Cleanup

**Files:**

- Modify `apps/desktop/src/features/conversation/Composer.tsx`
- Create `apps/desktop/src/features/conversation/composer/ComposerToolbar.tsx`
- Modify `apps/desktop/src/features/conversation/composer/ComposerEditor.tsx`
- Modify `apps/desktop/src/features/conversation/composer/ReferenceCombobox.tsx`
- Modify `apps/desktop/src/features/conversation/composer/SlashCommandMenu.tsx`
- Modify `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify `apps/desktop/src/features/conversation/Composer.stories.tsx`

**Design requirement:** Composer split must match the plan's component ownership. Compatibility naming such as `legacyComposerMode` must not remain in production code unless it is renamed to describe current behavior.

- [ ] Step 1: Add failing component or static import test proving `ComposerToolbar` is exported from `features/conversation/composer/ComposerToolbar.tsx`.
- [ ] Step 2: Move toolbar code out of `Composer.tsx` without changing composer behavior.
- [ ] Step 3: Rename `legacyComposerMode` to a current-domain helper name or inline it if it is no longer useful.
- [ ] Step 4: Confirm draft persistence, attachment controls, references, slash commands, model selector, and permission mode behavior still pass existing tests.
- [ ] Step 5: Run gates.

```bash
pnpm -C apps/desktop test -- src/features/conversation/Composer.test.tsx
pnpm check:desktop
pnpm check:test-architecture
git diff --check
```

- [ ] Step 6: Run read-only subagent audit.
- [ ] Step 7: Commit `refactor: finish composer component split`.

### Task 21: Final Coherence, Docs, And Gates

**Files:**

- Modify only files needed to fix remaining verification failures.
- Modify `docs/frontend/frontend-product-ux.md`
- Modify `docs/frontend/frontend-engineering.md`
- Modify `docs/backend/backend-runtime.md`
- Modify `docs/backend/backend-engineering.md`
- Modify `docs/testing/test-inventory.md` only by regenerating it with `pnpm audit:tests > docs/testing/test-inventory.md` when test structure changed.

**Design requirement:** The worktree must be coherent after all follow-up fixes. Docs must describe what production code does, not an aspirational state. The final status must not contain untracked source files or unrelated modified files.

- [ ] Step 1: Update frontend/backend docs to match the final inspector, evidence ref, artifact, and composer structure.
- [ ] Step 2: Run the forbidden leftover grep from Task 14 again.
- [ ] Step 3: Run `git status --short` and classify every remaining file as intended source change, generated inventory, or unrelated noise.
- [ ] Step 4: Remove generated noise only when it was created by this follow-up work and is not required by gates.
- [ ] Step 5: Run full gates.

```bash
pnpm audit:tests > docs/testing/test-inventory.md
pnpm audit:tests
pnpm check:testing-docs
pnpm check
pnpm check:docs
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:desktop
pnpm check:rust
pnpm check:test-architecture
pnpm check:agent-orchestration-no-fakes
pnpm check:agent-supervisor-sidecar
pnpm check:quick
pnpm check:frontend:fast
pnpm check:rust:fast
git diff --check
```

- [ ] Step 6: Run one final read-only audit over the full branch. The audit must explicitly verify every finding in this addendum is closed.
- [ ] Step 7: Commit `refactor: close conversation workbench audit findings`.

## Follow-Up Completion Criteria

The addendum is complete only when all of these are true:

- [ ] Inspector Decision and Tool panes resolve permissions through Rust.
- [ ] Context pane shows real context data inside the workbench inspector model.
- [ ] `ToolAttempt` projection includes redacted bounded arguments, origin, output summary, affected targets, start/end time, and duration when safe source data exists.
- [ ] Evidence refs live in the journal/read-model persistence lifecycle.
- [ ] Journal-backed evidence reads reload and hash-check source payloads.
- [ ] Prune and delete paths invalidate matching evidence refs before reads can succeed.
- [ ] Full patch copy fetches patch bytes through `getConversationDiffPatch`.
- [ ] Command copy actions are explicit and do not copy combined command/output/status blocks.
- [ ] Timeline command evidence no longer exposes the old ambiguous combined copy behavior.
- [ ] Artifact image preview is driven by a backend-owned ref.
- [ ] Artifact image rendering has stable dimensions or aspect-ratio constraints.
- [ ] `ToolInvocationCard` has no focusable no-op state.
- [ ] `ArtifactPane` and `ComposerToolbar` file placement matches the accepted file map.
- [ ] Final docs match production behavior.
- [ ] Final worktree state is coherent and has no accidental untracked source files.
- [ ] Required gates pass with exit code 0.
- [ ] A read-only audit returns PASS after these follow-up tasks.

## Follow-Up Execution Checkpoint 2026-07-05

This checkpoint records issues found while executing the follow-up plan. These items are part of the same completion checklist and must be closed before Task 21 can pass.

Current status:

- Tasks 16, 17, 18, and 20 have partial implementation evidence in the worktree, but they are not complete until their task gates and read-only audits pass.
- Task 19 is still open. The production path has moved toward evidence-backed artifact previews, but command tests currently expose gaps in projection, validation, and test setup.
- Task 21 is still open. Final docs, generated test inventory, full gates, final audit, and coherent worktree classification are not complete.

Additional execution findings:

1. **Task 19 blocker: artifact revisions still need a projected preview ref.**
   - Evidence:
     - `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
   - Required behavior:
     - Image artifact revisions must expose `preview_ref` or an equivalent evidence-backed identifier in the Rust projection.
     - The frontend must use that identifier when requesting image previews.

2. **Task 19 blocker: artifact preview command must keep revision policy checks.**
   - Evidence:
     - `apps/desktop/src-tauri/src/commands/artifacts.rs`
   - Required behavior:
     - `get_artifact_media_preview` must verify artifact id, revision id, content ref, revision status, artifact kind, evidence owner, evidence kind, retention, redaction provenance, byte length, and hash before returning image bytes.
     - Moving away from lifecycle `blob_ref` scanning must not remove the previous missing-artifact, non-ready, and non-image rejection semantics.

3. **Task 19 blocker: artifact preview tests must exercise the production evidence path.**
   - Evidence:
     - `apps/desktop/src-tauri/tests/commands/artifact_preview.rs`
   - Required behavior:
     - Success tests must register or discover artifact content evidence refs before calling `get_artifact_media_preview`.
     - Rejection tests must pass real or intentionally mismatched `content_ref` values so failures prove the production validator, not test-only event scanning.

4. **Task 17 follow-up: read-model-owned evidence registry needs lifecycle verification.**
   - Evidence:
     - `apps/desktop/src-tauri/src/commands/runtime.rs`
     - `crates/jyowo-harness-journal/src/conversation_read_model.rs`
   - Required behavior:
     - Desktop must not keep an independent evidence registry lifecycle outside the conversation read model.
     - SQLite schema ownership, restart persistence, prune invalidation, and GC live roots must be verified together.

5. **Task 21 follow-up: docs and gates must reflect the accepted file map.**
   - Evidence:
     - `docs/frontend/frontend-engineering.md`
     - `docs/frontend/frontend-product-ux.md`
     - `docs/backend/backend-runtime.md`
     - `docs/backend/backend-engineering.md`
   - Required behavior:
     - Docs must describe `features/workbench/artifacts` as inspector-owned artifact UI if that structure remains.
     - Test inventory must be regenerated only through `pnpm audit:tests > docs/testing/test-inventory.md` when test structure changes.

Execution checklist:

- [ ] Close the Task 19 projector gap by emitting a backend-owned `preview_ref` for image artifact revisions.
- [ ] Close the Task 19 command gap by validating artifact/revision/content-ref/status/kind before evidence bytes are read.
- [ ] Close the Task 19 test gap by replacing raw `blob_ref` preview assumptions with real evidence-ref setup.
- [ ] Re-run `cargo test -p jyowo-desktop-shell artifact_preview` and record exit code 0 before marking Task 19 done.
- [ ] Re-run `cargo test -p jyowo-harness-journal conversation_worktree_projector --test conversation_worktree_projector` after projector changes.
- [ ] Re-run `pnpm -C apps/desktop test -- src/features/workbench/WorkbenchInspector.artifacts.test.tsx` after frontend preview-ref changes.
- [ ] Confirm the evidence registry lives in the read-model lifecycle and prune/delete invalidates refs before Task 17 is marked done.
- [ ] Update frontend/backend docs to match the accepted inspector, artifact, evidence, and composer ownership.
- [ ] Run the Task 21 gates and classify every tracked or untracked file in `git status --short`.
- [ ] Run a final read-only audit that explicitly checks every item in this addendum and checkpoint.

## Follow-Up Resolution Checkpoint 2026-07-05

This checkpoint supersedes the open blocker status above. The blocker list remains in the plan as audit history. The current implementation must still pass final gates and a read-only audit before Task 21 is complete.

Production code resolution status:

1. **Task 17 evidence refs now use the read-model lifecycle.**
   - Evidence:
     - `crates/jyowo-harness-journal/src/evidence.rs`
     - `crates/jyowo-harness-journal/src/conversation_read_model.rs`
     - `crates/jyowo-harness-sdk/src/harness/events.rs`
     - `apps/desktop/src-tauri/src/commands/runtime.rs`
   - Current behavior:
     - The SQLite registry owns an `evidence_refs` table keyed by `(tenant_id, evidence_ref_id)`.
     - Journal-backed evidence reads reload the source event payload through the configured event store, extract by JSON pointer, and validate byte length and BLAKE3 hash.
     - Missing event-store access fails closed with `journal-backed evidence reader is unavailable`.
     - Conversation delete and prune wrappers invalidate matching evidence refs before the public operation returns.
   - Audit boundary:
     - Prune invalidation currently happens after the wrapped event-store prune call and before the wrapper returns. The code does not add a separate cross-reader lock for concurrent reads during the prune window.

2. **Task 17 projection refs now avoid mismatched raw/redacted journal payloads.**
   - Evidence:
     - `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
   - Current behavior:
     - Clean payload evidence can use `EvidenceRefSource::JournalPayload`.
     - Redacted or withheld evidence is stored as blob-backed evidence so hash and byte length match the projected bytes.
     - Diff patch pointers map from projected paths to typed journal event paths.
     - Command stdout/stderr refs map to typed event pointers.

3. **Task 18 copy semantics are split and backend-backed where required.**
   - Evidence:
     - `apps/desktop/src/features/conversation/evidence/DiffPane.tsx`
     - `apps/desktop/src/features/conversation/evidence/CommandExecutionView.tsx`
     - `apps/desktop/src/features/conversation/timeline/command-evidence-block.tsx`
   - Current behavior:
     - Full diff patch copy calls `getConversationDiffPatch`.
     - Command copy writes only `command.command`.
     - Visible output copy writes only visible stdout/stderr content.
     - The timeline command evidence block no longer exposes the old ambiguous combined copy action.

4. **Task 19 artifact preview now uses backend-owned refs.**
   - Evidence:
     - `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
     - `apps/desktop/src-tauri/src/commands/artifacts.rs`
     - `apps/desktop/src/features/workbench/artifacts/ArtifactPane.tsx`
     - `apps/desktop/src/shared/artifacts/ArtifactPreview.tsx`
   - Current behavior:
     - Image artifact revisions project `preview_ref` from the backend-owned content evidence ref.
     - `get_artifact_media_preview` validates artifact id, revision id, revision status, artifact/image kind, selected content ref, projected preview/content ref, evidence metadata, byte limit, declared MIME, detected MIME, and sanitized image bytes before returning a data URL.
     - The workbench artifact pane requests image previews by `previewRef` when available.
     - Image preview rendering has bounded dimensions.

5. **Task 20 composer split is implemented.**
   - Evidence:
     - `apps/desktop/src/features/conversation/Composer.tsx`
     - `apps/desktop/src/features/conversation/composer/ComposerToolbar.tsx`
     - `apps/desktop/src/features/conversation/composer/ReferenceCombobox.tsx`
     - `apps/desktop/src/features/conversation/composer/SlashCommandMenu.tsx`
   - Current behavior:
     - Toolbar ownership is split out of `Composer.tsx`.
     - Reference picker and slash command UI are separate composer-owned components.
     - `legacyComposerMode` was renamed to a current-domain fallback helper.

6. **Task 21 cleanup has started.**
   - Evidence:
     - Task 14 forbidden leftover grep was rerun.
   - Current behavior:
     - The grep has no production matches after renaming one unrelated Rust test function that caused a false positive.

Resolution implementation checklist:

- [x] Emit backend-owned `preview_ref` for image artifact revisions.
- [x] Validate artifact id, revision id, status, kind, selected content ref, preview/content ref, and evidence bytes before image preview reads.
- [x] Replace artifact preview tests that relied on raw `blob_ref` scanning with evidence-ref setup.
- [x] Confirm journal-backed evidence reads reload payloads and verify hash/length.
- [x] Keep redacted/withheld projected evidence blob-backed.
- [x] Confirm evidence refs are read-model lifecycle records and GC live roots include blob-backed refs.
- [x] Invalidate evidence refs on conversation deletion and prune wrapper completion.
- [x] Split command copy into command-only and visible-output actions.
- [x] Fetch full diff patch bytes through `getConversationDiffPatch`.
- [x] Split composer toolbar, reference combobox, and slash command menu into accepted files.
- [x] Regenerate `docs/testing/test-inventory.md` with `pnpm audit:tests > docs/testing/test-inventory.md`.
- [x] Run Task 14 forbidden leftover grep with no production matches.
- [ ] Run the remaining Task 21 gates after this checkpoint update.
- [ ] Classify every tracked and untracked file in `git status --short`.
- [ ] Run final read-only audit over the full branch.
- [ ] Update this checkpoint with final gate results and audit result before claiming Task 21 complete.

## Follow-Up Read-Only Audit Failure 2026-07-05

The final read-only audit returned **FAIL**. This section is now part of Task 21 and supersedes any completion claim above. The codebase must not be marked complete until every item below is fixed, verified, and re-audited.

Gate status before this audit:

- `pnpm check`: exit code 0.
- `pnpm check:quick`: exit code 0.
- `pnpm check:frontend:fast`: exit code 0 through `pnpm check:quick`.
- `pnpm check:rust:fast`: exit code 0 through `pnpm check:quick`.
- `git diff --check`: exit code 0.
- Task 14 forbidden leftover grep: exit code 1 with no production matches.

Audit findings:

1. **Task 17 blocker: prune evidence invalidation is not fail-closed.**
   - Evidence:
     - `crates/jyowo-harness-sdk/src/harness/events.rs`
   - Current behavior:
     - `prune_with_evidence_invalidation` calls `inner.prune(...)` before `evidence_ref_store.delete_for_conversation(...)`.
     - If evidence deletion fails after the event-store prune succeeds, pruned sessions can leave readable evidence registry rows.
   - Required behavior:
     - Prune must make matching evidence refs unreadable before event payloads become unreachable, or the operation must fail without leaving pruned events and readable refs inconsistent.

2. **Task 17 blocker: generic `delete_session` can bypass evidence invalidation.**
   - Evidence:
     - `crates/jyowo-harness-sdk/src/harness/events.rs`
   - Current behavior:
     - `ConversationDeletionGuardEventStore::delete_session` delegates directly to `inner.delete_session(...)`.
     - `Harness::event_store()` exposes the guarded event store, so callers using the generic store path can delete a session without deleting matching evidence refs.
   - Required behavior:
     - Every public session deletion path that can delete a conversation session must invalidate matching evidence refs through the same lifecycle.

3. **Task 17 blocker: `EvidenceRefStore::delete_for_conversation` deletes backing blobs before registry rows.**
   - Evidence:
     - `crates/jyowo-harness-journal/src/evidence.rs`
   - Current behavior:
     - `delete_for_conversation` lists records, deletes blob-backed payloads, then deletes registry rows.
     - For journal-backed refs, the registry row remains the read authority until the final registry delete.
   - Required behavior:
     - Registry rows must become unreadable before backing blob cleanup. Blob cleanup failure must not preserve readable evidence refs for deleted or pruned conversations.

4. **Task 21 blocker: worktree coherence still has untracked source files.**
   - Evidence:
     - `git status --short`
     - `apps/desktop/src/features/conversation/composer/ComposerToolbar.tsx`
     - `apps/desktop/src/features/conversation/composer/ReferenceCombobox.tsx`
     - `apps/desktop/src/features/conversation/composer/SlashCommandMenu.tsx`
   - Current behavior:
     - The files are intended implementation files but remain untracked in the worktree.
   - Required behavior:
     - Final handoff must classify every modified and untracked file. Intended new source and test files must be staged or otherwise explicitly included in the final file set before claiming coherence.

5. **Task 21 blocker: frontend docs reference a nonexistent workbench state file.**
   - Evidence:
     - `docs/frontend/frontend-engineering.md`
     - `apps/desktop/src/shared/state/ui-store.ts`
   - Current behavior:
     - The docs still say `features/workbench/workbench-state.ts` provides workbench hooks, but that file does not exist.
     - Current state lives in `shared/state/ui-store.ts` and `shared/state/workbench-selection.ts`.
   - Required behavior:
     - Docs must describe the production file map exactly. They must not refer to nonexistent feature state files.

Code quality and design risks:

- **P1:** Evidence ref invalidation is not atomic with prune/delete. The current order can leave an inconsistent state if the second phase fails.
- **P1:** Generic SDK event-store deletion remains a bypass around the conversation evidence lifecycle.
- **P2:** `delete_for_conversation` should remove read authority before backing storage cleanup.
- **P2:** Docs are not aligned with the implemented workbench state ownership.
- **P3:** New implementation files remain untracked, so final handoff is not coherent.

Implementation plan:

1. **Fix evidence deletion ordering.**
   - Change `EvidenceRefStore::delete_for_conversation` so registry rows become unreadable before blob cleanup.
   - Preserve enough blob refs for best-effort or fail-closed backing blob deletion after registry invalidation.
   - Add or update tests that prove a blob cleanup failure does not leave readable registry refs.

2. **Close generic session deletion bypass.**
   - Route `ConversationDeletionGuardEventStore::delete_session` through evidence invalidation.
   - Ensure `delete_session` and conversation facade deletion share the same evidence lifecycle.
   - Add or update SDK tests proving direct event-store deletion invalidates evidence refs.

3. **Make prune fail-closed or explicitly two-phase.**
   - Rework `prune_with_evidence_invalidation` so evidence refs are invalidated before pruned event payloads can become unreadable, or introduce a durable invalidation marker that makes reads fail before the event-store prune.
   - Keep candidate-session discovery bounded and tenant-scoped.
   - Add tests for prune failure ordering and stale-ref unreadability.

4. **Fix docs and worktree coherence.**
   - Update `docs/frontend/frontend-engineering.md` to name the actual workbench state files.
   - Classify all tracked and untracked files after fixes.
   - Ensure intended new files are included in the final source set and no generated noise remains.

5. **Re-run gates and audit.**
   - Re-run targeted Rust tests for evidence delete/prune lifecycle.
   - Re-run docs gates after docs changes.
   - Re-run full Task 21 gates.
   - Run a final read-only audit that explicitly verifies all five findings above.

Checklist:

- [ ] `EvidenceRefStore::delete_for_conversation` removes registry read authority before backing blob cleanup.
- [ ] Blob cleanup failure no longer leaves readable evidence refs for a deleted conversation.
- [ ] Generic `EventStore::delete_session` path invalidates matching evidence refs.
- [ ] Conversation facade deletion and generic event-store deletion share the same evidence lifecycle.
- [ ] Prune invalidates matching evidence refs before pruned event payloads become unreachable, or uses a durable invalidation marker that makes reads fail first.
- [ ] Tests cover direct `delete_session` evidence invalidation.
- [ ] Tests cover prune evidence invalidation ordering.
- [ ] Tests cover blob cleanup failure after registry invalidation.
- [ ] `docs/frontend/frontend-engineering.md` references only existing workbench state files.
- [ ] `git status --short --untracked-files=all` is classified and has no accidental untracked source files.
- [ ] `pnpm check:docs` passes after docs edits.
- [ ] Targeted evidence lifecycle tests pass.
- [ ] `pnpm check` passes.
- [ ] `pnpm check:quick` passes.
- [ ] `git diff --check` passes.
- [ ] Final read-only audit returns PASS.

## Follow-Up Implementation Checkpoint 2026-07-05

This checkpoint records the implementation of the five read-only audit failures above. It does not remove the failure record; that record remains audit history.

Production code changes:

1. **Evidence ref conversation deletion is fail-closed for read authority.**
   - `crates/jyowo-harness-journal/src/evidence.rs`
   - `EvidenceRefStore::delete_for_conversation` now lists blob-backed refs, deletes registry rows first, then deletes backing blobs.
   - If blob cleanup fails after registry deletion, the evidence ref is no longer readable through the registry.

2. **Generic SDK `EventStore::delete_session` no longer bypasses evidence invalidation.**
   - `crates/jyowo-harness-sdk/src/harness/events.rs`
   - `Harness::event_store()` returns `ConversationDeletionGuardEventStore` with the configured evidence ref store.
   - `ConversationDeletionGuardEventStore::delete_session` and `LifecycleHookEventStore::delete_session` call evidence invalidation before delegating to the inner event store.

3. **Prune invalidates evidence refs before event payload pruning.**
   - `crates/jyowo-harness-sdk/src/harness/events.rs`
   - `prune_with_evidence_invalidation` discovers tenant-scoped candidate sessions, deletes matching evidence refs, then calls `inner.prune(...)`.
   - This can leave events without evidence refs if `inner.prune(...)` fails after invalidation. That is an accepted fail-closed tradeoff: stale refs are unreadable before payload reachability can be removed.

4. **Frontend workbench state docs now match production files.**
   - `docs/frontend/frontend-engineering.md`
   - Current selection state is documented as shared UI state through `useUiStore` selectors, not a nonexistent `features/workbench/workbench-state.ts` wrapper.

5. **Test inventory was regenerated through the required command.**
   - `docs/testing/test-inventory.md`
   - Command used: `pnpm audit:tests > docs/testing/test-inventory.md`.

Verification:

- `cargo fmt --all --check`: exit code 0.
- `cargo test -p jyowo-harness-journal --test evidence_ref_store`: exit code 0.
- `cargo test -p jyowo-harness-sdk --features testing --test evidence_refs`: exit code 0.
- `cargo test -p jyowo-harness-sdk harness::events::tests`: exit code 0.
- `pnpm audit:tests`: exit code 0.
- `pnpm check:testing-docs`: exit code 0.
- `pnpm check:docs`: exit code 0.
- `pnpm check:desktop`: exit code 0.
- `pnpm check:rust`: exit code 0.
- `git diff --check`: exit code 0.
- `pnpm check:quick`: first run failed only because the filesystem had 572M free and Rust archiving hit `No space left on device`; `cargo clean` removed 70.3G of local build artifacts; rerun exit code 0.

Root gate note:

- `pnpm check` was not rerun as a single wrapper after the disk cleanup.
- Equivalent component coverage was run through `pnpm check:quick`, `pnpm check:desktop`, and `pnpm check:rust`; together these cover the `pnpm check` script components.

Worktree classification:

- Tauri command and IPC implementation:
  - `apps/desktop/src-tauri/src/commands/artifacts.rs`
  - `apps/desktop/src-tauri/src/commands/contracts.rs`
  - `apps/desktop/src-tauri/src/commands/mod.rs`
  - `apps/desktop/src-tauri/src/commands/runtime.rs`
  - `apps/desktop/src/shared/tauri/commands.ts`
- Tauri command tests and support:
  - `apps/desktop/src-tauri/tests/commands/artifact_listing.rs`
  - `apps/desktop/src-tauri/tests/commands/artifact_preview.rs`
  - `apps/desktop/src-tauri/tests/commands/support.rs`
- Desktop shell and conversation UI:
  - `apps/desktop/src/app/shell/AppShell.tsx`
  - `apps/desktop/src/features/conversation/Composer.tsx`
  - `apps/desktop/src/features/conversation/Composer.test.tsx`
  - `apps/desktop/src/features/conversation/composer/ComposerEditor.tsx`
  - `apps/desktop/src/features/conversation/composer/ComposerToolbar.tsx`
  - `apps/desktop/src/features/conversation/composer/ReferenceCombobox.tsx`
  - `apps/desktop/src/features/conversation/composer/SlashCommandMenu.tsx`
  - `apps/desktop/src/features/conversation/timeline/command-evidence-block.tsx`
  - `apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx`
- Conversation evidence UI and tests:
  - `apps/desktop/src/features/conversation/evidence/CommandExecutionView.tsx`
  - `apps/desktop/src/features/conversation/evidence/CommandExecutionView.test.tsx`
  - `apps/desktop/src/features/conversation/evidence/DiffPane.tsx`
  - `apps/desktop/src/features/conversation/evidence/DiffPane.test.tsx`
  - `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.tsx`
  - `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.test.tsx`
- Workbench artifact inspector and preview UI:
  - `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
  - `apps/desktop/src/features/workbench/WorkbenchInspector.test.tsx`
  - `apps/desktop/src/features/workbench/WorkbenchInspector.artifacts.test.tsx`
  - `apps/desktop/src/features/workbench/WorkbenchInspector.test-support.tsx`
  - `apps/desktop/src/features/workbench/artifacts/ArtifactPane.tsx`
  - `apps/desktop/src/shared/artifacts/ArtifactPreview.tsx`
- I18n updates:
  - `apps/desktop/src/shared/i18n/locales/en-US.ts`
  - `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Journal/read-model/evidence lifecycle:
  - `crates/jyowo-harness-journal/src/conversation_read_model.rs`
  - `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
  - `crates/jyowo-harness-journal/src/evidence.rs`
  - `crates/jyowo-harness-journal/src/retention.rs`
  - `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
  - `crates/jyowo-harness-journal/tests/conversation_workbench_projection.rs`
  - `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
  - `crates/jyowo-harness-journal/tests/evidence_ref_journal_payload.rs`
  - `crates/jyowo-harness-journal/tests/evidence_ref_retention.rs`
  - `crates/jyowo-harness-journal/tests/evidence_ref_store.rs`
  - `crates/jyowo-harness-journal/tests/l1b_stores.rs`
- SDK conversation/runtime/evidence lifecycle:
  - `crates/jyowo-harness-sdk/src/harness.rs`
  - `crates/jyowo-harness-sdk/src/harness/conversation.rs`
  - `crates/jyowo-harness-sdk/src/harness/events.rs`
  - `crates/jyowo-harness-sdk/src/harness/mcp_server.rs`
  - `crates/jyowo-harness-sdk/src/harness/read_model.rs`
  - `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
  - `crates/jyowo-harness-sdk/tests/conversation_read_model.rs`
  - `crates/jyowo-harness-sdk/tests/evidence_refs.rs`
  - `crates/jyowo-harness-sdk/tests/runtime_assembly_contract.rs`
- Architecture and testing docs:
  - `docs/backend/backend-engineering.md`
  - `docs/backend/backend-runtime.md`
  - `docs/frontend/frontend-engineering.md`
  - `docs/frontend/frontend-product-ux.md`
  - `docs/superpowers/plans/2026-07-04-agent-workbench-conversation-redesign.md`
  - `docs/testing/test-inventory.md`

Untracked file classification:

- Intended new source files:
  - `apps/desktop/src/features/conversation/composer/ComposerToolbar.tsx`
  - `apps/desktop/src/features/conversation/composer/ReferenceCombobox.tsx`
  - `apps/desktop/src/features/conversation/composer/SlashCommandMenu.tsx`
- Intended new test/support files:
  - `apps/desktop/src/features/conversation/evidence/CommandExecutionView.test.tsx`
  - `apps/desktop/src/features/conversation/evidence/DiffPane.test.tsx`
  - `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.test.tsx`
  - `apps/desktop/src/features/workbench/WorkbenchInspector.artifacts.test.tsx`
  - `apps/desktop/src/features/workbench/WorkbenchInspector.test-support.tsx`
  - `crates/jyowo-harness-journal/tests/evidence_ref_journal_payload.rs`
  - `crates/jyowo-harness-journal/tests/evidence_ref_retention.rs`
- No generated artifact noise is intentionally included in the source file set.
- The new files remain uncommitted and unstaged because this checkpoint does not perform git staging or commit operations.

Implementation checklist update:

- [x] `EvidenceRefStore::delete_for_conversation` removes registry read authority before backing blob cleanup.
- [x] Blob cleanup failure no longer leaves readable evidence refs for a deleted conversation.
- [x] Generic `EventStore::delete_session` path invalidates matching evidence refs.
- [x] Conversation facade deletion and generic event-store deletion share the same evidence lifecycle.
- [x] Prune invalidates matching evidence refs before pruned event payloads become unreachable.
- [x] Tests cover direct `delete_session` evidence invalidation.
- [x] Tests cover prune evidence invalidation ordering.
- [x] Tests cover blob cleanup failure after registry invalidation.
- [x] `docs/frontend/frontend-engineering.md` references only existing workbench state files.
- [x] `git status --short --untracked-files=all` is classified and has no accidental generated untracked files.
- [x] `pnpm check:docs` passes after docs edits.
- [x] Targeted evidence lifecycle tests pass.
- [ ] `pnpm check` passes as a single wrapper after this checkpoint.
- [x] `pnpm check:quick` passes after disk cleanup.
- [x] `git diff --check` passes.
- [ ] Final read-only audit returns PASS.

## Follow-Up Read-Only Audit Failure 2026-07-05 Round 2

The follow-up read-only audit returned **FAIL** with one remaining Task 17 issue. The earlier evidence deletion ordering, direct `delete_session` bypass, frontend docs, and worktree classification findings are now closed or classified. This section supersedes the completion interpretation of the implementation checkpoint above.

Audit result:

1. **Task 17 blocker: prune evidence invalidation is still not bound to the actual inner prune deletion set.**
   - Evidence:
     - `crates/jyowo-harness-sdk/src/harness/events.rs`
     - `crates/jyowo-harness-journal/src/jsonl.rs`
     - `apps/desktop/src-tauri/src/commands/runtime.rs`
   - Current behavior:
     - `prune_with_evidence_invalidation` calls `prune_candidate_sessions(...)`, invalidates evidence refs for that predicted set, then calls `inner.prune(...)`.
     - The production desktop runtime uses `JsonlEventStore`.
     - `JsonlEventStore::prune` independently calls `list_sessions(...)`, computes its own `cutoff`, computes its own candidates, then removes segment files.
     - The wrapper candidate set and the inner deletion set are not a single durable or transactional set.
   - Failure mode:
     - A session can cross the prune cutoff between the wrapper candidate calculation and the inner store candidate calculation.
     - In that case, `inner.prune(...)` can delete event payloads for a session whose evidence refs were not invalidated first.
   - Required behavior:
     - Evidence invalidation must be tied to the exact session ids that the inner prune will delete, or a durable invalidation marker must make matching evidence refs unreadable before any inner prune can remove their event payloads.

Closed findings from the previous audit:

- `EvidenceRefStore::delete_for_conversation` now removes registry read authority before backing blob cleanup.
- Blob cleanup failure no longer preserves readable evidence refs.
- `Harness::event_store().delete_session(...)` routes through evidence invalidation.
- `LifecycleHookEventStore::delete_session(...)` and generic guarded deletion share the evidence lifecycle.
- `docs/frontend/frontend-engineering.md` no longer describes a nonexistent current workbench state file.
- Current untracked source/test files are classified as intended, with no generated artifact noise intentionally included.

Implementation plan:

1. **Make prune use one deletion set.**
   - Prefer adding a backend-supported prune path that accepts or returns the exact candidate session ids to be deleted.
   - Avoid relying on two separate `now()` calls and two separate `list_sessions(...)` snapshots.
   - Keep tenant scoping and `keep_latest_n_sessions` semantics identical to the existing store behavior.

2. **Bind evidence invalidation to that deletion set.**
   - Invalidate evidence refs for the exact session ids before segment files or snapshots are removed.
   - If invalidation fails, do not delete event payloads.
   - If payload deletion later fails, keep refs unreadable and return the prune error.

3. **Update production store behavior, not only SDK wrapper behavior.**
   - Cover the production `JsonlEventStore` path used by desktop runtime.
   - Check other `EventStore::prune` implementations for the same two-snapshot risk.
   - Do not make tests pass by only mocking the wrapper candidate path.

4. **Add targeted regression tests.**
   - Add a test proving the actual inner deletion set is the same set invalidated before deletion.
   - Add a boundary-time or injected-candidate-drift test if the existing abstractions allow it.
   - Keep the test tied to production store behavior, not only a synthetic mock.

5. **Re-run gates and read-only audit.**
   - Re-run targeted prune/evidence tests.
   - Re-run `cargo fmt --all --check`.
   - Re-run `pnpm check:rust` or the narrower Rust gates plus the affected full gate if the change stays in Rust.
   - Re-run `pnpm check:docs` after this plan update.
   - Re-run final read-only audit focused on the actual deletion-set binding.

Checklist:

- [ ] Prune has one authoritative session deletion set for evidence invalidation and event payload removal.
- [ ] `JsonlEventStore` production prune cannot delete a session that was not invalidated first.
- [ ] The implementation no longer depends on two independent `now()` calls for wrapper invalidation and inner deletion.
- [ ] Invalidation failure prevents event payload deletion.
- [ ] Payload deletion failure does not restore evidence read authority.
- [ ] Other `EventStore::prune` implementations are checked for the same mismatch.
- [ ] A production-path regression test covers candidate drift or proves the shared deletion set.
- [ ] Targeted prune/evidence tests pass.
- [ ] `cargo fmt --all --check` passes after the prune fix.
- [ ] Rust gate passes after the prune fix.
- [ ] `pnpm check:docs` passes after this plan update.
- [ ] Final read-only audit returns PASS.

## Follow-Up Implementation Checkpoint 2026-07-05 Round 2

This checkpoint records the fix for the remaining prune/evidence lifecycle blocker from the Round 2 read-only audit.

Production code changes:

1. **Prune now has an exact-session deletion API.**
   - `crates/jyowo-harness-journal/src/store.rs`
   - `EventStore::prune_sessions(...)` deletes exactly the supplied session ids and defaults to fail-closed for stores that do not implement it.
   - `Arc<T>` forwards `prune_sessions(...)` to the wrapped store.

2. **Production stores support the same exact deletion set used by evidence invalidation.**
   - `crates/jyowo-harness-journal/src/jsonl.rs`
   - `crates/jyowo-harness-journal/src/memory.rs`
   - `crates/jyowo-harness-journal/src/sqlite.rs`
   - `crates/jyowo-harness-journal/src/version.rs`
   - Normal `prune(...)` still computes candidates from policy.
   - Payload deletion then runs through `prune_sessions(...)` using that already-computed set.
   - `JsonlEventStore::prune_sessions(...)` removes only the supplied session segment files and snapshots.
   - `SqliteEventStore::prune_sessions(...)` removes only the supplied session rows in one transaction.
   - `InMemoryEventStore::prune_sessions(...)` removes only the supplied session entries.
   - `VersionedEventStore::prune_sessions(...)` delegates to its inner store.

3. **SDK evidence invalidation is bound to the deletion set.**
   - `crates/jyowo-harness-sdk/src/harness/events.rs`
   - `prune_with_evidence_invalidation(...)` now computes candidate sessions once, invalidates evidence refs for exactly those session ids, then calls `inner.prune_sessions(...)`.
   - It no longer calls `inner.prune(...)` after invalidation, so the inner store cannot recompute a wider candidate set with a later `now()` or a different session snapshot.
   - If evidence invalidation fails, event payload deletion is not attempted.
   - If exact payload deletion fails after invalidation, evidence refs stay unreadable and the prune error is returned.

4. **Wrapper exact-prune paths preserve the lifecycle invariant.**
   - `ConversationDeletionGuardEventStore::prune_sessions(...)` invalidates evidence refs for the supplied ids before delegating.
   - `LifecycleHookEventStore::prune_sessions(...)` does the same.

5. **Production-path regression coverage was added.**
   - `crates/jyowo-harness-journal/tests/l1b_stores.rs`
   - `jsonl_prune_sessions_deletes_only_supplied_sessions` proves the production `JsonlEventStore` exact prune path deletes only the supplied session id and leaves another session readable.
   - `crates/jyowo-harness-sdk/src/harness/events.rs`
   - The prune failure test now panics if the old broad `prune(...)` path is used and fails only through `prune_sessions(...)`.

Verification:

- `cargo fmt --all --check`: exit code 0.
- `cargo test -p jyowo-harness-journal --features jsonl --test l1b_stores jsonl_prune_sessions_deletes_only_supplied_sessions`: exit code 0.
- `cargo test -p jyowo-harness-journal --all-features --test l1b_stores`: exit code 0.
- `cargo test -p jyowo-harness-sdk harness::events::tests::prune_invalidates_matching_evidence_refs_before_inner_prune`: exit code 0.
- `cargo test -p jyowo-harness-sdk harness::events::tests`: exit code 0.
- `cargo test -p jyowo-harness-sdk --features testing --test evidence_refs`: exit code 0.
- `cargo test -p jyowo-harness-journal --test evidence_ref_store`: exit code 0.
- `pnpm audit:tests > docs/testing/test-inventory.md`: exit code 0.
- `pnpm check:docs`: exit code 0.
- `pnpm check:rust`: exit code 0.

Round 2 checklist update:

- [x] Prune has one authoritative session deletion set for evidence invalidation and event payload removal.
- [x] `JsonlEventStore` production prune cannot delete a session that was not invalidated first.
- [x] The implementation no longer depends on two independent `now()` calls for wrapper invalidation and inner deletion.
- [x] Invalidation failure prevents event payload deletion.
- [x] Payload deletion failure does not restore evidence read authority.
- [x] Other `EventStore::prune` implementations are checked for the same mismatch.
- [x] A production-path regression test covers candidate drift or proves the shared deletion set.
- [x] Targeted prune/evidence tests pass.
- [x] `cargo fmt --all --check` passes after the prune fix.
- [x] Rust gate passes after the prune fix.
- [x] `pnpm check:docs` passes after this plan update.
- [x] Final read-only audit returns PASS.

Final read-only audit result:

- Result: PASS.
- `EventStore::prune_sessions(...)` is present and defaults to fail-closed for stores that do not implement exact-session pruning.
- `JsonlEventStore`, `InMemoryEventStore`, `SqliteEventStore`, and `VersionedEventStore` use the already-computed or supplied session deletion set instead of recomputing a wider prune set after evidence invalidation.
- `prune_with_evidence_invalidation(...)` computes candidate sessions once, invalidates evidence refs for those ids, then calls `inner.prune_sessions(...)`.
- The old broad `inner.prune(...)` path is locked by a regression test that panics if that path is used.
- Production-path JSONL coverage proves exact-session prune deletes only supplied sessions.

Residual design risk:

- Evidence registry invalidation and event payload deletion are still not one storage transaction across all backends.
- The current fail-closed ordering makes existing refs unreadable before payload deletion.
- Concurrent evidence writes racing with conversation deletion or prune still rely on the upper lifecycle stopping writes for that conversation before deletion begins.

## Task 21 Final Gate Checkpoint 2026-07-05

This checkpoint records the final Task 21 gate run after the Round 2 prune/evidence fix.

Gate results:

- Task 14 forbidden leftover grep: exit code 1 with no output. This is the expected no-match result.
- `pnpm audit:tests > docs/testing/test-inventory.md`: exit code 0.
- `pnpm audit:tests`: exit code 0.
- `pnpm check:testing-docs`: exit code 0.
- `pnpm check`: exit code 0.
- `pnpm check:quick`: exit code 0.
- `git diff --check`: exit code 0.
- `git diff --cached --check`: exit code 0.

Gate coverage notes:

- `pnpm check` executed the release, updater, docs, test architecture, no-fakes, sidecar, desktop, and Rust gates as one wrapper.
- `pnpm check:quick` executed `pnpm check:frontend:fast` and `pnpm check:rust:fast`.
- The explicit docs gates `check:agent-docs`, `check:frontend-docs`, `check:backend-docs`, and `check:testing-docs` ran through the wrappers above.
- `pnpm check:desktop` and `pnpm check:rust` ran through `pnpm check`.

Current worktree classification:

- Tracked modified files are intended implementation, test, architecture doc, plan, or regenerated test inventory changes for Tasks 15-21.
- New source/test/support files are staged as intended `A` entries.
- `git status --short --untracked-files=all` has no `??` entries after staging the intended file set.
- No generated artifact noise is intentionally included in the final file set.

Final audit attempt:

- Result: FAIL.
- Finding 1: `CommandExecutionView` copied stdout and stderr preview while rendering only stdout preview. This violated the visible-output copy requirement.
- Finding 2: this checkpoint still described intended new files as untracked after they had been staged, and did not record `git diff --cached --check`.
- Fix: `CommandExecutionView` now derives rendering and copy text from the same `visibleOutput` value, and this checkpoint now records the staged coherent state.

Task 21 checklist update:

- [x] Update frontend/backend docs to match final inspector, evidence ref, artifact, and composer structure.
- [x] Run the Task 14 forbidden leftover grep.
- [x] Regenerate `docs/testing/test-inventory.md` through `pnpm audit:tests > docs/testing/test-inventory.md`.
- [x] Run full Task 21 gates.
- [x] Classify tracked and untracked files.
- [x] Rerun final read-only audit over the full branch after the audit fixes above.
- [x] Commit `refactor: close conversation workbench audit findings`.

## Task 21 Final Audit Fixes 2026-07-05

The final read-only audit rerun returned **FAIL** after the staged-state fix. The Task 17 prune/evidence path and the Task 18 visible-output copy path passed, but Task 21 and Task 18 still had closure gaps.

Audit findings:

1. **Task 21 closure gap: plan tail still recorded the previous final audit as FAIL.**
   - The plan had recorded staged coherent state and `git diff --cached --check`, but the checklist still required another final audit rerun.

2. **Task 18 error-state gaps in production UI actions.**
   - `CommandExecutionView` showed no visible error when command output page fetch failed.
   - `DiffPane` showed no visible error when full patch page fetch failed.
   - `CommandEvidenceBlock` copied through clipboard without failure handling or visible error state.

Implementation plan:

1. Add visible failure state for command output page fetch.
   - Verification: targeted `CommandExecutionView` test asserts failed fetch renders an error.

2. Add visible failure state for diff patch page fetch.
   - Verification: targeted `DiffPane` test asserts failed fetch renders an error.

3. Add clipboard failure handling for the timeline command evidence block.
   - Verification: targeted `CommandEvidenceBlock` test asserts failed copy renders an error.

4. Regenerate test inventory and rerun gates.
   - Verification: `pnpm audit:tests > docs/testing/test-inventory.md`, `pnpm check:test-architecture`, frontend gates, docs gates, full wrappers, and diff checks.

5. Rerun final read-only audit over the staged diff.
   - Verification: final audit returns PASS and this section is updated.

Checklist:

- [x] Command output page fetch failure is visible in production UI.
- [x] Diff patch page fetch failure is visible in production UI.
- [x] Timeline command evidence copy failure is visible in production UI.
- [x] Targeted tests cover the three failure states.
- [x] `docs/testing/test-inventory.md` is regenerated after the new test file.
- [x] `pnpm check:test-architecture` passes after the new test file.
- [x] `pnpm check:desktop` passes after the frontend fixes.
- [x] `pnpm check` passes after the frontend fixes.
- [x] `pnpm check:quick` passes after the frontend fixes.
- [x] `pnpm check:docs` passes after this plan update.
- [x] `git diff --check` passes after this plan update.
- [x] `git diff --cached --check` passes after staging this plan update.
- [x] Final read-only audit returns PASS.
- [x] Commit `refactor: close conversation workbench audit findings`.

Final verification:

- Targeted Task 18 tests for command output fetch failure, diff patch fetch failure, and timeline command copy failure: exit code 0.
- `pnpm -C apps/desktop lint`: exit code 0.
- `pnpm audit:tests > docs/testing/test-inventory.md && pnpm check:test-architecture`: exit code 0.
- `pnpm check:desktop`: exit code 0.
- `pnpm check`: exit code 0.
- `pnpm check:quick`: exit code 0.
- `git diff --check`: exit code 0.
- `git diff --cached --check`: exit code 0.

Final read-only audit result:

- Result: PASS after this closure update.
- Production code review confirmed Task 17 exact-session prune/evidence invalidation is bound to one deletion set through `prune_with_evidence_invalidation(...)` and `EventStore::prune_sessions(...)`.
- Production code review confirmed Task 18 visible-output copy uses the rendered `visibleOutput` value, command output fetch failures are visible, diff patch fetch failures are visible, and timeline command copy failures are visible.
- Remaining note: evidence ref invalidation and payload deletion are still not one cross-backend transaction; the implemented ordering remains fail-closed because evidence refs are invalidated before payload deletion.
