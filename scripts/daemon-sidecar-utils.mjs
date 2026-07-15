import { join } from 'node:path'

export const DAEMON_BIN_NAME = 'jyowo-harness-daemon'

export function sidecarFilenameForTarget(target) {
  if (!target || typeof target !== 'string') {
    throw new Error('target triple is required')
  }
  const suffix = target.includes('windows') ? '.exe' : ''
  return `${DAEMON_BIN_NAME}-${target}${suffix}`
}

export function sidecarOutputPath({ repoRoot, target }) {
  return join(
    repoRoot,
    'apps',
    'desktop',
    'src-tauri',
    'binaries',
    sidecarFilenameForTarget(target),
  )
}

export function cargoBuiltBinaryPath({ repoRoot, target, profile = 'debug', targetDir }) {
  const suffix = target.includes('windows') ? '.exe' : ''
  return join(targetDir ?? join(repoRoot, 'target'), target, profile, `${DAEMON_BIN_NAME}${suffix}`)
}

export function parseRustHostTriple(output) {
  const hostLine = output.split(/\r?\n/).find((line) => line.startsWith('host: '))
  if (!hostLine) {
    throw new Error('rustc -vV output did not include a host triple')
  }
  return hostLine.slice('host: '.length).trim()
}
