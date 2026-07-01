import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const docsDir = join(repoRoot, 'docs', 'design2')

const requiredDocs = [
  'antigravity_2_0_design_system_specification.md',
]

const forbiddenStageLanguage = [
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

function read(path) {
  return readFileSync(path, 'utf8')
}

const design2DocEntries = readdirSync(docsDir, { withFileTypes: true })
const markdownFiles = design2DocEntries
  .filter((entry) => entry.isFile() && entry.name.endsWith('.md'))
  .map((entry) => entry.name)
  .sort()

const missingDocs = requiredDocs.filter((file) => !markdownFiles.includes(file))
const unexpectedDocs = design2DocEntries
  .filter((entry) => entry.isDirectory() || !requiredDocs.includes(entry.name))
  .map((entry) => (entry.isDirectory() ? `${entry.name}/` : entry.name))
  .sort()

const activeDocs = markdownFiles
  .map((file) => read(join(docsDir, file)))
  .join('\n')

const oldNameMatches = activeDocs.match(/octo[p]us|Octo[p]us|OCTO[P]US|\/data\/octo[p]us/g) ?? []
const stageLanguageMatches = forbiddenStageLanguage
  .filter(({ pattern }) => pattern.test(activeDocs))
  .map(({ label }) => label)

if (
  missingDocs.length > 0 ||
  unexpectedDocs.length > 0 ||
  oldNameMatches.length > 0 ||
  stageLanguageMatches.length > 0
) {
  console.error('Design2 docs check failed.')
  if (missingDocs.length > 0) {
    console.error('\nMissing active docs:')
    for (const file of missingDocs) {
      console.error(`- ${file}`)
    }
  }
  if (unexpectedDocs.length > 0) {
    console.error('\nUnexpected design2 docs:')
    for (const file of unexpectedDocs) {
      console.error(`- ${file}`)
    }
  }
  if (oldNameMatches.length > 0) {
    console.error('\nOld project names found in active design2 docs.')
  }
  if (stageLanguageMatches.length > 0) {
    console.error('\nStage-based language found in normative design2 docs:')
    for (const phrase of stageLanguageMatches) {
      console.error(`- ${phrase}`)
    }
  }
  process.exit(1)
}

console.log(`Design2 docs check passed: ${requiredDocs.length} active docs verified.`)
