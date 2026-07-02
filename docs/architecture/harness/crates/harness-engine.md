# harness-engine

`jyowo-harness-engine` owns run orchestration, model/tool loop, budgets, public event emission, and runtime coordination.

## Scope

Engine assembles prompts through ContextEngine, calls ModelProvider, dispatches tools, applies permissions, emits public events, and records usage.

Engine may load and store opaque ProviderContinuationRecord values.
Engine must not interpret provider-private payload fields.

## Turn Assembly

Turn assembly converts model stream events into visible assistant text, tool calls, usage, stop reason, and private provider continuation captures.
Only public text, tool calls, tool results, usage, errors, and lifecycle events may enter Journal.

## Provider Continuation

Continuation lookup uses final AssembledPrompt messages.
If compaction removes an assistant message, its continuation is not loaded.
If a provider requires a continuation for assistant tool replay and the record is absent, Engine fails closed before dispatching the provider request.

MiniMax is an explicit OpenAI-compatible dialect.
Its vertical regression must prove the Engine loop and model codec continue without provider-private replay.
