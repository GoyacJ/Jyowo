# Daemon Runtime Migration Design

## Goal

Finish the migration from the deleted desktop conversation runtime to the task daemon. The daemon becomes the only authority for task execution, tools, memory, automation runs, permissions, and child agents. Tauri remains responsible for non-task settings persistence and desktop integration.

## Ownership

Tauri owns settings CRUD for global and project configuration:

- provider profiles and credentials;
- provider capability routes;
- execution defaults and project overrides;
- MCP server definitions;
- plugin and skill installation and selection;
- reusable agent profiles;
- automation specifications.

The daemon reads those canonical files directly. It resolves one immutable runtime snapshot when a run starts. No task execution capability is assembled in Tauri, and no settings snapshot or credential is sent over task IPC.

Global configuration lives under `~/.jyowo/config`. Project configuration lives under `<workspace>/.jyowo/config`. Missing project records inherit global settings. Invalid configured capabilities fail the run with a bounded diagnostic instead of silently using a different tool set.

## Daemon Runtime Assembly

Introduce a daemon runtime configuration assembler. For each task run it receives the task workspace and persisted model configuration ID, then resolves:

- provider and model configuration;
- merged global defaults and project execution overrides;
- global and project provider capability routes;
- global and project MCP servers;
- enabled global and project plugins;
- enabled global and project skills;
- global agent profiles and project profile selection;
- brokered host capabilities available to the daemon process.

The assembler builds the SDK `Harness` with the same effective capability set used for execution. Desktop tool status must read a daemon-provided runtime description rather than the deleted settings runtime registry.

Runtime snapshots are immutable for one run. Settings changes affect the next run, not an active run.

## Memory

The daemon owns memory databases used by task runs and the Memory management API. Memory files stay under the daemon's Jyowo home instead of under an untrusted workspace.

- Workspace tasks use `~/.jyowo/runtime/workspaces/<blake3(canonical-workspace)>/memory/memory.sqlite3`.
- Tasks without a workspace use `~/.jyowo/runtime/memory/memory.sqlite3`.

Memory management requests are added to the daemon protocol. Tauri memory commands become thin daemon bridges or are removed in favor of the shared daemon client. The desktop settings runtime must not open a second memory database.

The workspace key is derived from the canonical workspace path, so accepted canonical-equivalent paths share one database and different workspaces remain isolated. Directory creation is anchored at the canonical Jyowo home and rejects symlink components. Replacing `<workspace>/.jyowo/runtime` cannot redirect SQLite or its WAL files. This prevents an untrusted workspace from choosing the daemon write target; it is not an isolation boundary against the same user replacing files inside the daemon's own home. Existing workspace or deleted conversation runtime memory is not migrated because there is no compatibility requirement.

## Background Agents and Task Listing

Background agents remain durable detached child tasks.

The Background Agents screen uses daemon task requests for listing, loading, input, stopping, continuing, archiving, and removing detached children. The daemon projection exposes child attachment mode so a client does not need to infer detachment by joining parent projections.

The ordinary task list excludes every child task. The Background Agents screen shows only detached child tasks.

## Automation

Automation specifications remain settings records. Execution moves into a daemon scheduler.

The scheduler:

- loads enabled global and project automation specifications;
- supports explicit run-now requests;
- creates normal daemon tasks and submits the saved prompt;
- records the created task and run identity in durable automation run history;
- prevents overlapping active runs for one automation;
- evaluates interval schedules from the last committed run;
- recovers after daemon restart;
- applies `skip` or `run_once` missed-run policy;
- records rejected and failed attempts with bounded diagnostics.

Automation protocol requests cover list, save, enable, delete, run now, and run history. This makes scheduler state changes visible to all connected windows and keeps run execution independent of Tauri lifetime.

## Removed Legacy Surface

Delete the obsolete conversation runtime surface instead of adding compatibility adapters:

- `/evals` and its navigation command;
- legacy conversation creation, listing, run, replay, activity, and inspector APIs;
- legacy artifact, attachment preview, evidence, and support-bundle APIs that depend on conversation projections;
- obsolete artifact/evidence/workbench/WelcomeWorkspace UI paths;
- background-agent command contracts superseded by daemon child tasks;
- test command-client implementations and fixtures for removed commands.

Only active settings commands and the versioned daemon bridge remain in the Tauri command client.

## Error Handling and Security

- Configuration parse and validation errors fail closed with redacted messages.
- Provider credentials remain in local configuration stores and are never serialized into task events or IPC responses.
- Workspace configuration and memory paths must remain within the canonical workspace.
- Invalid MCP, plugin, or skill configuration cannot fall back to a separate desktop runtime.
- Automation execution uses normal daemon permission, workspace lease, queue, and recovery rules.

## Verification

Add executable gates for:

- every `CommandClient` invoke name being present in Tauri `generate_handler!`;
- active routes containing no removed conversation/eval/background command calls;
- daemon run assembly resolving global and project tool configuration;
- Memory UI and task runs opening the same database for a workspace;
- ordinary task lists excluding child tasks and Background Agents selecting detached children;
- automation run-now, overlap rejection, interval scheduling, restart recovery, and missed-run policy;
- generated daemon protocol consistency;
- absence of deleted conversation runtime source and test fixtures.

The final gate includes affected Rust suites, the desktop Vitest suite, type checking, linting, formatting, architecture scripts, and a clean generated protocol diff.
