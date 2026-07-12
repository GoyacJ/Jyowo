#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

export function extractTauriInvokes(source) {
  const names = new Set()
  for (const match of source.matchAll(/const\s+command\s*=\s*['"]([a-z0-9_]+)['"]/g)) {
    names.add(match[1])
  }
  for (const match of source.matchAll(/\.invoke\(\s*['"]([a-z0-9_]+)['"]/g)) {
    names.add(match[1])
  }
  return [...names].sort()
}

export function extractRegisteredCommands(source) {
  const handler = source.match(/generate_handler!\s*\[([\s\S]*?)\]\s*\)/)?.[1] ?? ''
  return [...handler.matchAll(/commands::([a-z0-9_]+)/g)].map(match => match[1]).sort()
}

export function compareTauriCommandRegistration({ clientSources, registrationSource }) {
  const invoked = new Set(clientSources.flatMap(extractTauriInvokes))
  const registered = new Set(extractRegisteredCommands(registrationSource))
  return {
    missing: [...invoked].filter(name => !registered.has(name)).sort(),
    orphaned: [...registered].filter(name => !invoked.has(name)).sort(),
  }
}

export function checkTauriCommandRegistration(repoRoot) {
  return compareTauriCommandRegistration({
    clientSources: [
      readFileSync(join(repoRoot, 'apps/desktop/src/shared/tauri/commands.ts'), 'utf8'),
      readFileSync(join(repoRoot, 'apps/desktop/src/shared/daemon/client.ts'), 'utf8'),
    ],
    registrationSource: readFileSync(join(repoRoot, 'apps/desktop/src-tauri/src/lib.rs'), 'utf8'),
  })
}

const isMain = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href
if (isMain) {
  const repoRoot =
    process.env.JYOWO_TAURI_COMMAND_REGISTRATION_ROOT ??
    dirname(dirname(fileURLToPath(import.meta.url)))
  const result = checkTauriCommandRegistration(repoRoot)
  if (result.missing.length > 0 || result.orphaned.length > 0) {
    if (result.missing.length > 0) {
      console.error(`Missing Tauri registrations: ${result.missing.join(', ')}`)
    }
    if (result.orphaned.length > 0) {
      console.error(`Orphaned Tauri registrations: ${result.orphaned.join(', ')}`)
    }
    process.exit(1)
  }
  console.log('Tauri command registration is consistent.')
}
