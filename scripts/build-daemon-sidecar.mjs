import { copyFileSync, mkdirSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  cargoBuiltBinaryPath,
  parseRustHostTriple,
  sidecarOutputPath,
} from './daemon-sidecar-utils.mjs'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

function activeTargetTriple() {
  if (process.env.TARGET) return process.env.TARGET
  const rustc = spawnSync('rustc', ['-vV'], { cwd: repoRoot, encoding: 'utf8' })
  if (rustc.error) throw rustc.error
  if (rustc.status !== 0) {
    process.stdout.write(rustc.stdout)
    process.stderr.write(rustc.stderr)
    process.exit(rustc.status ?? 1)
  }
  return parseRustHostTriple(rustc.stdout)
}

const target = activeTargetTriple()
const cargo = spawnSync(
  'cargo',
  ['build', '-p', 'jyowo-harness-daemon', '--bin', 'jyowo-harness-daemon', '--target', target],
  {
    cwd: repoRoot,
    encoding: 'utf8',
    stdio: 'inherit',
    env: { ...process.env, JYOWO_BUILDING_DAEMON_SIDECAR: '1' },
  },
)
if (cargo.error) {
  console.error(cargo.error.message)
  process.exit(1)
}
if (cargo.status !== 0) process.exit(cargo.status ?? 1)

const targetDir = resolve(repoRoot, process.env.CARGO_TARGET_DIR ?? 'target')
const source = cargoBuiltBinaryPath({ repoRoot, target, targetDir })
const destination = sidecarOutputPath({ repoRoot, target })
mkdirSync(dirname(destination), { recursive: true })
copyFileSync(source, destination)
console.log(`Task daemon sidecar copied to ${destination}`)
