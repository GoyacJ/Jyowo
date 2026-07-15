import assert from 'node:assert/strict'
import test from 'node:test'

import { validateReleaseManifest, versionFromTag } from './verify-release-assets.mjs'

const tag = 'v0.2.0'
const releaseRoot = `https://github.com/GoyacJ/Jyowo/releases/download/${tag}`

function manifest(overrides = {}) {
  return {
    version: '0.2.0',
    platforms: {
      'darwin-aarch64': {
        signature: 'darwin-signature',
        url: `${releaseRoot}/Jyowo.app.tar.gz`,
      },
      'linux-x86_64': {
        signature: 'linux-signature',
        url: `${releaseRoot}/Jyowo.AppImage.tar.gz`,
      },
      'windows-x86_64': {
        signature: 'windows-signature',
        url: `${releaseRoot}/Jyowo-setup.exe`,
      },
    },
    ...overrides,
  }
}

test('accepts a signed three-platform updater manifest for the release tag', () => {
  assert.equal(versionFromTag(tag), '0.2.0')
  assert.equal(validateReleaseManifest(manifest(), tag).length, 3)
})

test('rejects invalid tags and version mismatches', () => {
  assert.throws(() => versionFromTag('latest'), /semantic version/)
  assert.throws(() => validateReleaseManifest(manifest({ version: '0.3.0' }), tag), /does not match/)
})

test('rejects missing platform families, signatures, and foreign asset URLs', () => {
  const missingLinux = manifest()
  delete missingLinux.platforms['linux-x86_64']
  assert.throws(() => validateReleaseManifest(missingLinux, tag), /missing a linux/)

  const unsigned = manifest()
  unsigned.platforms['darwin-aarch64'].signature = ''
  assert.throws(() => validateReleaseManifest(unsigned, tag), /missing its signature/)

  const foreignUrl = manifest()
  foreignUrl.platforms['windows-x86_64'].url = 'https://example.com/Jyowo-setup.exe'
  assert.throws(() => validateReleaseManifest(foreignUrl, tag), /unexpected URL/)
})
