import '@testing-library/jest-dom/vitest'

import { act, fireEvent, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { createTestCommandClient } from '@/testing/command-client'
import { artifactRevision } from '@/testing/conversation-worktree-builders'
import {
  inspectorTurn,
  renderInspector,
  setupStore,
  validEvidenceContentHash,
  worktreePage,
} from './WorkbenchInspector.test-support'

describe('WorkbenchInspector artifact pane', () => {
  it('renders the selected artifact pane and fetches content through the command client', async () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
        revisionId: 'revision-inspector',
      },
    })
    renderInspector(
      createTestCommandClient({
        conversationWorktreePage: worktreePage([inspectorTurn()]),
        artifactRevisionContent: {
          content: 'real artifact content',
          contentType: 'text/plain; charset=utf-8',
          byteLength: 21,
          contentBytes: 21,
          offsetBytes: 0,
          limitBytes: 65_536,
          totalBytes: 21,
          returnedBytes: 21,
          maxBytes: 65_536,
          truncated: false,
          hasMore: false,
          kind: 'artifact-content',
          refId: 'evidence-artifact-inspector',
          contentHash: validEvidenceContentHash,
          hashAlgorithm: 'blake3',
          redactionState: 'clean',
          artifactId: 'artifact-inspector',
          revisionId: 'revision-inspector',
        },
      }),
    )

    expect((await screen.findAllByText('Inspector notes')).length).toBeGreaterThan(0)
    expect(await screen.findByText('real artifact content')).toBeInTheDocument()
    expect(
      screen.queryByText(/Artifact artifact-inspector revision details/),
    ).not.toBeInTheDocument()
  })

  it('marks bounded artifact content pages as truncated', async () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
        revisionId: 'revision-inspector',
      },
    })
    renderInspector(
      createTestCommandClient({
        conversationWorktreePage: worktreePage([inspectorTurn()]),
        artifactRevisionContent: {
          artifactId: 'artifact-inspector',
          byteLength: 131_072,
          content: 'bounded artifact page',
          contentBytes: 131_072,
          contentType: 'text/plain; charset=utf-8',
          hasMore: true,
          contentHash: validEvidenceContentHash,
          hashAlgorithm: 'blake3',
          kind: 'artifact-content',
          limitBytes: 65_536,
          maxBytes: 65_536,
          nextCursor: '65536',
          offsetBytes: 0,
          redactionState: 'clean',
          refId: 'evidence-artifact-inspector',
          returnedBytes: 21,
          revisionId: 'revision-inspector',
          totalBytes: 131_072,
          truncated: true,
        },
      }),
    )

    expect(await screen.findByText('bounded artifact page')).toBeInTheDocument()
    expect(await screen.findByText('Artifact content page truncated')).toBeInTheDocument()
  })

  it('copies loaded artifact content from the backend content ref', async () => {
    const originalClipboard = navigator.clipboard
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>(
      async () => ({
        artifactId: 'artifact-inspector',
        byteLength: 21,
        content: 'real artifact content',
        contentBytes: 21,
        contentType: 'text/plain; charset=utf-8',
        hasMore: false,
        contentHash: validEvidenceContentHash,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: 'evidence-artifact-inspector',
        returnedBytes: 21,
        revisionId: 'revision-inspector',
        totalBytes: 21,
        truncated: false,
      }),
    )
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
        revisionId: 'revision-inspector',
      },
    })
    try {
      renderInspector(
        createTestCommandClient({
          artifactRevisionContent: getArtifactRevisionContent,
          conversationWorktreePage: worktreePage([inspectorTurn()]),
        }),
      )

      expect(await screen.findByText('real artifact content')).toBeInTheDocument()
      fireEvent.click(screen.getByRole('button', { name: 'Copy content' }))

      await waitFor(() =>
        expect(getArtifactRevisionContent).toHaveBeenCalledWith({
          conversationId: 'conversation-inspector',
          contentRef: 'evidence-artifact-inspector',
        }),
      )
      expect(writeText).toHaveBeenCalledWith('real artifact content')
    } finally {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      })
    }
  })

  it('copies every artifact content page from the backend content ref', async () => {
    const originalClipboard = navigator.clipboard
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>(
      async (request) => ({
        artifactId: 'artifact-inspector',
        byteLength: 10,
        content: request.cursor ? 'second page' : 'first page ',
        contentBytes: request.cursor ? 11 : 10,
        contentType: 'text/plain; charset=utf-8',
        hasMore: request.cursor === undefined,
        contentHash: validEvidenceContentHash,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        nextCursor: request.cursor === undefined ? 'cursor-2' : undefined,
        offsetBytes: request.cursor ? 10 : 0,
        redactionState: 'clean',
        refId: 'evidence-artifact-inspector',
        returnedBytes: request.cursor ? 11 : 10,
        revisionId: 'revision-inspector',
        totalBytes: 21,
        truncated: request.cursor === undefined,
      }),
    )
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
        revisionId: 'revision-inspector',
      },
    })
    try {
      renderInspector(
        createTestCommandClient({
          artifactRevisionContent: getArtifactRevisionContent,
          conversationWorktreePage: worktreePage([inspectorTurn()]),
        }),
      )

      expect(await screen.findByText('first page')).toBeInTheDocument()
      fireEvent.click(screen.getByRole('button', { name: 'Copy content' }))

      await waitFor(() => expect(writeText).toHaveBeenCalledWith('first page second page'))
      expect(getArtifactRevisionContent).toHaveBeenCalledWith({
        conversationId: 'conversation-inspector',
        contentRef: 'evidence-artifact-inspector',
        cursor: 'cursor-2',
      })
    } finally {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      })
    }
  })

  it('fails artifact copy when a paged response omits the next cursor', async () => {
    const originalClipboard = navigator.clipboard
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>(
      async () => ({
        artifactId: 'artifact-inspector',
        byteLength: 21,
        content: 'first page',
        contentBytes: 10,
        contentType: 'text/plain; charset=utf-8',
        hasMore: true,
        contentHash: validEvidenceContentHash,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: 'evidence-artifact-inspector',
        returnedBytes: 10,
        revisionId: 'revision-inspector',
        totalBytes: 21,
        truncated: true,
      }),
    )
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
        revisionId: 'revision-inspector',
      },
    })
    try {
      renderInspector(
        createTestCommandClient({
          artifactRevisionContent: getArtifactRevisionContent,
          conversationWorktreePage: worktreePage([inspectorTurn()]),
        }),
      )

      expect(await screen.findByText('first page')).toBeInTheDocument()
      fireEvent.click(screen.getByRole('button', { name: 'Copy content' }))

      expect(await screen.findByText('Copy failed')).toBeInTheDocument()
      expect(writeText).not.toHaveBeenCalled()
    } finally {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      })
    }
  })

  it('shows artifact revisions newest-first and switches selected revision content', async () => {
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>(
      async (request) => ({
        artifactId: 'artifact-inspector',
        byteLength: request.contentRef.length,
        content: `content:${request.contentRef}`,
        contentBytes: request.contentRef.length,
        contentType: 'text/markdown; charset=utf-8',
        hasMore: false,
        contentHash: validEvidenceContentHash,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: request.contentRef,
        returnedBytes: request.contentRef.length,
        revisionId:
          request.contentRef === 'evidence-artifact-old' ? 'revision-old' : 'revision-new',
        totalBytes: request.contentRef.length,
        truncated: false,
      }),
    )
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
      },
    })
    renderInspector(
      createTestCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'Generated notes.',
              id: 'artifact-inspector',
              kind: 'document',
              revisions: [
                {
                  contentRef: 'evidence-artifact-old',
                  kind: 'document',
                  revisionId: 'revision-old',
                  status: 'ready',
                  title: 'Old notes',
                  updatedAt: '2026-06-17T00:00:01.000Z',
                },
                {
                  contentRef: 'evidence-artifact-new',
                  kind: 'document',
                  revisionId: 'revision-new',
                  status: 'ready',
                  title: 'New notes',
                  updatedAt: '2026-06-17T00:00:03.000Z',
                },
              ],
              status: 'ready',
              title: 'Inspector notes',
              updatedAt: '2026-06-17T00:00:03.000Z',
            },
          ],
        },
        artifactRevisionContent: getArtifactRevisionContent,
        conversationInspectorItem: {
          item: {
            kind: 'artifact',
            segment: {
              kind: 'artifact',
              id: 'segment-artifact-inspector',
              order: 2,
              artifactId: 'artifact-inspector',
              artifactKind: 'document',
              status: 'ready',
              source: 'assistant',
              title: 'Inspector notes',
              revision: artifactRevision({
                artifactId: 'artifact-inspector',
                revisionId: 'revision-new',
                kind: 'document',
                sourceRunId: 'run-inspector',
                title: 'New notes',
                contentRef: 'evidence-artifact-new',
              }),
            },
          },
        },
      }),
    )

    expect(await screen.findByText('content:evidence-artifact-new')).toBeInTheDocument()

    const revisionButtons = screen.getAllByRole('button', { name: /revision-/ })
    expect(revisionButtons.map((button) => button.textContent)).toEqual([
      expect.stringContaining('revision-new'),
      expect.stringContaining('revision-old'),
    ])

    fireEvent.click(screen.getByRole('button', { name: /revision-old/ }))

    expect(await screen.findByText('content:evidence-artifact-old')).toBeInTheDocument()
    expect(getArtifactRevisionContent).toHaveBeenLastCalledWith({
      conversationId: 'conversation-inspector',
      contentRef: 'evidence-artifact-old',
    })
  })

  it('clears artifact copy status when switching revisions', async () => {
    const originalClipboard = navigator.clipboard
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>(
      async (request) => ({
        artifactId: 'artifact-inspector',
        byteLength: request.contentRef.length,
        content: `content:${request.contentRef}`,
        contentBytes: request.contentRef.length,
        contentType: 'text/markdown; charset=utf-8',
        hasMore: false,
        contentHash: validEvidenceContentHash,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: request.contentRef,
        returnedBytes: request.contentRef.length,
        revisionId:
          request.contentRef === 'evidence-artifact-old' ? 'revision-old' : 'revision-new',
        totalBytes: request.contentRef.length,
        truncated: false,
      }),
    )
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
      },
    })
    try {
      renderInspector(
        createTestCommandClient({
          artifacts: {
            artifacts: [
              {
                actionLabel: 'Open',
                description: 'Generated notes.',
                id: 'artifact-inspector',
                kind: 'document',
                revisions: [
                  {
                    contentRef: 'evidence-artifact-old',
                    kind: 'document',
                    revisionId: 'revision-old',
                    status: 'ready',
                    title: 'Old notes',
                    updatedAt: '2026-06-17T00:00:01.000Z',
                  },
                  {
                    contentRef: 'evidence-artifact-new',
                    kind: 'document',
                    revisionId: 'revision-new',
                    status: 'ready',
                    title: 'New notes',
                    updatedAt: '2026-06-17T00:00:03.000Z',
                  },
                ],
                status: 'ready',
                title: 'Inspector notes',
                updatedAt: '2026-06-17T00:00:03.000Z',
              },
            ],
          },
          artifactRevisionContent: getArtifactRevisionContent,
          conversationInspectorItem: {
            item: {
              kind: 'artifact',
              segment: {
                kind: 'artifact',
                id: 'segment-artifact-inspector',
                order: 2,
                artifactId: 'artifact-inspector',
                artifactKind: 'document',
                status: 'ready',
                source: 'assistant',
                title: 'Inspector notes',
                revision: artifactRevision({
                  artifactId: 'artifact-inspector',
                  revisionId: 'revision-new',
                  kind: 'document',
                  sourceRunId: 'run-inspector',
                  title: 'New notes',
                  contentRef: 'evidence-artifact-new',
                }),
              },
            },
          },
        }),
      )

      expect(await screen.findByText('content:evidence-artifact-new')).toBeInTheDocument()
      fireEvent.click(screen.getByRole('button', { name: 'Copy content' }))
      expect(await screen.findByText('Content copied')).toBeInTheDocument()

      fireEvent.click(screen.getByRole('button', { name: /revision-old/ }))

      await waitFor(() => expect(screen.queryByText('Content copied')).not.toBeInTheDocument())
      expect(await screen.findByText('content:evidence-artifact-old')).toBeInTheDocument()
    } finally {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      })
    }
  })

  it('opens the selected older artifact revision when inspector authority returns latest segment', async () => {
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>(
      async (request) => ({
        artifactId: 'artifact-inspector',
        byteLength: request.contentRef.length,
        content: `content:${request.contentRef}`,
        contentBytes: request.contentRef.length,
        contentType: 'text/markdown; charset=utf-8',
        hasMore: false,
        contentHash: validEvidenceContentHash,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: request.contentRef,
        returnedBytes: request.contentRef.length,
        revisionId:
          request.contentRef === 'evidence-artifact-old' ? 'revision-old' : 'revision-new',
        totalBytes: request.contentRef.length,
        truncated: false,
      }),
    )
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
        revisionId: 'revision-old',
      },
    })
    renderInspector(
      createTestCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'Generated notes.',
              id: 'artifact-inspector',
              kind: 'document',
              revisions: [
                {
                  contentRef: 'evidence-artifact-old',
                  kind: 'document',
                  revisionId: 'revision-old',
                  status: 'ready',
                  title: 'Old notes',
                  updatedAt: '2026-06-17T00:00:01.000Z',
                },
                {
                  contentRef: 'evidence-artifact-new',
                  kind: 'document',
                  revisionId: 'revision-new',
                  status: 'ready',
                  title: 'New notes',
                  updatedAt: '2026-06-17T00:00:03.000Z',
                },
              ],
              status: 'ready',
              title: 'Inspector notes',
              updatedAt: '2026-06-17T00:00:03.000Z',
            },
          ],
        },
        artifactRevisionContent: getArtifactRevisionContent,
        conversationInspectorItem: {
          item: {
            kind: 'artifact',
            segment: {
              kind: 'artifact',
              id: 'segment-artifact-inspector',
              order: 2,
              artifactId: 'artifact-inspector',
              artifactKind: 'document',
              status: 'ready',
              source: 'assistant',
              title: 'Inspector notes',
              revision: artifactRevision({
                artifactId: 'artifact-inspector',
                revisionId: 'revision-new',
                kind: 'document',
                sourceRunId: 'run-inspector',
                title: 'New notes',
                contentRef: 'evidence-artifact-new',
              }),
            },
          },
        },
      }),
    )

    expect(await screen.findByText('content:evidence-artifact-old')).toBeInTheDocument()
    expect(getArtifactRevisionContent).toHaveBeenCalledTimes(1)
    expect(getArtifactRevisionContent).toHaveBeenCalledWith({
      conversationId: 'conversation-inspector',
      contentRef: 'evidence-artifact-old',
    })
  })

  it('resets artifact revision state when inspector selection changes on the same artifact', async () => {
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>(
      async (request) => ({
        artifactId: 'artifact-inspector',
        byteLength: request.contentRef.length,
        content: `content:${request.contentRef}`,
        contentBytes: request.contentRef.length,
        contentType: 'text/markdown; charset=utf-8',
        hasMore: false,
        contentHash: validEvidenceContentHash,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: request.contentRef,
        returnedBytes: request.contentRef.length,
        revisionId:
          request.contentRef === 'evidence-artifact-old' ? 'revision-old' : 'revision-new',
        totalBytes: request.contentRef.length,
        truncated: false,
      }),
    )
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
        revisionId: 'revision-old',
      },
    })
    renderInspector(
      createTestCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'Generated notes.',
              id: 'artifact-inspector',
              kind: 'document',
              revisions: [
                {
                  contentRef: 'evidence-artifact-old',
                  kind: 'document',
                  revisionId: 'revision-old',
                  status: 'ready',
                  title: 'Old notes',
                  updatedAt: '2026-06-17T00:00:01.000Z',
                },
                {
                  contentRef: 'evidence-artifact-new',
                  kind: 'document',
                  revisionId: 'revision-new',
                  status: 'ready',
                  title: 'New notes',
                  updatedAt: '2026-06-17T00:00:03.000Z',
                },
              ],
              status: 'ready',
              title: 'Inspector notes',
              updatedAt: '2026-06-17T00:00:03.000Z',
            },
          ],
        },
        artifactRevisionContent: getArtifactRevisionContent,
        conversationInspectorItem: {
          item: {
            kind: 'artifact',
            segment: {
              kind: 'artifact',
              id: 'segment-artifact-inspector',
              order: 2,
              artifactId: 'artifact-inspector',
              artifactKind: 'document',
              status: 'ready',
              source: 'assistant',
              title: 'Inspector notes',
              revision: artifactRevision({
                artifactId: 'artifact-inspector',
                revisionId: 'revision-new',
                kind: 'document',
                sourceRunId: 'run-inspector',
                title: 'New notes',
                contentRef: 'evidence-artifact-new',
              }),
            },
          },
        },
      }),
    )

    expect(await screen.findByText('content:evidence-artifact-old')).toBeInTheDocument()

    act(() => {
      uiStore.setState({
        workbenchSelection: {
          kind: 'artifact',
          conversationId: 'conversation-inspector',
          artifactId: 'artifact-inspector',
          revisionId: 'revision-new',
        },
      })
    })

    expect(await screen.findByText('content:evidence-artifact-new')).toBeInTheDocument()
    expect(getArtifactRevisionContent).toHaveBeenLastCalledWith({
      conversationId: 'conversation-inspector',
      contentRef: 'evidence-artifact-new',
    })
  })

  it('loads the selected older image artifact revision by revision id', async () => {
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>()
    const getArtifactMediaPreview = vi.fn<CommandClient['getArtifactMediaPreview']>(async () => ({
      dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
      mimeType: 'image/png',
      sizeBytes: 67,
    }))
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-image',
        revisionId: 'revision-image-old',
      },
    })
    renderInspector({
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
                  contentRef: 'evidence-image-old',
                  previewRef: 'evidence-preview-image-old',
                  kind: 'image',
                  media: {
                    kind: 'image',
                    mimeType: 'image/png',
                    sizeBytes: 67,
                  },
                  revisionId: 'revision-image-old',
                  status: 'ready',
                  title: 'Old image',
                  updatedAt: '2026-06-17T00:00:01.000Z',
                },
                {
                  contentRef: 'evidence-image-new',
                  previewRef: 'evidence-preview-image-new',
                  kind: 'image',
                  media: {
                    kind: 'image',
                    mimeType: 'image/png',
                    sizeBytes: 68,
                  },
                  revisionId: 'revision-image-new',
                  status: 'ready',
                  title: 'New image',
                  updatedAt: '2026-06-17T00:00:03.000Z',
                },
              ],
              status: 'ready',
              title: 'Generated image',
              updatedAt: '2026-06-17T00:00:03.000Z',
            },
          ],
        },
        artifactRevisionContent: getArtifactRevisionContent,
        conversationInspectorItem: {
          item: {
            kind: 'artifact',
            segment: {
              kind: 'artifact',
              id: 'segment-artifact-image',
              order: 2,
              artifactId: 'artifact-image',
              artifactKind: 'image',
              status: 'ready',
              source: 'assistant',
              title: 'Generated image',
              revision: artifactRevision({
                artifactId: 'artifact-image',
                revisionId: 'revision-image-new',
                kind: 'image',
                sourceRunId: 'run-inspector',
                title: 'New image',
                contentRef: 'evidence-image-new',
                previewRef: 'evidence-preview-image-new',
                media: {
                  kind: 'image',
                  mimeType: 'image/png',
                  sizeBytes: 68,
                },
              }),
            },
          },
        },
      }),
      getArtifactMediaPreview,
    })

    expect(await screen.findByRole('img', { name: 'Old image' })).toHaveAttribute(
      'src',
      expect.stringMatching(/^data:image\/png;base64,/),
    )
    expect(getArtifactMediaPreview).toHaveBeenCalledTimes(1)
    expect(getArtifactMediaPreview).toHaveBeenCalledWith({
      conversationId: 'conversation-inspector',
      artifactId: 'artifact-image',
      contentRef: 'evidence-preview-image-old',
      revisionId: 'revision-image-old',
    })
    expect(getArtifactRevisionContent).not.toHaveBeenCalled()
  })

  it('updates artifact pane state when timeline selects another artifact', async () => {
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>(
      async (request) => ({
        artifactId:
          request.contentRef === 'evidence-artifact-two' ? 'artifact-two' : 'artifact-one',
        byteLength: request.contentRef.length,
        content: `content:${request.contentRef}`,
        contentBytes: request.contentRef.length,
        contentType: 'text/markdown; charset=utf-8',
        hasMore: false,
        contentHash: validEvidenceContentHash,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: request.contentRef,
        returnedBytes: request.contentRef.length,
        revisionId:
          request.contentRef === 'evidence-artifact-two' ? 'revision-two' : 'revision-one',
        totalBytes: request.contentRef.length,
        truncated: false,
      }),
    )
    const getConversationInspectorItem = vi.fn<CommandClient['getConversationInspectorItem']>(
      async (request) => {
        const artifactId =
          request.selection.kind === 'artifact' ? request.selection.artifactId : 'artifact-one'
        const revisionId = artifactId === 'artifact-two' ? 'revision-two' : 'revision-one'
        const contentRef =
          artifactId === 'artifact-two' ? 'evidence-artifact-two' : 'evidence-artifact-one'
        const title = artifactId === 'artifact-two' ? 'Second artifact' : 'First artifact'

        return {
          item: {
            kind: 'artifact',
            segment: {
              kind: 'artifact',
              id: `segment-${artifactId}`,
              order: 2,
              artifactId,
              artifactKind: 'document',
              status: 'ready',
              source: 'assistant',
              title,
              revision: artifactRevision({
                artifactId,
                revisionId,
                kind: 'document',
                sourceRunId: 'run-inspector',
                title,
                contentRef,
              }),
            },
          },
        }
      },
    )
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-one',
      },
    })
    renderInspector(
      createTestCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'First output.',
              id: 'artifact-one',
              kind: 'document',
              revisions: [
                {
                  contentRef: 'evidence-artifact-one',
                  kind: 'document',
                  revisionId: 'revision-one',
                  status: 'ready',
                  title: 'First artifact',
                  updatedAt: '2026-06-17T00:00:01.000Z',
                },
              ],
              status: 'ready',
              title: 'First artifact',
              updatedAt: '2026-06-17T00:00:01.000Z',
            },
            {
              actionLabel: 'Open',
              description: 'Second output.',
              id: 'artifact-two',
              kind: 'document',
              revisions: [
                {
                  contentRef: 'evidence-artifact-two',
                  kind: 'document',
                  revisionId: 'revision-two',
                  status: 'ready',
                  title: 'Second artifact',
                  updatedAt: '2026-06-17T00:00:02.000Z',
                },
              ],
              status: 'ready',
              title: 'Second artifact',
              updatedAt: '2026-06-17T00:00:02.000Z',
            },
          ],
        },
        artifactRevisionContent: getArtifactRevisionContent,
        conversationInspectorItem: getConversationInspectorItem,
      }),
    )

    expect(await screen.findByText('content:evidence-artifact-one')).toBeInTheDocument()

    act(() => {
      uiStore.setState({
        workbenchSelection: {
          kind: 'artifact',
          conversationId: 'conversation-inspector',
          artifactId: 'artifact-two',
        },
      })
    })

    expect(await screen.findByText('content:evidence-artifact-two')).toBeInTheDocument()
    expect(getArtifactRevisionContent).toHaveBeenLastCalledWith({
      conversationId: 'conversation-inspector',
      contentRef: 'evidence-artifact-two',
    })
  })

  it('renders failed artifact revisions without fetching content', async () => {
    const getArtifactRevisionContent = vi.fn()
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
        revisionId: 'revision-failed',
      },
    })
    renderInspector(
      createTestCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'Failed output.',
              id: 'artifact-inspector',
              kind: 'document',
              revisions: [
                {
                  contentRef: 'evidence-artifact-failed',
                  kind: 'document',
                  revisionId: 'revision-failed',
                  status: 'failed',
                  title: 'Failed notes',
                  updatedAt: '2026-06-17T00:00:04.000Z',
                },
              ],
              status: 'failed',
              title: 'Inspector notes',
              updatedAt: '2026-06-17T00:00:04.000Z',
            },
          ],
        },
        artifactRevisionContent: getArtifactRevisionContent,
        conversationInspectorItem: {
          item: {
            kind: 'artifact',
            segment: {
              kind: 'artifact',
              id: 'segment-artifact-inspector',
              order: 2,
              artifactId: 'artifact-inspector',
              artifactKind: 'document',
              status: 'failed',
              source: 'assistant',
              title: 'Inspector notes',
              revision: artifactRevision({
                artifactId: 'artifact-inspector',
                revisionId: 'revision-failed',
                kind: 'document',
                status: 'failed',
                sourceRunId: 'run-inspector',
                title: 'Failed notes',
                contentRef: 'evidence-artifact-failed',
              }),
            },
          },
        },
      }),
    )

    expect(await screen.findAllByText('Artifact revision failed')).toHaveLength(2)
    expect(getArtifactRevisionContent).not.toHaveBeenCalled()
  })

  it('renders html artifact revisions in a sandboxed iframe', async () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-html',
        revisionId: 'revision-html',
      },
    })
    renderInspector(
      createTestCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'Generated HTML.',
              id: 'artifact-html',
              kind: 'html',
              revisions: [
                {
                  contentRef: 'evidence-artifact-html',
                  kind: 'html',
                  revisionId: 'revision-html',
                  status: 'ready',
                  title: 'HTML artifact',
                  updatedAt: '2026-06-17T00:00:05.000Z',
                },
              ],
              status: 'ready',
              title: 'HTML artifact',
              updatedAt: '2026-06-17T00:00:05.000Z',
            },
          ],
        },
        artifactRevisionContent: {
          artifactId: 'artifact-html',
          byteLength: 28,
          content: '<main><h1>Preview</h1></main>',
          contentBytes: 28,
          contentType: 'text/html',
          hasMore: false,
          contentHash: validEvidenceContentHash,
          hashAlgorithm: 'blake3',
          kind: 'artifact-content',
          limitBytes: 65_536,
          maxBytes: 65_536,
          offsetBytes: 0,
          redactionState: 'clean',
          refId: 'evidence-artifact-html',
          returnedBytes: 28,
          revisionId: 'revision-html',
          totalBytes: 28,
          truncated: false,
        },
        conversationInspectorItem: {
          item: {
            kind: 'artifact',
            segment: {
              kind: 'artifact',
              id: 'segment-artifact-html',
              order: 2,
              artifactId: 'artifact-html',
              artifactKind: 'html',
              status: 'ready',
              source: 'assistant',
              title: 'HTML artifact',
              revision: artifactRevision({
                artifactId: 'artifact-html',
                revisionId: 'revision-html',
                kind: 'html',
                sourceRunId: 'run-inspector',
                title: 'HTML artifact',
                contentRef: 'evidence-artifact-html',
              }),
            },
          },
        },
      }),
    )

    const iframe = await screen.findByTitle('HTML artifact sandboxed preview')
    expect(iframe).toHaveAttribute('sandbox', '')
    expect(iframe).not.toHaveAttribute('sandbox', expect.stringContaining('allow-same-origin'))
    expect(iframe).toHaveAttribute('referrerpolicy', 'no-referrer')
  })
})
