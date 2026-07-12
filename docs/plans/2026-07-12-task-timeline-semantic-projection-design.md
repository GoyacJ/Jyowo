# Task Timeline Semantic Projection Design

## Problem

The daemon persists complete engine output, but the task timeline projects every `engine.*` event as a generic notice. Assistant text is discarded, internal lifecycle events appear in the primary conversation, engine events are not associated with their run segment, and the desktop keeps rendering stale task state from the initial snapshot.

The workbench compounds the problem by retaining an unrelated event selection when the user switches panels, so a non-command event can be shown under the Commands tab with an empty result.

## Product behavior

The primary timeline is a semantic conversation surface, not a raw event log.

It displays:

- user messages;
- assistant narrative;
- user-relevant tool, command, change, image, permission, compaction, subagent, and error activity;
- one run status boundary per segment.

Internal lifecycle and accounting events such as session creation, engine run start/end, duplicated engine user-message append, token usage, and raw stream completion stay available in Audit but do not create primary timeline rows.

The header, run segment, queue, permission surface, workbench, and sidebar must reflect committed live events without waiting for a reconnect or event gap.

## Architecture

### Canonical daemon projection

`jyowo-harness-journal` remains the authoritative snapshot projector. Engine events are matched by their typed `Event` variant rather than by an `engine.*` string fallback.

Assistant text deltas produce `assistant_text` timeline items containing the text chunk, engine run ID, and message ID. The completion event marks the message complete without duplicating the accumulated text. If a provider emits a completed message without text deltas, the completion event creates one assistant text item from the final content.

The timeline contract gains an optional semantic group identifier for content that must be coalesced without crossing message boundaries. Existing non-message items leave it absent.

Engine lifecycle/accounting events that have no user-facing meaning do not enter `timeline_projection`. Raw envelopes remain in `event_log`, so Audit remains complete.

Tool and artifact engine events are projected only when the UI can give them a stable user-facing kind and summary. Unsupported or internal variants remain audit-only instead of degrading to generic notices.

### Live desktop projection

The desktop uses one dedicated task projection module instead of component-local event fallbacks. It derives:

- the live `TaskProjection` fields needed by the UI;
- queue state;
- semantic timeline additions;
- completion and replacement of streamed assistant groups.

The reducer starts from the authoritative snapshot and applies only contiguous committed envelopes after `snapshotOffset`. It uses the same event-type mapping and grouping rules as the daemon. Unknown events are ignored by the primary UI and remain available to Audit.

`TaskWorkspace` consumes this derived view. It no longer reads stale snapshot state directly or contains a generic `engine.*` notice fallback.

### Streaming behavior

Adjacent assistant text chunks are grouped by both run segment and semantic group ID. During streaming, the latest group is incomplete. Completion changes the group to complete without rendering the final full message a second time.

The scroll anchor tracks changes to the grouped narrative length, so token batches continue autoscrolling only while the user is near the bottom.

### Workbench behavior

Timeline rows open only their compatible workbench panel. Changing tabs clears event-specific identity and artifact state when the selected event is not valid for the destination panel. Commands never present a run/session/audit event as though it had command output.

Audit consumes raw task envelopes and can inspect internal lifecycle events without exposing them in the primary timeline.

### Localization

Visible task status, connection state, run labels, timeline summaries, workbench labels, and empty states use the existing `shell` translation namespace. Protocol event names and identifiers remain untranslated only inside Audit.

## Error handling and recovery

- Invalid or unknown engine payloads do not fabricate conversation rows.
- A snapshot/event gap still triggers an authoritative resnapshot.
- Duplicate or replayed envelopes remain idempotent by global offset.
- A completed message without prior deltas remains visible from its final content.
- Interrupted output keeps the last committed assistant chunks and marks the group incomplete.

## Testing

Tests cover the complete path rather than hand-authored `assistant_text` fixtures:

1. Typed engine delta/completion events project to canonical Rust timeline rows.
2. Internal engine lifecycle events remain absent from the primary timeline and present in the raw event stream.
3. Rebuilding projections produces the same grouped assistant result.
4. The TypeScript live reducer turns real daemon envelopes into assistant narrative and current task state.
5. Completion does not duplicate streamed text; completion without deltas still renders.
6. Run IDs and message group IDs prevent cross-run or cross-message coalescing.
7. Workbench panel changes cannot retain an incompatible event/blob selection.
8. Chinese UI snapshots contain no hard-coded English task chrome.
9. A real browser fixture sends a message and verifies the response text, final state, clean primary timeline, Audit access, and zero console errors.

## Rejected approaches

### Frontend-only transformation

This would leave daemon snapshots semantically wrong and make reconnects differ from live rendering.

### Resnapshot after every event batch

This would hide the stale-state bug at the cost of extra daemon traffic and poor streaming behavior.

### CSS hiding of internal events

This would preserve incorrect projection data, missing assistant content, invalid run grouping, and misleading workbench selection.
