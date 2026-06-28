# Codex Style Conversation Evidence UI Supplement

This supplement completes the acceptance surface for `docs/plans/2026-06-28-codex-style-conversation-evidence-ui.md`.

It does not replace the original plan. It records the implementation state, adds hard acceptance criteria, and defines the gates required before this design is considered complete.

## Implementation Status

| Area | Status | Evidence |
| --- | --- | --- |
| Two-turn conversation context | 已实现 | Rust regression covers previous user, previous assistant, and current user in the second `ModelRequest.messages`. |
| Provider persona | 已实现 | Default Jyowo system prompt forbids self-identifying as the underlying provider. |
| Desktop `startRun` conversation reuse | 已实现 | `ConversationWorkspace` submits through `useConversationTimeline`; `startRun` receives `renderedConversation.id`. |
| Provider self-introduction fixture | 已实现 | Timeline fixtures no longer contain provider self-introduction as assistant prose. |
| Process history collapse | 已实现 | `ProcessStep[]` is converted to internal display items; completed file read, file search, and historical successful commands collapse into groups. |
| Forced-visible process steps | 已实现 | running, failed, and non-zero command steps remain visible and open. |
| Tool evidence summary | 已实现 | `ToolEvidenceSummary` is a disclosure entry for completed low-signal attempts. |
| Forced-visible tool attempts | 已实现 | failed, denied, running, and permission-pending attempts remain visible. |
| Completed tool attempt grouping | 已实现 | completed attempts default to hidden and use group-level disclosure state in `shared/state/ui-store.ts`. |
| Failed tool summary duplication | 已实现 | failed summary renders once on the failed attempt row. |
| zh-CN runtime copy | 已实现 | fixed runtime copy avoids `Activity` and English approval text in the zh-CN main canvas. |
| Run complete with failed step | 已实现 | UI must show the ended run as `已结束但存在失败步骤` when the run ended but a step failed. |
| Composer bottom reserve | 已实现 | timeline scroll content reserves bottom padding for the composer. |
| Redaction false positives | 已实现 | private absolute path fragments are redacted without replacing ordinary prose; obvious secrets still fail closed. |
| Public contract changes | 未实现 / 不需要 | Current fields express the required states. No new `code` field was added. |
| Regression risk | 回归风险 | Localization and disclosure defaults can regress through fixture or Storybook changes; keep the tests below. |

## Hard Acceptance Criteria

- Second-turn model requests include prior user, prior assistant, and current user messages.
- The default Jyowo prompt prevents provider identity leakage.
- The desktop submit path reuses the current conversation id.
- The main zh-CN conversation canvas does not show fixed English runtime copy such as `Activity` or `The runtime requires approval before continuing.`
- Historical completed process steps are grouped and collapsed by default.
- Failed, running, permission-pending, and non-zero command steps are visible by default.
- Completed low-signal tool attempts are hidden behind `ToolEvidenceSummary`.
- Tool failure summary appears once.
- A completed run with failed steps is not displayed as a clean success; it shows `已结束但存在失败步骤`.
- The composer does not cover the last conversation turn.
- Redaction protects secrets and private absolute paths without replacing ordinary PRD or prose blocks.

## Design Constraints

The UI view-model is frontend-internal:

```ts
type ProcessDisplayItem =
  | { kind: 'step'; step: ProcessStep }
  | {
      kind: 'group'
      id: string
      titleKey: 'timeline.processGroup.commandHistory' | 'timeline.processGroup.history'
      steps: ProcessStep[]
      defaultOpen: boolean
    }
```

It must not enter the Tauri contract.

Group titles must be rendered through i18n keys, not hard-coded runtime copy.

Fold state stays UI-only in `shared/state/ui-store.ts`. Keys must include conversation, run, segment, and group identity.

Rust remains responsible for facts, safe projection, and redaction. React only renders the projected state.

Do not solve provider self-introduction by regex-cleaning model output. Fix prompt assembly, fixtures, and tests.

## Verification Matrix

Rust:

```text
CARGO_TARGET_DIR=/Users/goya/Repo/Git/Jyowo/target cargo test -p jyowo-harness-contracts ui_safe_text_redacts_private_paths_and_obvious_secrets
CARGO_TARGET_DIR=/Users/goya/Repo/Git/Jyowo/target cargo test -p jyowo-harness-journal --test conversation_worktree_projector
CARGO_TARGET_DIR=/Users/goya/Repo/Git/Jyowo/target cargo test -p jyowo-harness-sdk --features testing default_conversation_system_prompt_keeps_jyowo_identity --test runtime_assembly
CARGO_TARGET_DIR=/Users/goya/Repo/Git/Jyowo/target cargo test -p jyowo-harness-session run_turn_sends_previous_user_and_assistant_messages_to_next_model_request --test run_turn
pnpm check:rust
```

Frontend:

```text
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
pnpm -C apps/desktop build-storybook
pnpm check:desktop
```

Docs:

```text
pnpm check:docs
```

Final gate:

```text
pnpm check
```

## Storybook Coverage

- `CodexEvidenceCollapsedHistory`
- `CodexEvidenceCompletedRunWithFailedStep`
- `CodexEvidencePermissionPending`
- `CodexEvidenceRepeatedSearchFailures`
- `CodexEvidenceBottomComposerOverlap`
