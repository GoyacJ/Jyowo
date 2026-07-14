# Skill Module Hardening Design

**Date:** 2026-07-13

**Status:** Approved

## Objective

Close the current skill-module gaps across Desktop, Tauri, Daemon, SDK, context recovery, registry, catalog, package loading, and script execution without changing the settings page's global scope.

## Product Decisions

- Skill settings remain global. The UI does not add a workspace/project scope selector.
- Non-secret configuration is stored in `~/.jyowo/config/skill-config.json`.
- Secret configuration is stored in the operating-system keychain. The JSON file stores only whether a secret is configured.
- Selecting a skill reference in Composer injects the rendered skill body into the first task turn.
- Skill references are persisted and participate in task recovery.
- `skills_invoke` never executes package scripts.
- Scripts are executed only through the explicit `skills_run_script` capability with its own permission and sandbox policy.
- MCP prompts are not represented as local skills. `McpSkillRecord` remains an extension boundary, but Desktop does not advertise a production MCP skill source that it does not provide.

## Configuration Contract

The shared configuration document is versioned and keyed by canonical skill identity. Public values and secret-presence metadata are serialized; secret values are never serialized.

```json
{
  "version": 1,
  "skills": {
    "user/example": {
      "values": {
        "region": "cn-east"
      },
      "secrets": {
        "apiToken": {
          "configured": true
        }
      }
    }
  }
}
```

Tauri and Daemon use the same read/merge/write contract. Secret access is abstracted behind a store interface so tests use an in-memory fake and production uses the system keychain. Clearing a secret deletes its keychain entry and updates the public document atomically.

Required configuration is evaluated per skill. A missing value makes only that skill unavailable and returns a typed prerequisite error. It must not prevent session creation or block unrelated skills.

Secret values are excluded from rendered model context, event payloads, logs, receipts, previews, and error text. A secret can enter a child-process environment only when a script declaration explicitly maps that configuration key to an environment variable.

## Typed Skill References and First-Turn Injection

`contextReferences` becomes a versioned tagged structure. Deserialization accepts legacy string entries and normalizes them to `WorkspaceFile`, matching the existing Daemon interpretation of string references as workspace paths.

```text
ContextReference::Skill {
  version,
  skill_id,
  parameters,
  source,
}
```

Composer candidates come from Daemon using the effective runtime configuration for the current workspace. Selecting a candidate stores only identity, source metadata, and non-secret parameters.

At task start, Daemon creates one turn-level registry snapshot. Candidate resolution, visibility checks, parameter validation, rendering, and first-turn context assembly all use that same snapshot. Missing required parameters return a specific validation error; this change does not add a parameter-form builder to Composer.

## Durable Recovery Model

Skill context injection uses the following persisted lifecycle:

```text
prepared -> context assembled -> provider accepted -> consumed
```

The prepared event contains the typed reference, non-secret parameters, source metadata, and a hash of the rendered body. It never contains the rendered body or a secret. Recovery resolves the reference against the current effective runtime, renders it again, and compares the hash before continuing. A mismatch stops recovery with a deterministic integrity error.

The delivery guarantee is at-least-once. A crash after provider acceptance but before the consumed marker can repeat the injection. The system must not silently lose an injection.

Each delivery uses a stable key derived from task ID, queue item ID, queue revision, and reference index. A queue edit therefore creates a distinct delivery identity. `provider accepted` means `model.infer(...)` returned a model stream successfully; it does not mean request construction started or that the first token arrived. The accepted and consumed events are persisted separately so the approved crash window remains observable.

## Registry and Hook Consistency

Registry mutations hold one write lock for the complete read-modify-write operation. Each canonical name retains an ordered candidate stack rather than only the winning item. Removing a higher-priority candidate reveals the next valid lower-priority candidate.

Hook handler identity includes source identity and a declaration fingerprint. Replacement is transactional from the registry's perspective: register the new handler successfully, publish it, then remove the old handler. A failed replacement leaves the previous handler active.

HTTP hook transport keeps the existing security validation. mTLS declarations are rejected explicitly until a real certificate source is implemented. Loader rendering receives the session renderer and its effective policy instead of constructing a policy-free rendering path.

## Explicit Script Runner

Skill frontmatter declares runnable scripts by stable ID:

```yaml
scripts:
  - id: collect
    path: scripts/collect.sh
    timeoutSeconds: 30
    network: deny
    env:
      API_TOKEN:
        config: apiToken
        secret: true
```

`skills_run_script` accepts a skill reference, declared script ID, and structured arguments. It rejects undeclared IDs, paths outside the package, symlinks that escape the package, unsupported network policies, missing permissions, and missing declared configuration.

Execution reuses the production process sandbox. If the host cannot enforce a requested isolation property, execution is rejected. The runner enforces an independent permission, timeout, bounded stdout/stderr, artifact limits, and cancellation. It reports only enforced properties. The legacy `execute_skill_script` path delegates to this runner or is removed from public capability exposure; it no longer reports unenforced `network_enabled` or `memory_mb` fields.

## Package Integrity and Catalog Operations

Runtime loading recomputes the installed package hash and compares it with the recorded hash. A mismatch marks the package rejected and excludes it from the registry.

Catalog HTTP uses explicit connect, request, and response-body timeouts. Download and validation occur outside the skill-store lock. The lock covers only the final copy/swap, index update, selection update, and registry reload boundary.

Install operations are persisted and identified by `operationId`. At startup, operations left in `running` become `interrupted` and can be retried. A historical completed uninstall/install operation does not block reinstalling the same package. Event reduction is keyed by operation ID and tolerates out-of-order delivery.

Catalog source IDs are open strings in the Desktop contract rather than a closed UI union. Package scanning includes auxiliary text files in addition to the primary skill markdown.

## Desktop Structure and Behavior

The current settings implementation is split under `apps/desktop/src/features/skills/`:

```text
api/
installed/
catalog/
config/
components/
```

The installed view uses `sourceKind: "user"` for globally managed skills. File-read failures become explicit page state. Mutation rejections are caught and surfaced. Catalog polling refreshes catalog data after terminal completion. Datetime fields use datetime schemas. Invalid event payloads become visible page errors instead of being ignored.

The configuration UI renders declared public fields and secret controls. Public values can be saved normally. Secrets can be set or cleared, but are never returned to or rendered by the frontend.

## Verification Strategy

Implementation follows test-first RED/GREEN cycles. Coverage includes:

- registry concurrent mutation and shadow restoration;
- hook replacement and mTLS rejection;
- fake keychain configuration merge, set, and clear;
- per-skill required-configuration isolation;
- typed-reference legacy compatibility and first-turn injection;
- prepared/consumed recovery and hash mismatch rejection;
- script permission, declaration, path, secret environment, timeout, and sandbox enforcement;
- package hash tamper rejection and auxiliary-file scanning;
- catalog timeout, interrupted-operation recovery, reinstall, and lock scope;
- Desktop configuration, read/mutation error handling, reinstall, datetime validation, and out-of-order events;
- Daemon -> SDK -> Context -> Model integration.

Desktop skill command tests use a dedicated test target so unrelated automation/app-info test compilation does not hide skill regressions.
