#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, extname, join, relative } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

export const legacyInvokeNames = [
  'archive_background_agent',
  'cancel_background_agent',
  'create_attachment_from_path',
  'create_conversation',
  'create_default_conversation',
  'create_project_conversation',
  'delete_automation',
  'delete_background_agent',
  'delete_conversation',
  'delete_project_conversation',
  'export_conversation_evidence',
  'export_support_bundle',
  'get_artifact_media_preview',
  'get_artifact_revision_content',
  'get_attachment_media_preview',
  'get_background_agent',
  'get_conversation',
  'get_conversation_command_output',
  'get_conversation_diff_patch',
  'get_conversation_inspector_item',
  'get_replay_timeline',
  'list_activity',
  'list_artifacts',
  'list_automation_runs',
  'list_automations',
  'list_background_agents',
  'list_conversations',
  'list_eval_cases',
  'list_project_conversation_groups',
  'pause_background_agent',
  'resume_background_agent',
  'run_automation_now',
  'run_eval_case',
  'save_automation',
  'send_background_agent_input',
  'set_automation_enabled',
]

const removedPaths = [
  'apps/desktop/src/routes/evals.tsx',
  'apps/desktop/src/routes/evals.lazy.tsx',
  'apps/desktop/src/features/evals',
  'apps/desktop/src/features/artifacts',
  'apps/desktop/src/features/conversation/WelcomeWorkspace.tsx',
  'apps/desktop/src/features/conversation/WelcomeWorkspace.test.tsx',
  'apps/desktop/src/features/conversation/evidence',
  'apps/desktop/src/features/conversation/timeline',
  'apps/desktop/src/features/workbench/WorkbenchInspector.tsx',
  'apps/desktop/src/features/workbench/WorkbenchInspector.test.tsx',
  'apps/desktop/src/features/workbench/WorkbenchInspector.test-support.tsx',
  'apps/desktop/src/features/workbench/WorkbenchInspector.artifact-media.test.tsx',
  'apps/desktop/src/features/workbench/WorkbenchInspector.artifacts.test.tsx',
  'apps/desktop/src/features/workbench/WorkbenchInspector.stories.tsx',
  'apps/desktop/src/features/workbench/artifacts',
]

export function findLegacyInvokeViolations(source) {
  return legacyInvokeNames.filter(name =>
    new RegExp(`(?:invoke|command\\s*=)[^\\n]*['\"]${name}['\"]`).test(source),
  )
}

export function checkLegacyConversationSurface(repoRoot) {
  const violations = []
  for (const path of removedPaths) {
    const absolutePath = join(repoRoot, path)
    if (
      existsSync(absolutePath) &&
      (!statSync(absolutePath).isDirectory() || readdirSync(absolutePath).length > 0)
    ) {
      violations.push(`removed path remains: ${path}`)
    }
  }

  const commandsPath = join(repoRoot, 'apps/desktop/src/shared/tauri/commands.ts')
  const source = readFileSync(commandsPath, 'utf8')
  for (const name of findLegacyInvokeViolations(source)) {
    violations.push(`legacy Tauri invoke remains: ${name}`)
  }

  const productionRoot = join(repoRoot, 'apps/desktop/src')
  const removedImportFragments = [
    '/features/evals/',
    '/features/artifacts/',
    '/conversation/evidence/',
    '/conversation/timeline/',
    '/workbench/artifacts/',
    '/workbench/WorkbenchInspector',
    '/conversation/WelcomeWorkspace',
  ]
  for (const file of collectSourceFiles(productionRoot)) {
    const content = readFileSync(file, 'utf8')
    for (const fragment of removedImportFragments) {
      if (content.includes(fragment)) {
        violations.push(
          `legacy import remains: ${relative(repoRoot, file).replaceAll('\\', '/')} (${fragment})`,
        )
      }
    }
  }
  return violations.sort()
}

function collectSourceFiles(path) {
  const files = []
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    if (entry.isDirectory() && entry.name === 'testing') continue
    const child = join(path, entry.name)
    if (entry.isDirectory()) files.push(...collectSourceFiles(child))
    else if (
      new Set(['.ts', '.tsx']).has(extname(entry.name)) &&
      !/\.(?:test|stories)\.[^.]+$/.test(entry.name)
    ) {
      files.push(child)
    }
  }
  return files
}

const isMain = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href
if (isMain) {
  const repoRoot =
    process.env.JYOWO_LEGACY_SURFACE_ROOT ?? dirname(dirname(fileURLToPath(import.meta.url)))
  const violations = checkLegacyConversationSurface(repoRoot)
  if (violations.length > 0) {
    console.error('Legacy conversation surface violations found:')
    for (const violation of violations) console.error(`  ${violation}`)
    process.exit(1)
  }
  console.log('Legacy conversation surface removed.')
}
