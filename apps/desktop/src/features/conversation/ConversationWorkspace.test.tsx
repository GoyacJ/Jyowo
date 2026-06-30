import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { appI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type {
  CommandClient,
  ConversationEventBatchPayload,
  ConversationTurn,
  ListProjectsResponse,
  ModelProviderCatalogResponse,
  PageConversationWorktreeResponse,
  StartRunResponse,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createRejectedTestCommandClient, createTestCommandClient } from '@/testing/command-client'

import { ConversationWorkspace } from './ConversationWorkspace'

type ModelCatalogEntry = ModelProviderCatalogResponse['providers'][number]['models'][number]

const timestamp = '2026-06-17T00:00:00.000Z'
const agentCapabilities = {
  agentTeamsAvailable: false,
  agentTeamsEnabled: false,
  backgroundAgentsAvailable: false,
  backgroundAgentsEnabled: false,
  subagentsAvailable: false,
  subagentsEnabled: false,
  unavailableReasons: [],
}

function cursor(conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
}

function renderConversationWorkspace(
  commandClient: CommandClient = createTestCommandClient(),
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

  const view = render(<ConversationWorkspace conversationId={conversationId} />, {
    wrapper: Wrapper,
  })

  return { ...view, queryClient }
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

  it('renders a MiniMax-style worktree page without raw tool or artifact internals', async () => {
    renderConversationWorkspace(
      createTestCommandClient({
        conversationWorktreePage: {
          turns: [minimaxTurn()],
          pageCursor: { turnId: 'turn:user-minimax', position: 0 },
          eventCursor: cursor(9),
          hasMoreBefore: false,
          hasMoreAfter: false,
          gap: false,
        },
      }),
      'conversation-minimax',
    )

    expect(await screen.findByText('帮我生成一张海报图')).toBeInTheDocument()
    expect(screen.getByText('正在检查可用的图像工具')).toBeInTheDocument()
    expect(screen.getByText('MiniMaxTextToImage')).toBeInTheDocument()
    expect(screen.getByText('工具执行失败。可在详情中查看。')).toBeInTheDocument()
    expect(screen.getByText('海报生成提示词')).toBeInTheDocument()
    expect(screen.getByText('可复用的图像生成提示词已准备好。')).toBeInTheDocument()
    expect(
      screen.getByText('图像工具失败后，我保留了可复用的提示词和下一步建议。'),
    ).toBeInTheDocument()

    const renderedText = document.body.textContent ?? ''
    for (const hiddenText of [
      'raw provider failure',
      '/Users/alice/private',
      'secret-token',
      'blob-secret',
      'hash-secret',
    ]) {
      expect(renderedText).not.toContain(hiddenText)
    }
  })

  it('loads reference candidates for the selected conversation', async () => {
    const commandClient = createTestCommandClient({
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
    const { unmount } = renderConversationWorkspace(createTestCommandClient({ delayMs: 10 }))

    expect(screen.getByText('Loading conversation...')).toBeInTheDocument()
    expect(
      await screen.findByRole('heading', { name: 'Build the desktop foundation' }),
    ).toBeInTheDocument()

    unmount()

    renderConversationWorkspace(
      createTestCommandClient({ conversations: { conversations: [] } }),
      'missing-conversation',
    )
    expect(await screen.findByRole('heading', { name: 'New conversation' })).toBeInTheDocument()

    unmount()

    renderConversationWorkspace(createRejectedTestCommandClient(new Error('IPC unavailable')))
    expect(await screen.findByText('IPC unavailable')).toBeInTheDocument()
  })

  it('leaves the model selector empty when no model is selected and no default exists', async () => {
    renderConversationWorkspace(
      createTestCommandClient({
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
              hasOfficialQuotaApiKey: false,
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
    const commandClient = createTestCommandClient()
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

  it('does not leak English timeline fallback labels in Chinese', async () => {
    await appI18n.changeLanguage('zh-CN')
    try {
      renderConversationWorkspace(createTestCommandClient(), 'conversation-001')

      expect(
        await screen.findByRole('heading', { name: 'Build the desktop foundation' }),
      ).toBeInTheDocument()
      const renderedText = document.body.textContent ?? ''

      for (const leakedLabel of ['Tools', 'Approved', 'Complete', 'failed', 'View raw events']) {
        expect(renderedText).not.toContain(leakedLabel)
      }
    } finally {
      await appI18n.changeLanguage('en-US')
    }
  })

  it('submits an optimistic user turn and passes clientMessageId into startRun', async () => {
    const commandClient = createTestCommandClient({
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
          permissionMode: 'default',
          prompt: 'Continue the Tauri setup',
          clientMessageId: expect.any(String),
        }),
      ])
    })

    resolveStartRun?.({ runId: 'run-001', status: 'started' })
  })

  it('redacts unsafe optimistic user text while sending the raw prompt to Rust', async () => {
    const commandClient = createTestCommandClient({
      conversationWorktreePage: pageWithTurn('complete'),
    })
    const startRunCalls: Array<Parameters<CommandClient['startRun']>[0]> = []
    const trackedClient = {
      ...commandClient,
      startRun: (request: Parameters<CommandClient['startRun']>[0]) => {
        startRunCalls.push(request)
        return new Promise<StartRunResponse>(() => undefined)
      },
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient)

    const prompt = 'Use token: abcdefghijklmnop from /Users/goya/.ssh/config'
    fireEvent.change(
      await screen.findByPlaceholderText('Ask Jyowo anything about this project...'),
      {
        target: { value: prompt },
      },
    )
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await screen.findByText('[REDACTED]')
    const optimisticTurn = screen
      .getAllByRole('article', { name: 'Conversation turn' })
      .find((turn) => within(turn).queryByText('[REDACTED]'))
    expect(optimisticTurn).toBeDefined()
    if (!optimisticTurn) {
      throw new Error('missing optimistic redacted turn')
    }
    expect(within(optimisticTurn).getByText('[REDACTED]')).toBeInTheDocument()
    expect(within(optimisticTurn).queryByText(prompt)).not.toBeInTheDocument()
    await waitFor(() => {
      expect(startRunCalls).toEqual([
        expect.objectContaining({
          prompt,
          permissionMode: 'default',
          clientMessageId: expect.any(String),
        }),
      ])
    })
  })

  it('initializes composer permission mode from execution settings', async () => {
    const commandClient = createTestCommandClient({
      conversationWorktreePage: pageWithTurn('complete'),
      executionSettings: {
        agentCapabilities,
        autoModeAvailable: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'bypass_permissions',
        toolProfile: 'full',
      },
    })
    const startRunCalls: Array<Parameters<CommandClient['startRun']>[0]> = []
    const trackedClient = {
      ...commandClient,
      startRun: (request: Parameters<CommandClient['startRun']>[0]) => {
        startRunCalls.push(request)
        return Promise.resolve({ runId: 'run-001', status: 'started' })
      },
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient)

    expect(
      await screen.findByRole('button', { name: 'Permission mode: Full access' }),
    ).toBeInTheDocument()
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Use the saved default mode' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => {
      expect(startRunCalls).toEqual([
        expect.objectContaining({
          permissionMode: 'bypass_permissions',
          prompt: 'Use the saved default mode',
        }),
      ])
    })
  })

  it('resets composer permission mode from the next workspace execution settings', async () => {
    const baseClient = createTestCommandClient({
      conversationWorktreePage: pageWithTurn('complete'),
    })
    const listProjects = vi
      .fn()
      .mockResolvedValueOnce(projectListResponse('/workspace-a'))
      .mockResolvedValue(projectListResponse('/workspace-b'))
    const getExecutionSettings = vi
      .fn()
      .mockResolvedValueOnce({
        agentCapabilities,
        autoModeAvailable: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default',
        toolProfile: 'full',
      })
      .mockResolvedValue({
        agentCapabilities,
        autoModeAvailable: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default',
        toolProfile: 'full',
      })
    const startRunCalls: Array<Parameters<CommandClient['startRun']>[0]> = []
    const trackedClient = {
      ...baseClient,
      getExecutionSettings,
      listProjects,
      startRun: (request: Parameters<CommandClient['startRun']>[0]) => {
        startRunCalls.push(request)
        return Promise.resolve({ runId: 'run-001', status: 'started' })
      },
    } satisfies CommandClient

    const { queryClient } = renderConversationWorkspace(trackedClient)

    fireEvent.pointerDown(
      await screen.findByRole('button', { name: 'Permission mode: Request approval' }),
    )
    fireEvent.click(await screen.findByRole('menuitem', { name: /Full access/i }))
    expect(
      await screen.findByRole('button', { name: 'Permission mode: Full access' }),
    ).toBeInTheDocument()

    await act(async () => {
      await queryClient.invalidateQueries({ queryKey: ['projects', 'list'] })
    })

    await waitFor(() => {
      expect(getExecutionSettings).toHaveBeenCalledTimes(2)
    })
    expect(getExecutionSettings).toHaveBeenNthCalledWith(1, { workspacePath: '/workspace-a' })
    expect(getExecutionSettings).toHaveBeenNthCalledWith(2, { workspacePath: '/workspace-b' })
    expect(
      await screen.findByRole('button', { name: 'Permission mode: Request approval' }),
    ).toBeInTheDocument()

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Use workspace B defaults' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => {
      expect(startRunCalls).toEqual([
        expect.objectContaining({
          permissionMode: 'default',
          prompt: 'Use workspace B defaults',
        }),
      ])
    })
  })

  it('does not submit before execution settings initialize the composer permission mode', async () => {
    const commandClient = createTestCommandClient({
      conversationWorktreePage: pageWithTurn('complete'),
    })
    const startRun = vi.fn(commandClient.startRun)
    const trackedClient = {
      ...commandClient,
      getExecutionSettings: () => new Promise<never>(() => undefined),
      startRun,
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient)

    fireEvent.change(
      await screen.findByPlaceholderText('Ask Jyowo anything about this project...'),
      {
        target: { value: 'Do not use default before settings load' },
      },
    )
    const sendButton = screen.getByRole('button', { name: 'Send message' })

    expect(sendButton).toBeDisabled()
    fireEvent.click(sendButton)
    expect(startRun).not.toHaveBeenCalled()
  })

  it('clears active run state when a terminal event triggers a completed worktree refetch', async () => {
    let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
    let terminalEventReceived = false
    const pageConversationWorktree = vi.fn(async () => {
      return pageWithTurn(terminalEventReceived ? 'complete' : 'running')
    })
    const commandClient = createTestCommandClient()
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
    expect(screen.getByPlaceholderText('Ask Jyowo anything about this project...')).toBeEnabled()
  })

  it('cancels the current active run from the composer', async () => {
    const commandClient = createTestCommandClient({
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
    const commandClient = createTestCommandClient({
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

function projectListResponse(path: string): ListProjectsResponse {
  return {
    activePath: path,
    projects: [
      {
        lastOpenedAt: timestamp,
        name: path.split('/').filter(Boolean).at(-1) ?? 'Project',
        path,
      },
    ],
  }
}

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

function minimaxTurn(): ConversationTurn {
  return {
    id: 'turn:user-minimax',
    conversationId: 'conversation-minimax',
    position: 0,
    user: {
      id: 'user:user-minimax',
      messageId: 'user-minimax',
      body: '帮我生成一张海报图',
      timestamp,
    },
    assistant: {
      id: 'assistant:run-minimax',
      runId: 'run-minimax',
      status: 'complete',
      segments: [
        {
          kind: 'thinking',
          id: 'segment:thinking:run-minimax',
          order: 0,
          status: 'running',
          summary: { text: '正在检查可用的图像工具' },
        },
        {
          kind: 'toolGroup',
          id: 'segment:tools:tool-minimax',
          order: 1,
          attempts: [
            {
              id: 'tool:tool-minimax',
              order: 0,
              toolUseId: 'tool-minimax',
              toolName: 'MiniMaxTextToImage',
              status: 'failed',
              permission: {
                id: 'permission:permission-minimax',
                requestId: 'permission-minimax',
                toolUseId: 'tool-minimax',
                status: 'approved',
              },
              failureSummary: '工具执行失败。可在详情中查看。',
            },
          ],
        },
        {
          kind: 'artifact',
          id: 'segment:artifact:artifact-minimax',
          order: 2,
          artifactId: 'artifact-minimax',
          title: '海报生成提示词',
          summary: '可复用的图像生成提示词已准备好。',
        },
        {
          kind: 'text',
          id: 'segment:text:assistant-final',
          order: 3,
          messageId: 'assistant-final',
          body: '图像工具失败后，我保留了可复用的提示词和下一步建议。',
        },
      ],
    },
  }
}
