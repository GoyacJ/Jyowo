import { execSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

function read(p) {
  return readFileSync(p, 'utf8')
}

const testingStrategyPath = join(repoRoot, 'docs', 'testing', 'testing-strategy.md')
const testInventoryPath = join(repoRoot, 'docs', 'testing', 'test-inventory.md')
const agentsPath = join(repoRoot, 'AGENTS.md')
const packageJsonPath = join(repoRoot, 'package.json')

const requiredTerms = [
  'PermissionBroker',
  'Redactor',
  'Journal',
  'Replay',
  'Secret',
  'Tauri command',
  'Zod',
  'serde',
  'Storybook',
  'Playwright',
  'manual-live',
  'stress',
  'fixture',
  'no mock',
]

const requiredScripts = [
  'audit:tests',
  'check:test-architecture',
  'check:testing-docs',
  'check:quick',
  'check:frontend:fast',
  'check:rust:fast',
  'check:agent-orchestration-no-fakes',
  'check:agent-supervisor-sidecar',
]

let errors = []

// Check testing strategy exists
if (!existsSync(testingStrategyPath)) {
  errors.push('docs/testing/testing-strategy.md does not exist')
} else {
  const strategyContent = read(testingStrategyPath)
  for (const term of requiredTerms) {
    if (!strategyContent.includes(term)) {
      errors.push(`testing-strategy.md missing required term: ${term}`)
    }
  }
}

// Check test inventory exists
if (!existsSync(testInventoryPath)) {
  errors.push('docs/testing/test-inventory.md does not exist')
}

// Check inventory matches audit output
if (existsSync(testInventoryPath)) {
  try {
    const currentInventory = read(testInventoryPath)
    const auditOutput = execSync('node scripts/audit-tests.mjs', {
      cwd: repoRoot,
      encoding: 'utf8',
      maxBuffer: 1024 * 1024,
    })
    if (currentInventory !== auditOutput) {
      errors.push(
        'docs/testing/test-inventory.md differs from pnpm audit:tests output. Run: pnpm audit:tests > docs/testing/test-inventory.md',
      )
    }
  } catch (e) {
    errors.push(`Failed to compare test inventory: ${e.message}`)
  }
}

// Check AGENTS.md references testing strategy
if (!existsSync(agentsPath)) {
  errors.push('AGENTS.md does not exist')
} else {
  const agentsContent = read(agentsPath)
  if (!agentsContent.includes('docs/testing/testing-strategy.md')) {
    errors.push('AGENTS.md does not reference docs/testing/testing-strategy.md')
  }
}

// Check package.json has required scripts
if (!existsSync(packageJsonPath)) {
  errors.push('package.json does not exist')
} else {
  const packageJson = JSON.parse(read(packageJsonPath))
  const scripts = packageJson.scripts ?? {}
  for (const script of requiredScripts) {
    if (typeof scripts[script] !== 'string') {
      errors.push(`package.json missing script: ${script}`)
    }
  }
  if (typeof scripts['check:docs'] === 'string' && !scripts['check:docs'].includes('pnpm check:testing-docs')) {
    errors.push('package.json check:docs must include pnpm check:testing-docs')
  }
  if (typeof scripts.check === 'string' && !scripts.check.includes('pnpm check:test-architecture')) {
    errors.push('package.json check must include pnpm check:test-architecture')
  }
  if (
    typeof scripts['check:quick'] === 'string' &&
    !scripts['check:quick'].includes('pnpm check:test-architecture')
  ) {
    errors.push('package.json check:quick must include pnpm check:test-architecture')
  }
  if (
    typeof scripts['check:quick'] === 'string' &&
    !scripts['check:quick'].includes('pnpm check:agent-orchestration-no-fakes')
  ) {
    errors.push('package.json check:quick must include pnpm check:agent-orchestration-no-fakes')
  }
  if (
    typeof scripts['check:quick'] === 'string' &&
    !scripts['check:quick'].includes('pnpm check:agent-supervisor-sidecar')
  ) {
    errors.push('package.json check:quick must include pnpm check:agent-supervisor-sidecar')
  }
}

if (errors.length > 0) {
  console.error('Testing docs check failed.')
  for (const err of errors) {
    console.error(`- ${err}`)
  }
  process.exit(1)
}

console.log('Testing docs check passed.')
