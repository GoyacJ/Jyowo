import { spawnSync } from 'node:child_process'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { upstreamHeldRustDependencies } from './rust-deps-policy.mjs'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

const heldByName = new Map(upstreamHeldRustDependencies.map((dependency) => [dependency.name, dependency]))

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
const unchangedDependencies = [
  ...output.matchAll(/^\s*Unchanged\s+([A-Za-z0-9_-]+)\s+v([^\s]+)\s+\(available:\s+v([^)]+)\)/gm),
].map((match) => ({
  name: match[1],
  current: match[2],
  available: match[3],
}))

const unexpected = unchangedDependencies.filter((dependency) => !heldByName.has(dependency.name))
const changed = unchangedDependencies.filter((dependency) => {
  const expected = heldByName.get(dependency.name)

  return expected && (expected.current !== dependency.current || expected.available !== dependency.available)
})
const missing = upstreamHeldRustDependencies.filter(
  (dependency) => !unchangedDependencies.some((unchanged) => unchanged.name === dependency.name),
)

if (updatePlanEntries.length > 0 || unexpected.length > 0 || changed.length > 0 || missing.length > 0) {
  console.error('Rust dependency audit failed.')

  if (updatePlanEntries.length > 0) {
    console.error('\nCargo update dry-run produced a lockfile update plan:')
    for (const entry of updatePlanEntries) {
      console.error(`- ${entry.action} ${entry.detail}`)
    }
  }

  if (unexpected.length > 0) {
    console.error('\nUnclassified outdated Rust dependencies:')
    for (const dependency of unexpected) {
      console.error(`- ${dependency.name} ${dependency.current} -> ${dependency.available}`)
    }
  }

  if (changed.length > 0) {
    console.error('\nDocumented upstream-held dependencies changed:')
    for (const dependency of changed) {
      const expected = heldByName.get(dependency.name)
      console.error(
        `- ${dependency.name}: documented ${expected.current} -> ${expected.available}, actual ${dependency.current} -> ${dependency.available}`,
      )
    }
  }

  if (missing.length > 0) {
    console.error('\nStale upstream-held dependency allowlist entries:')
    for (const dependency of missing) {
      console.error(`- ${dependency.name} held by ${dependency.owner}`)
    }
  }

  process.exit(1)
}

console.log(
  `Rust dependency audit passed: ${upstreamHeldRustDependencies.length} upstream-held transitive dependencies documented.`,
)
