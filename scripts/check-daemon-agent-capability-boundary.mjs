#!/usr/bin/env node

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, extname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot =
  process.env.JYOWO_DAEMON_AGENT_CAPABILITY_BOUNDARY_ROOT ??
  dirname(dirname(fileURLToPath(import.meta.url)))

const scopedPaths = [
  'apps/desktop/src-tauri/src',
  'apps/desktop/src/features/settings',
  'apps/desktop/src/shared/tauri/commands.ts',
  'crates/jyowo-harness-sdk/src',
]

const forbidden = [
  'AgentCapabilityResolutionContext',
  'AgentCapabilityEnvironment',
  'AgentCapabilityResolver',
  'AgentRuntimeStore',
  'EngineTeamMemberRunner',
  'active_run_teams',
  'resolve_agent_capabilities_with_context',
  'default_agent_capability_environment',
  'background_agents_compiled',
  'not_compiled',
  'notCompiled',
  'runtime_store_unavailable',
  'runtimeStoreUnavailable',
  'permission_runtime_unavailable',
  'permissionRuntimeUnavailable',
  'profile_registry_unavailable',
  'background_supervisor_unavailable',
  'backgroundSupervisorUnavailable',
  'workspace_isolation_unavailable',
  'workspaceIsolationUnavailable',
  'invalid_profile',
  'invalidAgentProfiles',
]

const sourceExtensions = new Set(['.rs', '.ts', '.tsx'])
const violations = []

for (const scopedPath of scopedPaths) {
  const absolutePath = join(repoRoot, scopedPath)
  for (const file of collectFiles(absolutePath)) {
    const content = readFileSync(file, 'utf8')
    const lines = content.split(/\r?\n/)
    for (let index = 0; index < lines.length; index += 1) {
      for (const token of forbidden) {
        if (lines[index].includes(token)) {
          violations.push({
            file: relative(repoRoot, file).replaceAll('\\', '/'),
            line: index + 1,
            token,
            excerpt: lines[index].trim(),
          })
        }
      }
    }
  }
}

if (violations.length > 0) {
  console.error('Legacy agent capability boundary violations found:')
  for (const violation of violations) {
    console.error(
      `  ${violation.file}:${violation.line} — ${violation.token}\n    ${violation.excerpt}`,
    )
  }
  process.exit(1)
}

console.log('Daemon agent capability boundary clean.')

function collectFiles(path) {
  if (!statSync(path).isDirectory()) {
    return sourceExtensions.has(extname(path)) ? [path] : []
  }
  const files = []
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    if (entry.name === 'target' || entry.name === 'node_modules') continue
    const child = join(path, entry.name)
    if (entry.isDirectory()) {
      files.push(...collectFiles(child))
    } else if (sourceExtensions.has(extname(entry.name))) {
      files.push(child)
    }
  }
  return files
}
