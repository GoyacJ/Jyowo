import '@testing-library/jest-dom/vitest'

import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import { commandDetail } from '@/testing/conversation-worktree-builders'
import { CommandExecutionView } from './CommandExecutionView'

const originalClipboard = navigator.clipboard

function renderCommandExecutionView({
  allowFullOutputFetch,
  client = createTestCommandClient(),
  fullOutputRef,
}: {
  allowFullOutputFetch?: boolean
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
        allowFullOutputFetch={allowFullOutputFetch}
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
      allowFullOutputFetch: true,
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

  it('does not render or copy withheld command previews', () => {
    render(
      <CommandClientProvider client={createTestCommandClient()}>
        <CommandExecutionView
          command={commandDetail({
            command: 'cat secret.txt',
            stdoutPreview: 'SECRET_TOKEN',
            redactionState: 'withheld',
          })}
          conversationId="conversation-1"
        />
      </CommandClientProvider>,
    )

    expect(screen.getByText('Output withheld')).toBeInTheDocument()
    expect(screen.queryByText('SECRET_TOKEN')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Copy output' })).not.toBeInTheDocument()
  })

  it('does not store or render withheld command output pages', async () => {
    const getConversationCommandOutput = vi.fn().mockResolvedValue({
      refId: 'output-ref-1',
      kind: 'command-output',
      output: 'SECRET_PAGE',
      contentType: 'text/plain; charset=utf-8',
      byteLength: 11,
      contentBytes: 11,
      offsetBytes: 0,
      limitBytes: 65_536,
      totalBytes: 11,
      returnedBytes: 11,
      maxBytes: 65_536,
      truncated: true,
      hasMore: false,
      nextCursor: '65536',
      contentHash: 'd'.repeat(64),
      hashAlgorithm: 'blake3',
      redactionState: 'withheld',
    })

    renderCommandExecutionView({
      allowFullOutputFetch: true,
      client: {
        ...createTestCommandClient(),
        getConversationCommandOutput,
      },
      fullOutputRef: 'output-ref-1',
    })

    fireEvent.click(screen.getByRole('button', { name: 'Load output page' }))

    await waitFor(() => expect(getConversationCommandOutput).toHaveBeenCalled())
    expect(await screen.findByText('Failed to load output page')).toBeInTheDocument()
    expect(screen.queryByText('SECRET_PAGE')).not.toBeInTheDocument()
  })

  it('hides previously fetched output when the command becomes withheld', async () => {
    const getConversationCommandOutput = vi.fn().mockResolvedValue({
      refId: 'output-ref-1',
      kind: 'command-output',
      output: 'SAFE_PAGE',
      contentType: 'text/plain; charset=utf-8',
      byteLength: 9,
      contentBytes: 9,
      offsetBytes: 0,
      limitBytes: 65_536,
      totalBytes: 9,
      returnedBytes: 9,
      maxBytes: 65_536,
      truncated: true,
      hasMore: false,
      nextCursor: '65536',
      contentHash: 'd'.repeat(64),
      hashAlgorithm: 'blake3',
      redactionState: 'clean',
    })
    const client = {
      ...createTestCommandClient(),
      getConversationCommandOutput,
    }
    const { rerender } = render(
      <CommandClientProvider client={client}>
        <CommandExecutionView
          allowFullOutputFetch={true}
          command={commandDetail({
            command: 'pnpm check:desktop',
            fullOutputRef: 'output-ref-1',
          })}
          conversationId="conversation-1"
        />
      </CommandClientProvider>,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Load output page' }))
    expect(await screen.findByText('SAFE_PAGE')).toBeInTheDocument()
    expect(screen.getByText('Output page truncated')).toBeInTheDocument()

    rerender(
      <CommandClientProvider client={client}>
        <CommandExecutionView
          allowFullOutputFetch={true}
          command={commandDetail({
            command: 'pnpm check:desktop',
            fullOutputRef: 'output-ref-1',
            redactionState: 'withheld',
          })}
          conversationId="conversation-1"
        />
      </CommandClientProvider>,
    )

    expect(screen.getByText('Output withheld')).toBeInTheDocument()
    expect(screen.queryByText('SAFE_PAGE')).not.toBeInTheDocument()
    expect(screen.queryByText('Output page truncated')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Copy output' })).not.toBeInTheDocument()
  })

  it('does not render previously fetched output during the ref-change commit', async () => {
    const getConversationCommandOutput = vi.fn().mockResolvedValue({
      refId: 'old-output-ref',
      kind: 'command-output',
      output: 'SAFE_PAGE',
      contentType: 'text/plain; charset=utf-8',
      byteLength: 9,
      contentBytes: 9,
      offsetBytes: 0,
      limitBytes: 65_536,
      totalBytes: 9,
      returnedBytes: 9,
      maxBytes: 65_536,
      truncated: false,
      hasMore: false,
      contentHash: 'd'.repeat(64),
      hashAlgorithm: 'blake3',
      redactionState: 'clean',
    })
    const client = {
      ...createTestCommandClient(),
      getConversationCommandOutput,
    }
    const container = document.createElement('div')
    document.body.append(container)
    const root = createRoot(container)

    try {
      await act(async () => {
        root.render(
          <CommandClientProvider client={client}>
            <CommandExecutionView
              allowFullOutputFetch={true}
              command={commandDetail({
                command: 'pnpm check:desktop',
                stdoutPreview: 'old preview',
                fullOutputRef: 'old-output-ref',
              })}
              conversationId="conversation-1"
            />
          </CommandClientProvider>,
        )
      })

      fireEvent.click(within(container).getByRole('button', { name: 'Load output page' }))
      expect(await within(container).findByText('SAFE_PAGE')).toBeInTheDocument()

      flushSync(() => {
        root.render(
          <CommandClientProvider client={client}>
            <CommandExecutionView
              allowFullOutputFetch={true}
              command={commandDetail({
                command: 'pnpm check:desktop',
                stdoutPreview: 'new preview',
                fullOutputRef: 'new-output-ref',
              })}
              conversationId="conversation-1"
            />
          </CommandClientProvider>,
        )
      })

      expect(within(container).getByText('new preview')).toBeInTheDocument()
      expect(within(container).queryByText('SAFE_PAGE')).not.toBeInTheDocument()
    } finally {
      await act(async () => {
        root.unmount()
      })
      container.remove()
    }
  })

  it('ignores an in-flight full output response after the command ref changes', async () => {
    let resolveOutput!: (
      value: Awaited<ReturnType<CommandClient['getConversationCommandOutput']>>,
    ) => void
    const outputPromise = new Promise<
      Awaited<ReturnType<CommandClient['getConversationCommandOutput']>>
    >((resolve) => {
      resolveOutput = resolve
    })
    const getConversationCommandOutput = vi.fn().mockReturnValue(outputPromise)
    const client = {
      ...createTestCommandClient(),
      getConversationCommandOutput,
    }
    const { rerender } = render(
      <CommandClientProvider client={client}>
        <CommandExecutionView
          allowFullOutputFetch={true}
          command={commandDetail({
            command: 'pnpm check:desktop',
            stdoutPreview: 'old preview',
            fullOutputRef: 'old-output-ref',
          })}
          conversationId="conversation-1"
        />
      </CommandClientProvider>,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Load output page' }))
    await waitFor(() =>
      expect(getConversationCommandOutput).toHaveBeenCalledWith({
        conversationId: 'conversation-1',
        fullOutputRef: 'old-output-ref',
      }),
    )

    rerender(
      <CommandClientProvider client={client}>
        <CommandExecutionView
          allowFullOutputFetch={true}
          command={commandDetail({
            command: 'pnpm check:desktop',
            stdoutPreview: 'new preview',
            fullOutputRef: 'new-output-ref',
          })}
          conversationId="conversation-1"
        />
      </CommandClientProvider>,
    )

    await act(async () => {
      resolveOutput({
        refId: 'old-output-ref',
        kind: 'command-output',
        output: 'OLD_PAGE',
        contentType: 'text/plain; charset=utf-8',
        byteLength: 8,
        contentBytes: 8,
        offsetBytes: 0,
        limitBytes: 65_536,
        totalBytes: 8,
        returnedBytes: 8,
        maxBytes: 65_536,
        truncated: false,
        hasMore: false,
        contentHash: 'd'.repeat(64),
        hashAlgorithm: 'blake3',
        redactionState: 'clean',
      })
      await outputPromise
    })

    expect(screen.getByText('new preview')).toBeInTheDocument()
    expect(screen.queryByText('OLD_PAGE')).not.toBeInTheDocument()
  })

  it('keeps the full output fetch path unavailable unless explicitly allowed', () => {
    renderCommandExecutionView({
      fullOutputRef: 'output-ref-1',
    })

    expect(screen.queryByRole('button', { name: 'Load output page' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy command' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy output' })).toBeInTheDocument()
  })

  it('keeps the full output fetch path unavailable in timeline density', () => {
    renderCommandExecutionView({
      allowFullOutputFetch: false,
      fullOutputRef: 'output-ref-1',
    })

    expect(screen.queryByRole('button', { name: 'Load output page' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy command' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy output' })).toBeInTheDocument()
  })
})
