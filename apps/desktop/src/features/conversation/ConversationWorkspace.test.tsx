import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { RunEvent } from '@/shared/events/run-event-schema'
import { uiStore } from '@/shared/state/ui-store'
import type {
  CommandClient,
  ConversationEventBatchPayload,
  ModelProviderCatalogResponse,
  StartRunResponse,
} from '@/shared/tauri/commands'
import { createMockCommandClient, createRejectedCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { ConversationWorkspace } from './ConversationWorkspace'

type ModelCatalogEntry = ModelProviderCatalogResponse['providers'][number]['models'][number]

const timestamp = '2026-06-17T00:00:00.000Z'

function cursor(_label: string, conversationSequence = 1) {
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

function runEvent(
  event: Omit<RunEvent, 'conversationSequence'> & { conversationSequence?: number },
): RunEvent {
  return { conversationSequence: event.sequence, ...event } as RunEvent
}

function createStreamingClient(events: RunEvent[]) {
  return createMockCommandClient({
    conversation: {
      conversation: {
        id: 'conversation-001',
        messages: [],
        modelConfigId: null,
        title: 'Streaming conversation',
        updatedAt: timestamp,
      },
    },
    subscribeConversationEvents: {
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      replayEvents: events,
      cursor: events.at(-1)
        ? cursor(events.at(-1)?.id ?? 'evt-stream', events.at(-1)?.conversationSequence ?? 1)
        : undefined,
      gap: false,
    },
  })
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

  it('renders the conversation body from timeline blocks', async () => {
    renderConversationWorkspace()

    expect(
      await screen.findByRole('heading', { name: 'Build the desktop foundation' }),
    ).toBeInTheDocument()
    expect(screen.getByText(/runtime conversation is connected/)).toBeInTheDocument()
    expect(screen.getByText('Desktop foundation created')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Run app' })).toBeInTheDocument()
    expect(screen.queryByRole('region', { name: 'Work progress' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Show source message' })).not.toBeInTheDocument()
  })

  it('loads artifacts and reference candidates for the selected conversation', async () => {
    const commandClient = createMockCommandClient()
    const listArtifacts = vi.fn(commandClient.listArtifacts)
    const listReferenceCandidates = vi.fn(commandClient.listReferenceCandidates)
    const trackedClient = {
      ...commandClient,
      listArtifacts,
      listReferenceCandidates,
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient, 'conversation-001')

    fireEvent.click(await screen.findByRole('button', { name: 'Reference project object' }))

    await waitFor(() => {
      expect(listArtifacts).toHaveBeenCalledWith({ conversationId: 'conversation-001' })
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

    renderConversationWorkspace(createMockCommandClient({ conversations: { conversations: [] } }))
    expect(await screen.findByText('No conversations yet')).toBeInTheDocument()

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
            messages: [
              {
                author: 'user',
                body: 'Start without a selected model',
                id: 'message-001',
                timestamp,
              },
            ],
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

  it('renders replayed streaming and final assistant events without route switching', async () => {
    renderConversationWorkspace(
      createStreamingClient([
        runEvent({
          id: 'evt-run',
          payload: { sessionId: 'conversation-001' },
          runId: 'run-001',
          sequence: 1,
          source: 'engine',
          timestamp,
          type: 'run.started',
          visibility: 'public',
        }),
        runEvent({
          id: 'evt-delta',
          payload: { text: 'Draft answer' },
          runId: 'run-001',
          sequence: 2,
          source: 'assistant',
          timestamp,
          type: 'assistant.delta',
          visibility: 'public',
        }),
        runEvent({
          id: 'evt-completed',
          payload: { messageId: 'message-002', body: 'Final answer' },
          runId: 'run-001',
          sequence: 3,
          source: 'assistant',
          timestamp,
          type: 'assistant.completed',
          visibility: 'public',
        }),
      ]),
    )

    expect(
      await screen.findByRole('heading', { name: 'Streaming conversation' }),
    ).toBeInTheDocument()
    expect(await screen.findByText('Final answer')).toBeInTheDocument()
    expect(screen.queryByText('Draft answer')).not.toBeInTheDocument()
  })

  it('renders permission requests as inline timeline blocks and sends decisions through commands', async () => {
    const commandClient = createStreamingClient([
      runEvent({
        id: 'evt-permission',
        payload: {
          requestId: '01HZ0000000000000000000001',
          operation: 'Install dependencies',
          reason: 'The run requested package installation.',
          target: 'workspace package manager',
          severity: 'high',
          decisionScope: 'current run',
          exposure: 'Can modify package metadata and lockfile.',
          workspaceBoundary: 'workspace://local',
        },
        runId: 'run-001',
        sequence: 1,
        source: 'policy',
        timestamp,
        type: 'permission.requested',
        visibility: 'public',
      }),
    ])
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
        requestId: '01HZ0000000000000000000001',
        decision: 'approve',
      }),
    )
  })

  it('submits an optimistic user block and passes clientMessageId into startRun', async () => {
    const commandClient = createMockCommandClient()
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

  it('keeps same-body submits distinct through separate clientMessageIds', async () => {
    const commandClient = createMockCommandClient()
    const startRunCalls: Array<Parameters<CommandClient['startRun']>[0]> = []
    let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
    let subscriptionId: string | undefined
    const trackedClient = {
      ...commandClient,
      listenConversationEventBatches: async (callback) => {
        listener = callback
        return () => undefined
      },
      subscribeConversationEvents: async (request) => {
        const subscription = await commandClient.subscribeConversationEvents(request)
        subscriptionId = subscription.subscriptionId
        return subscription
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
        target: { value: 'Repeat this' },
      },
    )
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))
    await waitFor(() => expect(startRunCalls).toHaveLength(1))
    listener?.({
      subscriptionId: subscriptionId ?? 'subscription-mock',
      conversationId: 'conversation-001',
      events: [
        runEvent({
          id: 'evt-first-ended',
          payload: { reason: 'completed' },
          runId: 'run-001',
          sequence: 1,
          source: 'engine',
          timestamp,
          type: 'run.ended',
          visibility: 'public',
        }),
      ],
      cursor: cursor(''),
      gap: false,
      phase: 'live',
    })
    await waitFor(() => {
      expect(
        screen.getByPlaceholderText('Ask Jyowo anything about this project...'),
      ).not.toBeDisabled()
    })

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Repeat this' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))
    await waitFor(() => expect(startRunCalls).toHaveLength(2))

    expect(startRunCalls[0].clientMessageId).toEqual(expect.any(String))
    expect(startRunCalls[1].clientMessageId).toEqual(expect.any(String))
    expect(startRunCalls[0].clientMessageId).not.toBe(startRunCalls[1].clientMessageId)
    expect(screen.getAllByText('Repeat this').length).toBeGreaterThanOrEqual(2)
  })

  it('clears active run state when the timeline observes run end', async () => {
    const commandClient = createMockCommandClient()
    let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
    let subscriptionId: string | undefined
    const trackedClient = {
      ...commandClient,
      listenConversationEventBatches: async (callback) => {
        listener = callback
        return () => undefined
      },
      subscribeConversationEvents: async (request) => {
        const subscription = await commandClient.subscribeConversationEvents(request)
        subscriptionId = subscription.subscriptionId
        return subscription
      },
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient, 'conversation-001')

    fireEvent.change(
      await screen.findByPlaceholderText('Ask Jyowo anything about this project...'),
      {
        target: { value: 'Finish the run' },
      },
    )
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => {
      expect(uiStore.getState().activeRunId).toBe('run-001')
    })

    listener?.({
      subscriptionId: subscriptionId ?? 'subscription-mock',
      conversationId: 'conversation-001',
      events: [
        runEvent({
          id: 'evt-started',
          payload: { sessionId: 'conversation-001' },
          runId: 'run-001',
          sequence: 1,
          source: 'engine',
          timestamp,
          type: 'run.started',
          visibility: 'public',
        }),
      ],
      cursor: cursor(''),
      gap: false,
      phase: 'live',
    })

    await act(async () => {
      await flushAnimationFrames()
    })

    listener?.({
      subscriptionId: subscriptionId ?? 'subscription-mock',
      conversationId: 'conversation-001',
      events: [
        runEvent({
          id: 'evt-ended',
          payload: { reason: 'completed' },
          runId: 'run-001',
          sequence: 2,
          source: 'engine',
          timestamp,
          type: 'run.ended',
          visibility: 'public',
        }),
      ],
      cursor: cursor(''),
      gap: false,
      phase: 'live',
    })

    await act(async () => {
      await flushAnimationFrames()
    })

    await waitFor(() => {
      expect(uiStore.getState().activeRunId).toBeUndefined()
      expect(uiStore.getState().activeRunsByConversation).toEqual({})
    })
  })

  it('clears only the ended conversation run when multiple conversations are active', async () => {
    const commandClient = createMockCommandClient()
    let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
    let subscriptionId: string | undefined
    const trackedClient = {
      ...commandClient,
      listenConversationEventBatches: async (callback) => {
        listener = callback
        return () => undefined
      },
      subscribeConversationEvents: async (request) => {
        const subscription = await commandClient.subscribeConversationEvents(request)
        subscriptionId = subscription.subscriptionId
        return subscription
      },
    } satisfies CommandClient

    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-001',
        runId: 'run-001',
      })
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-002',
        runId: 'run-002',
      })
    })

    renderConversationWorkspace(trackedClient, 'conversation-001')

    await waitFor(() => {
      expect(listener).toBeDefined()
    })

    await act(async () => {
      listener?.({
        subscriptionId: subscriptionId ?? 'subscription-mock',
        conversationId: 'conversation-001',
        events: [
          runEvent({
            id: 'evt-started',
            payload: { sessionId: 'conversation-001' },
            runId: 'run-001',
            sequence: 1,
            source: 'engine',
            timestamp,
            type: 'run.started',
            visibility: 'public',
          }),
        ],
        cursor: cursor(''),
        gap: false,
        phase: 'live',
      })
      await flushAnimationFrames()
    })

    await act(async () => {
      listener?.({
        subscriptionId: subscriptionId ?? 'subscription-mock',
        conversationId: 'conversation-001',
        events: [
          runEvent({
            id: 'evt-ended',
            payload: { reason: 'completed' },
            runId: 'run-001',
            sequence: 2,
            source: 'engine',
            timestamp,
            type: 'run.ended',
            visibility: 'public',
          }),
        ],
        cursor: cursor(''),
        gap: false,
        phase: 'live',
      })
      await flushAnimationFrames()
    })

    await waitFor(() => {
      expect(uiStore.getState().activeRunsByConversation).toEqual({
        'conversation-002': 'run-002',
      })
    })
  })

  it('keeps the loaded conversation visible and recoverable when startRun fails', async () => {
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
    expect(await screen.findAllByText('Runtime unavailable')).not.toHaveLength(0)
    expect(screen.getByDisplayValue('Continue the Tauri setup')).toBeInTheDocument()
    expect(screen.queryByText('Conversation unavailable')).not.toBeInTheDocument()
  })

  it('ignores stale live event batches after subscribing to the selected conversation', async () => {
    let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
    const commandClient = createMockCommandClient({
      conversation: {
        conversation: {
          id: 'conversation-001',
          messages: [],
          modelConfigId: null,
          title: 'Live conversation',
          updatedAt: timestamp,
        },
      },
    })
    const trackedClient = {
      ...commandClient,
      listenConversationEventBatches: async (callback) => {
        listener = callback
        return () => undefined
      },
      subscribeConversationEvents: async () => ({
        subscriptionId: 'subscription-001',
        conversationId: 'conversation-001',
        replayEvents: [],
        gap: false,
      }),
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient)

    expect(await screen.findByRole('heading', { name: 'Live conversation' })).toBeInTheDocument()
    listener?.({
      subscriptionId: 'subscription-old',
      conversationId: 'conversation-001',
      events: [
        runEvent({
          id: 'evt-stale',
          payload: { text: 'stale text' },
          runId: 'run-001',
          sequence: 1,
          source: 'assistant',
          timestamp,
          type: 'assistant.delta',
          visibility: 'public',
        }),
      ],
      cursor: cursor(''),
      gap: false,
      phase: 'live',
    })

    expect(screen.queryByText('stale text')).not.toBeInTheDocument()
  })

  it('does not reuse the previous conversation cursor when switching conversations', async () => {
    const subscribeConversationEvents = vi.fn(
      async ({ conversationId }: { conversationId: string }) => ({
        subscriptionId: `subscription-${conversationId}`,
        conversationId,
        replayEvents:
          conversationId === 'conversation-001'
            ? [
                runEvent({
                  id: 'evt-old-cursor',
                  payload: { text: 'Old live text' },
                  runId: 'run-001',
                  sequence: 1,
                  source: 'assistant',
                  timestamp,
                  type: 'assistant.delta',
                  visibility: 'public',
                }),
              ]
            : [],
        cursor: conversationId === 'conversation-001' ? cursor('evt-old-cursor') : undefined,
        gap: false,
      }),
    )
    const commandClient = {
      ...createMockCommandClient(),
      listConversations: async () => ({
        conversations: [
          {
            id: 'conversation-001',
            isEmpty: false,
            lastMessagePreview: 'Old',
            title: 'Old conversation',
            updatedAt: timestamp,
          },
          {
            id: 'conversation-002',
            isEmpty: false,
            lastMessagePreview: 'New',
            title: 'New conversation',
            updatedAt: timestamp,
          },
        ],
      }),
      getConversation: async (conversationId: string) => ({
        conversation: {
          id: conversationId,
          messages: [],
          modelConfigId: null,
          title: conversationId === 'conversation-001' ? 'Old conversation' : 'New conversation',
          updatedAt: timestamp,
        },
      }),
      subscribeConversationEvents,
    } satisfies CommandClient

    const rendered = renderConversationWorkspace(commandClient, 'conversation-001')

    expect(await screen.findByRole('heading', { name: 'Old conversation' })).toBeInTheDocument()
    await waitFor(() =>
      expect(subscribeConversationEvents).toHaveBeenCalledWith({
        conversationId: 'conversation-001',
      }),
    )

    rendered.rerender(<ConversationWorkspace conversationId="conversation-002" />)

    expect(await screen.findByRole('heading', { name: 'New conversation' })).toBeInTheDocument()
    expect(screen.queryByText('Old live text')).not.toBeInTheDocument()
    await waitFor(() =>
      expect(subscribeConversationEvents).toHaveBeenCalledWith({
        conversationId: 'conversation-002',
      }),
    )
    expect(subscribeConversationEvents).not.toHaveBeenCalledWith({
      conversationId: 'conversation-002',
      afterCursor: cursor(''),
    })
  })

  it('recovers a sequence gap by paging the conversation timeline', async () => {
    let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
    const subscribeConversationEvents = vi.fn().mockResolvedValueOnce({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      replayEvents: [
        runEvent({
          id: 'evt-run',
          payload: { sessionId: 'conversation-001' },
          runId: 'run-001',
          sequence: 1,
          source: 'engine',
          timestamp,
          type: 'run.started',
          visibility: 'public',
        }),
      ],
      cursor: cursor('evt-run', 1),
      gap: false,
    })
    const pageConversationTimeline = vi.fn(async () => ({
      events: [
        runEvent({
          conversationSequence: 2,
          id: 'evt-missing-delta',
          payload: { text: 'recovered missing ' },
          runId: 'run-001',
          sequence: 2,
          source: 'assistant',
          timestamp,
          type: 'assistant.delta',
          visibility: 'public',
        }),
        runEvent({
          conversationSequence: 3,
          id: 'evt-gap-delta',
          payload: { text: 'future delta' },
          runId: 'run-001',
          sequence: 3,
          source: 'assistant',
          timestamp,
          type: 'assistant.delta',
          visibility: 'public',
        }),
      ],
      cursor: cursor('evt-gap-delta', 3),
      gap: false,
    }))
    const commandClient = {
      ...createMockCommandClient({
        conversation: {
          conversation: {
            id: 'conversation-001',
            messages: [],
            modelConfigId: null,
            title: 'Gap recovery',
            updatedAt: timestamp,
          },
        },
      }),
      listenConversationEventBatches: vi.fn(async (callback) => {
        listener = callback
        return vi.fn()
      }),
      pageConversationTimeline,
      subscribeConversationEvents,
    } satisfies CommandClient

    renderConversationWorkspace(commandClient, 'conversation-001')

    expect(await screen.findByRole('heading', { name: 'Gap recovery' })).toBeInTheDocument()
    await waitFor(() => expect(subscribeConversationEvents).toHaveBeenCalledTimes(1))
    await act(async () => {
      await flushAnimationFrames()
    })

    listener?.({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      events: [
        runEvent({
          conversationSequence: 3,
          id: 'evt-gap-delta',
          payload: { text: 'untrusted future delta' },
          runId: 'run-001',
          sequence: 3,
          source: 'assistant',
          timestamp,
          type: 'assistant.delta',
          visibility: 'public',
        }),
      ],
      cursor: cursor('evt-gap-delta', 3),
      gap: false,
      phase: 'live',
    })

    await waitFor(() =>
      expect(pageConversationTimeline).toHaveBeenCalledWith({
        conversationId: 'conversation-001',
        afterCursor: cursor('evt-run', 1),
        limit: 200,
      }),
    )
    expect(subscribeConversationEvents).toHaveBeenCalledTimes(1)
    expect(screen.queryByText('untrusted future delta')).not.toBeInTheDocument()
    expect(await screen.findByText('recovered missing future delta')).toBeInTheDocument()
  })
})
