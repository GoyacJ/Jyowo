import { pathToFileURL } from 'node:url'

const repository = 'GoyacJ/Jyowo'
const platformFamilies = ['darwin', 'linux', 'windows']

export function versionFromTag(tag) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag)) {
    throw new Error(`Release tag must be semantic version vMAJOR.MINOR.PATCH; received ${tag}`)
  }

  return tag.slice(1)
}

export function validateReleaseManifest(manifest, tag) {
  const version = versionFromTag(tag)
  if (!manifest || typeof manifest !== 'object' || Array.isArray(manifest)) {
    throw new Error('latest.json must contain a JSON object')
  }
  if (manifest.version !== version) {
    throw new Error(`latest.json version ${String(manifest.version)} does not match ${version}`)
  }
  if (!manifest.platforms || typeof manifest.platforms !== 'object') {
    throw new Error('latest.json is missing platforms')
  }

  const entries = Object.entries(manifest.platforms)
  for (const family of platformFamilies) {
    if (!entries.some(([platform]) => platform.startsWith(`${family}-`))) {
      throw new Error(`latest.json is missing a ${family} updater artifact`)
    }
  }

  const releasePrefix = `https://github.com/${repository}/releases/download/${tag}/`
  for (const [platform, artifact] of entries) {
    if (!artifact || typeof artifact !== 'object') {
      throw new Error(`Updater artifact ${platform} must be an object`)
    }
    if (typeof artifact.signature !== 'string' || artifact.signature.trim() === '') {
      throw new Error(`Updater artifact ${platform} is missing its signature`)
    }
    if (typeof artifact.url !== 'string' || !artifact.url.startsWith(releasePrefix)) {
      throw new Error(`Updater artifact ${platform} has an unexpected URL`)
    }
  }

  return entries.map(([platform, artifact]) => ({ platform, url: artifact.url }))
}

async function fetchJson(url, init) {
  const response = await fetch(url, init)
  if (!response.ok) {
    throw new Error(`${url} returned HTTP ${response.status}`)
  }
  return response.json()
}

async function verifyArtifact(url, token) {
  const response = await fetch(url, {
    headers: {
      Accept: 'application/octet-stream',
      Authorization: `Bearer ${token}`,
      Range: 'bytes=0-0',
      'X-GitHub-Api-Version': '2022-11-28',
    },
    redirect: 'follow',
  })
  if (!response.ok) {
    throw new Error(`${url} returned HTTP ${response.status}`)
  }
  await response.body?.cancel()
}

export async function verifyPublishedRelease(tag, token) {
  const headers = {
    Accept: 'application/vnd.github+json',
    Authorization: `Bearer ${token}`,
    'X-GitHub-Api-Version': '2022-11-28',
  }
  const release = await fetchJson(
    `https://api.github.com/repos/${repository}/releases/tags/${encodeURIComponent(tag)}`,
    { headers },
  )
  if (release.draft || release.prerelease) {
    throw new Error(`Release ${tag} must be published and stable`)
  }

  const manifestUrl = `https://github.com/${repository}/releases/download/${tag}/latest.json`
  const manifest = await fetchJson(manifestUrl, { headers })
  const artifacts = validateReleaseManifest(manifest, tag)
  await Promise.all(artifacts.map((artifact) => verifyArtifact(artifact.url, token)))

  return { artifacts: artifacts.length, version: manifest.version }
}

async function main() {
  const tag = process.argv[2]
  const token = process.env.GITHUB_TOKEN
  if (!tag) throw new Error('Release tag argument is required')
  if (!token) throw new Error('GITHUB_TOKEN is required')

  const result = await verifyPublishedRelease(tag, token)
  console.log(
    `Release ${tag} verified: version ${result.version}, ${result.artifacts} updater artifacts.`,
  )
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  })
}
