import '@testing-library/jest-dom/vitest'

import { fireEvent, screen, render as testingLibraryRender, waitFor } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { createAppI18n } from '@/shared/i18n/i18n'

import { ArtifactRenderer } from './ArtifactRenderer'
import type { ArtifactDescriptor } from './model'
import { artifactRendererRegistry } from './renderers'
import type { ArtifactBlobLoader } from './resource'

function render(ui: React.ReactNode) {
  return testingLibraryRender(ui, {
    wrapper: ({ children }) => (
      <I18nextProvider i18n={createAppI18n('en-US')}>{children}</I18nextProvider>
    ),
  })
}

afterEach(() => vi.restoreAllMocks())

describe('ArtifactRenderer', () => {
  it.each([
    ['video/mp4', 'video', 'Video preview: Demo'],
    ['audio/mpeg', 'audio', 'Audio preview: Demo'],
  ])('renders %s media with native controls', async (mediaType, artifactKind, label) => {
    vi.spyOn(URL, 'createObjectURL').mockReturnValue(`blob:${artifactKind}`)
    const revoke = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined)
    const loader = blobLoader('media', mediaType)
    const { unmount } = render(
      <ArtifactRenderer
        artifact={descriptor({ artifactKind, mediaType })}
        loader={loader}
        surface="workbench"
      />,
    )

    const media = await screen.findByLabelText(label)
    expect(media).toHaveAttribute('controls')
    expect(media).toHaveAttribute('src', `blob:${artifactKind}`)
    unmount()
    expect(revoke).toHaveBeenCalledWith(`blob:${artifactKind}`)
  })

  it('renders GeoJSON features and keyboard-accessible zoom controls', async () => {
    const geoJson = JSON.stringify({
      features: [
        { geometry: { coordinates: [121.47, 31.23], type: 'Point' }, type: 'Feature' },
        {
          geometry: {
            coordinates: [
              [121.4, 31.2],
              [121.5, 31.25],
            ],
            type: 'LineString',
          },
          type: 'Feature',
        },
      ],
      type: 'FeatureCollection',
    })
    render(
      <ArtifactRenderer
        artifact={descriptor({ artifactKind: 'map', mediaType: 'application/geo+json' })}
        loader={blobLoader(geoJson, 'application/geo+json')}
        surface="inline"
      />,
    )

    const map = await screen.findByRole('img', { name: 'Map preview: Demo' })
    expect(map.querySelector('circle')).toBeInTheDocument()
    expect(map.querySelector('polyline')).toBeInTheDocument()
    const group = map.querySelector('g')
    const initialTransform = group?.getAttribute('transform')
    fireEvent.click(screen.getByRole('button', { name: 'Zoom in' }))
    expect(group?.getAttribute('transform')).not.toBe(initialTransform)
    fireEvent.click(screen.getByRole('button', { name: 'Reset map view' }))
    expect(group?.getAttribute('transform')).toBe(initialTransform)
  })

  it('reports invalid GeoJSON without crashing the conversation', async () => {
    render(
      <ArtifactRenderer
        artifact={descriptor({ artifactKind: 'map', mediaType: 'application/geo+json' })}
        loader={blobLoader('{broken', 'application/geo+json')}
        surface="card"
      />,
    )
    expect(
      await screen.findByText('This GeoJSON artifact has no valid geometry.'),
    ).toBeInTheDocument()
  })

  it('rejects GeoJSON that exceeds the preview complexity budget', async () => {
    render(
      <ArtifactRenderer
        artifact={descriptor({ artifactKind: 'map', mediaType: 'application/geo+json' })}
        loader={blobLoader(`"${'x'.repeat(2_000_001)}"`, 'application/geo+json')}
        surface="inline"
      />,
    )
    expect(
      await screen.findByText('This GeoJSON artifact is too complex to preview safely.'),
    ).toBeInTheDocument()
  })

  it.each([
    ['inline', 'preview-blob'],
    ['card', 'preview-blob'],
    ['workbench', 'original-blob'],
  ] as const)('loads the correct resource for the %s surface', async (surface, expectedBlobId) => {
    const loader = vi.fn<ArtifactBlobLoader>(async (requestedBlobId) =>
      blob(requestedBlobId, 'text/plain', requestedBlobId),
    )
    render(
      <ArtifactRenderer
        artifact={descriptor({
          blobId: 'original-blob',
          mediaType: 'text/plain',
          presentation: { previewBlobId: 'preview-blob' },
        })}
        loader={loader}
        surface={surface}
      />,
    )

    expect(await screen.findByText(expectedBlobId)).toBeInTheDocument()
    expect(loader).toHaveBeenCalledWith(expectedBlobId)
  })

  it('falls back to the preview resource in the workbench when the original is absent', async () => {
    const loader = vi.fn<ArtifactBlobLoader>(async (requestedBlobId) =>
      blob(requestedBlobId, 'text/plain', requestedBlobId),
    )
    render(
      <ArtifactRenderer
        artifact={descriptor({
          blobId: undefined,
          mediaType: 'text/plain',
          presentation: { previewBlobId: 'preview-only' },
        })}
        loader={loader}
        surface="workbench"
      />,
    )

    expect(await screen.findByText('preview-only')).toBeInTheDocument()
    expect(loader).toHaveBeenCalledWith('preview-only')
  })

  it('falls back to the original resource when an inline preview is missing', async () => {
    const loader = vi.fn<ArtifactBlobLoader>(async (requestedBlobId) =>
      requestedBlobId === 'preview-blob'
        ? { ...blob('', 'text/plain', requestedBlobId), bytes: null, missing: true, size: 0 }
        : blob('original content', 'text/plain', requestedBlobId),
    )
    render(
      <ArtifactRenderer
        artifact={descriptor({
          blobId: 'original-blob',
          mediaType: 'text/plain',
          presentation: { previewBlobId: 'preview-blob' },
        })}
        loader={loader}
        surface="inline"
      />,
    )

    expect(await screen.findByText('original content')).toBeInTheDocument()
    expect(loader.mock.calls.map(([blobId]) => blobId)).toEqual(['preview-blob', 'original-blob'])
  })

  it('falls back when the preferred resource contains no bytes', async () => {
    const loader = vi.fn<ArtifactBlobLoader>(async (requestedBlobId) =>
      requestedBlobId === 'preview-blob'
        ? blob('', 'text/plain', requestedBlobId)
        : blob('original content', 'text/plain', requestedBlobId),
    )
    render(
      <ArtifactRenderer
        artifact={descriptor({
          blobId: 'original-blob',
          mediaType: 'text/plain',
          presentation: { previewBlobId: 'preview-blob' },
        })}
        loader={loader}
        surface="card"
      />,
    )

    expect(await screen.findByText('original content')).toBeInTheDocument()
    expect(loader.mock.calls.map(([blobId]) => blobId)).toEqual(['preview-blob', 'original-blob'])
  })

  it('falls back to the preview resource when the workbench original fails', async () => {
    const loader = vi.fn<ArtifactBlobLoader>(async (requestedBlobId) => {
      if (requestedBlobId === 'original-blob') throw new Error('original unavailable')
      return blob('preview content', 'text/plain', requestedBlobId)
    })
    render(
      <ArtifactRenderer
        artifact={descriptor({
          blobId: 'original-blob',
          mediaType: 'text/plain',
          presentation: { previewBlobId: 'preview-blob' },
        })}
        loader={loader}
        surface="workbench"
      />,
    )

    expect(await screen.findByText('preview content')).toBeInTheDocument()
    expect(loader.mock.calls.map(([blobId]) => blobId)).toEqual(['original-blob', 'preview-blob'])
  })

  it('does not render a previous resource while a new resource is loading', async () => {
    const pending = new Promise<ReturnType<typeof blob>>(() => undefined)
    const loader = vi.fn<ArtifactBlobLoader>((requestedBlobId) =>
      requestedBlobId === 'first-blob'
        ? Promise.resolve(blob('first content', 'text/plain', requestedBlobId))
        : pending,
    )
    const { rerender } = render(
      <ArtifactRenderer
        artifact={descriptor({ blobId: 'first-blob', mediaType: 'text/plain' })}
        loader={loader}
        surface="workbench"
      />,
    )
    expect(await screen.findByText('first content')).toBeInTheDocument()

    rerender(
      <ArtifactRenderer
        artifact={descriptor({ blobId: 'second-blob', mediaType: 'text/plain' })}
        loader={loader}
        surface="workbench"
      />,
    )

    expect(screen.queryByText('first content')).not.toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Loading artifact…')
  })

  it('isolates renderer failures from the surrounding conversation', () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const unregister = artifactRendererRegistry.registerPlugin('boundary-test', [
      {
        id: 'throwing',
        matcher: { artifactKinds: ['broken'] },
        priority: 10_000,
        views: {
          inline: () => {
            throw new Error('renderer failed')
          },
        },
      },
    ])

    try {
      render(
        <ArtifactRenderer
          artifact={descriptor({ artifactKind: 'broken', blobId: undefined, preview: 'data' })}
          surface="inline"
        />,
      )
      expect(screen.getByText('The artifact preview could not be rendered.')).toBeInTheDocument()
      expect(consoleError).toHaveBeenCalled()
    } finally {
      unregister()
    }
  })

  it('retries failed resource loads', async () => {
    const loader = vi
      .fn<ArtifactBlobLoader>()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce(blob('recovered', 'text/plain'))
    render(
      <ArtifactRenderer
        artifact={descriptor({ mediaType: 'text/plain' })}
        loader={loader}
        surface="workbench"
      />,
    )

    expect(await screen.findByText('The resource could not be loaded.')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(screen.getByText('recovered')).toBeInTheDocument())
    expect(loader).toHaveBeenCalledTimes(2)
  })
})

function descriptor(overrides: Partial<ArtifactDescriptor>): ArtifactDescriptor {
  return {
    artifactKind: 'artifact',
    blobId: 'blob-1',
    mediaType: 'application/octet-stream',
    title: 'Demo',
    ...overrides,
  }
}

function blobLoader(value: string, mediaType: string): ArtifactBlobLoader {
  return vi.fn().mockResolvedValue(blob(value, mediaType))
}

function blob(value: string, mediaType: string, blobId = 'blob-1') {
  const bytes = new TextEncoder().encode(value)
  return {
    blobId,
    bytes,
    contentHash: Array(32).fill(1),
    mediaType,
    missing: false,
    size: bytes.byteLength,
  }
}
