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
  scopeOptions: Array<'once' | 'run' | 'workspace' | 'session'>
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
  fullOutputRef?: string
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
    fullPatchRef?: string
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
  contentRef?: string
  media?: ArtifactMediaPreview
}
```

Do not invent alternative field names during implementation. If Rust existing names differ internally, map them to this UI-facing projection.

## File Map

Backend contracts:

- Modify `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify `crates/jyowo-harness-contracts/src/events/artifact.rs`
- Modify `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify `crates/jyowo-harness-contracts/tests/core_contracts.rs`
- Create `crates/jyowo-harness-contracts/tests/conversation_workbench_contract.rs`
- Modify `crates/jyowo-harness-contracts/tests/fixtures/conversation_worktree_page.json`

Backend projection and read model:

- Modify `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
- Create `crates/jyowo-harness-journal/tests/conversation_workbench_projection.rs`

Tauri boundary:

- Modify `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify `apps/desktop/src-tauri/src/commands/artifacts.rs`
- Modify `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify `apps/desktop/src-tauri/src/lib.rs`
- Modify `apps/desktop/src-tauri/src/commands/tests.rs`

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
- Modify timeline tests under `apps/desktop/src/features/conversation/timeline/*.test.tsx`
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

- Modify `docs/frontend/frontend-product-ux.md` only if the implementation introduces new normative workbench language not already covered.
- Modify `docs/backend/backend-runtime.md` only if new projection or artifact ownership rules become normative.
- Do not create extra docs unless a gate requires it.

## Task 1: Replace The Conversation Projection Contract

**Files:**

- Modify `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify `crates/jyowo-harness-contracts/src/events/artifact.rs`
- Modify `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify `crates/jyowo-harness-contracts/tests/core_contracts.rs`
- Create `crates/jyowo-harness-contracts/tests/conversation_workbench_contract.rs`
- Modify `crates/jyowo-harness-contracts/tests/fixtures/conversation_worktree_page.json`
- Modify `apps/desktop/src/shared/tauri/commands.ts`
- Modify `apps/desktop/src/shared/tauri/commands.test.ts`

**Design requirement:** Replace the thin timeline projection with typed workbench projection. Do not add a second legacy shape. Do not keep `AssistantSegment::Thinking`.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing Rust contract tests in `conversation_workbench_contract.rs`.

Required tests:

```rust
#[test]
fn conversation_worktree_page_contains_typed_decision_tool_command_diff_and_artifact_refs() {
    let page: ConversationWorktreePage =
        serde_json::from_str(include_str!("fixtures/conversation_worktree_page.json")).unwrap();
    let assistant = page.turns[0].assistant.as_ref().unwrap();
    assert!(assistant.projection_version > 0);
    assert!(assistant.segments.iter().all(|segment| !matches!(segment, AssistantSegment::Thinking(_))));
    assert!(format!("{page:?}").contains("DecisionRequestState"));
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

Adjust exact constructors after reading current type names. The assertions must prove the old `thinking` segment is rejected.

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

- [ ] Step 5: Replace Rust projection structs.

Required Rust-facing changes:

- add `projection_version: u64` and `stream_version: u64` to `AssistantWork`
- remove `AssistantSegment::Thinking`
- replace `ToolPermissionState` with `DecisionRequestState`
- expand `ToolAttempt` exactly per target design
- replace command detail fields with `CommandExecution`
- replace diff detail with `ChangeSet`
- expand artifact segment with `ArtifactRevisionSummary`
- add `UiVisibility` to process steps and force user-safe or withheld rendering

- [ ] Step 6: Replace TypeScript Zod schemas in `commands.ts` with the same shape.
- [ ] Step 7: Update the fixture JSON to the new shape.
- [ ] Step 8: Run failing tests and confirm they fail before implementation, then implement until they pass.

Commands:

```bash
cargo test -p jyowo-harness-contracts conversation_workbench --test conversation_workbench_contract
pnpm vitest run apps/desktop/src/shared/tauri/commands.test.ts
cargo fmt --all --check
cargo check --workspace
pnpm check:desktop
git diff --check
```

- [ ] Step 9: Run Task Reality Check.
- [ ] Step 10: Run read-only subagent audit.
- [ ] Step 11: Commit `refactor: replace conversation projection contract`.

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

- `PermissionRequestedEvent` projects to `DecisionRequestState` with operation, target, risk, reason, policy, scope options, data exposure, confirmation, and evidence refs.
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

## Task 3: Add Evidence Fetch Commands For Large Data

**Files:**

- Modify `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify `apps/desktop/src-tauri/src/commands/artifacts.rs`
- Modify `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify `apps/desktop/src-tauri/src/lib.rs`
- Modify `apps/desktop/src/shared/tauri/commands.ts`
- Modify `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify Tauri command tests in `apps/desktop/src-tauri/src/commands/tests.rs`

**Design requirement:** Large command output, full diff patches, and artifact content are fetched by ref. They are not embedded in `ConversationTurn`.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing tests for three commands:

```text
get_conversation_command_output(conversation_id, output_ref)
get_conversation_diff_patch(conversation_id, patch_ref)
get_artifact_revision_content(conversation_id, artifact_id, revision_id)
```

Required behavior:

- invalid refs fail closed
- refs must belong to the conversation
- output is redacted before return
- response includes `truncated`, `redactionState`, and content type
- frontend Zod rejects oversized payloads

- [ ] Step 4: Implement Rust command contracts and command handlers.
- [ ] Step 5: Add TypeScript command client methods and Zod schemas.
- [ ] Step 6: Run gates.

```bash
cargo test -p jyowo-desktop commands
pnpm vitest run apps/desktop/src/shared/tauri/commands.test.ts
cargo fmt --all --check
cargo check --workspace
pnpm check:desktop
git diff --check
```

- [ ] Step 7: Run Task Reality Check.
- [ ] Step 8: Run read-only subagent audit.
- [ ] Step 9: Commit `refactor: add evidence fetch commands`.

## Task 4: Replace Timeline State With Paged Workbench State

**Files:**

- Modify `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-store.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-source.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline-selectors.ts`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-scroll-controller.ts`
- Modify related tests under `apps/desktop/src/features/conversation/timeline/*.test.tsx`

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
pnpm vitest run apps/desktop/src/features/conversation/timeline/conversation-timeline-store.test.ts
pnpm vitest run apps/desktop/src/features/conversation/timeline/use-conversation-timeline.test.tsx
pnpm vitest run apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: replace timeline state with paged workbench state`.

## Task 5: Build Workbench Shell And Inspector State

**Files:**

- Modify `apps/desktop/src/app/shell/AppShell.tsx`
- Modify `apps/desktop/src/app/shell/AppShell.test.tsx`
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

- [ ] Step 4: Implement `WorkbenchSelection`.

Required shape:

```ts
type WorkbenchSelection =
  | { kind: 'context' }
  | { kind: 'decision'; conversationId: string; requestId: string }
  | { kind: 'tool'; conversationId: string; toolUseId: string }
  | { kind: 'command'; conversationId: string; outputRef?: string; eventRef?: ConversationEventRef }
  | { kind: 'diff'; conversationId: string; changeSetId: string }
  | { kind: 'artifact'; conversationId: string; artifactId: string; revisionId?: string }
```

- [ ] Step 5: Replace disabled more-actions placeholder with useful layout controls.
- [ ] Step 6: Keep React state local to UI selection. Do not store policy decisions in UI store.
- [ ] Step 7: Run gates.

```bash
pnpm vitest run apps/desktop/src/app/shell/AppShell.test.tsx
pnpm vitest run apps/desktop/src/features/workbench/WorkbenchInspector.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: add workbench inspector shell`.

## Task 6: Replace Permission UI With DecisionPanel

**Files:**

- Delete `apps/desktop/src/features/conversation/timeline/permission-inline-panel.tsx`
- Create `apps/desktop/src/features/conversation/evidence/DecisionPanel.tsx`
- Create `apps/desktop/src/features/conversation/evidence/DecisionPanel.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/tool-attempt-row.tsx`
- Modify `apps/desktop/src/features/context/ContextPanel.tsx`
- Modify `apps/desktop/src/features/context/ContextPanel.test.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.permission.test.tsx`

**Design requirement:** Permissions are user-visible decisions with operation, target, risk, reason, policy, evidence, data exposure, and scope. Buttons must name the action.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing tests.

Required tests:

- pending write request shows target, risk, reason, policy, data exposure, and scope options
- high-risk request requires exact confirmation text
- approve button text includes operation and scope
- deny button remains available without confirmation
- submitted state disables duplicate actions
- failed submission keeps request visible with retry-safe state
- panel uses `aria-live` for state changes

- [ ] Step 4: Implement `DecisionPanel`.
- [ ] Step 5: Wire `ToolAttemptRow` and `WorkbenchInspector` to the same component.
- [ ] Step 6: Ensure `resolvePermission` receives only request id, decision, selected scope, and confirmation text. React must not send policy fields.
- [ ] Step 7: Run gates.

```bash
pnpm vitest run apps/desktop/src/features/conversation/evidence/DecisionPanel.test.tsx
pnpm vitest run apps/desktop/src/features/conversation/timeline/conversation-timeline.permission.test.tsx
pnpm vitest run apps/desktop/src/features/context/ContextPanel.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: replace permission UI with decision panel`.

## Task 7: Implement Tool And Command Evidence Views

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
pnpm vitest run apps/desktop/src/features/conversation/evidence/ToolInvocationCard.test.tsx
pnpm vitest run apps/desktop/src/features/conversation/evidence/CommandExecutionView.test.tsx
pnpm vitest run apps/desktop/src/features/conversation/timeline/conversation-timeline.render.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: implement tool and command evidence views`.

## Task 8: Implement ChangeSet Summary And Diff Pane

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
pnpm vitest run apps/desktop/src/features/conversation/evidence/ChangeSetSummary.test.tsx
pnpm vitest run apps/desktop/src/features/conversation/evidence/DiffPane.test.tsx
pnpm vitest run apps/desktop/src/features/conversation/timeline/conversation-timeline.render.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: implement changeset diff pane`.

## Task 9: Implement Artifact Revision Workspace

**Files:**

- Modify `crates/jyowo-harness-contracts/src/events/artifact.rs`
- Modify `apps/desktop/src-tauri/src/commands/artifacts.rs`
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

- artifact created event produces revision 1
- artifact updated event produces a new revision id
- artifact pane lists revisions newest-first
- image preview uses media preview ref
- HTML preview uses sandboxed iframe with no same-origin privilege
- code/document/data preview fetches content by ref
- failed artifact shows error state without broken image
- copy/download actions use backend-provided content refs

- [ ] Step 4: Add artifact revision fields to event and projection contracts.
- [ ] Step 5: Implement artifact revision lookup in Tauri command layer.
- [ ] Step 6: Implement `ArtifactPane` and timeline open action.
- [ ] Step 7: Run gates.

```bash
cargo test -p jyowo-harness-contracts artifact --test core_contracts
cargo test -p jyowo-desktop artifacts
pnpm vitest run apps/desktop/src/features/artifacts/ArtifactPane.test.tsx
pnpm vitest run apps/desktop/src/features/artifacts/ArtifactPreview.test.tsx
cargo fmt --all --check
cargo check --workspace
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: add artifact revision workspace`.

## Task 10: Redesign Composer As A Command Input Surface

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
pnpm vitest run apps/desktop/src/features/conversation/Composer.test.tsx
pnpm vitest run apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx
pnpm check:desktop
git diff --check
```

- [ ] Step 8: Run Task Reality Check.
- [ ] Step 9: Run read-only subagent audit.
- [ ] Step 10: Commit `refactor: redesign composer command input`.

## Task 11: Remove Legacy Timeline Components And Close Accessibility Gaps

**Files:**

- Delete `apps/desktop/src/features/conversation/timeline/thinking-panel.tsx`
- Delete replaced permission and diff components if still present
- Modify `apps/desktop/src/features/conversation/timeline/assistant-work-view.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/process-status-row.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/tool-evidence-summary.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/user-attachment-strip.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/artifact-segment-view.tsx`
- Modify `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`
- Modify or create stories for new evidence/workbench components

**Design requirement:** Remove old compatibility UI. Finish focus, aria, image dimensions, and story coverage for the new surfaces.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Add failing tests or Storybook stories for:

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

- [ ] Step 4: Remove legacy UI paths.
- [ ] Step 5: Add visible focus styles through existing shared button primitives or equivalent local classes.
- [ ] Step 6: Add width/height or stable aspect ratio for images and media.
- [ ] Step 7: Confirm semantic tokens are used. Terminal surfaces may use terminal tokens; do not hardcode arbitrary white/black outside that surface.
- [ ] Step 8: Run gates.

```bash
pnpm vitest run apps/desktop/src/features/conversation/timeline/conversation-timeline.render.test.tsx
pnpm vitest run apps/desktop/src/features/conversation/timeline/conversation-timeline.artifacts.test.tsx
pnpm vitest run apps/desktop/src/features/conversation/timeline/conversation-timeline.redaction.test.tsx
pnpm check:desktop
pnpm check:test-architecture
git diff --check
```

- [ ] Step 9: Run Task Reality Check.
- [ ] Step 10: Run read-only subagent audit.
- [ ] Step 11: Commit `refactor: remove legacy timeline UI`.

## Task 12: Update Product And Runtime Docs If Needed

**Files:**

- Modify `docs/frontend/frontend-product-ux.md` only if needed
- Modify `docs/frontend/frontend-engineering.md` only if needed
- Modify `docs/backend/backend-runtime.md` only if needed
- Modify `docs/testing/testing-strategy.md` only if new test categories or gates are required

**Design requirement:** Docs must match the implemented architecture. Do not create docs that describe future work. Do not write temporary plans into normative docs.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Read required context and every file above.
- [ ] Step 3: Compare implemented architecture against existing docs.
- [ ] Step 4: Update only normative gaps:
  - workbench inspector ownership
  - typed evidence projection
  - artifact revision workspace
  - no raw thinking in frontend state
  - large output refs instead of embedded payloads
- [ ] Step 5: Run gates.

```bash
pnpm check:docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:testing-docs
git diff --check
```

- [ ] Step 6: Run Task Reality Check.
- [ ] Step 7: Run read-only subagent audit.
- [ ] Step 8: Commit `refactor: document conversation workbench architecture`.

## Task 13: End-To-End Verification And Cleanup

**Files:**

- Modify only files needed to fix verification failures.
- Do not add new feature scope in this task.

**Design requirement:** The final branch must be coherent. No old thin chat implementation remains. No compatibility projection remains. All safety boundaries remain backend-owned.

- [ ] Step 1: Run the Task Intent Check.
- [ ] Step 2: Search for forbidden leftovers.

```bash
rg -n "ThinkingPanel|AssistantSegment::Thinking|ToolPermissionState|permission-inline-panel|JSON.stringify\\(segment\\)|visibleLines\\.map\\(formatDiffLineForCopy\\)|h-8 w-full resize-none|legacyThinking|legacy.*conversation" apps crates
```

Expected:

- no production references to removed compatibility UI or old projection names
- test names may mention legacy only when asserting rejection

- [ ] Step 3: Run full gates.

```bash
pnpm check
pnpm check:docs
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:desktop
pnpm check:rust
pnpm audit:tests
pnpm check:test-architecture
pnpm check:testing-docs
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
