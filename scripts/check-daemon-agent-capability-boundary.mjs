#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, extname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const defaultRepoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

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

const daemonCapabilityAssemblyCalls = [
  'with_mcp_config',
  'with_plugin_registry',
  'with_provider_capability_routes',
  'with_skill_config_snapshot',
  'with_skill_loader',
]

const sourceExtensions = new Set(['.rs', '.ts', '.tsx'])

export function scanDaemonAgentCapabilityBoundary(repoRoot) {
  const violations = []

  for (const scopedPath of scopedPaths) {
    for (const file of collectFiles(join(repoRoot, scopedPath))) {
      const content = productionSource(file, readFileSync(file, 'utf8'))
      const lines = content.split(/\r?\n/)
      for (let index = 0; index < lines.length; index += 1) {
        for (const token of forbidden) {
          if (lines[index].includes(token)) {
            violations.push(violation(repoRoot, file, index + 1, 'legacy-capability', token, lines[index]))
          }
        }
      }
    }
  }

  const tauriRoot = join(repoRoot, 'apps/desktop/src-tauri/src')
  for (const file of collectFiles(tauriRoot)) {
    const content = productionSource(file, readFileSync(file, 'utf8'))
    const taskAssemblyPattern = /\b(?:jyowo_harness_sdk::)?Harness::builder\s*\(|\b(?:TaskStore::open|RunCoordinator::new|SdkRunFactory::new)\s*\(/g
    for (const match of content.matchAll(taskAssemblyPattern)) {
      const line = content.slice(0, match.index).split(/\r?\n/).length
      violations.push(
        violation(repoRoot, file, line, 'tauri-task-runtime-assembly', match[0], match[0]),
      )
    }
  }

  const daemonRoot = join(repoRoot, 'crates/jyowo-harness-daemon/src')
  const daemonSource = collectFiles(daemonRoot)
    .map((file) => productionSource(file, readFileSync(file, 'utf8')))
    .join('\n')
  for (const token of daemonCapabilityAssemblyCalls) {
    if (!daemonSource.includes(`.${token}(`)) {
      violations.push({
        file: 'crates/jyowo-harness-daemon/src',
        line: 1,
        rule: 'daemon-capability-assembly-missing',
        token,
        excerpt: `missing .${token}(...)`,
      })
    }
  }

  return { ok: violations.length === 0, violations }
}

function violation(repoRoot, file, line, rule, token, sourceLine) {
  return {
    file: relative(repoRoot, file).replaceAll('\\', '/'),
    line,
    rule,
    token,
    excerpt: sourceLine.trim(),
  }
}

function collectFiles(path) {
  if (!existsSync(path)) return []
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

function productionSource(file, content) {
  if (!file.endsWith('.rs')) return content
  const testModule = content.search(/^\s*#\[cfg\(test\)\]\s*\n\s*mod\s+/m)
  return testModule === -1 ? content : content.slice(0, testModule)
}

function printResult(result) {
  if (result.ok) {
    console.log('Daemon agent capability boundary clean.')
    return
  }
  console.error('Daemon agent capability boundary violations found:')
  for (const item of result.violations) {
    console.error(`  ${item.file}:${item.line} — ${item.rule}: ${item.token}\n    ${item.excerpt}`)
  }
  process.exitCode = 1
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  const repoRoot = process.env.JYOWO_DAEMON_AGENT_CAPABILITY_BOUNDARY_ROOT ?? defaultRepoRoot
  printResult(scanDaemonAgentCapabilityBoundary(repoRoot))
}
