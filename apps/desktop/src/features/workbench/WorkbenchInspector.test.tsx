import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
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

    expect(await screen.findByText('Inspector notes')).toBeInTheDocument()
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
