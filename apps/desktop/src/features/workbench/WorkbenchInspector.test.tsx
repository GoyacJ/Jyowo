import '@testing-library/jest-dom/vitest'

import { fireEvent, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { uiStore } from '@/shared/state/ui-store'
import { createTestCommandClient } from '@/testing/command-client'
import { commandDetail, permissionState } from '@/testing/conversation-worktree-builders'
import {
  createInspectorQueryClient,
  inspectorTurn,
  renderInspector,
  setupStore,
  validEvidenceContentHash,
  worktreePage,
} from './WorkbenchInspector.test-support'

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
    renderInspector(undefined, undefined, <div>Real project context</div>)
    expect(screen.getByText('Real project context')).toBeInTheDocument()
    expect(screen.queryByText('Workspace context and runtime state.')).not.toBeInTheDocument()
  })

  it('resolves a selected decision pane through the backend command client', async () => {
    const resolvePermission = vi.fn().mockResolvedValue({
      conversationId: 'conversation-inspector',
      requestId: 'request-inspector',
      decision: 'approve',
      optionId: 'option-allow-once',
      status: 'resolved',
    })
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'decision',
        conversationId: 'conversation-inspector',
        requestId: 'request-inspector',
      },
    })
    renderInspector({
      ...createTestCommandClient({
        conversationInspectorItem: {
          item: {
            kind: 'decision',
            decision: permissionState({
              id: 'permission-inspector',
              requestId: 'request-inspector',
              status: 'pending',
            }),
          },
        },
      }),
      resolvePermission,
    })

    fireEvent.click(await screen.findByRole('button', { name: /Approve.*Allow once/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))

    await waitFor(() =>
      expect(resolvePermission).toHaveBeenCalledWith({
        conversationId: 'conversation-inspector',
        requestId: 'request-inspector',
        decision: 'approve',
        optionId: 'option-allow-once',
      }),
    )
  })

  it('resolves a selected tool permission pane through the backend command client', async () => {
    const resolvePermission = vi.fn().mockResolvedValue({
      conversationId: 'conversation-inspector',
      requestId: 'request-tool-inspector',
      decision: 'deny',
      optionId: 'option-deny-once',
      status: 'resolved',
    })
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'tool',
        conversationId: 'conversation-inspector',
        toolUseId: 'tool-use-inspector',
      },
    })
    renderInspector({
      ...createTestCommandClient({
        conversationInspectorItem: {
          item: {
            kind: 'tool',
            attempt: {
              id: 'tool-attempt-inspector',
              order: 0,
              toolUseId: 'tool-use-inspector',
              toolName: 'read_file',
              status: 'waitingPermission',
              permission: permissionState({
                id: 'permission-tool-inspector',
                requestId: 'request-tool-inspector',
                status: 'pending',
                toolUseId: 'tool-use-inspector',
              }),
            },
          },
        },
      }),
      resolvePermission,
    })

    fireEvent.click(await screen.findByRole('button', { name: 'Deny' }))

    await waitFor(() =>
      expect(resolvePermission).toHaveBeenCalledWith({
        conversationId: 'conversation-inspector',
        requestId: 'request-tool-inspector',
        decision: 'deny',
        optionId: 'option-deny-once',
      }),
    )
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
    const getConversationDiffPatch = vi.fn().mockResolvedValue({
      patch:
        'diff --git a/WorkbenchInspector.tsx b/WorkbenchInspector.tsx\n+ render real inspector pane\n',
      contentType: 'text/x-diff; charset=utf-8',
      byteLength: 84,
      contentBytes: 84,
      offsetBytes: 0,
      limitBytes: 65_536,
      totalBytes: 84,
      returnedBytes: 84,
      maxBytes: 65_536,
      truncated: false,
      hasMore: false,
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

      expect(await screen.findByText('Updated inspector UI')).toBeInTheDocument()
      expect(screen.getByText('WorkbenchInspector.tsx')).toBeInTheDocument()
      expect(screen.getByText('+ render real inspector pane')).toBeInTheDocument()
      expect(screen.queryByText('File changes and patch details.')).not.toBeInTheDocument()

      fireEvent.click(screen.getByRole('button', { name: 'Copy full patch' }))

      await waitFor(() =>
        expect(getConversationDiffPatch).toHaveBeenCalledWith({
          conversationId: 'conversation-inspector',
          fullPatchRef: 'evidence-diff-inspector',
        }),
      )
      expect(writeText).toHaveBeenCalledWith(
        'diff --git a/WorkbenchInspector.tsx b/WorkbenchInspector.tsx\n+ render real inspector pane\n',
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
