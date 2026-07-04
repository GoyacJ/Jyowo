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
    const getConversationDiffPatch = vi.fn().mockResolvedValue({
      patch: 'diff --git a/WorkbenchInspector.tsx b/WorkbenchInspector.tsx\n+full patch content\n',
      contentType: 'text/x-diff; charset=utf-8',
      byteLength: 82,
      truncated: false,
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

      expect(await screen.findByText('Updated inspector UI')).toBeInTheDocument()
      expect(screen.getByText('WorkbenchInspector.tsx')).toBeInTheDocument()
      expect(screen.getByText('+ render real inspector pane')).toBeInTheDocument()
      expect(screen.queryByText('File changes and patch details.')).not.toBeInTheDocument()

      fireEvent.click(screen.getByRole('button', { name: 'Copy' }))

      await waitFor(() =>
        expect(getConversationDiffPatch).toHaveBeenCalledWith({
          conversationId: 'conversation-inspector',
          fullPatchRef: 'evidence-diff-inspector',
        }),
      )
      expect(writeText).toHaveBeenCalledWith(
        'diff --git a/WorkbenchInspector.tsx b/WorkbenchInspector.tsx\n+full patch content\n',
      )
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
          truncated: false,
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
      pageConversationWorktree: vi.fn().mockRejectedValue(new Error('offline')),
    })

    expect(await screen.findByText('Inspector data failed to load')).toBeInTheDocument()
  })

  it('resolves selected evidence from the already loaded worktree projection cache', async () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'command',
        conversationId: 'conversation-inspector',
        fullOutputRef: 'evidence-command-inspector',
      },
    })
    const pageConversationWorktree = vi.fn().mockRejectedValue(new Error('should not refetch'))
    const queryClient = createInspectorQueryClient()
    queryClient.setQueryData(
      ['conversation-worktree', 'workspace-fixture', 'conversation-inspector'],
      worktreePage([inspectorTurn()]),
    )

    renderInspector(
      {
        ...createTestCommandClient(),
        pageConversationWorktree,
      },
      queryClient,
    )

    expect(await screen.findByText('$ pnpm check:desktop')).toBeInTheDocument()
    expect(pageConversationWorktree).not.toHaveBeenCalled()
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
