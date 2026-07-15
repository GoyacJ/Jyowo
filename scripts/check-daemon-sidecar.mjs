import { execSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'))
}

function repositoryFiles() {
  return execSync('git ls-files --cached --others --exclude-standard', {
    cwd: repoRoot,
    encoding: 'utf8',
    maxBuffer: 1024 * 1024,
  })
    .split(/\r?\n/)
    .filter(Boolean)
}

function externalBins(config) {
  return Array.isArray(config.bundle?.externalBin) ? config.bundle.externalBin : []
}

export function evaluateDaemonSidecarPolicy({
  packageJson,
  desktopPackageJson,
  tauriConfig,
  files,
  buildRs,
}) {
  const errors = []
  const entries = externalBins(tauriConfig)
  if (!entries.includes('binaries/jyowo-harness-daemon')) {
    errors.push('bundle.externalBin must include base path binaries/jyowo-harness-daemon')
  }
  for (const entry of entries) {
    if (/jyowo-harness-daemon-/.test(entry) || entry.endsWith('.exe')) {
      errors.push('bundle.externalBin must use the daemon base path without a target suffix')
    }
  }
  if (
    packageJson.scripts?.['build:daemon-sidecar'] !==
    'node scripts/build-daemon-sidecar.mjs'
  ) {
    errors.push('package.json must expose build:daemon-sidecar')
  }
  for (const script of ['tauri:build', 'tauri:dev', 'tauri:release']) {
    if (!desktopPackageJson.scripts?.[script]?.includes('build:daemon-sidecar')) {
      errors.push(`desktop ${script} script must build the daemon sidecar`)
    }
  }
  if (!files.includes('scripts/build-daemon-sidecar.mjs')) {
    errors.push('scripts/build-daemon-sidecar.mjs is missing')
  }
  if (!files.includes('apps/desktop/src-tauri/binaries/README.md')) {
    errors.push('apps/desktop/src-tauri/binaries/README.md is missing')
  }
  if (!buildRs.includes('jyowo-harness-daemon')) {
    errors.push('apps/desktop/src-tauri/build.rs must validate the daemon sidecar path')
  }
  return errors
}

export function collectDaemonSidecarPolicyErrors() {
  const buildRsPath = join(repoRoot, 'apps', 'desktop', 'src-tauri', 'build.rs')
  return evaluateDaemonSidecarPolicy({
    packageJson: readJson(join(repoRoot, 'package.json')),
    desktopPackageJson: readJson(join(repoRoot, 'apps', 'desktop', 'package.json')),
    tauriConfig: readJson(join(repoRoot, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json')),
    files: repositoryFiles(),
    buildRs: existsSync(buildRsPath) ? readFileSync(buildRsPath, 'utf8') : '',
  })
}

export function main() {
  const errors = collectDaemonSidecarPolicyErrors()
  if (errors.length > 0) {
    console.error('Task daemon sidecar check failed.')
    for (const error of errors) console.error(`- ${error}`)
    process.exit(1)
  }
  console.log('Task daemon sidecar check passed.')
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main()
