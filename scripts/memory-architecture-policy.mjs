import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const defaultRepoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

const SCOPED_RELATIVE_PATHS = [
  'apps/desktop/src/features/memory',
  'apps/desktop/src/shared/tauri/commands.ts',
  'apps/desktop/src-tauri/src/commands/memory.rs',
  'apps/desktop/src-tauri/src/commands/memory_settings.rs',
  'crates/jyowo-harness-contracts',
  'crates/jyowo-harness-memory',
  'crates/jyowo-harness-sdk/src/harness',
  'crates/jyowo-harness-sdk/src/builder.rs',
  'crates/jyowo-harness-sdk/src/harness/tool_pool.rs',
  'crates/jyowo-harness-tool/src/builtin/memory.rs',
  'crates/jyowo-harness-tool/src/builder.rs',
  'crates/jyowo-harness-tool/src/context.rs',
]

const EXCLUDED_PATH_SEGMENTS = ['node_modules', 'target', 'dist', 'storybook-static']
const TEST_PATH_PATTERNS = [
  /(^|\/)tests\//,
  /\.test\.[cm]?[jt]sx?$/,
  /\.test\.mjs$/,
  /(^|\/)src\/testing\//,
]

const DREAMS_ALLOWED_PATTERNS = [
  /(^|\/)crates\/jyowo-harness-memory\/src\/memdir\/migration\.rs$/,
  /(^|\/)crates\/jyowo-harness-memory\/tests\//,
  /(^|\/)docs\/plans\//,
  /(^|\/)docs\/superpowers\/plans\//,
]

const RULES = [
  {
    id: 'memory-tool-empty-result',
    applies: (rel) => rel === 'crates/jyowo-harness-tool/src/builtin/memory.rs',
    pattern: /(?:\brecords\b|["']records["'])\s*:\s*(?:vec!\s*\[\s*\]|\[\s*\])|(?:\brecord\b|["']record["'])\s*:\s*(?:None|null)\b/,
  },
  {
    id: 'memory-tool-hardcoded-completed',
    applies: (rel) => rel === 'crates/jyowo-harness-tool/src/builtin/memory.rs',
    pattern: /(?:\bstate\b|["']state["'])\s*:\s*(?:MemoryToolState::Completed|["']completed["'])|(?:\bstatus\b|["']status["'])\s*:\s*['"]completed['"]/,
  },
  {
    id: 'memory-manager-external-runtime-path',
    applies: (rel) =>
      rel.startsWith('crates/jyowo-harness-memory/src/') && !isAllowedMigrationOrTest(rel),
    pattern: /\bexternal\s*\(\s*\)/,
  },
  {
    id: 'first-writable-provider-runtime-path',
    applies: (rel) =>
      rel.startsWith('crates/jyowo-harness-memory/src/') && !isAllowedMigrationOrTest(rel),
    pattern: /\bwritable[\s\S]{0,160}\b(?:first|next)\s*\(\s*\)|\bfind\s*\([^)]*writable|first writable provider/i,
  },
  {
    id: 'production-memory-mutex-hashmap',
    applies: (rel) =>
      rel.startsWith('crates/jyowo-harness-memory/src/') && !isAllowedMigrationOrTest(rel),
    pattern: /Mutex\s*<\s*HashMap|Mutex\s*<[^>\n]*hash_map::HashMap|LazyLock\s*<\s*Mutex\s*<\s*HashMap/,
  },
  {
    id: 'fake-memory-extraction-empty-result',
    applies: (rel) =>
      rel.startsWith('crates/jyowo-harness-memory/src/extraction/') &&
      !isAllowedMigrationOrTest(rel),
    pattern: /\b(?:Vec::new\s*\(\s*\)|vec!\s*\[\s*\]|candidates\s*:\s*vec!\s*\[\s*\])/,
  },
  {
    id: 'min-similarity-bypass',
    applies: (rel) =>
      rel.startsWith('crates/jyowo-harness-memory/src/') && !isAllowedMigrationOrTest(rel),
    pattern: /min_similarity[\s\S]{0,120}(?:\|\|\s*true|<=\s*&?0\.0|<=\s*0\.0)/,
  },
  {
    id: 'global-memory-settings',
    applies: (rel) => rel.startsWith('crates/') || rel.startsWith('apps/desktop/src-tauri/'),
    pattern: /\bGLOBAL_SETTINGS\b/,
  },
  {
    id: 'dreams-runtime-reference',
    applies: (rel) => !isDreamsAllowed(rel),
    pattern: /\bDREAMS\.md\b|\bMemdirFile(?:Tag)?::Dreams\b/,
  },
  {
    id: 'frontend-memory-as-any',
    applies: (rel) => rel.startsWith('apps/desktop/src/features/memory/'),
    pattern: /\bas\s+any\b/,
  },
]

/** @typedef {{ file: string, line: number, rule: string, excerpt: string }} Violation */

export function scanMemoryArchitecturePolicy(repoRoot = defaultRepoRoot) {
  const files = collectScopedFiles(repoRoot)
  /** @type {Violation[]} */
  const violations = []

  for (const absolutePath of files) {
    const rel = relative(repoRoot, absolutePath)
    const content = readFileSync(absolutePath, 'utf8')

    for (const rule of RULES) {
      if (!rule.applies(rel)) {
        continue
      }

      const contentForRule =
        rule.id === 'dreams-runtime-reference' ? stripLineComments(content) : content
      const pattern = new RegExp(rule.pattern.source, rule.pattern.flags.includes('g') ? rule.pattern.flags : `${rule.pattern.flags}g`)

      for (const match of contentForRule.matchAll(pattern)) {
        violations.push({
          file: rel,
          line: lineNumberForIndex(contentForRule, match.index ?? 0),
          rule: rule.id,
          excerpt: excerptForIndex(contentForRule, match.index ?? 0),
        })
      }
    }
  }

  return { ok: violations.length === 0, violations }
}

function collectScopedFiles(repoRoot) {
  /** @type {string[]} */
  const files = []

  for (const scopedPath of SCOPED_RELATIVE_PATHS) {
    const absolute = join(repoRoot, scopedPath)
    if (!existsSync(absolute)) {
      continue
    }

    if (!statSync(absolute).isDirectory()) {
      if (shouldScanFile(scopedPath)) {
        files.push(absolute)
      }
      continue
    }

    walkDirectory(absolute, repoRoot, files)
  }

  return [...new Set(files)].sort()
}

function walkDirectory(dir, repoRoot, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolutePath = join(dir, entry.name)
    const rel = relative(repoRoot, absolutePath)

    if (entry.isDirectory()) {
      if (EXCLUDED_PATH_SEGMENTS.includes(entry.name)) {
        continue
      }
      walkDirectory(absolutePath, repoRoot, files)
      continue
    }

    if (shouldScanFile(rel)) {
      files.push(absolutePath)
    }
  }
}

function shouldScanFile(relPath) {
  if (
    relPath.endsWith('.rs') ||
    relPath.endsWith('.ts') ||
    relPath.endsWith('.tsx') ||
    relPath.endsWith('.mjs')
  ) {
    return true
  }

  return false
}

function isAllowedMigrationOrTest(rel) {
  return TEST_PATH_PATTERNS.some((pattern) => pattern.test(rel))
}

function isDreamsAllowed(rel) {
  return (
    isAllowedMigrationOrTest(rel) ||
    DREAMS_ALLOWED_PATTERNS.some((pattern) => pattern.test(rel))
  )
}

function lineNumberForIndex(content, index) {
  return content.slice(0, index).split(/\r?\n/).length
}

function excerptForIndex(content, index) {
  const start = content.lastIndexOf('\n', index) + 1
  const end = content.indexOf('\n', index)
  return content.slice(start, end === -1 ? undefined : end).trim()
}

function stripLineComments(content) {
  return content
    .split(/\r?\n/)
    .map((line) => {
      if (/^\s*\/\//.test(line) || /^\s*#/.test(line)) {
        return ''
      }
      return line
    })
    .join('\n')
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const result = scanMemoryArchitecturePolicy(defaultRepoRoot)

  if (!result.ok) {
    console.error('Memory architecture policy failed.')
    for (const violation of result.violations) {
      console.error(
        `- ${violation.file}:${violation.line} ${violation.rule}: ${violation.excerpt}`,
      )
    }
    process.exit(1)
  }

  console.log('Memory architecture policy passed.')
}
