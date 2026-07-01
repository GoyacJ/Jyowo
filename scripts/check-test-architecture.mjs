import { execSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { basename, dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

function repoRel(p) {
  return relative(repoRoot, p)
}

function read(p) {
  return readFileSync(p, 'utf8')
}

function lineCount(p) {
  return read(p).split(/\r?\n/).length
}

// ── Temporary allowlist ──
// Format: { file: 'relative/path', reason: '...', followUp: '...' }
// Entries are allowed only for historical files outside the current cleanup plan.

const temporaryAllowlist = [
  {
    file: 'apps/desktop/src/shared/tauri/commands.test.ts',
    reason: 'historical IPC schema coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split shared Tauri command schema coverage',
  },
  {
    file: 'crates/jyowo-harness-plugin/tests/registry.rs',
    reason: 'historical plugin registry coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split plugin registry coverage',
  },
  {
    file: 'crates/jyowo-harness-engine/tests/subagent_tool_feature.rs',
    reason: 'historical subagent tool coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split engine subagent tool coverage',
  },
  {
    file: 'crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs',
    reason: 'historical conversation projection coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split journal projector coverage',
  },
  {
    file: 'crates/jyowo-harness-journal/tests/conversation_read_model.rs',
    reason: 'historical read-model coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split journal read-model coverage',
  },
  {
    file: 'crates/jyowo-harness-engine/tests/main_loop.rs',
    reason: 'historical engine loop coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split engine main-loop coverage',
  },
  {
    file: 'crates/jyowo-harness-engine/tests/hook_pipeline.rs',
    reason: 'historical hook pipeline coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split engine hook pipeline coverage',
  },
  {
    file: 'apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx',
    reason: 'historical settings component coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split provider settings component coverage',
  },
  {
    file: 'crates/jyowo-harness-mcp/tests/server_protocol.rs',
    reason: 'historical MCP protocol coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split MCP server protocol coverage',
  },
  {
    file: 'crates/jyowo-harness-team/tests/team_e2e.rs',
    reason: 'historical team integration coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split team integration coverage',
  },
  {
    file: 'apps/desktop/src/shared/events/run-event-schema.test.ts',
    reason: 'historical RunEvent schema coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split RunEvent schema coverage',
  },
  {
    file: 'crates/jyowo-harness-plugin/tests/sources.rs',
    reason: 'historical plugin source coverage outside the 2026-07-01 cleanup targets',
    followUp: 'test-architecture follow-up: split plugin source coverage',
  },
]

const allowlistedFiles = new Set(temporaryAllowlist.map((e) => e.file))

// ── Collect all tracked test files ──

let allTracked
try {
  allTracked = execSync('git ls-files', { cwd: repoRoot, encoding: 'utf8', maxBuffer: 1024 * 1024 })
    .split(/\r?\n/)
    .filter(Boolean)
} catch {
  console.error('check-test-architecture: failed to run git ls-files')
  process.exit(1)
}

function isTestFile(p) {
  return (
    (p.startsWith('crates/') && basename(dirname(p)) === 'tests' && p.endsWith('.rs')) ||
    (p.startsWith('apps/desktop/src-tauri/tests/') && p.endsWith('.rs')) ||
    (p.startsWith('apps/desktop/src/') && (p.endsWith('.test.ts') || p.endsWith('.test.tsx') || p.endsWith('.stories.tsx') || p.endsWith('.stories.ts'))) ||
    (p.startsWith('apps/desktop/e2e/') && p.endsWith('.spec.ts')) ||
    (p.startsWith('scripts/') && (p.endsWith('.test.mjs') || p.endsWith('.test.ts')))
  )
}

function isTestFixtureFile(p) {
  return p.startsWith('apps/desktop/src/testing/') && (p.endsWith('.ts') || p.endsWith('.tsx'))
}

const trackedTestFiles = allTracked.filter(isTestFile)
const trackedFixtureFiles = allTracked.filter(isTestFixtureFile)

let errors = []
let warnings = []

// ── Disallowed names check ──

for (const f of trackedTestFiles) {
  const name = basename(f)
  if (allowlistedFiles.has(f)) continue
  if (name.startsWith('spike_')) {
    errors.push(`${f}: disallowed name prefix 'spike_'`)
  }
  if (/^m\d+_/.test(name)) {
    errors.push(`${f}: disallowed name prefix 'mN_'`)
  }
  if (/^t\d+_/.test(name)) {
    errors.push(`${f}: disallowed name prefix 'tN_'`)
  }
  if (/_e2e\.rs$/.test(name) && !f.startsWith('apps/desktop/src-tauri/tests/')) {
    errors.push(`${f}: disallowed Rust _e2e.rs name outside real desktop/browser E2E`)
  }
}

// ── Size checks for test and fixture files ──

const allAnalyzableFiles = [...trackedTestFiles, ...trackedFixtureFiles]

for (const f of allAnalyzableFiles) {
  const absPath = join(repoRoot, f)
  if (!existsSync(absPath)) continue
  const lines = lineCount(absPath)

  if (lines > 1200) {
    if (allowlistedFiles.has(f)) continue
    errors.push(`${f}: ${lines} lines exceeds 1200-line hard limit`)
  } else if (lines > 800) {
    warnings.push(`${f}: ${lines} lines exceeds 800-line warning threshold`)
  }
}

// ── manual_live_*.rs must be #[ignore] ──

for (const f of trackedTestFiles) {
  const name = basename(f)
  if (name.startsWith('manual_live_')) {
    const absPath = join(repoRoot, f)
    if (existsSync(absPath)) {
      const content = read(absPath)
      const hasIgnore = content.split(/\r?\n/).some((l) => /^\s*#\[ignore\]/.test(l.trim()))
      if (!hasIgnore) {
        errors.push(`${f}: manual_live test must have #[ignore]`)
      }
    }
  }
}

// ── stress_*.rs must be #[ignore] ──

for (const f of trackedTestFiles) {
  const name = basename(f)
  if (name.startsWith('stress_')) {
    const absPath = join(repoRoot, f)
    if (existsSync(absPath)) {
      const content = read(absPath)
      const hasIgnore = content.split(/\r?\n/).some((l) => /^\s*#\[ignore\]/.test(l.trim()))
      if (!hasIgnore) {
        errors.push(`${f}: stress test must have #[ignore]`)
      }
    }
  }
}

// ── command-client.ts monolith check ──

const cmdClientPath = join(repoRoot, 'apps', 'desktop', 'src', 'testing', 'command-client.ts')
if (existsSync(cmdClientPath) && !allowlistedFiles.has('apps/desktop/src/testing/command-client.ts')) {
  errors.push('apps/desktop/src/testing/command-client.ts: monolithic fixture should be split (see Task 6)')
}

const agentsPath = join(repoRoot, 'AGENTS.md')
if (!existsSync(agentsPath) || !read(agentsPath).includes('docs/testing/testing-strategy.md')) {
  errors.push('AGENTS.md must reference docs/testing/testing-strategy.md')
}

const inventoryPath = join(repoRoot, 'docs/testing/test-inventory.md')
if (!existsSync(inventoryPath)) {
  errors.push('docs/testing/test-inventory.md does not exist')
} else {
  const inventory = read(inventoryPath)
  const expectedInventory = execSync('node scripts/audit-tests.mjs', {
    cwd: repoRoot,
    encoding: 'utf8',
    maxBuffer: 1024 * 1024,
  })
  if (inventory !== expectedInventory) {
    errors.push('docs/testing/test-inventory.md differs from node scripts/audit-tests.mjs output')
  }
}

// ── Report ──

if (warnings.length > 0) {
  console.warn('Test architecture warnings:')
  for (const w of warnings) {
    console.warn(`  ${w}`)
  }
}

if (errors.length > 0) {
  console.error('Test architecture check failed.')
  for (const e of errors) {
    console.error(`  ${e}`)
  }
  process.exit(1)
}

console.log('Test architecture check passed.')
