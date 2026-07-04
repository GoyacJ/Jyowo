import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { CommandClient, ListArtifactsResponse } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createRejectedTestCommandClient, createTestCommandClient } from '@/testing/command-client'

import { ArtifactsPage } from './ArtifactsPage'

const artifactPreviewProps = vi.hoisted(
  () => [] as Array<{ content?: string; imageDataUrl?: string; state: string }>,
)
const validEvidenceContentHash = 'c'.repeat(64)

vi.mock('./ArtifactPreview', async (importOriginal) => {
  const original = await importOriginal<typeof import('./ArtifactPreview')>()

  return {
    ...original,
    ArtifactPreview: (props: import('./ArtifactPreview').ArtifactPreviewProps) => {
      artifactPreviewProps.push({
        content: props.content,
        imageDataUrl: props.imageDataUrl,
        state: props.state,
      })

      return original.ArtifactPreview(props)
    },
  }
})

const artifacts: ListArtifactsResponse = {
  artifacts: [
    {
      actionLabel: 'Open',
      description: 'Generated implementation plan and app shell review output.',
      id: 'artifact-foundation-plan',
      kind: 'markdown',
      preview: '# Foundation review',
      revisions: [
        {
          contentRef: 'artifact-content-foundation',
          revisionId: 'revision-foundation',
          updatedAt: '2026-06-17T00:00:02.000Z',
        },
      ],
      status: 'ready',
      title: 'Foundation implementation review',
      updatedAt: '2026-06-17T00:00:02.000Z',
    },
    {
      actionLabel: 'Open',
      description: 'Generated verification checklist.',
      id: 'artifact-verification',
      kind: 'markdown',
      preview: '# Verification',
      revisions: [
        {
          contentRef: 'artifact-content-verification',
          revisionId: 'revision-verification',
          updatedAt: '2026-06-17T00:00:01.000Z',
        },
      ],
      status: 'pending',
      title: 'Verification notes',
      updatedAt: '2026-06-17T00:00:01.000Z',
    },
  ],
}

function renderArtifactsPage(commandClient: CommandClient = createTestCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return {
    queryClient,
    ...render(<ArtifactsPage />, { wrapper: Wrapper }),
  }
}

describe('ArtifactsPage', () => {
  beforeEach(() => {
    artifactPreviewProps.length = 0
  })

  it('loads artifact history from the command client and switches previews', async () => {
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>(
      async (request) => ({
        artifactId: request.contentRef.includes('verification')
          ? 'artifact-verification'
          : 'artifact-foundation-plan',
        byteLength: request.contentRef.length,
        content: `loaded:${request.contentRef}`,
        contentHash: validEvidenceContentHash,
        contentBytes: request.contentRef.length,
        contentType: 'text/markdown; charset=utf-8',
        hasMore: false,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: request.contentRef,
        returnedBytes: request.contentRef.length,
        revisionId: request.contentRef.includes('verification')
          ? 'revision-verification'
          : 'revision-foundation',
        totalBytes: request.contentRef.length,
        truncated: false,
      }),
    )
    const commandClient = createTestCommandClient({
      artifactRevisionContent: getArtifactRevisionContent,
      artifacts,
    })
    const listArtifacts = vi.fn(commandClient.listArtifacts)
    const trackedClient = {
      ...commandClient,
      listArtifacts,
    } satisfies CommandClient

    renderArtifactsPage(trackedClient)

    expect(
      await screen.findByRole('article', { name: 'Foundation implementation review' }),
    ).toBeInTheDocument()
    const history = screen.getByRole('region', { name: 'Artifact history' })
    expect(
      within(history).getByRole('article', { name: 'Foundation implementation review' }),
    ).toBeInTheDocument()
    expect(await screen.findByText('loaded:artifact-content-foundation')).toBeInTheDocument()

    fireEvent.click(
      within(within(history).getByRole('article', { name: 'Verification notes' })).getByRole(
        'button',
        { name: 'Open' },
      ),
    )

    await waitFor(() => {
      expect(listArtifacts).toHaveBeenCalled()
    })
    expect(getArtifactRevisionContent).toHaveBeenCalledWith({
      conversationId: 'conversation-001',
      contentRef: 'artifact-content-verification',
    })
    expect(await screen.findByText('loaded:artifact-content-verification')).toBeInTheDocument()
  })

  it('renders html revisions in a sandboxed iframe', async () => {
    const commandClient = createTestCommandClient({
      artifactRevisionContent: () => ({
        artifactId: 'artifact-html',
        byteLength: 28,
        content: '<main><h1>Preview</h1></main>',
        contentHash: validEvidenceContentHash,
        contentBytes: 28,
        contentType: 'text/html',
        hasMore: false,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: 'artifact-content-html',
        returnedBytes: 28,
        revisionId: 'revision-html',
        totalBytes: 28,
        truncated: false,
      }),
      artifacts: {
        artifacts: [
          {
            actionLabel: 'Open',
            description: 'Generated HTML preview.',
            id: 'artifact-html',
            kind: 'html',
            revisions: [
              {
                contentRef: 'artifact-content-html',
                revisionId: 'revision-html',
                updatedAt: '2026-06-17T00:00:03.000Z',
              },
            ],
            status: 'ready',
            title: 'HTML preview',
            updatedAt: '2026-06-17T00:00:03.000Z',
          },
        ],
      },
    })

    renderArtifactsPage(commandClient)

    const iframe = await screen.findByTitle('HTML preview sandboxed preview')
    expect(iframe).toHaveAttribute('sandbox', '')
    expect(iframe).not.toHaveAttribute('sandbox', expect.stringContaining('allow-same-origin'))
    expect(iframe).toHaveAttribute('referrerpolicy', 'no-referrer')
    expect(iframe.getAttribute('srcdoc')).toContain("default-src 'none'")
    expect(iframe.getAttribute('srcdoc')).toContain("connect-src 'none'")
    expect(iframe.getAttribute('srcdoc')).toContain('<main><h1>Preview</h1></main>')
  })

  it('renders image previews from backend media data urls', async () => {
    const getArtifactMediaPreview = vi.fn<CommandClient['getArtifactMediaPreview']>(async () => ({
      dataUrl:
        'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=',
      mimeType: 'image/png',
      sizeBytes: 68,
    }))
    const commandClient = {
      ...createTestCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'Generated image.',
              id: 'artifact-image',
              kind: 'image',
              revisions: [
                {
                  revisionId: 'revision-image',
                  updatedAt: '2026-06-17T00:00:04.000Z',
                },
              ],
              status: 'ready',
              title: 'Image preview',
              updatedAt: '2026-06-17T00:00:04.000Z',
            },
          ],
        },
      }),
      getArtifactMediaPreview,
    } satisfies CommandClient

    renderArtifactsPage(commandClient)

    const image = await screen.findByRole('img', { name: 'Image preview' })
    expect(getArtifactMediaPreview).toHaveBeenCalledWith({
      artifactId: 'artifact-image',
      conversationId: 'conversation-001',
    })
    expect(image).toHaveAttribute('src', expect.stringMatching(/^data:image\/png;base64,/))
    expect(artifactPreviewProps.at(-1)).toMatchObject({
      imageDataUrl: expect.stringMatching(/^data:image\/png;base64,/),
      state: 'ready',
    })
  })

  it('falls back to the summary preview when a text artifact has no content ref', async () => {
    const getArtifactRevisionContent = vi.fn()
    renderArtifactsPage(
      createTestCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'Generated notes.',
              id: 'artifact-missing-ref',
              kind: 'markdown',
              preview: '# Do not render this as content',
              revisions: [
                {
                  revisionId: 'revision-missing-ref',
                  updatedAt: '2026-06-17T00:00:05.000Z',
                },
              ],
              status: 'ready',
              title: 'Missing content ref',
              updatedAt: '2026-06-17T00:00:05.000Z',
            },
          ],
        },
        artifactRevisionContent: getArtifactRevisionContent,
      }),
    )

    expect(await screen.findByText('# Do not render this as content')).toBeInTheDocument()
    expect(getArtifactRevisionContent).not.toHaveBeenCalled()
  })

  it('renders empty, loading, and error states without raw backend details', async () => {
    const { unmount } = renderArtifactsPage(createTestCommandClient({ delayMs: 10 }))

    expect(screen.getByText('Loading artifacts')).toBeInTheDocument()
    expect(
      await screen.findByRole('article', { name: 'Desktop foundation created' }),
    ).toBeInTheDocument()

    unmount()

    const { unmount: unmountEmpty } = renderArtifactsPage(
      createTestCommandClient({ artifacts: { artifacts: [] } }),
    )

    expect(await screen.findByText('No artifacts for this conversation.')).toBeInTheDocument()
    expect(screen.getByText('No artifact selected.')).toBeInTheDocument()

    unmountEmpty()

    renderArtifactsPage(
      createRejectedTestCommandClient(
        new Error('artifact failed with Authorization Bearer secret'),
      ),
    )

    expect(await screen.findByText('Artifact history could not be loaded.')).toBeInTheDocument()
    expect(screen.getByText('Artifact preview unavailable.')).toBeInTheDocument()
    expect(screen.queryByText(/Authorization Bearer/)).not.toBeInTheDocument()
  })

  it('does not keep stale preview visible after artifact refetch fails', async () => {
    const listArtifacts = vi
      .fn()
      .mockResolvedValueOnce(artifacts)
      .mockRejectedValueOnce(new Error('artifact failed with Authorization Bearer secret'))
    const commandClient = {
      ...createTestCommandClient(),
      listArtifacts,
    } satisfies CommandClient
    const { queryClient } = renderArtifactsPage(commandClient)

    expect(await screen.findByText('fixture artifact content')).toBeInTheDocument()

    await act(async () => {
      await queryClient.invalidateQueries({ queryKey: ['artifacts'] })
    })

    await waitFor(() => {
      expect(screen.getByText('Artifact history could not be loaded.')).toBeInTheDocument()
    })
    expect(screen.getByText('Artifact preview unavailable.')).toBeInTheDocument()
    expect(screen.queryByText('fixture artifact content')).not.toBeInTheDocument()
    expect(artifactPreviewProps.at(-1)).toMatchObject({
      content: undefined,
      state: 'error',
    })
    expect(screen.queryByText(/Authorization Bearer/)).not.toBeInTheDocument()
  })
})
