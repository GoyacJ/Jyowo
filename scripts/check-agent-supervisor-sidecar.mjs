import { execSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'))
}

function trackedFiles() {
  return execSync('git ls-files', { cwd: repoRoot, encoding: 'utf8', maxBuffer: 1024 * 1024 })
    .split(/\r?\n/)
    .filter(Boolean)
}

function externalBins(tauriConfig) {
  const value = tauriConfig.bundle?.externalBin
  return Array.isArray(value) ? value : []
}

function hasSupervisorExternalBin(tauriConfig) {
  return externalBins(tauriConfig).some((entry) => entry.includes('jyowo-agent-supervisor'))
}

export function evaluateAgentSupervisorSidecarPolicy({
  packageJson,
  tauriConfig,
  files,
  buildRs,
}) {
  const errors = []
  const hasSidecar = hasSupervisorExternalBin(tauriConfig)
  const hasTrackedSupervisorBinary = files.some((file) =>
    /^apps\/desktop\/src-tauri\/binaries\/jyowo-agent-supervisor/.test(file),
  )

  if (!hasSidecar) {
    if (hasTrackedSupervisorBinary) {
      errors.push('tracked supervisor sidecar binary exists without bundle.externalBin policy')
    }
    return errors
  }

  const scripts = packageJson.scripts ?? {}
  if (scripts['build:agent-supervisor-sidecar'] !== 'node scripts/build-agent-supervisor-sidecar.mjs') {
    errors.push('package.json must expose build:agent-supervisor-sidecar')
  }

  if (!files.includes('scripts/build-agent-supervisor-sidecar.mjs')) {
    errors.push('scripts/build-agent-supervisor-sidecar.mjs is missing')
  }

  if (!files.includes('apps/desktop/src-tauri/binaries/README.md')) {
    errors.push('apps/desktop/src-tauri/binaries/README.md is missing')
  }

  for (const entry of externalBins(tauriConfig)) {
    if (/jyowo-agent-supervisor-/.test(entry) || entry.endsWith('.exe')) {
      errors.push('bundle.externalBin must use base path binaries/jyowo-agent-supervisor')
    }
  }

  if (!buildRs.includes('jyowo-agent-supervisor')) {
    errors.push('apps/desktop/src-tauri/build.rs must validate the supervisor sidecar path')
  }

  return errors
}

export function collectAgentSupervisorSidecarPolicyErrors() {
  const packageJson = readJson(join(repoRoot, 'package.json'))
  const tauriConfig = readJson(join(repoRoot, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json'))
  const buildRsPath = join(repoRoot, 'apps', 'desktop', 'src-tauri', 'build.rs')

  return evaluateAgentSupervisorSidecarPolicy({
    packageJson,
    tauriConfig,
    files: trackedFiles(),
    buildRs: existsSync(buildRsPath) ? readFileSync(buildRsPath, 'utf8') : '',
  })
}

export function main() {
  const errors = collectAgentSupervisorSidecarPolicyErrors()

  if (errors.length > 0) {
    console.error('Agent supervisor sidecar check failed.')
    for (const error of errors) {
      console.error(`- ${error}`)
    }
    process.exit(1)
  }

  console.log('Agent supervisor sidecar check passed.')
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main()
}
