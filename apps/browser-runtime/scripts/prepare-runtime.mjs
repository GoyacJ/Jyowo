import { spawnSync } from 'node:child_process'
import {
  chmodSync,
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const expectedNodeVersion = '24.12.0'
const chromeBuildId = '150.0.7871.124'
const packageRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const repoRoot = dirname(dirname(packageRoot))
const resourceRoot = join(repoRoot, 'apps', 'desktop', 'src-tauri', 'browser-runtime')
const stagingRoot = join(repoRoot, 'target', 'jyowo-browser-runtime')
const pnpmCli = process.env.npm_execpath

if (process.versions.node !== expectedNodeVersion) {
  throw new Error(
    `browser runtime packaging requires Node.js ${expectedNodeVersion}; received ${process.versions.node}`,
  )
}
if (!pnpmCli) {
  throw new Error('browser runtime packaging requires npm_execpath from pnpm')
}

rmSync(stagingRoot, { force: true, recursive: true })
mkdirSync(dirname(stagingRoot), { recursive: true })
run(
  process.execPath,
  [
    pnpmCli,
    '--offline',
    '--config.node-linker=hoisted',
    '--config.inject-workspace-packages=true',
    '--filter',
    '@jyowo/browser-runtime',
    'deploy',
    '--prod',
    stagingRoot,
  ],
  { cwd: repoRoot },
)
const deployedNodeModules = join(stagingRoot, 'node_modules')
for (const entry of ['.bin', '.modules.yaml', '.pnpm', '.pnpm-workspace-state-v1.json']) {
  rmSync(join(deployedNodeModules, entry), { force: true, recursive: true })
}
for (const entry of ['pnpm-lock.yaml', 'pnpm-workspace.yaml']) {
  rmSync(join(stagingRoot, entry), { force: true })
}

const nodeRelativePath = process.platform === 'win32' ? join('node', 'node.exe') : join('node', 'node')
const packagedNode = join(stagingRoot, nodeRelativePath)
mkdirSync(dirname(packagedNode), { recursive: true })
copyFileSync(process.execPath, packagedNode)
if (process.platform !== 'win32') chmodSync(packagedNode, 0o755)
copyNodeLicense(stagingRoot)

const chromeRoot = join(stagingRoot, 'chrome')
const browser = run(
  process.execPath,
  [
    pnpmCli,
    'exec',
    'browsers',
    'install',
    `chrome@${chromeBuildId}`,
    '--path',
    chromeRoot,
    '--format={{path}}',
  ],
  { cwd: packageRoot, capture: true },
).trim()
const chromeExecutable = browser.split(/\r?\n/).at(-1)
if (!chromeExecutable || !existsSync(chromeExecutable)) {
  throw new Error(`Chrome for Testing executable was not created: ${browser}`)
}

writeFileSync(
  join(stagingRoot, 'runtime-manifest.json'),
  `${JSON.stringify(
    {
      schemaVersion: 1,
      nodeVersion: expectedNodeVersion,
      chromeBuildId,
      nodePath: portableRelative(stagingRoot, packagedNode),
      scriptPath: 'src/runtime.mjs',
      chromeExecutable: portableRelative(stagingRoot, chromeExecutable),
    },
    null,
    2,
  )}\n`,
)

mkdirSync(resourceRoot, { recursive: true })
for (const entry of readdirSync(resourceRoot)) {
  if (entry !== '.gitkeep') rmSync(join(resourceRoot, entry), { force: true, recursive: true })
}
for (const entry of readdirSync(stagingRoot)) {
  renameOrCopy(join(stagingRoot, entry), join(resourceRoot, entry))
}
console.log(`Browser runtime prepared at ${resourceRoot}`)

function run(command, args, { cwd, capture = false }) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    stdio: capture ? ['ignore', 'pipe', 'inherit'] : 'inherit',
  })
  if (result.error) throw result.error
  if (result.status !== 0) process.exit(result.status ?? 1)
  return result.stdout ?? ''
}

function portableRelative(root, path) {
  return relative(root, resolve(path)).split(sep).join('/')
}

function copyNodeLicense(target) {
  const candidates = [
    join(dirname(process.execPath), '..', 'LICENSE'),
    join(dirname(process.execPath), '..', 'LICENSE.md'),
    join(dirname(process.execPath), 'LICENSE'),
  ]
  const source = candidates.find(existsSync)
  if (source) copyFileSync(source, join(target, 'node', 'LICENSE'))
}

function renameOrCopy(source, destination) {
  try {
    renameSync(source, destination)
  } catch {
    cpSync(source, destination, { recursive: true })
    rmSync(source, { force: true, recursive: true })
  }
}
