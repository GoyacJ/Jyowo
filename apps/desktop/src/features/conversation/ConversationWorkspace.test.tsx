import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { createMockCommandClient, createRejectedCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { ConversationWorkspace } from './ConversationWorkspace'

function renderConversationWorkspace(
  commandClient: CommandClient = createMockCommandClient(),
  conversationId?: string,
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<ConversationWorkspace conversationId={conversationId} />, { wrapper: Wrapper })
}

describe('ConversationWorkspace', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    uiStore.getState().clearActiveRun()
  })

  it('renders the command-backed conversation surface', async () => {
    const scrollIntoView = vi.spyOn(Element.prototype, 'scrollIntoView')

    renderConversationWorkspace()

    expect(
      await screen.findByRole('heading', { name: 'Build the desktop foundation' }),
    ).toBeInTheDocument()
    expect(
      screen.getByPlaceholderText('Ask Jyowo anything about this project...'),
    ).toBeInTheDocument()
    expect(screen.queryByText('Plan')).not.toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Work progress' })).toBeInTheDocument()
    expect(screen.getByText('Desktop foundation created')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Run app' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Review request' })).toBeInTheDocument()
    expect(screen.getByText('Verification notes')).toBeInTheDocument()
    expect(screen.queryByText('Loading artifact preview.')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Inspect' }))
    expect(screen.getByText('Loading artifact preview.')).toBeInTheDocument()

    fireEvent.click(screen.getAllByRole('button', { name: 'Show source message' })[0] as Element)
    expect(scrollIntoView).toHaveBeenCalled()
    expect(screen.queryByText(/Tauri command boundary/)).not.toBeInTheDocument()
  })

  it('shows loading, empty, and error states for command-backed data', async () => {
    const { unmount } = renderConversationWorkspace(createMockCommandClient({ delayMs: 10 }))

    expect(screen.getByText('Loading conversation')).toBeInTheDocument()
    expect(
      await screen.findByRole('heading', { name: 'Build the desktop foundation' }),
    ).toBeInTheDocument()

    unmount()

    renderConversationWorkspace(
      createMockCommandClient({
        conversations: { conversations: [] },
      }),
    )

    expect(await screen.findByText('No conversations yet')).toBeInTheDocument()

    unmount()

    renderConversationWorkspace(createRejectedCommandClient(new Error('IPC unavailable')))

    expect(await screen.findByText('IPC unavailable')).toBeInTheDocument()
  })

  it('renders object-shaped command errors through their message', async () => {
    renderConversationWorkspace(
      createRejectedCommandClient({
        code: 'RUNTIME_OPERATION_FAILED',
        message: 'conversation read failed',
      }),
    )

    expect(await screen.findByText('conversation read failed')).toBeInTheDocument()
    expect(screen.queryByText('[object Object]')).not.toBeInTheDocument()
  })

  it('mounts conversation work once on the latest assistant message', async () => {
    const scrollIntoView = vi.spyOn(Element.prototype, 'scrollIntoView')

    renderConversationWorkspace(
      createMockCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Run app',
              description: 'Tauri + React + TypeScript with Vite',
              id: 'artifact-desktop-foundation',
              kind: 'app',
              preview:
                'Tauri command boundary, React renderer shell, and Vite development scripts.',
              sourceMessageId: 'message-004',
              sourceRunId: 'run-001',
              status: 'ready',
              title: 'Desktop foundation created',
            },
          ],
        },
        conversation: {
          conversation: {
            id: 'conversation-001',
            messages: [
              {
                author: 'user',
                body: 'Start',
                id: 'message-001',
                timestamp: '2026-06-17T00:00:00.000Z',
              },
              {
                author: 'assistant',
                body: 'First assistant turn',
                id: 'message-002',
                timestamp: '2026-06-17T00:00:01.000Z',
              },
              {
                author: 'user',
                body: 'Continue',
                id: 'message-003',
                timestamp: '2026-06-17T00:00:02.000Z',
              },
              {
                author: 'assistant',
                body: 'Second assistant turn\n\nPrepare shell and add verification tests.',
                id: 'message-004',
                timestamp: '2026-06-17T00:00:03.000Z',
              },
            ],
            title: 'Build the desktop foundation',
            updatedAt: '2026-06-17T00:00:03.000Z',
          },
        },
      }),
    )

    expect(await screen.findByText('Second assistant turn')).toBeInTheDocument()
    expect(screen.queryByText('Plan')).not.toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Run app' })).toHaveLength(1)

    fireEvent.click(screen.getAllByRole('button', { name: 'Show source message' })[0] as Element)

    expect(scrollIntoView).toHaveBeenCalled()
    expect(document.activeElement).toHaveAttribute('id', 'conversation-message-message-004')
  })

  it('submits composer text through start_run without putting it in the query cache', async () => {
    const commandClient = createMockCommandClient()
    let listConversationsCallCount = 0
    const startRunCalls: Array<Parameters<CommandClient['startRun']>[0]> = []
    const trackedClient = {
      ...commandClient,
      listConversations: async () => {
        listConversationsCallCount += 1
        return commandClient.listConversations()
      },
      startRun: async (request: Parameters<CommandClient['startRun']>[0]) => {
        startRunCalls.push(request)
        return commandClient.startRun(request)
      },
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient)

    fireEvent.change(
      await screen.findByPlaceholderText('Ask Jyowo anything about this project...'),
      {
        target: { value: 'Continue the Tauri setup' },
      },
    )
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    expect(screen.getByText('Continue the Tauri setup')).toBeInTheDocument()
    await waitFor(() => {
      expect(startRunCalls).toEqual([
        {
          conversationId: 'conversation-001',
          prompt: 'Continue the Tauri setup',
        },
      ])
      expect(uiStore.getState().activeRunConversationId).toBe('conversation-001')
      expect(uiStore.getState().activeRunId).toBe('run-001')
      expect(listConversationsCallCount).toBeGreaterThanOrEqual(2)
    })
  })

  it('loads the selected conversation id from props', async () => {
    const commandClient = createMockCommandClient()
    const getConversationCalls: string[] = []
    const trackedClient = {
      ...commandClient,
      getConversation: async (conversationId: string) => {
        getConversationCalls.push(conversationId)
        return commandClient.getConversation(conversationId)
      },
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient, 'conversation-002')

    await screen.findByRole('heading', { name: 'Build the desktop foundation' })

    expect(getConversationCalls).toEqual(['conversation-002'])
  })

  it('keeps the loaded conversation visible when start_run fails', async () => {
    const commandClient = createMockCommandClient()
    const failingClient = {
      ...commandClient,
      startRun: () => Promise.reject(new Error('Runtime unavailable')),
    } satisfies CommandClient

    renderConversationWorkspace(failingClient)

    fireEvent.change(
      await screen.findByPlaceholderText('Ask Jyowo anything about this project...'),
      {
        target: { value: 'Continue the Tauri setup' },
      },
    )
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    expect(
      screen.getByRole('heading', { name: 'Build the desktop foundation' }),
    ).toBeInTheDocument()
    expect(screen.getByText('Continue the Tauri setup')).toBeInTheDocument()
    expect(await screen.findByText('Runtime unavailable')).toBeInTheDocument()
    expect(screen.queryByText('Conversation unavailable')).not.toBeInTheDocument()
  })

  it('allows repeated user messages with identical text', async () => {
    const commandClient = createMockCommandClient()
    renderConversationWorkspace(commandClient)

    fireEvent.change(
      await screen.findByPlaceholderText('Ask Jyowo anything about this project...'),
      {
        target: { value: 'Continue the Tauri setup' },
      },
    )
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Continue the Tauri setup' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    expect(screen.getAllByText('Continue the Tauri setup')).toHaveLength(2)
  })

  it('merges an optimistic message when the refreshed conversation contains it', async () => {
    const commandClient = createMockCommandClient()
    let includePersistedMessage = false
    let getConversationCallCount = 0
    const trackedClient = {
      ...commandClient,
      getConversation: async () => {
        getConversationCallCount += 1
        return {
          conversation: {
            id: 'conversation-001',
            messages: includePersistedMessage
              ? [
                  {
                    author: 'user',
                    body: 'Persist this turn',
                    id: 'message-persisted-user',
                    timestamp: '2026-06-17T00:00:00.000Z',
                  },
                ]
              : [],
            title: 'Build the desktop foundation',
            updatedAt: includePersistedMessage
              ? '2026-06-17T00:00:01.000Z'
              : '2026-06-17T00:00:00.000Z',
          },
        }
      },
      startRun: async (request: Parameters<CommandClient['startRun']>[0]) => {
        includePersistedMessage = true
        return commandClient.startRun(request)
      },
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient)

    fireEvent.change(
      await screen.findByPlaceholderText('Ask Jyowo anything about this project...'),
      {
        target: { value: 'Persist this turn' },
      },
    )
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => {
      expect(getConversationCallCount).toBeGreaterThanOrEqual(2)
      expect(screen.getAllByText('Persist this turn')).toHaveLength(1)
    })
  })

  it('shows the ask, work, review, and continue path after submit', async () => {
    renderConversationWorkspace()

    fireEvent.change(
      await screen.findByPlaceholderText('Ask Jyowo anything about this project...'),
      {
        target: { value: 'Build the local app foundation' },
      },
    )
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    expect(screen.getByText('Build the local app foundation')).toBeInTheDocument()
    expect(screen.queryByText('Plan')).not.toBeInTheDocument()
    expect(screen.getByText('Working: run')).toBeInTheDocument()
    expect(screen.getByText('Desktop foundation created')).toBeInTheDocument()
    expect(screen.getByText('Review generated foundation')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Continue' })).toBeInTheDocument()
  })
})
