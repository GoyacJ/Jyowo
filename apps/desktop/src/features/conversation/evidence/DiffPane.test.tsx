import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import { changeSetFile } from '@/testing/conversation-worktree-builders'
import { DiffPane } from './DiffPane'

const originalClipboard = navigator.clipboard
const validEvidenceContentHash = 'd'.repeat(64)

function renderDiffPane(client = createTestCommandClient()) {
  return render(
    <CommandClientProvider client={client}>
      <DiffPane
        conversationId="conversation-1"
        files={[
          changeSetFile({
            path: 'src/App.tsx',
            addedLines: 1,
            removedLines: 0,
            preview: '+preview line',
            fullPatchRef: 'diff-ref-1',
          }),
        ]}
      />
    </CommandClientProvider>,
  )
}

describe('DiffPane', () => {
  afterEach(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: originalClipboard,
    })
  })

  it('copies full patch bytes through the diff patch command', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    const getConversationDiffPatch = vi.fn().mockResolvedValue({
      refId: 'diff-ref-1',
      kind: 'diff-patch',
      patch: 'diff --git a/src/App.tsx b/src/App.tsx\n+patched content\n',
      contentType: 'text/x-diff; charset=utf-8',
      byteLength: 58,
      contentBytes: 58,
      offsetBytes: 0,
      limitBytes: 65_536,
      totalBytes: 58,
      returnedBytes: 58,
      maxBytes: 65_536,
      truncated: false,
      hasMore: false,
      contentHash: validEvidenceContentHash,
      hashAlgorithm: 'blake3',
      redactionState: 'clean',
    })
    const exportConversationEvidence = vi.fn()

    renderDiffPane({
      ...createTestCommandClient({
        conversationDiffPatch: getConversationDiffPatch,
      }),
      exportConversationEvidence,
    })

    const copyButton = screen.getByRole('button', { name: 'Copy full patch' })
    expect(copyButton).toHaveClass('focus-visible:ring-2')

    fireEvent.click(copyButton)

    await waitFor(() =>
      expect(getConversationDiffPatch).toHaveBeenCalledWith({
        conversationId: 'conversation-1',
        fullPatchRef: 'diff-ref-1',
      }),
    )
    expect(writeText).toHaveBeenCalledWith(
      'diff --git a/src/App.tsx b/src/App.tsx\n+patched content\n',
    )
    expect(exportConversationEvidence).not.toHaveBeenCalled()
  })

  it('shows an error when patch page fetch fails', async () => {
    const getConversationDiffPatch = vi.fn().mockRejectedValue(new Error('fetch failed'))

    renderDiffPane({
      ...createTestCommandClient({
        conversationDiffPatch: getConversationDiffPatch,
      }),
    })

    fireEvent.click(screen.getByRole('button', { name: 'Load patch page' }))

    await waitFor(() =>
      expect(getConversationDiffPatch).toHaveBeenCalledWith({
        conversationId: 'conversation-1',
        fullPatchRef: 'diff-ref-1',
      }),
    )
    expect(await screen.findByText('Failed to load patch page')).toBeInTheDocument()
  })

  it('uses semantic file status token classes instead of hardcoded product colors', () => {
    renderDiffPane()

    expect(screen.getByText('modified')).toHaveClass('bg-warning/10', 'text-warning')
  })
})
