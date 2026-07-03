import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const defaultRepoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

/** @typedef {{ file: string, line: number, rule: string, excerpt: string }} Violation */

export const AGENT_CONTEXT_PATTERNS = [
  /subagent/i,
  /agent\s+team/i,
  /background\s+agent/i,
  /agent\s+runtime/i,
  /agent\s+orchestration/i,
]

export const AUTHORIZATION_CONTEXT_PATTERNS = [
  /\bauthorization\b/i,
  /\bpermission\b/i,
  /\bsandbox\b/i,
  /\bauthorization[_\s]?ticket\b/i,
  /\bpreflight\b/i,
  /\bPermissionBroker\b/,
  /\bPermissionAuthority\b/,
  /\bAuthorizationService\b/,
  /\bTicketLedger\b/,
  /\bToolActionPlan\b/,
  /\bAuthorizationEventSink\b/,
  /\bDecisionStore\b/,
  /\bDecisionHistory\b/,
  /\bSandboxPolicy\b/,
  /\bAuthorizationTicket\b/,
  /\bresolve_permission\b/,
  /\bexecute_authorized\b/,
  /\bSandboxBackend\b/,
  /\bSandboxPreflight\b/,
  /\bActionResource\b/,
  /\bPermissionReview\b/,
  /\bPermissionConfirmation\b/,
  /\bAuthorizationRequest\b/,
  /\bAuthorizationOutcome\b/,
  /\bhard[_\s]?policy\b/i,
]

export const PLACEHOLDER_PATTERNS = [
  { id: 'experimental-label', pattern: /\bexperimental\b/i },
  { id: 'coming-soon', pattern: /coming\s+soon/i },
  { id: 'unimplemented', pattern: /\bunimplemented\b/i },
  { id: 'not-yet-implemented', pattern: /not\s+yet\s+implemented/i },
  { id: 'placeholder-marker', pattern: /\bplaceholder\b/i },
  { id: 'fake-marker', pattern: /\bfake\b/i },
  { id: 'mock-marker', pattern: /\bmock\b/i },
  { id: 'noop-marker', pattern: /\bno-?op\b/i },
  { id: 'todo-marker', pattern: /\bTODO\b/i },
  { id: 'future-tense', pattern: /\b(will be implemented|coming in a future|not available yet)\b/i },
]

export const AUTHORIZATION_SPECIFIC_PATTERNS = [
  { id: 'allow-all', pattern: /\ballow[_\s-]all\b/i },
  { id: 'bypass-policy', pattern: /\bbypass[_\s-](?:policy|permission)\b/i },
]

export const FAKE_RUNTIME_PATTERNS = [
  { id: 'fake-agent-runner', pattern: /fake\s+(agent|subagent|background)/i },
  { id: 'mock-agent-runtime', pattern: /mock\s+agent\s+runtime/i },
  { id: 'fake-background-provider', pattern: /fake\s+background\s+provider/i },
  { id: 'fake-filename', pattern: /Fake(?:Subagent|Background|Agent)/ },
]

export const HARDCODED_UNAVAILABLE_ASSIGNMENTS = [
  { id: 'hardcoded-subagents-unavailable', pattern: /\bsubagents_available\s*[:=]\s*false\b/ },
  { id: 'hardcoded-agent-teams-unavailable', pattern: /\bagent_teams_available\s*[:=]\s*false\b/ },
  {
    id: 'hardcoded-background-unavailable',
    pattern: /\bbackground_agents_available\s*[:=]\s*false\b/,
  },
]

export const TEMPORARY_ALLOWLIST_PATTERNS = [
  {
    id: 'temporary-availability-allowlist',
    pattern: /\b(?:temporary|temp|intermediate)\w*(?:allowlist|allow-list|allow_list)\w*(?:agent|capabilit|availab)|\b(?:allowlist|allow-list|allow_list)\w*(?:hardcoded|agent|capabilit|availab)/i,
  },
]

const FRONTEND_CAPABILITY_STATE_PATTERNS = [
  /\bsubagentsAvailable\s*:\s*true\b/,
  /\bagentTeamsAvailable\s*:\s*true\b/,
  /\bbackgroundAgentsAvailable\s*:\s*true\b/,
]

const RUNTIME_DELEGATION_KEYWORDS = [
  'harness',
  'sdk',
  'AgentRuntime',
  'agent_runtime',
  'jyowo_harness',
  'runtime_state',
  'BackgroundAgentManager',
  'AgentCapabilityResolver',
]

const EXCLUDED_PATH_SEGMENTS = [
  'node_modules',
  'target',
  'dist',
  'storybook-static',
  'tests',
]

const EXCLUDED_FILE_SUFFIXES = ['.test.ts', '.test.tsx', '.test.mjs']

const SCOPED_RELATIVE_PATHS = [
  'apps/desktop/src-tauri/src/commands',
  'apps/desktop/src-tauri/src/lib.rs',
  'apps/desktop/src-tauri/src/agent_supervisor.rs',
  'apps/desktop/src-tauri/src/bin/jyowo-agent-supervisor.rs',
  'apps/desktop/src-tauri/build.rs',
  'apps/desktop/src-tauri/capabilities/default.json',
  'apps/desktop/src-tauri/tauri.conf.json',
  'apps/desktop/src/shared/tauri/commands.ts',
  'apps/desktop/src/features/settings/ExecutionSettings.tsx',
  'apps/desktop/src/features/conversation/Composer.tsx',
  'apps/desktop/src/features/conversation/use-conversation.ts',
  'apps/desktop/src/features/conversation/use-agent-profiles.ts',
  'apps/desktop/src/features/conversation/ConversationWorkspace.tsx',
  'apps/desktop/src/features/conversation/AgentActivitySegment.tsx',
  'apps/desktop/src/features/conversation/timeline/conversation-timeline-selectors.ts',
  'apps/desktop/src/features/background-agents',
  'crates/jyowo-harness-contracts',
  'crates/jyowo-harness-journal/src/conversation_worktree_projector.rs',
  'crates/jyowo-harness-journal/src/conversation_read_model.rs',
  'crates/jyowo-harness-agent-runtime',
  'crates/jyowo-harness-execution',
  'crates/jyowo-harness-permission',
  'crates/jyowo-harness-sandbox',
  'crates/jyowo-harness-sdk',
  'crates/jyowo-harness-subagent',
  'crates/jyowo-harness-team',
  'scripts/build-agent-supervisor-sidecar.mjs',
  'package.json',
]

/**
 * @param {string} repoRoot
 * @param {{ scopedPaths?: string[] }} [options]
 * @returns {{ ok: boolean, violations: Violation[] }}
 */
export function scanAgentOrchestrationNoFakes(repoRoot, options = {}) {
  const scopedPaths = options.scopedPaths ?? SCOPED_RELATIVE_PATHS
  const files = collectScopedFiles(repoRoot, scopedPaths)
  /** @type {Violation[]} */
  const violations = []

  for (const absolutePath of files) {
    const rel = relative(repoRoot, absolutePath)
    if (isExcludedProductionFile(rel)) continue
    const content = readFileSync(absolutePath, 'utf8')
    const lines = content.split(/\r?\n/)

    violations.push(...scanPlaceholderPatterns(rel, lines))
    violations.push(...scanFakeRuntimePatterns(rel, content, lines))
    violations.push(...scanHardcodedUnavailableAssignments(rel, lines))
    violations.push(...scanTemporaryAvailabilityAllowlists(rel, lines))
    violations.push(...scanNoopAgentCommands(rel, content, lines))
    violations.push(...scanFrontendOnlyAgentCapabilityState(rel, lines))
      violations.push(...scanAuthorizationSpecificPatterns(rel, lines))
  }

  return { ok: violations.length === 0, violations }
}

/**
 * @param {string} repoRoot
 * @param {string[]} scopedPaths
 */
function collectScopedFiles(repoRoot, scopedPaths) {
  /** @type {string[]} */
  const files = []

  for (const scopedPath of scopedPaths) {
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

/**
 * @param {string} dir
 * @param {string} repoRoot
 * @param {string[]} files
 */
function walkDirectory(dir, repoRoot, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolutePath = join(dir, entry.name)
    const rel = relative(repoRoot, absolutePath)

    if (entry.isDirectory()) {
      if (EXCLUDED_PATH_SEGMENTS.includes(entry.name) || rel.startsWith('docs/')) {
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

/**
 * @param {string} relPath
 */
function shouldScanFile(relPath) {
  if (relPath.startsWith('docs/')) {
    return false
  }

  for (const segment of EXCLUDED_PATH_SEGMENTS) {
    if (relPath.split('/').includes(segment)) {
      return false
    }
  }

  for (const suffix of EXCLUDED_FILE_SUFFIXES) {
    if (relPath.endsWith(suffix)) {
      return false
    }
  }

  return (
    relPath.endsWith('.rs') ||
    relPath.endsWith('.ts') ||
    relPath.endsWith('.tsx') ||
    relPath.endsWith('.mjs') ||
    relPath.endsWith('.json')
  )
}

/**
 * @param {string} rel
 * @param {string[]} lines
 */
function scanPlaceholderPatterns(rel, lines) {
  /** @type {Violation[]} */
  const violations = []

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    if (isCommentOnlyLine(line)) {
      continue
    }
    for (const { id, pattern } of PLACEHOLDER_PATTERNS) {
      if (!pattern.test(line)) {
        continue
      }
      if (hasFeatureGateContextNearby(lines, index)) {
        continue
      }
      if (!hasAgentContextNearby(lines, index) && !hasAuthorizationContextNearby(lines, index)) {
        continue
      }
      violations.push({
        file: rel,
        line: index + 1,
        rule: id,
        excerpt: line.trim(),
      })
    }
  }

  return violations
}

/**
 * @param {string} rel
 * @param {string} content
 * @param {string[]} lines
 */
function scanFakeRuntimePatterns(rel, content, lines) {
  /** @type {Violation[]} */
  const violations = []

  for (const { id, pattern } of FAKE_RUNTIME_PATTERNS) {
    if (id === 'fake-filename') {
      if (pattern.test(rel)) {
        violations.push({
          file: rel,
          line: 1,
          rule: id,
          excerpt: rel,
        })
      }
      continue
    }

    if (!pattern.test(content)) {
      continue
    }

    for (let index = 0; index < lines.length; index += 1) {
      if (!pattern.test(lines[index])) {
        continue
      }
      violations.push({
        file: rel,
        line: index + 1,
        rule: id,
        excerpt: lines[index].trim(),
      })
    }
  }

  return violations
}

/**
 * @param {string} rel
 * @param {string[]} lines
 */
function scanHardcodedUnavailableAssignments(rel, lines) {
  /** @type {Violation[]} */
  const violations = []

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    if (isCommentOnlyLine(line)) {
      continue
    }

    for (const { id, pattern } of HARDCODED_UNAVAILABLE_ASSIGNMENTS) {
      if (!pattern.test(line)) {
        continue
      }
      violations.push({
        file: rel,
        line: index + 1,
        rule: id,
        excerpt: line.trim(),
      })
    }
  }

  return violations
}

/**
 * @param {string} rel
 * @param {string[]} lines
 */
/**
 * @param {string} rel
 * @param {string[]} lines
 */
function scanAuthorizationSpecificPatterns(rel, lines) {
  /** @type {Violation[]} */
  const violations = []

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    if (isCommentOnlyLine(line)) {
      continue
    }

    for (const { id, pattern } of AUTHORIZATION_SPECIFIC_PATTERNS) {
      if (!pattern.test(line)) {
        continue
      }
      if (!hasAuthorizationContextNearby(lines, index)) {
        continue
      }
      violations.push({
        file: rel,
        line: index + 1,
        rule: id,
        excerpt: line.trim(),
      })
    }
  }

  return violations
}

function scanTemporaryAvailabilityAllowlists(rel, lines) {
  /** @type {Violation[]} */
  const violations = []

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    if (isCommentOnlyLine(line)) {
      continue
    }

    for (const { id, pattern } of TEMPORARY_ALLOWLIST_PATTERNS) {
      if (!pattern.test(line)) {
        continue
      }
      if (!hasAvailabilityContextNearby(lines, index)) {
        continue
      }
      violations.push({
        file: rel,
        line: index + 1,
        rule: id,
        excerpt: line.trim(),
      })
    }
  }

  return violations
}

/**
 * @param {string} rel
 * @param {string} content
 * @param {string[]} lines
 */
function scanNoopAgentCommands(rel, content, lines) {
  if (!rel.includes('commands') || !rel.endsWith('.rs')) {
    return []
  }

  /** @type {Violation[]} */
  const violations = []
  const commandRegex =
    /#\[tauri::command[^\]]*\][\s\S]*?pub\s+(?:async\s+)?fn\s+([a-zA-Z0-9_]*(?:agent|subagent|background|team)[a-zA-Z0-9_]*)\s*\([^)]*\)[^{]*\{([\s\S]*?)\n\}/g

  let match = commandRegex.exec(content)
  while (match) {
    const fnName = match[1]
    const body = match[2]
    const startIndex = content.indexOf(match[0])
    const line = content.slice(0, startIndex).split(/\r?\n/).length

    const returnsOk =
      /\bOk\s*\(/.test(body) || /\bOk\s*\{/.test(body) || /return\s+Ok/.test(body)
    const delegatesToRuntime = RUNTIME_DELEGATION_KEYWORDS.some((keyword) => body.includes(keyword))

    if (returnsOk && !delegatesToRuntime) {
      violations.push({
        file: rel,
        line,
        rule: 'noop-agent-command',
        excerpt: `fn ${fnName} returns Ok without SDK/runtime delegation`,
      })
    }

    match = commandRegex.exec(content)
  }

  return violations
}

/**
 * @param {string} rel
 * @param {string[]} lines
 */
function scanFrontendOnlyAgentCapabilityState(rel, lines) {
  if (!rel.startsWith('apps/desktop/src/features/')) {
    return []
  }

  /** @type {Violation[]} */
  const violations = []

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    for (const pattern of FRONTEND_CAPABILITY_STATE_PATTERNS) {
      if (!pattern.test(line)) {
        continue
      }
      violations.push({
        file: rel,
        line: index + 1,
        rule: 'frontend-only-agent-capability-state',
        excerpt: line.trim(),
      })
    }
  }

  return violations
}

/**
 * @param {string[]} lines
 * @param {number} lineIndex
 * @param {number} [radius]
 */
export function hasAgentContextNearby(lines, lineIndex, radius = 12) {
  const start = Math.max(0, lineIndex - radius)
  const end = Math.min(lines.length - 1, lineIndex + radius)
  const windowText = lines.slice(start, end + 1).filter(skipCompilerDirectives).join('\n')
  return AGENT_CONTEXT_PATTERNS.some((pattern) => pattern.test(windowText))
}

/**
 * @param {string[]} lines
 * @param {number} lineIndex
 * @param {number} [radius]
 */
export function hasAuthorizationContextNearby(lines, lineIndex, radius = 12) {
  const start = Math.max(0, lineIndex - radius)
  const end = Math.min(lines.length - 1, lineIndex + radius)
  const windowText = lines.slice(start, end + 1).filter(skipCompilerDirectives).join('\n')
  return AUTHORIZATION_CONTEXT_PATTERNS.some((pattern) => pattern.test(windowText))
}

function skipCompilerDirectives(line) {
  const trimmed = line.trim()
  return !(trimmed.startsWith('#[cfg(') || trimmed.startsWith('cfg!('))
}

/**
 * @param {string[]} lines
 * @param {number} lineIndex
 * @param {number} [radius]
 */
function hasAvailabilityContextNearby(lines, lineIndex, radius = 8) {
  const start = Math.max(0, lineIndex - radius)
  const end = Math.min(lines.length - 1, lineIndex + radius)
  const windowText = lines.slice(start, end + 1).join('\n')
  return /subagents_available|agent_teams_available|background_agents_available|agent\s+capabilit|backgroundAgentsAvailable|agentTeamsAvailable|subagentsAvailable/i.test(
    windowText,
  )
}

/**
 * @param {string} line
 */
function isCommentOnlyLine(line) {
  const trimmed = line.trim()
  return (
    trimmed.startsWith('//') ||
    trimmed.startsWith('/*') ||
    trimmed.startsWith('*') ||
    trimmed.startsWith('*/') ||
    trimmed.startsWith('#[cfg(') ||
    trimmed.startsWith('cfg!(')
  )
}

function hasFeatureGateContextNearby(lines, lineIndex, radius = 5) {
  const start = Math.max(0, lineIndex - radius)
  const end = Math.min(lines.length - 1, lineIndex + radius)
  const windowText = lines.slice(start, end + 1).join('\n')
  return /cfg!\s*\(/.test(windowText) || /push_feature\s*\(/.test(windowText)
}

/**
 * @param {string} relPath
 */
function isExcludedProductionFile(relPath) {
  return relPath.endsWith('/noop.rs')
}

function main() {
  const result = scanAgentOrchestrationNoFakes(defaultRepoRoot)

  if (result.ok) {
    console.log('Agent orchestration no-fakes check passed.')
    return
  }

  console.error('Agent orchestration no-fakes check failed.\n')
  for (const violation of result.violations) {
    console.error(
      `- ${violation.file}:${violation.line} [${violation.rule}] ${violation.excerpt}`,
    )
  }
  process.exit(1)
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main()
}
