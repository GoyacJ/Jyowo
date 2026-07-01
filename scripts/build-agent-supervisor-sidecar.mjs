import { copyFileSync, mkdirSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  cargoBuiltBinaryPath,
  parseRustHostTriple,
  sidecarOutputPath,
} from './agent-supervisor-sidecar-utils.mjs'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

function activeTargetTriple() {
  if (process.env.TARGET) {
    return process.env.TARGET
  }

  const rustc = spawnSync('rustc', ['-vV'], {
    cwd: repoRoot,
    encoding: 'utf8',
  })
  if (rustc.error) {
    throw rustc.error
  }
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
  [
    'build',
    '-p',
    'jyowo-desktop-shell',
    '--bin',
    'jyowo-agent-supervisor',
    '--target',
    target,
  ],
  {
    cwd: repoRoot,
    encoding: 'utf8',
    stdio: 'inherit',
    env: {
      ...process.env,
      JYOWO_BUILDING_AGENT_SUPERVISOR_SIDECAR: '1',
    },
  },
)

if (cargo.error) {
  console.error(cargo.error.message)
  process.exit(1)
}
if (cargo.status !== 0) {
  process.exit(cargo.status ?? 1)
}

const source = cargoBuiltBinaryPath({ repoRoot, target })
const destination = sidecarOutputPath({ repoRoot, target })
mkdirSync(dirname(destination), { recursive: true })
copyFileSync(source, destination)

console.log(`Agent supervisor sidecar copied to ${destination}`)
