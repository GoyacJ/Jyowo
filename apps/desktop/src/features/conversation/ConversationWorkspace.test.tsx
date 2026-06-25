import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { uiStore } from '@/shared/state/ui-store'
import type {
  CommandClient,
  ConversationEventBatchPayload,
  ConversationTurn,
  ModelProviderCatalogResponse,
  PageConversationWorktreeResponse,
  StartRunResponse,
} from '@/shared/tauri/commands'
import { createMockCommandClient, createRejectedCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { ConversationWorkspace } from './ConversationWorkspace'

type ModelCatalogEntry = ModelProviderCatalogResponse['providers'][number]['models'][number]

const timestamp = '2026-06-17T00:00:00.000Z'

function cursor(conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
}

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

  return render(<ConversationWorkspace conversationId={conversationId} />, {
    wrapper: Wrapper,
  })
}

const openAiModelDescriptor: ModelCatalogEntry = {
  protocol: 'responses',
  conversationCapability: {
    inputModalities: ['text'],
    outputModalities: ['text'],
    contextWindow: 128000,
    maxOutputTokens: 16384,
    streaming: true,
    toolCalling: true,
    reasoning: false,
    promptCache: false,
    structuredOutput: true,
  },
  contextWindow: 128000,
  displayName: 'GPT-5.4 mini',
  lifecycle: { kind: 'stable' },
  maxOutputTokens: 16384,
  modelId: 'gpt-5.4-mini',
  runtimeStatus: { kind: 'runnable' },
}

function flushAnimationFrames(frameCount = 2) {
  return new Promise<void>((resolve) => {
    const step = (remaining: number) => {
      if (remaining <= 0) {
        resolve()
        return
      }

      requestAnimationFrame(() => step(remaining - 1))
    }

    step(frameCount)
  })
}

describe('ConversationWorkspace', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    uiStore.getState().clearActiveRun()
  })

  it('renders the conversation body from worktree turns', async () => {
    renderConversationWorkspace()

    expect(
      await screen.findByRole('heading', { name: 'Build the desktop foundation' }),
    ).toBeInTheDocument()
    expect(screen.getByText('Restore the product shell')).toBeInTheDocument()
    expect(screen.getByText('I am checking the workspace state.')).toBeInTheDocument()
    expect(screen.getAllByText('Tools').length).toBeGreaterThan(0)
    expect(
      screen.queryByText('Tool error withheld from conversation timeline.'),
    ).not.toBeInTheDocument()
  })

  it('loads reference candidates for the selected conversation', async () => {
    const commandClient = createMockCommandClient({
      conversationWorktreePage: pageWithTurn('complete'),
    })
    const listReferenceCandidates = vi.fn(commandClient.listReferenceCandidates)
    const trackedClient = {
      ...commandClient,
      listReferenceCandidates,
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient, 'conversation-001')

    fireEvent.click(await screen.findByRole('button', { name: 'Reference project object' }))

    await waitFor(() => {
      expect(listReferenceCandidates).toHaveBeenCalledWith({ conversationId: 'conversation-001' })
    })
  })

  it('shows loading, empty, and command error states', async () => {
    const { unmount } = renderConversationWorkspace(createMockCommandClient({ delayMs: 10 }))

    expect(screen.getByText('Loading conversation...')).toBeInTheDocument()
    expect(
      await screen.findByRole('heading', { name: 'Build the desktop foundation' }),
    ).toBeInTheDocument()

    unmount()

    renderConversationWorkspace(
      createMockCommandClient({ conversations: { conversations: [] } }),
      'missing-conversation',
    )
    expect(await screen.findByRole('heading', { name: 'New conversation' })).toBeInTheDocument()

    unmount()

    renderConversationWorkspace(createRejectedCommandClient(new Error('IPC unavailable')))
    expect(await screen.findByText('IPC unavailable')).toBeInTheDocument()
  })

  it('leaves the model selector empty when no model is selected and no default exists', async () => {
    renderConversationWorkspace(
      createMockCommandClient({
        conversation: {
          conversation: {
            id: 'conversation-001',
            messages: [],
            modelConfigId: null,
            title: 'No selected model',
            updatedAt: timestamp,
          },
        },
        providerSettingsList: {
          defaultConfigId: null,
          configs: [
            {
              protocol: 'responses',
              displayName: 'OpenAI Work',
              hasApiKey: true,
              id: 'openai-work',
              isDefault: false,
              modelDescriptor: openAiModelDescriptor,
              modelId: 'gpt-5.4-mini',
              providerId: 'openai',
            },
          ],
        },
      }),
    )

    const modelSelector = (await screen.findByLabelText('Model')) as HTMLSelectElement

    expect(modelSelector.value).toBe('')
  })

  it('renders nested permission state and sends decisions through commands', async () => {
    const commandClient = createMockCommandClient()
    const resolvePermission = vi.fn(commandClient.resolvePermission)
    const trackedClient = {
      ...commandClient,
      resolvePermission,
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient)

    fireEvent.click(await screen.findByRole('button', { name: 'Approve' }))

    await waitFor(() =>
      expect(resolvePermission).toHaveBeenCalledWith({
        conversationId: 'conversation-001',
        requestId: '01HZ0000000000000000000002',
        decision: 'approve',
      }),
    )
  })

  it('submits an optimistic user turn and passes clientMessageId into startRun', async () => {
    const commandClient = createMockCommandClient({
      conversationWorktreePage: pageWithTurn('complete'),
    })
    const startRunCalls: Array<Parameters<CommandClient['startRun']>[0]> = []
    let resolveStartRun: ((response: StartRunResponse) => void) | undefined
    const trackedClient = {
      ...commandClient,
      startRun: (request: Parameters<CommandClient['startRun']>[0]) => {
        startRunCalls.push(request)
        return new Promise<StartRunResponse>((resolve) => {
          resolveStartRun = resolve
        })
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

    expect(await screen.findByText('Continue the Tauri setup')).toBeInTheDocument()
    await waitFor(() => {
      expect(startRunCalls).toEqual([
        expect.objectContaining({
          attachments: [],
          conversationId: 'conversation-001',
          contextReferences: [],
          prompt: 'Continue the Tauri setup',
          clientMessageId: expect.any(String),
        }),
      ])
    })

    resolveStartRun?.({ runId: 'run-001', status: 'started' })
  })

  it('clears active run state when a terminal event triggers a completed worktree refetch', async () => {
    let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
    let terminalEventReceived = false
    const pageConversationWorktree = vi.fn(async () => {
      return pageWithTurn(terminalEventReceived ? 'complete' : 'running')
    })
    const commandClient = createMockCommandClient()
    const trackedClient = {
      ...commandClient,
      listenConversationEventBatches: async (callback) => {
        listener = callback
        return () => undefined
      },
      pageConversationWorktree,
      subscribeConversationEvents: async () => ({
        subscriptionId: 'subscription-001',
        conversationId: 'conversation-001',
        replayEvents: [],
        gap: false,
      }),
    } satisfies CommandClient

    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-001',
        runId: 'run-001',
      })
    })

    renderConversationWorkspace(trackedClient, 'conversation-001')

    await waitFor(() => {
      expect(uiStore.getState().activeRunId).toBe('run-001')
      expect(listener).toBeDefined()
    })

    terminalEventReceived = true
    listener?.({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      events: [
        {
          id: 'evt-ended',
          conversationSequence: 2,
          payload: { reason: 'completed' },
          runId: 'run-001',
          sequence: 2,
          source: 'engine',
          timestamp,
          type: 'run.ended',
          visibility: 'public',
        },
      ],
      cursor: cursor(2),
      gap: false,
      phase: 'live',
    })

    await act(async () => {
      await flushAnimationFrames()
    })

    await waitFor(() => {
      expect(pageConversationWorktree.mock.calls.length).toBeGreaterThan(1)
      expect(uiStore.getState().activeRunId).toBeUndefined()
    })
  })

  it('cancels the current active run from the composer', async () => {
    const commandClient = createMockCommandClient({
      conversationWorktreePage: pageWithTurn('running'),
    })
    const cancelRun = vi.fn(commandClient.cancelRun)
    const trackedClient = {
      ...commandClient,
      cancelRun,
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient, 'conversation-001')

    fireEvent.click(await screen.findByRole('button', { name: 'Cancel run' }))

    await waitFor(() => expect(cancelRun).toHaveBeenCalledWith('run-001'))
    expect(screen.getByPlaceholderText('Ask Jyowo anything about this project...')).toBeDisabled()
  })

  it('keeps the loaded conversation visible and recoverable when startRun fails', async () => {
    const commandClient = createMockCommandClient({
      conversationWorktreePage: pageWithTurn('complete'),
    })
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
    expect(await screen.findAllByText('Runtime unavailable')).not.toHaveLength(0)
    expect(screen.getByDisplayValue('Continue the Tauri setup')).toBeInTheDocument()
    expect(screen.queryByText('Conversation unavailable')).not.toBeInTheDocument()
  })
})

function pageWithTurn(status: 'running' | 'complete'): PageConversationWorktreeResponse {
  return {
    turns: [turn(status)],
    pageCursor: { turnId: 'turn:user-message-001', position: 0 },
    eventCursor: cursor(1),
    hasMoreBefore: false,
    hasMoreAfter: false,
    gap: false,
  }
}

function turn(status: 'running' | 'complete'): ConversationTurn {
  return {
    id: 'turn:user-message-001',
    conversationId: 'conversation-001',
    position: 0,
    user: {
      id: 'user:user-message-001',
      messageId: 'user-message-001',
      body: 'Finish the run',
      timestamp,
    },
    assistant: {
      id: 'assistant:run-001',
      runId: 'run-001',
      status,
      segments:
        status === 'running'
          ? []
          : [
              {
                kind: 'text',
                id: 'segment:text:assistant-message-001',
                order: 0,
                messageId: 'assistant-message-001',
                body: 'Finished.',
              },
            ],
    },
  }
}
