import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it } from 'vitest'

import type {
  CommandClient,
  ConversationTurn,
  ListProviderSettingsResponse,
  ModelProviderCatalogResponse,
  PageConversationWorktreeResponse,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import { assistantWork } from '@/testing/conversation-worktree-builders'

import { ConversationWorkspace } from './ConversationWorkspace'

type ModelCatalogEntry = ModelProviderCatalogResponse['providers'][number]['models'][number]

const timestamp = '2026-06-17T00:00:00.000Z'

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

const switchableProviderSettings: ListProviderSettingsResponse = {
  defaultConfigId: 'deepseek-config',
  selectionScope: 'global',
  configs: [
    {
      protocol: 'chat_completions',
      displayName: 'DeepSeek',
      hasApiKey: true,
      hasOfficialQuotaApiKey: false,
      id: 'deepseek-config',
      isDefault: true,
      modelDescriptor: {
        ...openAiModelDescriptor,
        protocol: 'chat_completions',
        displayName: 'DeepSeek V4 Flash',
        modelId: 'deepseek-v4-flash',
      },
      modelId: 'deepseek-v4-flash',
      providerId: 'deepseek',
    },
    {
      protocol: 'chat_completions',
      displayName: 'MiniMax',
      hasApiKey: true,
      hasOfficialQuotaApiKey: false,
      id: 'minimax-config',
      isDefault: false,
      modelDescriptor: {
        ...openAiModelDescriptor,
        protocol: 'chat_completions',
        displayName: 'MiniMax M3',
        modelId: 'MiniMax-M3',
      },
      modelId: 'MiniMax-M3',
      providerId: 'minimax',
    },
  ],
}

function renderConversationWorkspace(commandClient: CommandClient = createTestCommandClient()) {
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

  return render(<ConversationWorkspace conversationId="conversation-001" />, {
    wrapper: Wrapper,
  })
}

describe('ConversationWorkspace model config selection', () => {
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
          selectionScope: 'global',
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

  it('keeps model selection local and submits the selected model config', async () => {
    const commandClient = createTestCommandClient({
      conversationWorktreePage: pageWithTurn('complete'),
      providerSettingsList: switchableProviderSettings,
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

    const modelSelector = (await screen.findByLabelText('Model')) as HTMLSelectElement
    expect(modelSelector.value).toBe('deepseek-config')

    fireEvent.change(modelSelector, { target: { value: 'minimax-config' } })
    expect(startRunCalls).toHaveLength(0)

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Use MiniMax for this run' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => {
      expect(startRunCalls).toEqual([
        expect.objectContaining({
          modelConfigId: 'minimax-config',
          prompt: 'Use MiniMax for this run',
        }),
      ])
    })
  })

  it('submits review continue without forcing a model config', async () => {
    const commandClient = createTestCommandClient({
      conversationWorktreePage: pageWithReviewRequest(),
      providerSettingsList: { defaultConfigId: null, selectionScope: 'global', configs: [] },
    })
    const startRunCalls: Array<Parameters<CommandClient['startRun']>[0]> = []
    const trackedClient = {
      ...commandClient,
      startRun: (request: Parameters<CommandClient['startRun']>[0]) => {
        startRunCalls.push(request)
        return Promise.resolve({ runId: 'run-review-continue', status: 'started' })
      },
    } satisfies CommandClient

    renderConversationWorkspace(trackedClient)

    expect(
      await screen.findByRole('button', { name: 'Permission mode: Request approval' }),
    ).toBeInTheDocument()
    fireEvent.click(await screen.findByRole('button', { name: 'Continue' }))

    await waitFor(() => {
      expect(startRunCalls).toEqual([
        expect.not.objectContaining({
          modelConfigId: expect.anything(),
        }),
      ])
    })
  })

  it('submits without a model config when no configured model has an API key', async () => {
    const commandClient = createTestCommandClient({
      conversationWorktreePage: pageWithTurn('complete'),
      providerSettingsList: {
        defaultConfigId: null,
        selectionScope: 'global',
        configs: [
          {
            protocol: 'responses',
            displayName: 'OpenAI Work',
            hasApiKey: false,
            hasOfficialQuotaApiKey: false,
            id: 'openai-work',
            isDefault: true,
            modelDescriptor: openAiModelDescriptor,
            modelId: 'gpt-5.4-mini',
            providerId: 'openai',
          },
        ],
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

    fireEvent.change(
      await screen.findByPlaceholderText('Ask Jyowo anything about this project...'),
      {
        target: { value: 'Use the backend default model' },
      },
    )
    const sendButton = await screen.findByRole('button', { name: 'Send message' })

    await waitFor(() => {
      expect(sendButton).toBeEnabled()
    })
    fireEvent.click(sendButton)
    await waitFor(() => {
      expect(startRunCalls).toEqual([
        expect.objectContaining({
          prompt: 'Use the backend default model',
        }),
      ])
    })
    expect(startRunCalls[0]).not.toHaveProperty('modelConfigId')
  })
})

function cursor(conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
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

function pageWithReviewRequest(): PageConversationWorktreeResponse {
  return {
    turns: [
      {
        ...turn('complete'),
        assistant: assistantWork({
          id: 'assistant:run-review',
          runId: 'run-review',
          status: 'complete',
          segments: [
            {
              kind: 'reviewRequest',
              id: 'segment:review-request',
              order: 0,
              requestId: 'review-request-001',
              title: 'Review generated changes',
              body: 'Continue after review',
            },
          ],
        }),
      },
    ],
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
    assistant: assistantWork({
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
    }),
  }
}
