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
