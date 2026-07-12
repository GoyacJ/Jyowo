# Codex-style task workspaces and sidebar design

## Goal

Fix daemon task execution and replace the state-based task list with a Codex-style workspace sidebar.

Users can create conversations in a safe default workspace or inside an added project. The sidebar has independently collapsible Pinned, Projects, and Conversations sections.

## Workspace model

- The default workspace root is `~/.jyowo/workspaces/default`.
- A project conversation uses the selected project directory as its workspace root.
- Every foreground task acquires a durable current-workspace lease before its first run starts.
- The lease is reused by later runs for the same task while valid.
- Task creation remains durable even if workspace acquisition fails. A rejected submit surfaces the acquisition error instead of recording an opaque failed run.

## Task metadata

The daemon task projection is the source of truth for sidebar state. It stores:

- workspace selection;
- pinned state;
- archived state;
- editable title;
- last activity offset.

Daemon commands provide rename, pin/unpin, archive/unarchive, and delete operations. These mutations use the existing command metadata, optimistic stream version, and idempotency rules.

## Sidebar information architecture

The expanded sidebar contains:

1. A primary `New conversation` action. It creates a task in the default workspace.
2. Pinned. It contains pinned, non-archived tasks from all workspaces.
3. Projects. It contains registered project folders. Each project is collapsible and contains its non-archived tasks. A project action creates a task in that project.
4. Conversations. It contains non-pinned, non-archived tasks in the default workspace.

Pinned, Projects, and Conversations remember their independent collapsed state. Project rows also remember whether their child task list is expanded.

Project controls support adding an existing folder, renaming its display label, manual ordering, and removing it from the sidebar without deleting the folder. Task controls support pinning, renaming, archiving, and permanent deletion. Archived tasks are excluded from the three primary sections.

Compact sidebar mode retains the global new-conversation action and task selection affordances without rendering section labels.

## Data flow

Project records continue to come from the desktop project store. Daemon task projections are joined to projects by canonical workspace root. Tasks whose root is the default workspace belong to Conversations. Tasks whose root matches a registered project belong to that project. Unknown roots are shown under Conversations so tasks never disappear because project configuration changed.

Creating a task performs these steps:

1. Resolve or create the target workspace directory.
2. Send `create_task` with the canonical root.
3. Acquire and persist a current-workspace lease for the new task.
4. Navigate to the accepted task.

Submitting a message verifies that an active lease exists before committing `run.started`. If a lease cannot be acquired, the command is rejected with a user-visible reason and no failed run is created.

## Error handling

- Project and task mutation errors render next to the affected sidebar section.
- Submit command rejection remains in the composer and preserves the draft.
- Runtime failures include a durable error timeline item rather than only `Run failed`.
- Removing a project does not delete tasks or files. Its tasks fall back to the Conversations section until the project is added again.

## Testing

- Rust contract and daemon tests cover workspace acquisition, lease reuse, failure rejection, and task metadata commands.
- React tests cover grouping, independent collapse state, default/project task creation, project actions, task actions, and Chinese labels.
- Existing task composer tests verify rejected submissions preserve drafts.
- A desktop browser smoke test creates one default task and one project task, sends a message, folds all three sections, and verifies persistence after rerender.

