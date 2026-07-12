# Runtime Tools Settings Design

## Purpose

The Tools settings page must show the complete tool catalog registered in the desktop settings runtime and the capability status that applies to that catalog.

The task daemon remains the sole owner of task execution. It does not become a second owner of the global settings catalog.

## Ownership

`DesktopSettingsRuntime` already owns non-task configuration and catalog APIs that have not moved into the daemon protocol. Its `ToolRegistry` includes built-in, MCP, plugin, and skill-provided tools loaded for the active desktop settings scope.

Two read-only Tauri commands expose that state:

- `get_runtime_execution_status`
- `list_runtime_tools`

These are settings queries, not legacy task-runtime commands. They are registered in the active Tauri handler and implemented in a dedicated runtime-tools settings module.

## Data flow

```text
Tools settings page
  -> CommandClient
  -> read-only Tauri settings command
  -> ManagedDesktopRuntime
  -> DesktopSettingsRuntime
  -> capability status or ToolRegistry snapshot
```

The renderer does not infer capabilities or maintain a static tool list. The daemon does not duplicate the settings registry.

## Catalog response

`list_runtime_tools` reads one registry snapshot and returns its generation plus sorted summaries. Each summary preserves the existing UI contract:

- identity, display name, description, category, and group;
- origin and origin identifier;
- read-only, mutating, or destructive access;
- execution channel and required capabilities;
- defer policy, long-running status, and service binding.

Mapping uses descriptor metadata and typed enums. Compatibility fallbacks are limited to non-exhaustive upstream enum variants and map to `custom`; there is no separate static catalog.

## Runtime status

`get_runtime_execution_status` delegates to `DesktopSettingsRuntime::runtime_execution_status()`. The backend remains authoritative for sandbox, broker, and per-tool availability.

If the settings runtime has not initialized, both commands return the structured `RUNTIME_NOT_READY` command error.

## Error reporting

Tauri can reject an invocation with a primitive string. The renderer error formatter preserves that string instead of replacing it with `Unknown command error`. Structured errors and JavaScript `Error` instances keep their current behavior.

## Removed obsolete design

The architecture test no longer classifies these two settings queries as forbidden legacy task commands. Conversation and run commands remain forbidden. No compatibility command, daemon-side mirror, static frontend catalog, or duplicate registry is retained.

## Verification

Tests cover:

- handler registration and the remaining legacy-command boundary;
- runtime-not-ready behavior;
- complete descriptor-to-summary mapping and stable sorting;
- status delegation to the settings runtime;
- frontend invocation names and primitive-string error preservation;
- Tools settings page success and failure rendering.

