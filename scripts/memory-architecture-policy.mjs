import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const defaultRepoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

const SCOPED_RELATIVE_PATHS = [
  'Cargo.toml',
  'apps/desktop/src',
  'apps/desktop/src-tauri/Cargo.toml',
  'apps/desktop/src-tauri/src',
  'apps/desktop/src/features/memory',
  'apps/desktop/src/shared/tauri/commands.ts',
  'crates/jyowo-harness-agent-runtime/Cargo.toml',
  'crates/jyowo-harness-agent-runtime/src',
  'crates/jyowo-harness-context/Cargo.toml',
  'crates/jyowo-harness-context/src/engine.rs',
  'crates/jyowo-harness-context/src/prompt.rs',
  'crates/jyowo-harness-contracts',
  'crates/jyowo-harness-engine/Cargo.toml',
  'crates/jyowo-harness-engine/src',
  'crates/jyowo-harness-memory/Cargo.toml',
  'crates/jyowo-harness-memory',
  'crates/jyowo-harness-plugin/Cargo.toml',
  'crates/jyowo-harness-plugin/src/registry.rs',
  'crates/jyowo-harness-sdk/Cargo.toml',
  'crates/jyowo-harness-sdk/src/harness',
  'crates/jyowo-harness-session/Cargo.toml',
  'crates/jyowo-harness-session/src',
  'crates/jyowo-harness-subagent/Cargo.toml',
  'crates/jyowo-harness-subagent/src',
  'crates/jyowo-harness-team/Cargo.toml',
  'crates/jyowo-harness-team/src',
  'crates/jyowo-harness-sdk/src/builder.rs',
  'crates/jyowo-harness-sdk/src/lib.rs',
  'crates/jyowo-harness-sdk/src/harness/tool_pool.rs',
  'crates/jyowo-harness-tool/Cargo.toml',
  'crates/jyowo-harness-tool/src/builtin/memory.rs',
  'crates/jyowo-harness-tool/src/builder.rs',
  'crates/jyowo-harness-tool/src/context.rs',
  'docs/plans',
  'docs/superpowers/plans',
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
  /(^|\/)crates\/jyowo-harness-memory\/tests\/.*migration.*\.rs$/,
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
    pattern: /->\s*Vec\s*<\s*MemoryCandidate\s*>[\s\S]{0,240}\bVec::new\s*\(\s*\)|\bcandidates\s*:\s*(?:Vec::new\s*\(\s*\)|vec!\s*\[\s*\])/,
  },
  {
    id: 'legacy-external-slot-feature-name',
    applies: (rel) =>
      (rel.endsWith('Cargo.toml') || rel.startsWith('crates/') || rel.startsWith('apps/')) &&
      !isAllowedMigrationOrTest(rel),
    pattern: /\b(?:memory-external-slot|external-slot)\b/,
  },
  {
    id: 'legacy-external-slot-cfg',
    applies: (rel) =>
      (rel.startsWith('crates/') || rel.startsWith('apps/')) && !isAllowedMigrationOrTest(rel),
    pattern: /#\s*\[\s*cfg\s*\([^\)]*feature\s*=\s*["'](?:memory-external-slot|external-slot)["']/,
  },
  {
    id: 'legacy-single-external-provider-slot',
    applies: (rel) =>
      (rel.startsWith('crates/') || rel.startsWith('apps/')) && !isAllowedMigrationOrTest(rel),
    pattern: /\bexternal\s*:\s*RwLock\s*<\s*Option\s*<\s*Arc\s*<\s*dyn\s+MemoryProvider\s*>\s*>\s*>/,
  },
  {
    id: 'legacy-with-external-memory-provider',
    applies: (rel) =>
      (rel.startsWith('crates/') || rel.startsWith('apps/')) && !isAllowedMigrationOrTest(rel),
    pattern: /\bwith_external_memory_provider\b/,
  },
  {
    id: 'production-in-memory-memory-provider',
    applies: (rel) =>
      !isAllowedMigrationOrTest(rel) &&
      (rel.startsWith('apps/desktop/src-tauri/src/') ||
        rel.startsWith('crates/jyowo-harness-agent-runtime/src/') ||
        rel.startsWith('crates/jyowo-harness-sdk/src/') ||
        rel.startsWith('crates/jyowo-harness-engine/src/') ||
        rel.startsWith('crates/jyowo-harness-context/src/') ||
        rel.startsWith('crates/jyowo-harness-session/src/') ||
        rel.startsWith('crates/jyowo-harness-subagent/src/') ||
        rel.startsWith('crates/jyowo-harness-team/src/') ||
        rel.startsWith('crates/jyowo-harness-tool/src/')),
    pattern: /\bInMemoryMemoryProvider::new\s*\(/,
  },
  {
    id: 'label-only-memory-reference-rendering',
    applies: (rel) =>
      !isAllowedMigrationOrTest(rel) &&
      (rel.startsWith('apps/') ||
        rel.startsWith('crates/jyowo-harness-sdk/src/') ||
        rel.startsWith('crates/jyowo-harness-context/src/') ||
        rel.startsWith('crates/jyowo-harness-engine/src/') ||
        rel.startsWith('crates/jyowo-harness-session/src/')),
    pattern: /["'`]-\s*memory:\s*\{\}\s*\(\{\}\)["'`]|format!\s*\(\s*["'`]-\s*memory:/,
  },
  {
    id: 'memory-trace-forbidden-raw-field',
    applies: (rel) => rel === 'crates/jyowo-harness-contracts/src/events/types.rs',
    pattern: /pub\s+struct\s+Memory(?:RecallTrace|ProviderTrace|CandidateTrace|InjectedTrace|DroppedTrace)\s*\{[\s\S]*?\bpub\s+(?:content|raw_content|prompt|message_text)\s*:/,
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
  {
    id: 'memory-recalled-event-missing-trace',
    applies: (rel) => rel === 'crates/jyowo-harness-context/src/engine.rs',
    pattern: /MemoryRecalledEvent\s*\{[\s\S]{0,900}trace_id\s*:\s*None\b/,
  },
  {
    id: 'local-provider-empty-evidence-json',
    applies: (rel) => rel === 'crates/jyowo-harness-memory/src/local/provider.rs',
    pattern: /evidence_json[\s\S]{0,120}(?:String::from\s*\(\s*["']\{\}["']\s*\)|["']\{\}["']\.to_owned\s*\(\s*\)|["']\{\}["'])/,
  },
  {
    id: 'local-provider-unknown-tombstone-hash',
    applies: (rel) => rel === 'crates/jyowo-harness-memory/src/local/provider.rs',
    pattern: /(?:content_hash|tombstone)[\s\S]{0,160}["']unknown["']/,
  },
  {
    id: 'memory-tool-runtime-value-response',
    applies: (rel) =>
      rel === 'crates/jyowo-harness-tool/src/builtin/memory.rs' ||
      rel === 'crates/jyowo-harness-sdk/src/harness/memory.rs',
    pattern: /MemoryToolRuntimeCap[\s\S]{0,220}Result\s*<\s*Value\s*,\s*ToolError\s*>/,
  },
  {
    id: 'legacy-export-memory-items-no-request',
    applies: (rel) =>
      rel === 'apps/desktop/src-tauri/src/commands/memory.rs' ||
      rel === 'apps/desktop/src-tauri/src/commands/mod.rs',
    pattern: /export_memory_items(?:_with_runtime_state)?\s*\(\s*(?:runtime_handle|state)\s*:/,
  },
  {
    id: 'export-memory-items-missing-explicit-action-gate',
    applies: (rel) => rel === 'apps/desktop/src-tauri/src/commands/memory.rs',
    pattern: /pub\s+async\s+fn\s+export_memory_items_with_runtime_state[\s\S]*?\{\s*if\s+request\.scope\b/,
  },
  {
    id: 'memory-tool-list-preview-hash',
    applies: (rel) => rel === 'crates/jyowo-harness-sdk/src/harness/memory.rs',
    pattern: /content_hash\s*:\s*content_hash\s*\(\s*&summary\.content_preview\s*\)/,
  },
  {
    id: 'direct-memory-policy-best-effort-upsert',
    applies: (rel) => rel === 'crates/jyowo-harness-memory/src/external.rs',
    pattern: /pub\s+async\s+fn\s+upsert_with_policy[\s\S]{0,900}\bself\.upsert\s*\(\s*record\s*,\s*run_id\s*\)\s*\.await/,
  },
  {
    id: 'memory-tool-response-drops-action-plan',
    applies: (rel) => rel === 'crates/jyowo-harness-sdk/src/harness/memory.rs',
    pattern: /fn\s+memory_tool_response[\s\S]{0,900}action_plan_id\s*:\s*None/,
  },
  {
    id: 'memory-export-options-ignored',
    applies: (rel) => rel === 'apps/desktop/src-tauri/src/commands/memory.rs',
    pattern: /let\s+items\s*=\s*records[\s\S]{0,160}\.map\s*\(\s*memory_item_summary_payload\s*\)/,
  },
  {
    id: 'team-shared-memory-unchecked-write',
    applies: (rel) => rel === 'crates/jyowo-harness-team/src/lib.rs',
    pattern: /upsert_record_unchecked|Arc\s*<\s*Mutex\s*<\s*Vec\s*<\s*MemoryRecord\s*>\s*>\s*>/,
  },
  {
    id: 'team-shared-memory-best-effort-upsert',
    applies: (rel) => rel === 'crates/jyowo-harness-team/src/lib.rs',
    pattern: /\bupsert_with_policy\s*\(/,
  },
  {
    id: 'team-shared-memory-false-durable',
    applies: (rel) => rel === 'crates/jyowo-harness-team/src/lib.rs',
    pattern: /provider_kind\s*:\s*MemoryProviderKind::Team[\s\S]{0,500}durability\s*:\s*MemoryProviderDurability::Durable/,
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
    relPath.endsWith('.mjs') ||
    relPath.endsWith('.md') ||
    relPath.endsWith('Cargo.toml')
  ) {
    return true
  }

  return false
}

function isAllowedMigrationOrTest(rel) {
  return TEST_PATH_PATTERNS.some((pattern) => pattern.test(rel))
}

function isDreamsAllowed(rel) {
  return DREAMS_ALLOWED_PATTERNS.some((pattern) => pattern.test(rel))
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
