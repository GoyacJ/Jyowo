# Provider Capability Routing Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Each task also requires an independent subagent audit before the task can be marked complete.

**Goal:** Add complete provider capability routing so Jyowo can use one conversation model as the main agent model and separate configured provider profiles for image, video, audio, speech, music, and other model-backed services.

**Architecture:** Keep the conversation model as the only main model. Add a backend-owned capability routing layer that maps service kinds to configured provider profiles and runtime adapters. Tools expose only enabled, validated service routes; service outputs become typed artifacts through one generic artifact pipeline.

**Tech Stack:** Rust 1.96, serde, schemars, Tauri 2, React 19, TypeScript 6, Zod, TanStack Query, React Hook Form, Vitest, Testing Library, cargo test, pnpm gates.

---

## Required Execution Mode

Implementation must happen in an isolated git worktree. Do not implement in the current dirty workspace.

Use branch prefix `goya`.

```bash
git status --short
git worktree add ../Jyowo-provider-capability-routing -b goya/provider-capability-routing
cd ../Jyowo-provider-capability-routing
git status --short
```

Expected:

```text
clean worktree, or only files created by the implementation agent
```

If the branch name already exists, use:

```bash
git worktree add ../Jyowo-provider-capability-routing -b goya/provider-capability-routing-2
```

Do not copy uncommitted changes from the original workspace.

All task commits must be created from the isolated worktree path. Never stage or commit from the original dirty workspace.

## Mandatory Reading

Before any implementation task, read these files in the worktree:

```text
AGENTS.md
docs/frontend/agent-harness-frontend-development-guidelines.md
docs/frontend/frontend-product-ux.md
docs/frontend/frontend-engineering.md
docs/frontend/frontend-quality.md
docs/backend/agent-harness-backend-development-guidelines.md
docs/backend/backend-runtime.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
```

Also read the exact task section in this plan before editing files.

## Current Code Facts

These facts are the design baseline. Do not invent a different architecture.

- `crates/jyowo-harness-contracts/src/model_capability.rs` already separates `ConversationModelCapability` and `ProviderServiceCapability`.
- `ConversationModelCapability` describes the main chat model: input modalities, output modalities, tool calling, reasoning, streaming, prompt cache, structured output.
- `ProviderServiceCapability` describes provider services: operation id, category, input modalities, output artifact, execution mode, polling, permission subject, cost risk.
- `crates/jyowo-harness-model/src/registry.rs` currently returns non-empty service capabilities only for `minimax`.
- `apps/desktop/src/features/settings/ProviderSettingsForm.tsx` already reads provider catalog and provider settings and shows service capabilities as read-only data.
- `apps/desktop/src/features/conversation/ConversationWorkspace.tsx` selects only one conversation model profile.
- `apps/desktop/src/features/conversation/Composer.tsx` filters user attachments by the main model input modalities.
- `apps/desktop/src-tauri/src/commands.rs` stores provider profiles in `.jyowo/runtime/provider-settings.json`.
- `apps/desktop/src-tauri/src/commands.rs` stores per-conversation main model binding in `.jyowo/runtime/conversation-model-settings.json`.
- `DesktopProviderCredentialResolver` currently resolves credentials by provider id only, using the conversation-bound profile or default profile.
- `crates/jyowo-harness-engine/src/turn.rs` hides all tools when the main model has `tool_calling = false`.
- `crates/jyowo-harness-engine/src/turn.rs` currently creates image artifacts by checking tool names. This must be replaced with typed service/artifact output.
- `crates/jyowo-harness-tool/src/builder.rs` registers MiniMax image, video, TTS, voice, music, file, and model tools when the feature is enabled.
- There is no Seedance service adapter in the current source. Doubao chat provider is not Seedance video generation.

## Non-Negotiable Design Rules

- Main conversation model stays separate from service models.
- User attachments are validated only against the main model input modalities.
- Media generation is a routed provider service, not `ConversationModelCapability.output_modalities`.
- A model supporting image input does not mean it supports image generation.
- A provider profile can be both main model and service route, but only through explicit route config.
- Unconfigured service tools must not be exposed to the main model.
- Main models without tool calling must not receive autonomous service tools.
- Backend validates every route. Frontend never decides security or runtime eligibility.
- Provider credential resolution must include operation or route context. Do not infer route from tool name.
- Engine must not know MiniMax, Seedance, OpenAI Image, or any provider-specific service name.
- No production fake implementations.
- No hardcoded success responses.
- No demo provider keys.
- No TODO placeholders for required behavior.
- No string matching such as `tool_name.contains("image")` to decide artifact creation.
- No direct `@tauri-apps/api` imports outside `apps/desktop/src/shared/tauri`.
- No external payload accepted by React without Zod validation.
- `unsafe_code = "forbid"` remains untouched.

## Allowed Test Fixtures

The implementation must not use mock data to make product behavior appear complete.

Allowed:

- Temporary directories for stores.
- Local in-process HTTP test servers that exercise the real provider adapter code.
- Minimal provider response fixtures in tests, only when shaped from documented provider responses or existing source contracts.

Forbidden:

- Production fake providers.
- UI tests that pass by hardcoding imaginary route responses.
- Adapter code that returns canned media instead of calling the provider client path.
- Tests that skip backend validation and assert frontend-only behavior.

## Architecture Target

```text
User message
  -> main conversation model
  -> optional tool call
  -> ToolPool exposes only route-enabled service tools
  -> tool carries service binding metadata
  -> route resolver selects configured provider profile
  -> credential resolver returns operation-scoped credential
  -> provider service adapter executes operation
  -> BlobStore stores media output
  -> engine creates ArtifactCreated / ArtifactUpdated
  -> read model projects artifact
  -> frontend renders typed artifact preview
```

## Target Concepts

### Capability Route

A workspace-level policy that binds a service kind to a provider profile and provider operation ids.

Internal shape:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRouteKind {
    ImageGeneration,
    VideoGeneration,
    TextToSpeech,
    SpeechToText,
    MusicGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCapabilityRoute {
    pub kind: CapabilityRouteKind,
    pub config_id: String,
    pub provider_id: String,
    pub operation_ids: Vec<String>,
    pub enabled: bool,
}
```

Stored at:

```text
.jyowo/runtime/provider-capability-routes.json
```

Example:

```json
{
  "version": 1,
  "routes": [
    {
      "kind": "image_generation",
      "configId": "minimax-main",
      "providerId": "minimax",
      "operationIds": ["minimax.image_generation"],
      "enabled": true
    },
    {
      "kind": "video_generation",
      "configId": "minimax-video",
      "providerId": "minimax",
      "operationIds": ["minimax.video_generation", "minimax.video_generation.query"],
      "enabled": true
    }
  ]
}
```

### Service Binding

Metadata on a tool descriptor or adjacent registry metadata that identifies the provider service operation.

Target shape:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ToolServiceBinding {
    pub provider_id: String,
    pub operation_id: String,
    pub route_kind: CapabilityRouteKind,
    pub output_artifact: ModelModality,
}
```

Use this binding for route filtering and credential resolution.

### Service Output

Provider service tools should return explicit artifact metadata instead of relying on engine heuristics.

Target shape:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceToolOutput {
    Structured {
        value: serde_json::Value,
    },
    Artifact {
        artifact_kind: ModelModality,
        content_type: String,
        blob_ref: BlobRef,
        title: String,
        preview: Option<String>,
    },
    AsyncJob {
        job_id: String,
        poll_operation_id: String,
        artifact_kind: ModelModality,
        title: String,
    },
}
```

Implement this as a typed `ToolResultPart::Artifact`. Do not add a top-level `ToolResult::Artifact` variant. If existing orchestration cannot carry result parts, stop and revise this plan before implementing a different public contract.

## Required Task Loop

Every task must follow this exact loop.

1. **Pre-task analysis**
   - Restate the task goal in one short note.
   - List exact files to edit.
   - List exact tests to add or update.
   - Confirm no task requirement conflicts with existing docs or contracts.
   - If any file path or type is missing, stop and inspect. Do not guess.

2. **Write failing tests first**
   - Add focused tests for the behavior.
   - Run the focused command.
   - Confirm failure is for the expected missing behavior.

3. **Implement minimal production code**
   - Use existing patterns.
   - Keep changes close to the task.
   - Remove unused imports, variables, and orphan code.

4. **Run focused verification**
   - Run the exact task commands.
   - Record the commands and exit codes in the task note.

5. **Pre-completion self-audit**
   - Map every task requirement to changed files and tests.
   - Confirm no fake implementation or mock data was introduced.
   - Confirm no route, credential, artifact, or permission bypass exists.

6. **Subagent audit**
   - Dispatch a fresh subagent.
   - The subagent must read this task section, inspect the diff, and run or review the focused gates.
   - The subagent must return `PASS` or `FAIL`.
   - If `FAIL`, fix findings and repeat the subagent audit.
   - `FAIL` blocks commit and blocks moving to the next task.
   - Do not mark the task complete without a passing subagent audit.

7. **Commit**
   - Commit only the files touched by the task.
   - Do not commit unrelated workspace changes.

Subagent audit prompt template:

```text
Audit Task N from docs/plans/2026-06-29-provider-capability-routing-implementation.md.

Check:
- task goal is fully implemented
- tests were added before implementation
- focused gates pass or failure is unrelated and documented
- no mock data, fake runtime behavior, hardcoded success, or TODO placeholder
- provider/service boundaries match the plan
- Rust remains policy authority
- frontend uses Zod and CommandClient only
- no unrelated refactor

Return:
- PASS or FAIL
- findings with file:line
- inspected files
- gates run or reviewed
- missing gates
- explicit decision: may commit / may not commit
```

Security review requirement:

- Any task touching API keys, credential resolution, permission checks, provider base URLs, external API payloads, or artifact download must run a security subagent audit before commit.
- If `/security-review` is available, use it. If not, dispatch a subagent with the security-specific prompt above.

## Global Gates

Run these before final delivery:

```bash
pnpm check
```

If `pnpm check` fails, run the narrower gates to locate the failure:

```bash
pnpm check:docs
pnpm check:desktop
pnpm check:rust
```

Rust gates:

```bash
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
```

Frontend gates:

```bash
pnpm -C apps/desktop typecheck
pnpm -C apps/desktop lint
pnpm -C apps/desktop test
pnpm -C apps/desktop build
pnpm -C apps/desktop knip
```

Docs gates:

```bash
pnpm check:docs
```

No gate may be skipped silently.

---

## Task 0: Isolated Worktree And Baseline Audit

**Files:**

- Read: `AGENTS.md`
- Read: `docs/frontend/agent-harness-frontend-development-guidelines.md`
- Read: `docs/frontend/frontend-product-ux.md`
- Read: `docs/frontend/frontend-engineering.md`
- Read: `docs/frontend/frontend-quality.md`
- Read: `docs/backend/agent-harness-backend-development-guidelines.md`
- Read: `docs/backend/backend-runtime.md`
- Read: `docs/backend/backend-engineering.md`
- Read: `docs/backend/backend-quality.md`
- Read: `docs/plans/2026-06-29-provider-capability-routing-implementation.md`

**Step 1: Create isolated worktree**

```bash
git status --short
git worktree add ../Jyowo-provider-capability-routing -b goya/provider-capability-routing
cd ../Jyowo-provider-capability-routing
git status --short
```

Expected:

```text
The new worktree is clean.
```

**Step 2: Read required docs**

```bash
sed -n '1,260p' AGENTS.md
sed -n '1,220p' docs/frontend/agent-harness-frontend-development-guidelines.md
sed -n '1,260p' docs/frontend/frontend-product-ux.md
sed -n '1,260p' docs/frontend/frontend-engineering.md
sed -n '1,220p' docs/frontend/frontend-quality.md
sed -n '1,220p' docs/backend/agent-harness-backend-development-guidelines.md
sed -n '1,220p' docs/backend/backend-runtime.md
sed -n '1,260p' docs/backend/backend-engineering.md
sed -n '1,220p' docs/backend/backend-quality.md
```

If a file is longer than the shown range, continue reading until EOF.

**Step 3: Inspect current model and tool surfaces**

```bash
rg -n "ConversationModelCapability|ProviderServiceCapability|ProviderCredentialResolveContext|ProviderCredentialResolverCap|ToolResult|ArtifactCreatedEvent|DesktopProviderCredentialResolver|service_capabilities|MiniMaxTextToImage|prompt_visible_tools_for_model|image_artifact_blob" crates apps
```

Expected:

```text
All referenced current code surfaces are found.
```

**Step 4: Run baseline focused gates**

```bash
pnpm check:docs
cargo check --workspace
```

Expected:

```text
Both commands pass before implementation.
```

If they fail from existing main-branch issues, stop and document exact failure before continuing.

**Step 5: Subagent audit**

Dispatch a subagent to verify the worktree is isolated, docs were read, and baseline gates were run.

**Step 6: Commit**

No code commit is required for this task.

---

## Task 1: Route Contracts

**Goal:** Add stable public contracts for provider capability routes and tool service bindings.

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/model_capability.rs`
- Modify: `crates/jyowo-harness-contracts/src/tool.rs`
- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Test: `crates/jyowo-harness-contracts/tests/provider_capability_routes.rs`

**Step 1: Pre-task analysis**

Confirm the exact public contract names:

```text
CapabilityRouteKind
ProviderCapabilityRoute
ProviderCapabilityRouteSettings
ToolServiceBinding
ProviderCredentialResolveContext.operation_id
ProviderCredentialResolveContext.route_kind
```

Use these names unless there is a compile conflict. If there is a conflict, record the reason before renaming.

Do not introduce `CapabilityRoute` or `ProviderCapabilityRouteRecord` as separate public types. If a private persistence wrapper becomes necessary, name it in the pre-task analysis and keep public serde contracts limited to `ProviderCapabilityRoute` and `ProviderCapabilityRouteSettings`.

**Step 2: Write failing contract tests**

Create `crates/jyowo-harness-contracts/tests/provider_capability_routes.rs`.

Test requirements:

- `CapabilityRouteKind` serializes as snake_case.
- `ProviderCapabilityRoute` serializes as camelCase.
- `ProviderCapabilityRouteSettings` rejects unknown fields.
- Empty `operation_ids` is rejected by validation helper.
- `ToolServiceBinding` serializes as camelCase.
- `ProviderCredentialResolveContext` can round-trip with `operationId` and `routeKind`.

Run:

```bash
cargo test -p jyowo-harness-contracts --test provider_capability_routes -- --nocapture
```

Expected:

```text
FAIL because the new contracts do not exist.
```

**Step 3: Implement contracts**

Add route contracts in `model_capability.rs` near `ProviderServiceCapability`.

Required shape:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRouteKind {
    ImageGeneration,
    VideoGeneration,
    TextToSpeech,
    SpeechToText,
    MusicGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCapabilityRoute {
    pub kind: CapabilityRouteKind,
    pub config_id: String,
    pub provider_id: String,
    pub operation_ids: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCapabilityRouteSettings {
    pub version: u32,
    pub routes: Vec<ProviderCapabilityRoute>,
}
```

Add a validation helper in the same crate:

```rust
pub fn validate_provider_capability_route(
    route: &ProviderCapabilityRoute,
) -> Result<(), String>
```

Rules:

- `config_id`, `provider_id`, and all `operation_ids` must be non-empty after trim.
- `operation_ids` must not be empty.
- `operation_ids` must not contain duplicates.

Add `ToolServiceBinding` to `tool.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ToolServiceBinding {
    pub provider_id: String,
    pub operation_id: String,
    pub route_kind: CapabilityRouteKind,
    pub output_artifact: ModelModality,
}
```

Add optional field to `ToolDescriptor`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub service_binding: Option<ToolServiceBinding>,
```

Update the existing descriptor factory/helper constructors so most call sites do not manually add `service_binding: None`. Do not scatter repeated `None` assignments across unrelated tool constructors when one local helper can preserve the existing style.

Add fields to `ProviderCredentialResolveContext`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub operation_id: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub route_kind: Option<CapabilityRouteKind>,
```

These fields must be optional for compatibility with existing tools.

Add schema exports for new public contracts.

**Step 4: Run focused tests**

```bash
cargo test -p jyowo-harness-contracts --test provider_capability_routes -- --nocapture
cargo test -p jyowo-harness-contracts
```

Expected:

```text
PASS
```

**Step 5: Run compile checks for dependent crates**

```bash
cargo check --workspace
```

Expected:

```text
PASS
```

**Step 6: Pre-completion self-audit**

Confirm:

- Public serde shape is stable.
- Existing `ProviderCredentialResolveContext` call sites compile with new optional fields.
- No route validation depends on frontend data.
- No provider-specific behavior was added.

**Step 7: Subagent audit**

Run the required subagent audit for Task 1.

**Step 8: Commit**

```bash
git add crates/jyowo-harness-contracts/src/model_capability.rs crates/jyowo-harness-contracts/src/tool.rs crates/jyowo-harness-contracts/src/capability.rs crates/jyowo-harness-contracts/src/schema_export.rs crates/jyowo-harness-contracts/tests/provider_capability_routes.rs
git commit -m "feat: add provider capability route contracts"
```

---

## Task 2: Desktop Route Store And Validation

**Goal:** Persist workspace capability routes, expose backend-derived route options, and validate routes against provider settings, provider catalog, and runtime adapter availability.

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`

**Step 1: Pre-task analysis**

Inspect existing store patterns:

```bash
rg -n "ProviderSettingsStore|ConversationModelConfigStore|DesktopProviderSettingsStore|DesktopConversationModelConfigStore|ensure_provider_settings_record|save_provider_settings_with_store" apps/desktop/src-tauri/src/commands.rs
```

Use the same temp-file, symlink, `0o600`, and JSON serialization patterns.

**Step 2: Write failing backend tests**

Add tests in `apps/desktop/src-tauri/tests/commands.rs`.

Required scenarios:

- Missing route file returns `{ version: 1, routes: [] }`.
- Saving a route writes `.jyowo/runtime/provider-capability-routes.json`.
- Route fails when `configId` does not exist.
- Route fails when `providerId` does not match the profile provider.
- Route fails when profile has no API key.
- Route fails when operation id is not in provider catalog.
- Route fails when operation id exists in catalog but no runtime adapter is registered.
- Route options expose `runtimeSupported = true` only for operations present in the injected adapter availability.
- Route options never expose API key values.
- Disabled route is saved but does not enable tools later.
- Saving empty routes writes `{ version: 1, routes: [] }`.
- Route file with invalid JSON or unknown fields is removed and returns `{ version: 1, routes: [] }`.

Run:

```bash
cargo test -p jyowo-desktop-shell --test commands provider_capability_route -- --nocapture
```

Expected:

```text
FAIL because route store and commands do not exist.
```

**Step 3: Implement store**

Add:

```rust
pub trait ProviderCapabilityRouteStore: Send + Sync {
    fn load_record(&self) -> Result<Option<ProviderCapabilityRouteSettings>, CommandErrorPayload>;
    fn save_record(&self, record: &ProviderCapabilityRouteSettings) -> Result<(), CommandErrorPayload>;
}

#[derive(Clone)]
pub struct DesktopProviderCapabilityRouteStore {
    workspace_root: PathBuf,
}
```

Store path:

```text
.jyowo/runtime/provider-capability-routes.json
```

Use the same filesystem safeguards as provider settings:

- parent directory symlink check
- temp file write
- atomic rename
- private file mode on Unix
- invalid JSON does not leak secrets in error messages
- invalid JSON or unknown fields remove the route file and return empty version 1 settings

**Step 4: Implement route validation**

Add backend validation helper:

```rust
fn ensure_provider_capability_route_settings(
    routes: &ProviderCapabilityRouteSettings,
    provider_settings: &ProviderSettingsRecord,
    adapter_availability: &ProviderServiceAdapterAvailability,
) -> Result<(), CommandErrorPayload>
```

Rules:

- `version == 1`.
- Missing route file is normalized to `ProviderCapabilityRouteSettings { version: 1, routes: vec![] }`.
- Empty route lists are valid and must not expose any service tools.
- Each route passes contract validation.
- Each enabled route references an existing provider config.
- The config has an API key.
- `route.provider_id == config.provider_id`.
- Every operation id is declared by provider catalog for that provider.
- Every enabled operation has a runtime adapter.
- Same enabled `CapabilityRouteKind` cannot point to multiple configs in the same settings file.

Do not accept frontend-provided provider display names or model descriptors as authority.

**Step 5: Add backend route option builder**

Add a backend value object for runtime adapter availability:

```rust
pub struct ProviderServiceAdapterAvailability {
    pub bindings: Vec<ToolServiceBinding>,
}
```

Implement a local helper on this value object:

```rust
fn has_service_adapter(
    availability: &ProviderServiceAdapterAvailability,
    provider_id: &str,
    operation_id: &str,
    route_kind: CapabilityRouteKind,
) -> bool
```

Production adapter availability must be populated from actual registered tool descriptors with `ToolServiceBinding`. Do not maintain a hardcoded MiniMax operation allowlist in `commands.rs`.

Task 2 may use test-only `ProviderServiceAdapterAvailability` values inside tests to prove validation branches. Those values must live in test code only and must not be used by production commands.

Add a route option payload:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCapabilityRouteOption {
    pub kind: CapabilityRouteKind,
    pub config_id: String,
    pub provider_id: String,
    pub operation_id: String,
    pub output_artifact: ModelModality,
    pub execution: ProviderServiceExecution,
    pub cost_risk: ProviderServiceCostRisk,
    pub runtime_supported: bool,
    pub unavailable_reason: Option<String>,
}
```

Build options from provider settings, provider catalog service capabilities, and adapter availability. Options are for UX only. Save/delete validation remains backend authority.

Do not construct production adapter availability in Task 2. Route validation and option building must accept `ProviderServiceAdapterAvailability` as an input. Tests may inject populated availability; production wiring is completed by Task 5 and Task 10.

**Step 6: Add command payload types**

Add:

```rust
pub struct ListProviderCapabilityRoutesResponse
pub struct SaveProviderCapabilityRouteRequest
pub struct SaveProviderCapabilityRouteResponse
pub struct DeleteProviderCapabilityRouteRequest
pub struct DeleteProviderCapabilityRouteResponse
pub struct ListProviderCapabilityRouteOptionsResponse
```

Use camelCase serde.

**Step 7: Add functions**

Add:

```rust
pub async fn list_provider_capability_routes_with_runtime_state(...)
pub async fn save_provider_capability_route_with_runtime_state(...)
pub async fn delete_provider_capability_route_with_runtime_state(...)
pub async fn list_provider_capability_route_options_with_runtime_state(...)
```

Use a route settings lock on `DesktopRuntimeState`.

**Step 8: Run focused tests**

```bash
cargo test -p jyowo-desktop-shell --test commands provider_capability_route -- --nocapture
cargo test -p jyowo-desktop-shell --test commands provider_settings -- --nocapture
```

Expected:

```text
PASS
```

**Step 9: Security audit**

Run a security subagent audit. It must inspect:

- API keys never appear in route payloads.
- API keys never appear in route option payloads.
- route file does not store API keys.
- invalid route errors do not reveal secrets.
- symlink protections match provider settings.

**Step 10: Subagent audit**

Run the required subagent audit for Task 2.

**Step 11: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/tests/commands.rs
git commit -m "feat: persist provider capability routes"
```

---

## Task 3: Tauri Commands And Frontend Command Client

**Goal:** Expose route commands through Tauri and typed frontend command schemas.

**Files:**

- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`
- Test: `apps/desktop/src/shared/tauri/default-client.test.ts`

**Step 1: Pre-task analysis**

Inspect command registration and command client patterns:

```bash
rg -n "list_provider_settings|save_provider_settings|set_conversation_model_config|listProviderSettings|saveProviderSettings|parsePayload|parseArgs" apps/desktop/src-tauri/src apps/desktop/src/shared/tauri
```

**Step 2: Write failing frontend command tests**

Add tests for:

- `listProviderCapabilityRoutes` parses a valid response.
- `listProviderCapabilityRouteOptions` parses options with `runtimeSupported` and `unavailableReason`.
- `saveProviderCapabilityRoute` rejects unknown request fields.
- `deleteProviderCapabilityRoute` validates `kind`.
- command client exposes all four methods.
- default client forwards exact command names.

Run:

```bash
pnpm -C apps/desktop test -- commands.test.ts default-client.test.ts
```

Expected:

```text
FAIL because schemas and methods do not exist.
```

**Step 3: Register Tauri commands**

Add commands to `apps/desktop/src-tauri/src/lib.rs`:

```rust
commands::list_provider_capability_routes,
commands::list_provider_capability_route_options,
commands::save_provider_capability_route,
commands::delete_provider_capability_route,
```

Add command functions in `commands.rs` that call the runtime-state helpers from Task 2.

**Step 4: Add TypeScript schemas**

In `apps/desktop/src/shared/tauri/commands.ts`, add:

```ts
const capabilityRouteKindSchema = z.enum([
  'image_generation',
  'video_generation',
  'text_to_speech',
  'speech_to_text',
  'music_generation',
])
```

Add route schemas with `.strict()`.

Add route option schemas with `.strict()`. The route option schema must include:

```ts
runtimeSupported: z.boolean()
unavailableReason: z.string().optional()
```

Do not export API key fields.

Add command client methods:

```ts
listProviderCapabilityRoutes: () => Promise<ListProviderCapabilityRoutesResponse>
listProviderCapabilityRouteOptions: () => Promise<ListProviderCapabilityRouteOptionsResponse>
saveProviderCapabilityRoute: (
  request: SaveProviderCapabilityRouteRequest,
) => Promise<SaveProviderCapabilityRouteResponse>
deleteProviderCapabilityRoute: (
  request: DeleteProviderCapabilityRouteRequest,
) => Promise<DeleteProviderCapabilityRouteResponse>
```

**Step 5: Run focused tests**

```bash
pnpm -C apps/desktop test -- commands.test.ts default-client.test.ts
cargo test -p jyowo-desktop-shell --test commands provider_capability_route -- --nocapture
```

Expected:

```text
PASS
```

**Step 6: Pre-completion self-audit**

Confirm:

- Tauri command names match frontend command names.
- Zod schemas are strict.
- No route payload contains secret values.
- No route option payload contains secret values.
- Frontend has a typed source for backend-reported runtime support.
- No direct Tauri imports were added outside `shared/tauri`.

**Step 7: Subagent audit**

Run the required subagent audit for Task 3.

**Step 8: Commit**

```bash
git add apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src/shared/tauri/commands.ts apps/desktop/src/shared/tauri/commands.test.ts apps/desktop/src/shared/tauri/default-client.test.ts
git commit -m "feat: expose provider capability route commands"
```

---

## Task 4: Tool Service Binding And Route-Based Filtering

**Goal:** Add service binding metadata, export runtime adapter availability from real tool descriptors, and provide pure route-based filtering.

**Files:**

- Modify: `crates/jyowo-harness-tool/src/builtin/minimax.rs`
- Modify: `crates/jyowo-harness-tool/src/builder.rs`
- Modify: `crates/jyowo-harness-tool/src/registry.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Test: `crates/jyowo-harness-tool/tests/registry.rs`
- Test: `crates/jyowo-harness-engine/tests/*`

**Step 1: Pre-task analysis**

Inspect tool descriptor creation and existing tool filtering:

```bash
rg -n "fn descriptor|fn image_descriptor|ToolDescriptor|filter_unavailable_tools|ToolPoolFilter|ToolPool::assemble|prompt_visible_tools_for_model|MiniMaxTextToImage" crates/jyowo-harness-tool crates/jyowo-harness-sdk crates/jyowo-harness-engine
```

**Step 2: Write failing tests**

Add tests for:

- MiniMax image tool descriptor has service binding `minimax.image_generation`.
- MiniMax video tools have video generation bindings.
- MiniMax TTS tools have text-to-speech bindings.
- Adapter availability reports runtime support from real descriptor `service_binding` values.
- Adapter availability does not report operations that only exist in provider catalog.
- Pure route filter denies service-bound tools when no route is enabled.
- Pure route filter allows service-bound tools when route kind/provider/operation match.
- Non-service tools are not affected.
- Main model without tool calling still sees no prompt-visible tools.

Run:

```bash
cargo test -p jyowo-harness-tool --test registry minimax_service_binding -- --nocapture
cargo test -p jyowo-harness-engine capability_route_filter -- --nocapture
```

Expected:

```text
FAIL because descriptors and filters do not support routes.
```

**Step 3: Add service bindings to MiniMax descriptors**

Update MiniMax descriptor helpers so every provider service tool declares its binding.

Examples:

```rust
MiniMaxTextToImage -> minimax.image_generation -> ImageGeneration -> Image
MiniMaxImageToImage -> minimax.image_generation -> ImageGeneration -> Image
MiniMaxTextToVideo -> minimax.video_generation -> VideoGeneration -> Video
MiniMaxVideoGenerationQuery -> minimax.video_generation.query -> VideoGeneration -> Video
MiniMaxTextToSpeech -> minimax.text_to_speech.sync -> TextToSpeech -> Audio
MiniMaxMusicGeneration -> minimax.music_generation -> MusicGeneration -> Audio
```

Do not bind conversation compatibility tools such as MiniMax responses unless a product route exists for them.

**Step 4: Add production adapter availability source**

Add a production helper that extracts `Vec<ToolServiceBinding>` from the registered tool descriptors or registry snapshot.

Rules:

```text
runtime-supported operation = at least one registered tool descriptor has matching ToolServiceBinding
provider catalog operation alone is not runtime support
commands.rs must not contain provider operation allowlists
```

Task 10 will pass these bindings to desktop route validation and route options as `ProviderServiceAdapterAvailability`.

Do not add desktop route store wiring in Task 5. That belongs to Task 10.

**Step 5: Implement filter**

Extend `filter_unavailable_tools` or add a nearby helper:

```rust
fn filter_unrouted_service_tools(
    filter: &mut ToolPoolFilter,
    snapshot: &ToolRegistrySnapshot,
    routes: &ProviderCapabilityRouteSettings,
)
```

Rules:

- If descriptor has no `service_binding`, do nothing.
- If descriptor has service binding and no enabled matching route, denylist the tool.
- Match by `route_kind`, `provider_id`, and `operation_id`.
- Disabled route does not match.
- Missing route settings means no service tools are exposed.

**Step 6: Run focused tests**

```bash
cargo test -p jyowo-harness-tool --test registry minimax_service_binding -- --nocapture
cargo test -p jyowo-harness-engine capability_route_filter -- --nocapture
cargo check --workspace
```

Expected:

```text
PASS
```

**Step 7: Pre-completion self-audit**

Confirm:

- No service tool can leak into the prompt without route config.
- Existing non-service tools still appear when allowed.
- Tool hiding for no tool-calling model remains unchanged.
- No provider-specific logic was added to engine.
- Runtime adapter support is derived from descriptors, not a duplicated allowlist.
- No desktop route store or `startRun` wiring was added in this task.

**Step 8: Subagent audit**

Run the required subagent audit for Task 4.

**Step 9: Commit**

```bash
git add crates/jyowo-harness-tool/src/builtin/minimax.rs crates/jyowo-harness-tool/src/builder.rs crates/jyowo-harness-tool/src/registry.rs crates/jyowo-harness-engine/src/turn.rs crates/jyowo-harness-tool/tests/registry.rs
git commit -m "feat: filter service tools by capability routes"
```

---

## Task 5: Operation-Scoped Credential Resolution

**Goal:** Resolve provider credentials by explicit service operation, not only provider id.

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/minimax.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Test: `crates/jyowo-harness-tool/tests/minimax_tools.rs`

**Step 1: Pre-task analysis**

Inspect resolver and call sites:

```bash
rg -n "ProviderCredentialResolveContext|resolve_provider_credential|minimax_credential|DesktopProviderCredentialResolver" crates apps
```

**Step 2: Write failing tests**

Backend resolver tests:

- Image operation resolves the config selected by image route.
- Video operation resolves the config selected by video route.
- Missing route denies credential.
- Wrong provider denies credential.
- Disabled route denies credential.
- Existing provider-only resolution still works for non-service tools where allowed.

MiniMax tool tests:

- Image tool passes `operation_id = minimax.image_generation`.
- Video tool passes video operation id.
- TTS tool passes TTS operation id.

Run:

```bash
cargo test -p jyowo-desktop-shell --test commands provider_credential_route -- --nocapture
cargo test -p jyowo-harness-tool --test minimax_tools credential_route -- --nocapture
```

Expected:

```text
FAIL because operation-scoped resolution does not exist.
```

**Step 3: Update DesktopProviderCredentialResolver**

Resolution order:

```text
if operation_id and route_kind are present:
  find enabled capability route matching operation + kind + provider
  find provider config by route.config_id
  validate provider id and API key
  return route credential
else:
  preserve existing provider-only behavior for existing non-service tools
```

Do not fall back from routed service operation to default provider config.

Fail closed for routed operations:

```text
route missing -> PermissionDenied
config missing -> PermissionDenied
provider mismatch -> PermissionDenied
api key missing -> PermissionDenied
```

**Step 4: Update MiniMax tools**

Change `minimax_credential` to accept service metadata:

```rust
async fn minimax_credential(
    ctx: &ToolContext,
    operation_id: &'static str,
    route_kind: CapabilityRouteKind,
) -> Result<ProviderCredential, ToolError>
```

Every MiniMax service tool must pass its operation id.

**Step 5: Run focused tests**

```bash
cargo test -p jyowo-desktop-shell --test commands provider_credential_route -- --nocapture
cargo test -p jyowo-harness-tool --test minimax_tools credential_route -- --nocapture
cargo check --workspace
```

Expected:

```text
PASS
```

**Step 6: Security audit**

Run security subagent audit.

Audit must verify:

- Routed service operations never fall back to default credential.
- Wrong route cannot borrow another provider API key.
- API key is not serialized to frontend or logs.
- Permission denial messages do not reveal key material.

**Step 7: Subagent audit**

Run the required subagent audit for Task 5.

**Step 8: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/tests/commands.rs crates/jyowo-harness-tool/src/builtin/minimax.rs crates/jyowo-harness-tool/tests/minimax_tools.rs
git commit -m "feat: resolve provider credentials by service route"
```

---

## Task 6: Generic Service Artifact Output

**Goal:** Replace provider-name artifact heuristics with typed artifact output from tools.

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/messages.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Modify: `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src/shared/events/run-event-schema.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Test: `crates/jyowo-harness-engine/tests/*`
- Test: `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Test: `apps/desktop/src/shared/events/run-event-schema.test.ts`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`

**Step 1: Pre-task analysis**

Inspect current artifact paths:

```bash
rg -n "image_artifact_blob|ArtifactCreated|ArtifactUpdated|ArtifactStatus|ArtifactSource|ToolResult::Blob|ToolResultPart::Blob|artifact_media_kind_from_label|artifactMediaPreviewSchema" crates apps
```

**Step 2: Write failing tests**

Add tests for:

- `ToolResult` can carry typed image artifact output.
- `ToolResult` can carry typed video artifact output.
- `ToolResult` can carry typed audio artifact output.
- Engine creates `ArtifactCreated` from typed artifact output.
- Engine does not use provider name or tool name to infer artifact kind.
- Mismatched content type and artifact kind is rejected.
- Read model projects image/video/audio media consistently.
- Tauri payload validates media kind and MIME type.
- Frontend Zod rejects mismatched media kind and MIME type.

Run focused tests matching the changed files.

Expected:

```text
FAIL because generic typed artifact output does not exist.
```

**Step 3: Add typed artifact result part**

Extend `ToolResultPart` with a typed artifact part:

```rust
Artifact {
    artifact_kind: ModelModality,
    content_type: String,
    blob_ref: BlobRef,
    title: String,
    preview: Option<String>,
}
```

Do not add provider-specific variants. Do not add a top-level `ToolResult::Artifact` variant. If `ToolResultPart::Artifact` cannot work with existing orchestration, stop and revise this plan before changing the public result contract.

**Step 4: Update engine artifact creation**

Replace `image_artifact_blob` and `is_image_artifact_tool` with typed artifact extraction:

```rust
fn artifact_from_tool_result(result: &ToolResult) -> Option<TypedArtifactOutput>
```

Rules:

- Only typed artifact output creates artifacts.
- `artifact_kind` must be image, video, audio, or file.
- Content type must match kind.
- Blob content type must also be safe.
- Title and preview are sanitized through existing event/read-model paths.
- Unknown or unsupported artifact kind is ignored or rejected fail-closed.

**Step 5: Preserve existing image behavior through typed output**

Do not regress existing MiniMax image artifact behavior. Task 7 will update MiniMax image tool to return typed artifact output. Until then, tests may keep old behavior only if needed, but the final state must not rely on tool name matching.

**Step 6: Update frontend schemas**

Ensure Zod schemas accept all safe artifact media kinds already supported:

```text
image/png, image/jpeg, image/gif, image/webp, image/avif
video/mp4, video/webm, video/quicktime
audio/mpeg, audio/mp4, audio/ogg, audio/wav, audio/webm
```

Do not loosen MIME checks.

**Step 7: Run focused tests**

```bash
cargo test -p jyowo-harness-engine artifact -- --nocapture
cargo test -p jyowo-harness-journal --test conversation_read_model artifact -- --nocapture
cargo test -p jyowo-desktop-shell --test commands artifact -- --nocapture
pnpm -C apps/desktop test -- run-event-schema.test.ts commands.test.ts
cargo check --workspace
pnpm -C apps/desktop typecheck
```

Expected:

```text
PASS
```

**Step 8: Security audit**

Run security subagent audit.

Audit must verify:

- Unsafe MIME types are rejected.
- Artifact kind cannot be spoofed through title, preview, or content type text.
- Blob references are not exposed without existing authorization path.
- No raw provider URL becomes a trusted local artifact without validation.

**Step 9: Subagent audit**

Run the required subagent audit for Task 6.

**Step 10: Commit**

```bash
git add crates/jyowo-harness-contracts/src/messages.rs crates/jyowo-harness-contracts/src/schema_export.rs crates/jyowo-harness-engine/src/turn.rs crates/jyowo-harness-journal/src/conversation_read_model.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src/shared/events/run-event-schema.ts apps/desktop/src/shared/tauri/commands.ts crates/jyowo-harness-journal/tests/conversation_read_model.rs apps/desktop/src-tauri/tests/commands.rs apps/desktop/src/shared/events/run-event-schema.test.ts apps/desktop/src/shared/tauri/commands.test.ts
git commit -m "feat: create artifacts from typed tool output"
```

---

## Task 7: MiniMax Route-Backed Service Tools

**Goal:** Convert existing MiniMax image, video, audio, and music tools to the route-backed credential and typed artifact pipeline.

**Files:**

- Modify: `crates/jyowo-harness-tool/src/builtin/minimax.rs`
- Create: `crates/jyowo-harness-tool/src/provider_media.rs`
- Modify: `crates/jyowo-harness-tool/src/lib.rs`
- Modify: `crates/jyowo-harness-tool/tests/minimax_tools.rs`
- Modify: `crates/jyowo-harness-model/tests/minimax_api.rs`

**Step 1: Pre-task analysis**

Inspect MiniMax API client and tools:

```bash
rg -n "image_generation|video_generation|query_video_generation|text_to_speech|music_generation|image_tool_result_from_response|execute_image_request|execute_request|minimax_credential" crates/jyowo-harness-model/src/minimax.rs crates/jyowo-harness-tool/src/builtin/minimax.rs crates/jyowo-harness-tool/tests/minimax_tools.rs
```

**Step 2: Write failing tests**

Add tests for:

- Image generation returns typed image artifact output.
- Image-to-image returns typed image artifact output.
- Video generation returns structured async job output with task id and poll operation.
- Video query with final video URL writes video blob and returns typed video artifact output.
- TTS sync returns typed audio artifact output when provider returns audio bytes or URL.
- TTS async returns async job output.
- TTS async query returns typed audio artifact output when completed.
- Music generation returns typed audio artifact output when provider returns audio bytes or URL.
- Provider URLs are downloaded only through allowed host and content-type checks.
- Provider URL redirects to untrusted hosts are rejected.
- Provider URL downloads with missing or excessive content length are rejected according to existing project limits.

Run:

```bash
cargo test -p jyowo-harness-tool --test minimax_tools minimax_service_artifact -- --nocapture
cargo test -p jyowo-harness-model --test minimax_api -- --nocapture
```

Expected:

```text
FAIL because MiniMax tools still return structured or image-only outputs.
```

**Step 3: Add provider media download helper**

Create a shared provider media download helper before changing MiniMax tools.

Required policy:

```text
input = provider id, operation id, candidate URL, expected artifact kind, expected MIME set
only http/https URLs are accepted
host must match an explicit provider media allowlist or an already trusted signed CDN host for that provider
redirects must be disabled or revalidated at every hop
content length must be bounded before writing to BlobStore
response MIME and sniffed MIME must match expected artifact kind
raw provider URL is never exposed as trusted artifact metadata
downloaded bytes are written through existing BlobWriter path
errors are redacted and do not include credentials or signed query strings
```

Do not put this logic only inside MiniMax video code. Future providers must reuse this helper or a stricter provider-specific wrapper around it.

**Step 4: Update image tools**

Update MiniMax image tools to return typed artifact output.

Requirements:

- Use existing `BlobWriter`.
- Preserve current image MIME detection.
- Preserve allowed host checks for provider image download.
- Return artifact title such as `Generated image`.
- Do not rely on engine tool-name heuristics.

**Step 5: Update video tools**

Video generation create tools return async job output:

```text
job_id = provider task id
poll_operation_id = minimax.video_generation.query or minimax.video_template.query
artifact_kind = video
```

Video query tools:

- If not completed, return structured status.
- If completed and provider supplies video URL or bytes, use the provider media helper to download/write blob and return typed video artifact.
- Validate video MIME type.
- Reject unsafe URLs.

**Step 6: Update audio and music tools**

TTS and music tools:

- Write audio bytes to BlobStore when provider returns audio.
- Download audio only through the provider media helper.
- Validate audio MIME type.
- Return typed audio artifact.

**Step 7: Run focused tests**

```bash
cargo test -p jyowo-harness-tool --test minimax_tools minimax_service_artifact -- --nocapture
cargo test -p jyowo-harness-model --test minimax_api -- --nocapture
cargo test -p jyowo-harness-tool --test minimax_tools -- --nocapture
cargo check --workspace
```

Expected:

```text
PASS
```

**Step 8: Security audit**

Run security subagent audit.

Audit must verify:

- URL allowlist logic is not weakened.
- redirects are disabled or revalidated.
- signed query strings are not logged.
- MIME sniffing remains fail-closed.
- Blob writes use existing `BlobWriter`.
- No provider response field is trusted without validation.

**Step 9: Subagent audit**

Run the required subagent audit for Task 7.

**Step 10: Commit**

```bash
git add crates/jyowo-harness-tool/src/builtin/minimax.rs crates/jyowo-harness-tool/src/provider_media.rs crates/jyowo-harness-tool/src/lib.rs crates/jyowo-harness-tool/tests/minimax_tools.rs crates/jyowo-harness-model/tests/minimax_api.rs
git commit -m "feat: route MiniMax service tools to typed artifacts"
```

---

## Optional Task 8: Seedance Video Service Adapter

**Goal:** Add real Seedance video generation support as a provider service route only when official API evidence is available. Do not create a fake Seedance adapter.

This task is optional and must not block the core provider capability routing implementation. If official API facts are unavailable, mark this task `SKIPPED: official Seedance API evidence unavailable`, do not create code or tests, and continue with Task 9.

**Files:**

- Modify: `crates/jyowo-harness-model/src/registry.rs`
- Modify: `crates/jyowo-harness-model/src/lib.rs`
- Create or modify: `crates/jyowo-harness-model/src/seedance.rs`
- Modify: `crates/jyowo-harness-tool/src/builder.rs`
- Create or modify: `crates/jyowo-harness-tool/src/builtin/seedance.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/mod.rs`
- Test: `crates/jyowo-harness-model/tests/seedance_api.rs`
- Test: `crates/jyowo-harness-tool/tests/seedance_tools.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`

**Step 1: Pre-task analysis**

Before writing code, collect source-backed endpoint facts.

Allowed sources:

- official Volcengine/ByteDance Seedance API documentation
- existing internal source code in this repo
- official SDK documentation

Record in the task note:

```text
API base URL
auth header shape
create video endpoint
query video endpoint
request fields
success response fields
error response fields
video URL or binary output fields
polling status values
content-type expectations
official source URL
verified date
```

If official source-backed API facts are unavailable, stop this task. Do not implement a fake adapter. Do not leave failing Seedance tests in the branch. Record the skipped status in the final delivery summary.

**Step 2: Write failing tests from documented contract**

Tests must use the real documented request and response field names.

Add model client tests for:

- create video task request
- query running task
- query completed task with video output
- provider error mapping
- auth header shape

Add tool tests for:

- text-to-video returns async job output.
- image-to-video returns async job output if documented.
- query completed task returns typed video artifact.
- unsafe output URL is rejected.
- wrong credential provider is rejected.

Run:

```bash
cargo test -p jyowo-harness-model --test seedance_api -- --nocapture
cargo test -p jyowo-harness-tool --test seedance_tools -- --nocapture
```

Expected:

```text
FAIL because Seedance adapter does not exist.
```

**Step 3: Add provider catalog service capabilities**

In `registry.rs`, add a provider catalog entry only if Seedance is a separate provider in product terms.

If Seedance is under existing Doubao/Volcengine credentials, add service capabilities to the correct provider id. Do not guess.

Minimum required capabilities:

```text
seedance.video_generation
seedance.video_generation.query
```

Use:

```text
category = Video
input_modalities = documented supported inputs
output_artifact = Video
execution = AsyncJob for create
requires_polling = true for create
cost_risk = High for create
cost_risk = Low for query
```

**Step 4: Add model client**

Implement a small provider API client in `crates/jyowo-harness-model/src/seedance.rs`.

Requirements:

- No runtime fake.
- No hardcoded successful media.
- Uses existing HTTP/client patterns from MiniMax or OpenAI-compatible providers.
- Error mapping does not expose secrets.
- Base URL validation follows existing provider patterns.

**Step 5: Add tool adapter**

Add Seedance tools:

```text
SeedanceTextToVideo
SeedanceImageToVideo if documented
SeedanceVideoGenerationQuery
```

Each descriptor must include `ToolServiceBinding`.

Each tool must:

- resolve credential through operation-scoped resolver
- request permission through existing network permission path
- return async job or typed video artifact
- write video artifact into BlobStore

**Step 6: Register tools**

Register tools in builder under an explicit feature gate if the project uses provider feature flags.

Do not register Seedance tools without service binding.

**Step 7: Run focused tests**

```bash
cargo test -p jyowo-harness-model --test seedance_api -- --nocapture
cargo test -p jyowo-harness-tool --test seedance_tools -- --nocapture
cargo test -p jyowo-harness-sdk --test runtime_assembly seedance -- --nocapture
cargo check --workspace
```

Expected:

```text
PASS
```

**Step 8: Security audit**

Run security subagent audit.

Audit must verify:

- official API source was used
- auth shape is correct
- base URL cannot be abused
- output URL download is safe
- provider errors do not leak credentials

**Step 9: Subagent audit**

Run the required subagent audit for Optional Task 8 if this task is implemented.

**Step 10: Commit**

Only commit this task if real Seedance code and tests were implemented from official source-backed API facts. If the task was skipped, there is no Seedance commit.

```bash
git add crates/jyowo-harness-model/src/registry.rs crates/jyowo-harness-model/src/lib.rs crates/jyowo-harness-model/src/seedance.rs crates/jyowo-harness-tool/src/builder.rs crates/jyowo-harness-tool/src/builtin/seedance.rs crates/jyowo-harness-tool/src/builtin/mod.rs crates/jyowo-harness-model/tests/seedance_api.rs crates/jyowo-harness-tool/tests/seedance_tools.rs crates/jyowo-harness-sdk/tests/runtime_assembly.rs
git commit -m "feat: add Seedance video service route"
```

---

## Task 9: Conversation Runtime Integration

**Goal:** Make a normal conversation run use main model plus route-enabled service tools end to end.

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- Test: `crates/jyowo-harness-engine/tests/*`

**Step 1: Pre-task analysis**

Trace current start run:

```bash
rg -n "start_run_with_runtime_state|build_desktop_harness|engine_for_session|ToolPool::assemble|prompt_visible_tools_for_model|model_request_tools|ArtifactCreated" apps/desktop/src-tauri/src/commands.rs crates/jyowo-harness-sdk/src/harness.rs crates/jyowo-harness-engine/src/turn.rs
```

Confirm Task 4 already added `ToolServiceBinding`, descriptor-derived adapter availability, and the pure route filter helper. Do not recreate those concepts in Task 10.

**Step 2: Write failing integration tests**

Add tests for:

- Conversation with GPT-like main model and MiniMax image route exposes MiniMax image tool.
- Same conversation without route does not expose MiniMax image tool.
- Main model without tool calling exposes no service tools even when route exists.
- Route points to MiniMax image profile while main model profile is OpenAI-like.
- Tool call resolves MiniMax route credential, not main model credential.
- Typed image artifact appears in conversation/read-model payload.
- Video route exposes video create and query tools only when route exists.
- TTS route exposes TTS tools only when route exists.

Run:

```bash
cargo test -p jyowo-desktop-shell --test commands capability_route_conversation -- --nocapture
cargo test -p jyowo-harness-sdk --test runtime_assembly capability_route -- --nocapture
cargo test -p jyowo-harness-engine capability_route -- --nocapture
```

Expected:

```text
FAIL because full runtime integration is incomplete.
```

**Step 3: Wire route store into DesktopRuntimeState**

Add:

- route settings store field
- route settings lock
- route settings reload on save/delete
- route settings passed into harness build
- route option command receives `ProviderServiceAdapterAvailability` built from descriptor-derived bindings, not a hardcoded provider list

Default behavior:

```text
missing route file -> empty routes -> no service tools exposed
```

**Step 4: Ensure conversation model selection remains separate**

Do not add `modelConfigId` to `startRun`.

Conversation main model continues to come from:

```text
conversation-model-settings.json
or provider-settings defaultConfigId
```

Capability routes come from:

```text
provider-capability-routes.json
```

**Step 5: Add route state to harness options or builder**

Keep backend ownership.

Acceptable direction:

```rust
Harness::builder()
    .with_provider_capability_routes(routes)
```

or equivalent internal option.

Do not put desktop-only store code inside lower-level crates.

Apply the pure route filter from Task 5 during ToolPool assembly. This is the only task that wires persisted route settings into a conversation run.

Also pass the descriptor-derived `Vec<ToolServiceBinding>` from Task 5 into desktop route validation and route option building as `ProviderServiceAdapterAvailability`.

**Step 6: Run focused tests**

```bash
cargo test -p jyowo-desktop-shell --test commands capability_route_conversation -- --nocapture
cargo test -p jyowo-harness-sdk --test runtime_assembly capability_route -- --nocapture
cargo test -p jyowo-harness-engine capability_route -- --nocapture
cargo check --workspace
```

Expected:

```text
PASS
```

**Step 7: Pre-completion self-audit**

Confirm:

- Start run request shape remains stable.
- Main model and route model cannot overwrite each other.
- Route changes affect newly built harness/runtime consistently.
- Route missing means service tools hidden, not runtime failure after prompt exposure.
- Route filtering uses Task 4 helper and does not duplicate filtering logic.
- No provider operation allowlist was added to desktop commands.

**Step 8: Subagent audit**

Run the required subagent audit for Task 9.

**Step 9: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs crates/jyowo-harness-sdk/src/harness.rs crates/jyowo-harness-engine/src/turn.rs apps/desktop/src-tauri/tests/commands.rs crates/jyowo-harness-sdk/tests/runtime_assembly.rs
git commit -m "feat: integrate capability routes into conversations"
```

---

## Task 10: Settings UI For Capability Routing

**Goal:** Let users configure route-backed service models from backend-eligible route options after runtime route wiring is complete.

**Files:**

- Modify: `apps/desktop/src/features/settings/ProviderSettingsForm.tsx`
- Modify: `apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx`
- Modify: `apps/desktop/src/features/settings/ProviderSettingsForm.stories.tsx`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`

**Step 1: Pre-task analysis**

Inspect existing form state and mutation patterns:

```bash
rg -n "ProviderSettingsForm|providerSettingsQuery|catalogQuery|saveSettingsMutation|serviceCapabilities|modelCapabilities|useMutation|useQuery" apps/desktop/src/features/settings apps/desktop/src/shared/i18n/locales
```

**Step 2: Write failing component tests**

Add tests for:

- Shows capability routing section.
- Shows empty state when backend returns no eligible route options.
- Shows image/video/TTS/music options returned by `listProviderCapabilityRouteOptions`.
- Does not show backend options with `runtimeSupported = false` as selectable routes.
- Does not derive runtime adapter support from provider catalog on the frontend.
- Does not show `speech_to_text` unless the backend returns an eligible option for it.
- Saves a route through `saveProviderCapabilityRoute`.
- Deletes or disables a route through `deleteProviderCapabilityRoute`.
- Shows warning when selected main model lacks tool calling.
- Distinguishes image input from image generation in labels.
- Handles loading, empty, error, and ready states.

Run:

```bash
pnpm -C apps/desktop test -- ProviderSettingsForm.test.tsx
```

Expected:

```text
FAIL because route UI does not exist.
```

**Step 3: Add derived route option model**

Inside `ProviderSettingsForm.tsx`, derive route UI state from:

- `listProviderCapabilityRouteOptions`
- `listProviderCapabilityRoutes`

Eligibility:

```text
option.runtimeSupported === true
option returned by backend route option command
```

Do not let frontend bypass backend validation. The frontend may hide `runtimeSupported = false` options for user experience, but it must not declare an option runnable by reading provider catalog data locally.

**Step 4: Add UI section**

Add a section under the existing model/profile settings:

```text
Capability routing
  rows grouped by route kind returned by backend options
```

Each row shows:

- current route status
- selected provider profile
- operation id
- output artifact
- sync or async job
- cost risk
- route save/delete action

Do not hardcode a visible `Speech to text` row unless `listProviderCapabilityRouteOptions` returns at least one `speech_to_text` option.

Use existing form, badge, button, input, and layout patterns. Do not introduce a new design system.

Do not use a hero, marketing block, or nested cards.

**Step 5: Add i18n strings**

Add English and Chinese labels.

Required wording distinction:

```text
Image input
Image generation
Video input
Video generation
```

**Step 6: Run focused tests**

```bash
pnpm -C apps/desktop test -- ProviderSettingsForm.test.tsx
pnpm -C apps/desktop typecheck
```

Expected:

```text
PASS
```

**Step 7: Pre-completion self-audit**

Confirm:

- UI does not imply unsupported provider services are runnable.
- UI consumes backend route options as the only runtime support source.
- API key values never enter React state except existing reveal flow.
- External payloads are parsed by Zod command schemas.
- Loading, empty, error, ready states exist.
- Main model input capability labels are not mixed with generation routes.

**Step 8: Subagent audit**

Run the required subagent audit for Task 10.

**Step 9: Commit**

```bash
git add apps/desktop/src/features/settings/ProviderSettingsForm.tsx apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx apps/desktop/src/features/settings/ProviderSettingsForm.stories.tsx apps/desktop/src/shared/i18n/locales/en-US.ts apps/desktop/src/shared/i18n/locales/zh-CN.ts
git commit -m "feat: add capability routing settings"
```

---

## Task 11: Documentation And Developer Guardrails

**Goal:** Document capability routing in active frontend/backend specs and add guardrails so future providers do not bypass the route architecture.

**Files:**

- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`
- Modify: `docs/frontend/frontend-product-ux.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `scripts/check-backend-docs.mjs` only if a new required backend concept must be enforced.
- Modify: `scripts/check-frontend-docs.mjs` only if a new required frontend concept must be enforced.

**Step 1: Pre-task analysis**

Inspect current docs references:

```bash
rg -n "Model|ProviderSettings|Tool|Artifact|PermissionBroker|CommandClient|Zod|provider|capability" docs/backend docs/frontend scripts/check-*-docs.mjs
```

**Step 2: Write docs gate failing test if needed**

If adding a new required concept to docs gates, update the check script first and run:

```bash
pnpm check:docs
```

Expected:

```text
FAIL because docs do not yet include the new required concept.
```

If no script change is needed, record why in the task note.

**Step 3: Update backend docs**

Document:

- main conversation model vs capability route
- route store location
- route validation rules
- backend route option source and payload
- credential resolver operation scope
- service adapter boundary
- typed artifact output
- fail-closed behavior
- provider service adapter onboarding checklist

**Step 4: Update frontend docs**

Document:

- Settings -> Models capability routing UX
- image input vs image generation distinction
- frontend filtering is not security authority
- Zod command schemas for route commands
- loading/empty/error/ready requirements

**Step 5: Add provider onboarding checklist**

Checklist must include:

```text
official API docs verified
provider catalog service capability added
runtime adapter implemented
descriptor service binding added
route validation recognizes adapter through descriptor-derived service binding
backend route option command exposes only backend-evaluated runtime support
credential resolver passes operation id
artifact output typed
provider media download uses shared fail-closed URL/MIME policy
tests cover success and fail-closed errors
frontend eligibility shows only runnable options
```

**Step 6: Run docs gate**

```bash
pnpm check:docs
```

Expected:

```text
PASS
```

**Step 7: Subagent audit**

Run the required subagent audit for Task 11.

**Step 8: Commit**

```bash
git add docs/backend/backend-runtime.md docs/backend/backend-engineering.md docs/backend/backend-quality.md docs/frontend/frontend-product-ux.md docs/frontend/frontend-engineering.md docs/frontend/frontend-quality.md scripts/check-backend-docs.mjs scripts/check-frontend-docs.mjs
git commit -m "docs: document provider capability routing"
```

Only include scripts in the commit if they changed.

---

## Task 12: Final End-To-End Verification

**Goal:** Prove the feature works across backend contracts, runtime, frontend settings, tool routing, credentials, and artifacts.

**Files:**

- No new source files unless fixing findings.

**Step 1: Pre-task analysis**

List every route kind with at least one backend eligible route option:

```text
image_generation
video_generation
text_to_speech
music_generation
speech_to_text only if a real runtime adapter and backend eligible option exist
```

For each route kind, list:

- provider operation ids
- runtime adapter
- UI setting row
- tests
- artifact kind if any

Do not count contract-only route kinds as implemented.

**Step 2: Run full gates**

```bash
pnpm check:docs
pnpm check:desktop
pnpm check:rust
pnpm check
```

Expected:

```text
PASS
```

**Step 3: Run focused evidence commands**

```bash
cargo test -p jyowo-harness-contracts --test provider_capability_routes -- --nocapture
cargo test -p jyowo-desktop-shell --test commands provider_capability_route -- --nocapture
cargo test -p jyowo-desktop-shell --test commands provider_credential_route -- --nocapture
cargo test -p jyowo-desktop-shell --test commands capability_route_conversation -- --nocapture
cargo test -p jyowo-harness-tool --test minimax_tools -- --nocapture
cargo test -p jyowo-harness-sdk --test runtime_assembly capability_route -- --nocapture
pnpm -C apps/desktop test -- ProviderSettingsForm.test.tsx commands.test.ts default-client.test.ts run-event-schema.test.ts
```

Expected:

```text
PASS
```

**Step 4: Manual runtime smoke test**

Run the app:

```bash
pnpm dev
```

Verify manually in a real workspace:

- Add main conversation provider profile.
- Add MiniMax profile with API key.
- Configure image generation route.
- Start conversation with main model that supports tool calling.
- Ask for an image.
- Confirm image tool is visible to model only after route exists.
- Confirm artifact appears as image.
- Remove route.
- Confirm image tool is no longer visible.

If no real API key is available, do not fake success. Record manual smoke test as blocked by missing credentials and rely on local HTTP integration tests.

**Step 5: Final subagent audit**

Dispatch a fresh subagent to review the full branch.

Audit prompt:

```text
Audit the full provider capability routing implementation against docs/plans/2026-06-29-provider-capability-routing-implementation.md.

Check every task:
- contracts
- route store
- Tauri commands
- frontend settings UI
- service tool filtering
- operation-scoped credentials
- typed artifacts
- MiniMax adapter
- optional Seedance adapter if implemented
- docs
- gates

Return PASS only if no task is partially implemented and no fake runtime behavior exists.
```

**Step 6: Final security audit**

Run security subagent audit for the full branch.

Required focus:

- API key handling
- route credential isolation
- provider base URL validation
- artifact URL download safety
- MIME validation
- permission checks
- secret redaction
- frontend state

**Step 7: Fix audit findings**

If either audit fails:

- fix one finding at a time
- add or update tests
- rerun focused gates
- rerun the failed audit

Do not proceed with known audit failures.

**Step 8: Final commit**

If any final fixes were made:

```bash
git add <changed-files>
git commit -m "fix: complete provider capability routing verification"
```

**Step 9: Final delivery summary**

Final response must include:

- implementation branch
- worktree path
- commits
- gates run with pass/fail status
- any manual test limitation
- optional provider adapter skipped because official docs were unavailable

## Acceptance Criteria

The feature is complete only when all criteria are met.

- Users can configure main conversation model separately from service routes.
- Users can configure image/video/TTS/music routes only from eligible configured profiles.
- Backend rejects invalid or unsupported routes.
- Route settings persist in `.jyowo/runtime/provider-capability-routes.json`.
- `startRun` request shape does not carry route decisions.
- Main model input modalities still control user attachments.
- Main model tool calling still controls autonomous tool use.
- Service tools are hidden unless route-enabled.
- Routed service operations resolve route credentials, not default credentials.
- Service tools produce typed artifacts without provider-name heuristics.
- Image, video, and audio artifact MIME checks are fail-closed.
- MiniMax existing service tools are migrated to route-backed behavior.
- Optional Seedance follow-up is either implemented with official API evidence or skipped with no fake adapter and no failing tests.
- Frontend route UI covers loading, empty, error, ready states.
- Zod validates all new Tauri payloads.
- Docs describe route architecture and onboarding rules.
- Per-task subagent audits passed.
- Final full-branch subagent audit passed.
- Security audit passed.
- `pnpm check` passed, or any failure is documented as pre-existing and unrelated with evidence.
