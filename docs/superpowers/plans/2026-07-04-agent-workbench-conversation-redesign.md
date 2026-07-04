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
