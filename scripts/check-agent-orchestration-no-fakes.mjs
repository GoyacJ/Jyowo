import { execSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

const exactProductionPaths = new Set([
  'apps/desktop/src-tauri/src/commands.rs',
  'apps/desktop/src/features/settings/ExecutionSettings.tsx',
  'apps/desktop/src/features/conversation/Composer.tsx',
  'apps/desktop/src/features/conversation/AgentActivitySegment.tsx',
])

const productionPrefixes = [
  'apps/desktop/src/features/background-agents/',
  'crates/jyowo-harness-agent-runtime/',
  'crates/jyowo-harness-subagent/',
  'crates/jyowo-harness-team/',
]

const excludedPatterns = [
  /^docs\//,
  /(^|\/)tests\//,
  /\.test\.[cm]?[jt]sx?$/,
  /^target\//,
  /(^|\/)node_modules\//,
  /(^|\/)dist\//,
  /(^|\/)storybook-static\//,
]

const agentContextPattern =
  /\b(?:subagent|agent team|agent teams|background agent|background agents|agent runtime|agent orchestration)\b/i

const placeholderPattern =
  /\b(?:TODO|FIXME|coming soon|experimental|not implemented|unimplemented|placeholder|stub)\b/i

const fakeAgentPattern =
  /\b(?:fake|mock)[A-Za-z0-9_-]*(?:subagent|agent|background)[A-Za-z0-9_-]*|\b(?:fake|mock)\b[\s\S]{0,120}\b(?:subagent|agent team|background agent|agent runtime|agent orchestration|agent runner)\b|\b(?:subagent|agent team|background agent|agent runtime|agent orchestration|agent runner)\b[\s\S]{0,120}\b(?:fake|mock)\b/i

const fixedSuccessPattern =
  /\b(?:async\s+fn|fn)\s+(?:start|create|run|spawn|list|get|set|save|delete|cancel|resume|launch)_[A-Za-z0-9_]*(?:agent|subagent|background)[A-Za-z0-9_]*[\s\S]{0,500}\b(?:Ok\(\s*\(\s*\)|status:\s*["']success["']|available:\s*true)/i

export function isScopedProductionPath(path) {
  if (excludedPatterns.some((pattern) => pattern.test(path))) {
    return false
  }

  return exactProductionPaths.has(path) || productionPrefixes.some((prefix) => path.startsWith(prefix))
}

function nearbyAgentContext(content, index) {
  const start = Math.max(index - 220, 0)
  const end = Math.min(index + 220, content.length)
  return agentContextPattern.test(content.slice(start, end))
}

function lineNumberAt(content, index) {
  return content.slice(0, index).split(/\r?\n/).length
}

export function scanAgentOrchestrationContent(content, path = '<memory>') {
  const failures = []

  for (const match of content.matchAll(new RegExp(placeholderPattern, 'gi'))) {
    if (nearbyAgentContext(content, match.index ?? 0)) {
      failures.push({
        path,
        line: lineNumberAt(content, match.index ?? 0),
        reason: `agent-context placeholder marker: ${match[0]}`,
      })
    }
  }

  const fakeMatch = fakeAgentPattern.exec(content)
  if (fakeMatch) {
    failures.push({
      path,
      line: lineNumberAt(content, fakeMatch.index),
      reason: 'fake/mock agent orchestration production surface',
    })
  }

  const fixedSuccessMatch = fixedSuccessPattern.exec(content)
  if (fixedSuccessMatch) {
    failures.push({
      path,
      line: lineNumberAt(content, fixedSuccessMatch.index),
      reason: 'agent command appears to return fixed success without runtime context',
    })
  }

  return failures
}

function trackedFiles() {
  return execSync('git ls-files', { cwd: repoRoot, encoding: 'utf8', maxBuffer: 1024 * 1024 })
    .split(/\r?\n/)
    .filter(Boolean)
}

export function collectAgentOrchestrationFailures(files = trackedFiles()) {
  return files
    .filter(isScopedProductionPath)
    .filter((path) => existsSync(path.startsWith('/') ? path : `${repoRoot}/${path}`))
    .flatMap((path) => {
      const absolutePath = path.startsWith('/') ? path : `${repoRoot}/${path}`
      const displayPath = path.startsWith('/') ? relative(repoRoot, path) : path
      return scanAgentOrchestrationContent(readFileSync(absolutePath, 'utf8'), displayPath)
    })
}

export function main() {
  const failures = collectAgentOrchestrationFailures()

  if (failures.length > 0) {
    console.error('Agent orchestration no-fakes check failed.')
    for (const failure of failures) {
      console.error(`- ${failure.path}:${failure.line}: ${failure.reason}`)
    }
    process.exit(1)
  }

  console.log('Agent orchestration no-fakes check passed.')
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main()
}
