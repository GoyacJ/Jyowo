# Workspace Lease Release Design

## Problem

A completed or removed task can retain an active `current` workspace write lease. A new task targeting the same workspace receives a waiting lease, and its first `submit_message` is rejected. The daemon reduces the rejection to `invalid_command`, so the desktop cannot show the underlying `workspace is busy` message.

## Design

Keep a foreground workspace lease while a run is active or queued messages remain. Release all leases owned by the task after a terminal run leaves no queued work, and after an accepted task removal. This keeps queued segment promotion on the same lease while allowing the next task to acquire the workspace immediately.

Add an optional rejection message to `CommandRejected`. Populate it for `InvalidCommand` and prefer it in the desktop error. Existing structured rejection reasons remain unchanged for retry and version-conflict handling.

Because the application is still in development, remove the current daemon data directory once. No migration for malformed historical leases is required.

## Verification

- A task completes with no queued messages and its active workspace lease becomes released.
- A second task using the same workspace can submit immediately.
- Removing an idle task releases any nonterminal lease it owns.
- An invalid command surfaces its concrete daemon message in the composer.
- Existing task actor, IPC, protocol generation, and desktop tests remain green.
