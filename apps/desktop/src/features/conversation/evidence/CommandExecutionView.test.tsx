import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import { commandDetail } from '@/testing/conversation-worktree-builders'
import { CommandExecutionView } from './CommandExecutionView'

const originalClipboard = navigator.clipboard

function renderCommandExecutionView({
  client = createTestCommandClient(),
  fullOutputRef,
}: {
  client?: CommandClient
  fullOutputRef?: string
} = {}) {
  return render(
    <CommandClientProvider client={client}>
      <CommandExecutionView
        command={commandDetail({
          command: 'pnpm check:desktop',
          stdoutPreview: 'desktop passed',
          stderrPreview: 'warning only',
          exitCode: 0,
          durationMs: 42,
          fullOutputRef,
        })}
        conversationId="conversation-1"
      />
    </CommandClientProvider>,
  )
}

describe('CommandExecutionView', () => {
  afterEach(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: originalClipboard,
    })
  })

  it('copies only the command from the copy command action', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })

    renderCommandExecutionView()

    fireEvent.click(screen.getByRole('button', { name: 'Copy command' }))

    await waitFor(() => expect(writeText).toHaveBeenCalledWith('pnpm check:desktop'))
  })

  it('copies only visible output from the copy output action', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })

    renderCommandExecutionView()

    fireEvent.click(screen.getByRole('button', { name: 'Copy output' }))

    await waitFor(() => expect(writeText).toHaveBeenCalledWith('desktop passed\nwarning only'))
    expect(writeText).not.toHaveBeenCalledWith(expect.stringContaining('$ pnpm check:desktop'))
    expect(writeText).not.toHaveBeenCalledWith(expect.stringContaining('exit 0'))
  })

  it('shows an error when output page fetch fails', async () => {
    const getConversationCommandOutput = vi.fn().mockRejectedValue(new Error('fetch failed'))

    renderCommandExecutionView({
      client: {
        ...createTestCommandClient(),
        getConversationCommandOutput,
      },
      fullOutputRef: 'output-ref-1',
    })

    fireEvent.click(screen.getByRole('button', { name: 'Load output page' }))

    await waitFor(() => expect(getConversationCommandOutput).toHaveBeenCalled())
    expect(await screen.findByText('Failed to load output page')).toBeInTheDocument()
  })
})
