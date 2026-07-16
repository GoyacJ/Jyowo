import { describe, expect, it, vi } from 'vitest'

import type { ArtifactDescriptor } from './model'
import { ArtifactRendererRegistry, installArtifactRendererPlugins } from './registry'

const artifact: ArtifactDescriptor = {
  artifactKind: 'image',
  format: 'png',
  mediaType: 'image/png',
  title: 'diagram.png',
}
const View = () => null

describe('ArtifactRendererRegistry', () => {
  it('selects exact MIME before prefix and fallback renderers', () => {
    const registry = new ArtifactRendererRegistry()
    registry.register({
      id: 'fallback',
      matcher: { mediaTypes: ['*/*'] },
      priority: -1_000,
      views: { inline: View },
    })
    registry.register({
      id: 'image',
      matcher: { mediaTypes: ['image/*'] },
      views: { inline: View },
    })
    registry.register({
      id: 'png',
      matcher: { mediaTypes: ['image/png'] },
      views: { inline: View },
    })

    expect(registry.resolve(artifact, 'inline')?.id).toBe('png')
    expect(registry.resolve(artifact, 'workbench')).toBeUndefined()
  })

  it('accepts plugin-owned format renderers and returns an unregister function', () => {
    const registry = new ArtifactRendererRegistry()
    const unregister = registry.registerPlugin('terrain', [
      {
        id: 'contours',
        matcher: { formats: ['contours'] },
        views: { card: View },
      },
    ])

    expect(registry.resolve({ ...artifact, format: 'contours' }, 'card')).toMatchObject({
      id: 'plugin:terrain/contours',
      source: 'plugin:terrain',
    })
    unregister()
    expect(registry.resolve({ ...artifact, format: 'contours' }, 'card')).toBeUndefined()
  })

  it('rejects duplicate renderer identifiers', () => {
    const registry = new ArtifactRendererRegistry()
    const definition = { id: 'same', matcher: {}, views: { inline: View } }
    registry.register(definition)
    expect(() => registry.register(definition)).toThrow('Artifact renderer already registered')
  })

  it('registers plugin renderer groups atomically', () => {
    const registry = new ArtifactRendererRegistry()
    expect(() =>
      registry.registerPlugin('terrain', [
        { id: 'same', matcher: {}, views: { inline: View } },
        { id: 'same', matcher: {}, views: { inline: View } },
      ]),
    ).toThrow('Artifact renderer already registered')
    expect(registry.list()).toHaveLength(0)
    expect(() => registry.registerPlugin('../unsafe', [])).toThrow(
      'Invalid artifact renderer plugin identifier',
    )
  })

  it('installs build-time plugin contributions atomically with idempotent cleanup', () => {
    const registry = new ArtifactRendererRegistry()
    const cleanup = installArtifactRendererPlugins(registry, [
      {
        id: 'terrain',
        renderers: [{ id: 'contours', matcher: { formats: ['contours'] }, views: { card: View } }],
      },
      {
        id: 'media',
        renderers: [
          { id: 'waveform', matcher: { formats: ['waveform'] }, views: { inline: View } },
        ],
      },
    ])

    expect(registry.list().map((renderer) => renderer.id)).toEqual([
      'plugin:terrain/contours',
      'plugin:media/waveform',
    ])
    cleanup()
    cleanup()
    expect(registry.list()).toHaveLength(0)
  })

  it('rolls back earlier plugins when a later contribution is invalid', () => {
    const registry = new ArtifactRendererRegistry()
    expect(() =>
      installArtifactRendererPlugins(registry, [
        {
          id: 'valid',
          renderers: [{ id: 'first', matcher: {}, views: { card: View } }],
        },
        {
          id: 'invalid',
          renderers: [{ id: '../unsafe', matcher: {}, views: { card: View } }],
        },
      ]),
    ).toThrow('Invalid artifact renderer identifier')
    expect(registry.list()).toHaveLength(0)
  })

  it('does not let a stale cleanup remove a later registration with the same id', () => {
    const registry = new ArtifactRendererRegistry()
    const first = { id: 'replaceable', matcher: {}, views: { inline: View } }
    const unregisterFirst = registry.register(first)
    unregisterFirst()
    registry.register({ ...first, priority: 1 })

    unregisterFirst()
    expect(registry.resolve(artifact, 'inline')?.priority).toBe(1)
  })

  it('skips a renderer whose custom matcher throws', () => {
    const registry = new ArtifactRendererRegistry()
    registry.register({
      id: 'throwing',
      matcher: {
        test: () => {
          throw new Error('broken matcher')
        },
      },
      priority: 10_000,
      views: { inline: View },
    })
    registry.register({
      id: 'fallback',
      matcher: { mediaTypes: ['*/*'] },
      views: { inline: View },
    })

    expect(registry.resolve(artifact, 'inline')?.id).toBe('fallback')
  })

  it('evaluates a custom matcher once while resolving', () => {
    const test = vi.fn(() => true)
    const registry = new ArtifactRendererRegistry()
    registry.register({ id: 'custom', matcher: { test }, views: { inline: View } })

    expect(registry.resolve(artifact, 'inline')?.id).toBe('custom')
    expect(test).toHaveBeenCalledOnce()
  })

  it.each([
    ['format', { formats: ['geojson'] }],
    ['artifact kind', { artifactKinds: ['map'] }],
  ] as Array<
    [string, { artifactKinds?: string[]; formats?: string[] }]
  >)('does not match a declared %s matcher when the artifact field is absent', (_, matcher) => {
    const registry = new ArtifactRendererRegistry()
    registry.register({ id: 'specific', matcher, views: { inline: View } })

    expect(
      registry.resolve({ mediaType: 'application/octet-stream', title: 'untitled' }, 'inline'),
    ).toBeUndefined()
  })
})
