# harness-model

`jyowo-harness-model` owns model provider abstraction, runtime descriptors, and
provider catalog metadata.

## Scope

The crate exposes:

- `ModelProvider` implementations
- runtime-runnable `ModelDescriptor` values
- provider catalog and inventory snapshots
- model request validation helpers
- provider protocol adapters

It does not own tool service clients, product IPC payloads, permission decisions,
Journal, Replay, Audit, Redactor, sandbox, filesystem, network policy, or
provider secret storage.

Public serde contracts belong in `jyowo-harness-contracts`. Stable public shapes
use `serde` and `JsonSchema`.

## Runtime Boundary

`ModelDescriptor` means the harness can attempt runtime inference for that model
through the selected provider adapter.

`ModelInventoryEntry` may also include inventory-only models. Inventory-only
models exist so product UI can show provider catalog coverage and unsupported
service areas. They are not runtime-runnable descriptors.

`resolve_model_descriptor()` returns only runtime-runnable models. It must not
resolve inventory-only models.

Unknown model ids are fail-closed. Provider snapshot resolution returns an error
instead of synthesizing default runtime capabilities.

## Protocols

`ModelProtocol::Responses` and `ModelProtocol::ChatCompletions` describe runtime
wire protocol selection.

OpenAI uses the Responses protocol. The internal OpenAI-compatible adapter
remains available for providers whose runtime API is Chat Completions compatible,
such as OpenRouter, Local Llama, DeepSeek, MiniMax, Qwen, Doubao, Zhipu, and
Kimi.

## Provider Catalog

Provider catalog entries describe runtime capability, auth scheme, base URL
regions, service capabilities, and supported runtime models.

Provider inventory entries extend catalog data with inventory-only models. These
entries do not grant runtime capability and do not bypass provider adapter
validation.

## MiniMax Boundary

`MinimaxProvider` remains in this crate because it is a model inference provider.

MiniMax non-inference service clients belong in `jyowo-harness-tool`. MiniMax
image, audio, video, file, voice, lyrics, music, and model-list service calls are
tool operations, not model runtime abstraction.
