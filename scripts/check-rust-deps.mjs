import { spawnSync } from 'node:child_process'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

const cargoUpdate = spawnSync('cargo', ['update', '--dry-run', '--workspace', '--verbose'], {
  cwd: repoRoot,
  encoding: 'utf8',
})

if (cargoUpdate.error) {
  console.error(cargoUpdate.error.message)
  process.exit(1)
}

if (cargoUpdate.status !== 0) {
  process.stdout.write(cargoUpdate.stdout)
  process.stderr.write(cargoUpdate.stderr)
  process.exit(cargoUpdate.status ?? 1)
}

const output = `${cargoUpdate.stdout}\n${cargoUpdate.stderr}`
const updatePlanEntries = [
  ...output.matchAll(/^\s+(Adding|Downgrading|Removing|Updating)\s+(.+)$/gm),
].map((match) => ({
  action: match[1],
  detail: match[2],
}))
if (updatePlanEntries.length > 0) {
  console.error('Rust dependency audit failed.')
  console.error('\nCargo update dry-run produced a lockfile update plan:')
  for (const entry of updatePlanEntries) {
    console.error(`- ${entry.action} ${entry.detail}`)
  }

  process.exit(1)
}

console.log('Rust dependency audit passed: Cargo.lock has no resolvable updates.')
