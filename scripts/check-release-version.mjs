import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'))
}

function readCargoWorkspaceVersion(root) {
  const cargoToml = readFileSync(join(root, 'Cargo.toml'), 'utf8')
  const workspacePackageMatch = cargoToml.match(
    /^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
  )

  if (!workspacePackageMatch) {
    throw new Error('Cargo.toml is missing [workspace.package] version')
  }

  return workspacePackageMatch[1]
}

export function checkReleaseVersion(root = repoRoot, env = process.env) {
  const versions = [
    {
      label: 'root package.json',
      version: readJson(join(root, 'package.json')).version,
    },
    {
      label: 'desktop package.json',
      version: readJson(join(root, 'apps', 'desktop', 'package.json')).version,
    },
    {
      label: 'Cargo workspace',
      version: readCargoWorkspaceVersion(root),
    },
    {
      label: 'tauri.conf.json',
      version: readJson(join(root, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json')).version,
    },
  ]
  const expected = versions[0]?.version
  const mismatches = versions.filter((entry) => entry.version !== expected)

  if (mismatches.length > 0) {
    return {
      ok: false,
      message: [
        'Project versions must match before release:',
        ...versions.map((entry) => `- ${entry.label}: ${entry.version}`),
      ].join('\n'),
    }
  }

  if (env.GITHUB_REF_TYPE === 'tag') {
    const tagVersion = env.GITHUB_REF_NAME?.match(/^v(\d+\.\d+\.\d+)$/)?.[1]

    if (tagVersion && tagVersion !== expected) {
      return {
        ok: false,
        message: `Release tag v${tagVersion} does not match project version ${expected}.`,
      }
    }
  }

  return { ok: true, version: expected }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const result = checkReleaseVersion()

  if (!result.ok) {
    console.error(result.message)
    process.exit(1)
  }

  console.log(`Release version check passed: ${result.version}`)
}
