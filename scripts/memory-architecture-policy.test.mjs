import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'

import { scanMemoryArchitecturePolicy } from './memory-architecture-policy.mjs'

function writeFixture(files) {
  const root = join(tmpdir(), `jyowo-memory-policy-${randomUUID()}`)

  for (const [relativePath, content] of Object.entries(files)) {
    const absolutePath = join(root, relativePath)
    mkdirSync(join(absolutePath, '..'), { recursive: true })
    writeFileSync(absolutePath, content, 'utf8')
  }

  return root
}

test('fails MemoryTool empty records response in production code', () => {
  const root = writeFixture({
    'crates/jyowo-harness-tool/src/builtin/memory.rs': `
pub fn search() -> MemoryToolResponse {
  MemoryToolResponse { records: vec![], record: None, state: MemoryToolState::Completed }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'memory-tool-empty-result'))
  assert.ok(result.violations.some((violation) => violation.rule === 'memory-tool-hardcoded-completed'))
})

test('fails MemoryTool JSON macro empty and completed responses', () => {
  const root = writeFixture({
    'crates/jyowo-harness-tool/src/builtin/memory.rs': `
pub fn search() -> serde_json::Value {
  serde_json::json!({
    "state": "completed",
    "records": [],
    "record": null,
  })
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'memory-tool-empty-result'))
  assert.ok(result.violations.some((violation) => violation.rule === 'memory-tool-hardcoded-completed'))
})

test('fails MemoryManager external provider runtime path', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/external.rs': `
impl MemoryManager {
  pub async fn recall(&self) {
    let Some(provider) = self.external() else { return };
    provider.recall().await;
  }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'memory-manager-external-runtime-path'))
})

test('fails first writable provider selected through iterator next', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/external.rs': `
impl MemoryManager {
  pub fn write(&self) {
    let provider = self.registry.writable_providers_sorted().into_iter().next();
  }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'first-writable-provider-runtime-path'))
})

test('allows provider id near unrelated iterator next in fanout paths', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/external.rs': `
impl MemoryManager {
  pub async fn get_for_actor(&self) {
    let records = self.scan_records(
      vec![record],
      provider.provider_id().to_owned(),
      actor.session_id,
    ).await;
    if let Some(record) = records.into_iter().next() {
      return Ok(record);
    }
  }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, true)
})

test('fails production Mutex HashMap memory store and global settings', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/inbox.rs': `
static GLOBAL_SETTINGS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
pub struct MemoryInbox {
  items: Mutex<HashMap<String, Candidate>>,
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'production-memory-mutex-hashmap'))
  assert.ok(result.violations.some((violation) => violation.rule === 'global-memory-settings'))
})

test('fails fake extraction and min similarity bypass', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/extraction/job.rs': `
pub async fn run() -> Vec<MemoryCandidate> {
  Vec::new()
}
`,
    'crates/jyowo-harness-memory/src/local/provider.rs': `
let passed = query.min_similarity <= 0.0 || true;
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'fake-memory-extraction-empty-result'))
  assert.ok(result.violations.some((violation) => violation.rule === 'min-similarity-bypass'))
})

test('allows extraction output with empty consolidation list when candidates are real', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/extraction/worker.rs': `
let candidates = excerpt
  .lines()
  .filter_map(heuristic_candidate_from_line)
  .collect();
let output = ExtractionOutput {
  candidates,
  consolidations: Vec::new(),
  summary: None,
};
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, true)
})

test('fails extraction vec empty and multiline min similarity bypass', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/extraction/worker.rs': `
let output = ExtractionOutput {
  candidates: vec![],
};
`,
    'crates/jyowo-harness-memory/src/local/provider.rs': `
let allowed = query
  .min_similarity
  .le(&0.0)
  || true;
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'fake-memory-extraction-empty-result'))
  assert.ok(result.violations.some((violation) => violation.rule === 'min-similarity-bypass'))
})

test('fails legacy external slot feature names in Cargo manifests', () => {
  const root = writeFixture({
    'crates/jyowo-harness-sdk/Cargo.toml': `
[features]
memory-external-slot = ["jyowo-harness-engine/external-slot"]
`,
    'crates/jyowo-harness-engine/Cargo.toml': `
[features]
external-slot = []
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'legacy-external-slot-feature-name',
    ),
  )
})

test('fails cfg-gated external slot paths and single provider slot fields', () => {
  const root = writeFixture({
    'crates/jyowo-harness-engine/src/engine.rs': `
pub struct Engine {
  external: RwLock<Option<Arc<dyn MemoryProvider>>>,
}

#[cfg(feature = "external-slot")]
pub fn external_slot_enabled() {}
`,
    'crates/jyowo-harness-sdk/src/builder.rs': `
#[cfg(feature = "memory-external-slot")]
pub fn with_external_memory_provider(self, provider: Arc<dyn MemoryProvider>) -> Self {
  self
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'legacy-external-slot-cfg'))
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'legacy-single-external-provider-slot',
    ),
  )
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'legacy-with-external-memory-provider',
    ),
  )
})

test('fails production in-memory memory providers', () => {
  const root = writeFixture({
    'apps/desktop/src-tauri/src/commands/runtime.rs': `
pub fn build_runtime() {
  builder.with_memory_provider(InMemoryMemoryProvider::new("desktop-memory"));
}
`,
    'crates/jyowo-harness-sdk/src/builder.rs': `
pub fn default_memory() {
  let provider = InMemoryMemoryProvider::new("sdk-memory");
}
`,
    'crates/jyowo-harness-engine/src/engine.rs': `
pub fn default_memory() {
  let provider = InMemoryMemoryProvider::new("engine-memory");
}
`,
    'crates/jyowo-harness-context/src/engine.rs': `
pub fn default_memory() {
  let provider = InMemoryMemoryProvider::new("context-memory");
}
`,
    'crates/jyowo-harness-tool/src/builtin/memory.rs': `
pub fn default_memory() {
  let provider = InMemoryMemoryProvider::new("tool-memory");
}
`,
    'crates/jyowo-harness-agent-runtime/src/teams.rs': `
pub fn default_memory() {
  let provider = InMemoryMemoryProvider::new("agent-runtime-memory");
}
`,
    'crates/jyowo-harness-session/src/session.rs': `
pub fn default_memory() {
  let provider = InMemoryMemoryProvider::new("session-memory");
}
`,
    'crates/jyowo-harness-team/src/lib.rs': `
pub fn default_memory() {
  let provider = InMemoryMemoryProvider::new("team-memory");
}
`,
    'crates/jyowo-harness-subagent/src/lib.rs': `
pub fn default_memory() {
  let provider = InMemoryMemoryProvider::new("subagent-memory");
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'production-in-memory-memory-provider',
    ),
  )
})

test('fails label-only memory reference rendering', () => {
  const root = writeFixture({
    'crates/jyowo-harness-sdk/src/harness/conversation.rs': `
match reference {
  ConversationContextReference::Memory { id, label, .. } => {
    lines.push(format!("- memory: {} ({})", label, id));
  }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'label-only-memory-reference-rendering',
    ),
  )
})

test('fails forbidden raw fields in memory trace contracts', () => {
  const root = writeFixture({
    'crates/jyowo-harness-contracts/src/events/types.rs': `
pub struct MemoryRecallTrace {
  pub trace_id: MemoryTraceId,
  pub prompt: String,
}

pub struct MemoryInjectedTrace {
  pub content_hash: ContentHash,
  pub message_text: String,
}

pub struct MemoryCandidateTrace {
  pub content: String,
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'memory-trace-forbidden-raw-field',
    ),
  )
})

test('allows redacted and hash fields in memory trace contracts', () => {
  const root = writeFixture({
    'crates/jyowo-harness-contracts/src/events/types.rs': `
pub struct MemoryRecallTrace {
  pub trace_id: MemoryTraceId,
  pub redacted_count: u32,
  pub query_text_hash: ContentHash,
}

pub struct MemoryModelRequestPreviewSection {
  pub redacted_content: String,
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, true)
})

test('fails DREAMS runtime references and frontend as any in memory code', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/memdir/file.rs': `
pub fn runtime_file() -> &'static str {
  "DREAMS.md"
}
pub fn runtime_tag() -> MemdirFileTag {
  MemdirFileTag::Dreams
}
`,
    'apps/desktop/src/features/memory/MemorySettings.tsx': `
const payload = value as any
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'dreams-runtime-reference'))
  assert.ok(result.violations.some((violation) => violation.rule === 'frontend-memory-as-any'))
})

test('allows migration and test-only historical DREAMS references', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/memdir/migration.rs': `
pub const LEGACY_FILE: &str = "DREAMS.md";
`,
    'crates/jyowo-harness-memory/tests/dreams_migration.rs': `
#[test]
fn imports_legacy_dreams() {
  assert_eq!("DREAMS.md", "DREAMS.md");
}
`,
    'docs/plans/2026-07-04-agent-harness-memory-platform-implementation.md': `
Legacy DREAMS.md migration plan.
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, true)
})

test('fails arbitrary test-only DREAMS references outside migration tests', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/tests/recall.rs': `
#[test]
fn recall_does_not_use_dreams() {
  assert_eq!("DREAMS.md", "DREAMS.md");
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'dreams-runtime-reference'))
})

test('does not allow arbitrary runtime code under migrations directory', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/local/migrations/runtime.rs': `
pub fn bypass() {
  let items: Mutex<HashMap<String, String>>;
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'production-memory-mutex-hashmap'))
})

test('ignores DREAMS references in Rust comments', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/inbox.rs': `
// Historical DREAMS.md text is handled elsewhere.
pub fn inbox() {}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, true)
})

test('allows non-provider first hit access in memory reports', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/memdir/mod.rs': `
pub fn summarize(report: Report) {
  let hit = report.hits.first();
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, true)
})

test('fails memory recalled events without trace ids', () => {
  const root = writeFixture({
    'crates/jyowo-harness-context/src/engine.rs': `
Event::MemoryRecalled(MemoryRecalledEvent {
  session_id,
  run_id,
  trace_id: None,
})
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'memory-recalled-event-missing-trace'))
})

test('fails local provider empty evidence and unknown tombstone hash', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/local/provider.rs': `
let evidence_json = "{}".to_owned();
let tombstone_content_hash = "unknown";
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'local-provider-empty-evidence-json'))
  assert.ok(result.violations.some((violation) => violation.rule === 'local-provider-unknown-tombstone-hash'))
})

test('fails legacy memory tool runtime Value responses', () => {
  const root = writeFixture({
    'crates/jyowo-harness-tool/src/builtin/memory.rs': `
impl MemoryToolRuntimeCap for Runtime {
  async fn execute(&self) -> Result<Value, ToolError> {
    todo!()
  }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'memory-tool-runtime-value-response'))
})

test('fails legacy memory export without explicit request payload', () => {
  const root = writeFixture({
    'apps/desktop/src-tauri/src/commands/mod.rs': `
pub async fn export_memory_items(runtime_handle: State<'_, RuntimeHandle>) -> Result<Response, Error> {
  todo!()
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'legacy-export-memory-items-no-request'))
})

test('fails memory export without explicit user action gate', () => {
  const root = writeFixture({
    'apps/desktop/src-tauri/src/commands/memory.rs': `
pub async fn export_memory_items_with_runtime_state(
  request: ExportMemoryItemsRequest,
  state: &DesktopRuntimeState,
) -> Result<ExportMemoryItemsResponse, CommandErrorPayload> {
  if request.scope != "visible" {
    return Err(invalid_payload("memory export scope must be visible".to_owned()));
  }
  todo!()
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'export-memory-items-missing-explicit-action-gate'))
})

test('fails memory tool list hashes derived from redacted preview', () => {
  const root = writeFixture({
    'crates/jyowo-harness-sdk/src/harness/memory.rs': `
fn memory_tool_summary_view(summary: &MemorySummary) -> MemoryToolRecordView {
  MemoryToolRecordView {
    content_hash: content_hash(&summary.content_preview),
  }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'memory-tool-list-preview-hash'))
})

test('fails direct policy writes that use best-effort audit', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/src/external.rs': `
impl MemoryManager {
  pub async fn upsert_with_policy(&self, record: MemoryRecord, run_id: Option<RunId>) -> Result<MemoryId, MemoryError> {
    self.upsert(record, run_id).await
  }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'direct-memory-policy-best-effort-upsert'))
})

test('fails MemoryTool responses that drop action plan ids', () => {
  const root = writeFixture({
    'crates/jyowo-harness-sdk/src/harness/memory.rs': `
fn memory_tool_response() -> MemoryToolResponse {
  MemoryToolResponse {
    action_plan_id: None,
  }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'memory-tool-response-drops-action-plan'))
})

test('fails memory exports that ignore metadata and hash flags', () => {
  const root = writeFixture({
    'apps/desktop/src-tauri/src/commands/memory.rs': `
let items = records
  .into_iter()
  .map(memory_item_summary_payload)
  .collect::<Vec<_>>();
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'memory-export-options-ignored'))
})

test('fails unchecked team shared memory writes', () => {
  const root = writeFixture({
    'crates/jyowo-harness-team/src/lib.rs': `
pub struct SharedMemory {
  entries: Arc<Mutex<Vec<MemoryRecord>>>,
}

impl SharedMemory {
  async fn write(&self, record: MemoryRecord) {
    self.upsert_record_unchecked(record).await;
  }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'team-shared-memory-unchecked-write'))
})

test('fails team shared memory best-effort upsert and false durable descriptor', () => {
  const root = writeFixture({
    'crates/jyowo-harness-team/src/lib.rs': `
impl SharedMemory {
  async fn write_for_agent(&self, record: MemoryRecord, policy: MemoryOperationPolicy) {
    manager.upsert_with_policy(record, run_id, &policy).await.unwrap();
  }
}

impl harness_memory::MemoryProvider for TeamSharedMemoryProvider {
  fn descriptor(&self) -> MemoryProviderDescriptor {
    MemoryProviderDescriptor {
      provider_kind: MemoryProviderKind::Team,
      durability: MemoryProviderDurability::Durable,
    }
  }
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'team-shared-memory-best-effort-upsert'))
  assert.ok(result.violations.some((violation) => violation.rule === 'team-shared-memory-false-durable'))
})

test('scans plugin registry and Cargo paths for retired external-slot APIs', () => {
  const root = writeFixture({
    'crates/jyowo-harness-plugin/Cargo.toml': `
[features]
memory-external-slot = []
`,
    'crates/jyowo-harness-plugin/src/registry.rs': `
pub fn register_provider(builder: Builder) {
  builder.with_external_memory_provider(provider);
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'legacy-external-slot-feature-name'))
  assert.ok(result.violations.some((violation) => violation.rule === 'legacy-with-external-memory-provider'))
})

test('allows DREAMS references only in migration code or migration tests', () => {
  const root = writeFixture({
    'crates/jyowo-harness-memory/tests/dreams_runtime_semantics.rs': `
#[test]
fn dreams_runtime_semantics() {
  assert_eq!(file_name(), "DREAMS.md");
}
`,
    'crates/jyowo-harness-memory/tests/memdir_migration.rs': `
#[test]
fn imports_legacy_dreams() {
  assert_eq!(file_name(), "DREAMS.md");
}
`,
  })

  const result = scanMemoryArchitecturePolicy(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'dreams-runtime-reference'))
  assert.ok(
    result.violations.every(
      (violation) => violation.file !== 'crates/jyowo-harness-memory/tests/memdir_migration.rs',
    ),
  )
})
