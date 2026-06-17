import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const agentsPath = join(repoRoot, 'AGENTS.md')

const requiredReferences = [
  'docs/frontend/agent-harness-frontend-development-guidelines.md',
  'docs/frontend/frontend-product-ux.md',
  'docs/frontend/frontend-engineering.md',
  'docs/frontend/frontend-quality.md',
  'docs/backend/agent-harness-backend-development-guidelines.md',
  'docs/backend/backend-runtime.md',
  'docs/backend/backend-engineering.md',
  'docs/backend/backend-quality.md',
]

const requiredCommands = [
  'pnpm check',
  'pnpm check:docs',
  'pnpm check:agent-docs',
  'pnpm check:frontend-docs',
  'pnpm check:backend-docs',
  'pnpm check:desktop',
  'pnpm check:rust',
  'cargo fmt --all --check',
  'cargo check --workspace',
  'cargo test --workspace',
]

const requiredSections = [
  '## 读取顺序',
  '## 执行规则',
  '## 修改边界',
  '## 前端规则',
  '## 后端规则',
  '## 安全边界',
  '## 质量门禁',
  '## 提交前自检',
]

const forbiddenLanguage = [
  { label: 'old project name', pattern: /octo[p]us|Octo[p]us|OCTO[P]US|\/data\/octo[p]us/ },
  { label: 'DEFERRED', pattern: /\bdeferred\b/i },
  { label: 'current phase', pattern: /\bcurrent\s+phase\b/i },
  { label: 'target phase', pattern: /\btarget\s+phase\b/i },
  { label: 'first phase', pattern: /\bfirst\s+phase\b/i },
  { label: 'foundation phase', pattern: /\bfoundation\s+phase\b/i },
  { label: 'future', pattern: /\bfuture\b/i },
  { label: 'MVP', pattern: /\bmvp\b/i },
  { label: 'P0', pattern: /\bP0\b/ },
  { label: 'P1', pattern: /\bP1\b/ },
  { label: 'P2', pattern: /\bP2\b/ },
  { label: 'phase number', pattern: /\bphase\s+\d+\b/i },
  { label: 'stage number', pattern: /\bstage\s+\d+\b/i },
  { label: '当前阶段', pattern: /当前阶段/ },
  { label: '目标阶段', pattern: /目标阶段/ },
  { label: '后续阶段', pattern: /后续阶段/ },
  { label: '阶段编号', pattern: /第[一二三四五六七八九十0-9]+阶段/ },
  { label: '期次编号', pattern: /[一二三四五六七八九十0-9]+期/ },
  { label: '未来计划', pattern: /未来计划/ },
]

const missingReferenceFiles = requiredReferences.filter((path) => !existsSync(join(repoRoot, path)))

if (!existsSync(agentsPath)) {
  console.error('Agent docs check failed.')
  console.error('\nMissing root agent instruction file:')
  console.error('- AGENTS.md')
  process.exit(1)
}

const agentsDoc = readFileSync(agentsPath, 'utf8')
const agentsDocLines = agentsDoc.split(/\r?\n/).map((line) => line.trim())
const referencedMarkdownFiles = Array.from(
  new Set(agentsDoc.match(/docs\/[A-Za-z0-9._/-]+\.md/g) ?? []),
)
const missingReferences = requiredReferences.filter((path) => !agentsDoc.includes(path))
const missingReferencedMarkdownFiles = referencedMarkdownFiles.filter(
  (path) => !existsSync(join(repoRoot, path)),
)
const missingCommands = requiredCommands.filter((command) => !agentsDocLines.includes(command))
const missingSections = requiredSections.filter((section) => !agentsDoc.includes(section))
const forbiddenMatches = forbiddenLanguage.filter(({ pattern }) => pattern.test(agentsDoc))

if (
  missingReferenceFiles.length > 0 ||
  missingReferencedMarkdownFiles.length > 0 ||
  missingReferences.length > 0 ||
  missingCommands.length > 0 ||
  missingSections.length > 0 ||
  forbiddenMatches.length > 0
) {
  console.error('Agent docs check failed.')
  if (missingReferenceFiles.length > 0) {
    console.error('\nReferenced specification files do not exist:')
    for (const path of missingReferenceFiles) {
      console.error(`- ${path}`)
    }
  }
  if (missingReferencedMarkdownFiles.length > 0) {
    console.error('\nAGENTS.md references Markdown files that do not exist:')
    for (const path of missingReferencedMarkdownFiles) {
      console.error(`- ${path}`)
    }
  }
  if (missingReferences.length > 0) {
    console.error('\nAGENTS.md is missing specification references:')
    for (const path of missingReferences) {
      console.error(`- ${path}`)
    }
  }
  if (missingCommands.length > 0) {
    console.error('\nAGENTS.md is missing quality gate commands:')
    for (const command of missingCommands) {
      console.error(`- ${command}`)
    }
  }
  if (missingSections.length > 0) {
    console.error('\nAGENTS.md is missing required sections:')
    for (const section of missingSections) {
      console.error(`- ${section}`)
    }
  }
  if (forbiddenMatches.length > 0) {
    console.error('\nForbidden language found in AGENTS.md:')
    for (const match of forbiddenMatches) {
      console.error(`- ${match.label}`)
    }
  }
  process.exit(1)
}

console.log('Agent docs check passed: AGENTS.md verified.')
