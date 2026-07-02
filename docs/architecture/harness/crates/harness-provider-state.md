# harness-provider-state

`jyowo-harness-provider-state` owns private provider continuation persistence.

## Scope

The crate stores opaque provider continuation records needed to replay future provider requests.
Examples include provider-private reasoning replay payloads, provider-native cache replay handles, and dialect-specific replay metadata.

It does not own public messages, public events, Journal, Replay, frontend payloads, provider HTTP clients, credentials, permission decisions, or prompt assembly.

## Layer

This is an L1 runtime primitive.
It depends on `jyowo-harness-contracts` and third-party serialization/storage crates only.
It must not depend on Engine, SDK, desktop shell, provider clients, Journal, Context, Tool, Permission, or frontend code.

## Privacy

Provider continuation payloads are private.
They must not be written to public events, logs, traces, screenshots, snapshots, support bundles, frontend state, or exported transcripts.
Types carrying continuation payloads must use redacted debug output or avoid Debug.

For this plan, provider continuation payloads are stored as local plaintext runtime data under `.jyowo/runtime`.
The privacy boundary is public-surface exclusion, not encryption at rest.
Encrypted-at-rest storage requires a separate design.

## Lookup

Continuation lookup is keyed by provider id, model config id, protocol, dialect, tenant id, session id, final prompt message ids, and continuation kind.
Final prompt message ids means the ids after ContextEngine assembly and compaction.

## Development Reset

This project is in development.
Existing conversation runtime state created before provider continuation support is cleared once.
It is not migrated.
User configuration, provider settings, execution settings, memory, agent runtime state, MCP settings, skills, and plugin stores are preserved.

## Deletion Lifecycle

Successful conversation deletion prunes provider continuation records for the exact deleted session.
