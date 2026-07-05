import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { UiState } from '@/shared/state/ui-store'
import { uiStore } from '@/shared/state/ui-store'
import type {
  CommandClient,
  ConversationTurn,
  PageConversationWorktreeResponse,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import {
  artifactRevision,
  assistantWork,
  changeSetFile,
  commandDetail,
  diffDetail,
  permissionState,
} from '@/testing/conversation-worktree-builders'
import { WorkbenchInspector } from './WorkbenchInspector'

const validEvidenceContentHash = 'd'.repeat(64)

function setupStore(overrides?: Partial<UiState>) {
  uiStore.setState({
    inspectorOpen: true,
    workbenchSelection: null,
    ...overrides,
  } as Partial<UiState>)
}

function createInspectorQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  })
}

function renderInspector(
  commandClient: CommandClient = createTestCommandClient(),
  queryClient = createInspectorQueryClient(),
) {
  return render(
    <CommandClientProvider client={commandClient}>
      <QueryClientProvider client={queryClient}>
        <WorkbenchInspector />
      </QueryClientProvider>
    </CommandClientProvider>,
  )
}

function worktreePage(turns: ConversationTurn[]): PageConversationWorktreeResponse {
  return {
    turns,
    hasMoreBefore: false,
    hasMoreAfter: false,
    gap: false,
  }
}

function inspectorTurn(): ConversationTurn {
  return {
    id: 'turn-inspector',
    conversationId: 'conversation-inspector',
    position: 0,
    user: {
      id: 'user-inspector',
      messageId: 'message-user-inspector',
      body: 'Inspect this run',
      timestamp: '2026-06-17T00:00:00.000Z',
    },
    assistant: assistantWork({
      id: 'assistant-inspector',
      runId: 'run-inspector',
      status: 'complete',
      segments: [
        {
          kind: 'process',
          id: 'segment-process-inspector',
          order: 0,
          status: 'complete',
          summary: 'Collected execution evidence',
          steps: [
            {
              id: 'step-command-inspector',
              order: 0,
              kind: 'command',
              status: 'complete',
              title: 'Ran desktop checks',
              detail: commandDetail({
                command: 'pnpm check:desktop',
                stdoutPreview: 'desktop checks passed',
                fullOutputRef: 'evidence-command-inspector',
                exitCode: 0,
              }),
            },
            {
              id: 'step-diff-inspector',
              order: 1,
              kind: 'diff',
              status: 'complete',
              title: 'Updated inspector',
              detail: diffDetail({
                id: 'change-set-inspector',
                summary: 'Updated inspector UI',
                files: [
                  changeSetFile({
                    path: 'apps/desktop/src/features/workbench/WorkbenchInspector.tsx',
                    addedLines: 12,
                    removedLines: 2,
                    preview: '+ render real inspector pane',
                    fullPatchRef: 'evidence-diff-inspector',
                  }),
                ],
              }),
            },
          ],
        },
        {
          kind: 'toolGroup',
          id: 'segment-tools-inspector',
          order: 1,
          attempts: [
            {
              id: 'tool-attempt-inspector',
              order: 0,
              toolUseId: 'tool-use-inspector',
              toolName: 'read_file',
              status: 'completed',
              outputSummary: 'Read WorkbenchInspector.tsx',
              durationMs: 23,
              permission: permissionState({
                id: 'permission-inspector',
                requestId: 'request-inspector',
                status: 'approved',
                toolUseId: 'tool-use-inspector',
              }),
            },
          ],
        },
        {
          kind: 'artifact',
          id: 'segment-artifact-inspector',
          order: 2,
          artifactId: 'artifact-inspector',
          title: 'Inspector notes',
          revision: artifactRevision({
            artifactId: 'artifact-inspector',
            revisionId: 'revision-inspector',
            kind: 'document',
            sourceRunId: 'run-inspector',
            title: 'Inspector notes',
            summary: 'Implementation notes',
            contentRef: 'evidence-artifact-inspector',
          }),
        },
      ],
    }),
  }
}

describe('WorkbenchInspector', () => {
  it('renders empty state when no selection', () => {
    setupStore({ inspectorOpen: true, workbenchSelection: null })
    renderInspector()
    expect(screen.getByText('No Selection')).toBeDefined()
  })

  it('renders context pane when context is selected', () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: { kind: 'context' },
    })
    renderInspector()
    expect(screen.getByText('Context')).toBeDefined()
  })

  it('renders the selected decision pane from the worktree projection', async () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'decision',
        conversationId: 'conversation-inspector',
        requestId: 'request-inspector',
      },
    })
    renderInspector(
      createTestCommandClient({
        conversationWorktreePage: worktreePage([inspectorTurn()]),
      }),
    )

    expect(await screen.findByText('workspace file')).toBeInTheDocument()
    expect(screen.getByText('Allow once')).toBeInTheDocument()
    expect(screen.queryByText(/Permission decision for request/)).not.toBeInTheDocument()
  })

  it('renders the selected command pane from the worktree projection', async () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'command',
        conversationId: 'conversation-inspector',
        fullOutputRef: 'evidence-command-inspector',
      },
    })
    renderInspector(
      createTestCommandClient({
        conversationWorktreePage: worktreePage([inspectorTurn()]),
      }),
    )

    expect(await screen.findByText('$ pnpm check:desktop')).toBeInTheDocument()
    expect(screen.getByText('desktop checks passed')).toBeInTheDocument()
    expect(screen.queryByText('Command execution output and details.')).not.toBeInTheDocument()
  })

  it('loads command output as a bounded evidence page', async () => {
    const getConversationCommandOutput = vi.fn().mockResolvedValue({
      refId: 'evidence-command-inspector',
      kind: 'command-output',
      output: 'bounded output page',
      contentType: 'text/plain; charset=utf-8',
      byteLength: 131_072,
      contentBytes: 131_072,
      offsetBytes: 0,
      limitBytes: 65_536,
      totalBytes: 131_072,
      returnedBytes: 65_536,
      maxBytes: 65_536,
      truncated: true,
      hasMore: true,
      nextCursor: '65536',
      contentHash: validEvidenceContentHash,
      hashAlgorithm: 'blake3',
      redactionState: 'clean',
    })
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'command',
        conversationId: 'conversation-inspector',
        fullOutputRef: 'evidence-command-inspector',
      },
    })
    renderInspector(
      createTestCommandClient({
        conversationCommandOutput: getConversationCommandOutput,
        conversationWorktreePage: worktreePage([inspectorTurn()]),
      }),
    )

    fireEvent.click(await screen.findByRole('button', { name: 'Load output page' }))

    await waitFor(() =>
      expect(getConversationCommandOutput).toHaveBeenCalledWith({
        conversationId: 'conversation-inspector',
        fullOutputRef: 'evidence-command-inspector',
      }),
    )
    expect(await screen.findByText('bounded output page')).toBeInTheDocument()
    expect(screen.getByText('Output page loaded')).toBeInTheDocument()
  })

  it('renders the selected tool pane from the worktree projection', async () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'tool',
        conversationId: 'conversation-inspector',
        toolUseId: 'tool-use-inspector',
      },
    })
    renderInspector(
      createTestCommandClient({
        conversationWorktreePage: worktreePage([inspectorTurn()]),
      }),
    )

    expect(await screen.findByText('read_file')).toBeInTheDocument()
    expect(screen.getByText('Read WorkbenchInspector.tsx')).toBeInTheDocument()
    expect(screen.queryByText('Tool invocation details.')).not.toBeInTheDocument()
  })

  it('renders the selected diff pane from the worktree projection', async () => {
    const originalClipboard = navigator.clipboard
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    const exportConversationEvidence = vi.fn().mockResolvedValue({
      byteLength: 82,
      contentType: 'text/x-diff; charset=utf-8',
      exportedAt: '2026-06-17T02:22:00.000Z',
      kind: 'diff-patch',
      path: '.jyowo/runtime/exports/evidence-diff-patch-fixture.diff',
      refId: 'evidence-diff-inspector',
    })
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'diff',
        conversationId: 'conversation-inspector',
        changeSetId: 'change-set-inspector',
      },
    })
    try {
      renderInspector(
        createTestCommandClient({
          conversationEvidenceExport: exportConversationEvidence,
          conversationWorktreePage: worktreePage([inspectorTurn()]),
        }),
      )

      expect(await screen.findByText('Updated inspector UI')).toBeInTheDocument()
      expect(screen.getByText('WorkbenchInspector.tsx')).toBeInTheDocument()
      expect(screen.getByText('+ render real inspector pane')).toBeInTheDocument()
      expect(screen.queryByText('File changes and patch details.')).not.toBeInTheDocument()

      fireEvent.click(screen.getByRole('button', { name: 'Copy' }))

      await waitFor(() =>
        expect(exportConversationEvidence).toHaveBeenCalledWith({
          conversationId: 'conversation-inspector',
          kind: 'diff-patch',
          refId: 'evidence-diff-inspector',
        }),
      )
      expect(writeText).toHaveBeenCalledWith(
        '.jyowo/runtime/exports/evidence-diff-patch-fixture.diff',
      )
    } finally {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      })
    }
  })

  it('loads diff patches as bounded evidence pages without copying truncated content', async () => {
    const originalClipboard = navigator.clipboard
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    const getConversationDiffPatch = vi.fn().mockResolvedValue({
      patch: 'bounded patch page',
      contentType: 'text/x-diff; charset=utf-8',
      byteLength: 131_072,
      contentBytes: 131_072,
      offsetBytes: 0,
      limitBytes: 65_536,
      totalBytes: 131_072,
      returnedBytes: 65_536,
      maxBytes: 65_536,
      truncated: true,
      hasMore: true,
      nextCursor: '65536',
      kind: 'diff-patch',
      refId: 'evidence-diff-inspector',
      contentHash: validEvidenceContentHash,
      hashAlgorithm: 'blake3',
      redactionState: 'clean',
    })
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'diff',
        conversationId: 'conversation-inspector',
        changeSetId: 'change-set-inspector',
      },
    })
    try {
      renderInspector(
        createTestCommandClient({
          conversationDiffPatch: getConversationDiffPatch,
          conversationWorktreePage: worktreePage([inspectorTurn()]),
        }),
      )

      fireEvent.click(await screen.findByRole('button', { name: 'Load patch page' }))

      await waitFor(() =>
        expect(getConversationDiffPatch).toHaveBeenCalledWith({
          conversationId: 'conversation-inspector',
          fullPatchRef: 'evidence-diff-inspector',
        }),
      )
      expect(await screen.findByText('bounded patch page')).toBeInTheDocument()
      expect(screen.getByText('Patch page truncated')).toBeInTheDocument()
      expect(writeText).not.toHaveBeenCalled()
    } finally {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      })
    }
  })

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

  it('keeps non-image media revisions metadata-only without content or image preview fetches', async () => {
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>()
    const getArtifactMediaPreview = vi.fn<CommandClient['getArtifactMediaPreview']>()
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-video',
        revisionId: 'revision-video',
      },
    })
    renderInspector({
      ...createTestCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'Generated video.',
              id: 'artifact-video',
              kind: 'video',
              revisions: [
                {
                  kind: 'video',
                  media: {
                    kind: 'video',
                    mimeType: 'video/mp4',
                    sizeBytes: 2048,
                  },
                  revisionId: 'revision-video',
                  status: 'ready',
                  title: 'Generated video',
                  updatedAt: '2026-06-17T00:00:06.000Z',
                },
              ],
              status: 'ready',
              title: 'Generated video',
              updatedAt: '2026-06-17T00:00:06.000Z',
            },
          ],
        },
        artifactRevisionContent: getArtifactRevisionContent,
        conversationInspectorItem: {
          item: {
            kind: 'artifact',
            segment: {
              kind: 'artifact',
              id: 'segment-artifact-video',
              order: 2,
              artifactId: 'artifact-video',
              artifactKind: 'media',
              status: 'ready',
              source: 'assistant',
              title: 'Generated video',
              revision: artifactRevision({
                artifactId: 'artifact-video',
                revisionId: 'revision-video',
                kind: 'media',
                sourceRunId: 'run-inspector',
                title: 'Generated video',
                media: {
                  kind: 'video',
                  mimeType: 'video/mp4',
                  sizeBytes: 2048,
                },
              }),
            },
          },
        },
      }),
      getArtifactMediaPreview,
    })

    expect(await screen.findByText('Artifact content unavailable')).toBeInTheDocument()
    expect(getArtifactRevisionContent).not.toHaveBeenCalled()
    expect(getArtifactMediaPreview).not.toHaveBeenCalled()
  })

  it('exports artifact content by ref instead of copying loaded content', async () => {
    const originalClipboard = navigator.clipboard
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    const exportConversationEvidence = vi.fn().mockResolvedValue({
      byteLength: 21,
      contentType: 'text/plain; charset=utf-8',
      exportedAt: '2026-06-17T02:22:00.000Z',
      kind: 'artifact-content',
      path: '.jyowo/runtime/exports/evidence-artifact-content-fixture.txt',
      refId: 'evidence-artifact-inspector',
    })
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
          conversationWorktreePage: worktreePage([inspectorTurn()]),
          conversationEvidenceExport: exportConversationEvidence,
          artifactRevisionContent: {
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
          },
        }),
      )

      fireEvent.click(await screen.findByRole('button', { name: 'Export content' }))

      await waitFor(() =>
        expect(exportConversationEvidence).toHaveBeenCalledWith({
          conversationId: 'conversation-inspector',
          kind: 'artifact-content',
          refId: 'evidence-artifact-inspector',
        }),
      )
      expect(writeText).not.toHaveBeenCalled()
      expect(
        await screen.findByText('.jyowo/runtime/exports/evidence-artifact-content-fixture.txt'),
      ).toBeInTheDocument()
    } finally {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      })
    }
  })

  it('shows an error state when selected worktree data fails to load', async () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'command',
        conversationId: 'conversation-inspector',
        fullOutputRef: 'evidence-command-inspector',
      },
    })
    renderInspector({
      ...createTestCommandClient(),
      getConversationInspectorItem: vi.fn().mockRejectedValue(new Error('offline')),
    })

    expect(await screen.findByText('Inspector data failed to load')).toBeInTheDocument()
  })

  it('resolves selected evidence through the backend inspector authority', async () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'command',
        conversationId: 'conversation-inspector',
        fullOutputRef: 'evidence-command-inspector',
      },
    })
    const pageConversationWorktree = vi.fn().mockRejectedValue(new Error('should not refetch'))
    const getConversationInspectorItem = vi.fn().mockResolvedValue({
      item: {
        kind: 'command',
        command: commandDetail({
          command: 'pnpm check:desktop',
          stdoutPreview: 'desktop checks passed',
          fullOutputRef: 'evidence-command-inspector',
          exitCode: 0,
        }),
      },
    })
    const queryClient = createInspectorQueryClient()
    queryClient.setQueryData(
      ['conversation-worktree', 'workspace-fixture', 'conversation-inspector'],
      worktreePage([inspectorTurn()]),
    )

    renderInspector(
      {
        ...createTestCommandClient(),
        getConversationInspectorItem,
        pageConversationWorktree,
      },
      queryClient,
    )

    expect(await screen.findByText('$ pnpm check:desktop')).toBeInTheDocument()
    expect(getConversationInspectorItem).toHaveBeenCalledWith({
      conversationId: 'conversation-inspector',
      selection: {
        kind: 'command',
        fullOutputRef: 'evidence-command-inspector',
      },
    })
    expect(pageConversationWorktree).not.toHaveBeenCalled()
  })

  it('renders a typed empty state when the backend inspector authority misses', async () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'command',
        conversationId: 'conversation-inspector',
        fullOutputRef: 'missing-evidence-ref',
      },
    })
    renderInspector(
      createTestCommandClient({
        conversationInspectorItem: { item: { kind: 'empty' } },
      }),
    )

    expect(await screen.findByText('Selection unavailable')).toBeInTheDocument()
  })

  it('keeps selection when the inspector is closed', () => {
    const selection = {
      kind: 'diff' as const,
      conversationId: 'conversation-inspector',
      changeSetId: 'change-set-inspector',
    }
    setupStore({
      inspectorOpen: true,
      workbenchSelection: selection,
    })
    renderInspector()

    fireEvent.click(screen.getByRole('button', { name: 'Close inspector' }))

    expect(uiStore.getState().inspectorOpen).toBe(false)
    expect(uiStore.getState().workbenchSelection).toEqual(selection)
  })

  it('hides when inspector is closed', () => {
    setupStore({ inspectorOpen: false, workbenchSelection: null })
    const { container } = renderInspector()
    expect(container.innerHTML).toBe('')
  })
})
