import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type {
  CommandClient,
  ConversationModelCapability,
  ModelProviderCatalogResponse,
  ProviderConfig,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { ModelConfigDialog } from './ModelConfigDialog'

function renderDialog({
  client = createTestCommandClient(),
  onOpenChange = vi.fn(),
  profile = existingProfile,
}: {
  client?: CommandClient
  onOpenChange?: (open: boolean) => void
  profile?: ProviderConfig | null
} = {}) {
  uiStore.getState().setLocale('en-US')
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={client}>
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(
    <ModelConfigDialog
      catalog={catalog}
      onOpenChange={onOpenChange}
      onSaved={vi.fn()}
      open
      profile={profile}
    />,
    { wrapper: Wrapper },
  )
}

describe('ModelConfigDialog', () => {
  it('saves edits through saveProviderSettings without resubmitting an unchanged key', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: {
        ...existingProfile,
        displayName: 'Primary OpenAI Edited',
      },
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('Configuration name'), {
      target: { value: 'Primary OpenAI Edited' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings).toHaveBeenCalledWith({
      baseUrl: 'https://api.openai.com/v1',
      configId: 'cfg-openai',
      displayName: 'Primary OpenAI Edited',
      modelId: 'gpt-4.1',
      modelOptions: {},
      providerId: 'openai',
      setDefault: true,
    })
    expect(saveProviderSettings.mock.calls[0][0]).not.toHaveProperty('apiKey')
    expect(saveProviderSettings.mock.calls[0][0]).not.toHaveProperty('officialQuotaApiKey')
  })

  it('saves a new API key only from the password field and does not show raw keys by default', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: existingProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    const apiKey = within(dialog).getByLabelText('API key')
    expect(apiKey).toHaveAttribute('type', 'password')
    expect(apiKey).toHaveValue('')
    expect(dialog).not.toHaveTextContent('sk-existing-secret')

    fireEvent.change(apiKey, { target: { value: 'sk-new-secret' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      apiKey: 'sk-new-secret',
      configId: 'cfg-openai',
    })
  })

  it('saves a new official quota admin key only when typed', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: {
        ...existingProfile,
        hasOfficialQuotaApiKey: true,
      },
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    const officialQuotaApiKey = within(dialog).getByLabelText('Official quota admin key')
    expect(officialQuotaApiKey).toHaveAttribute('type', 'password')
    expect(officialQuotaApiKey).toHaveValue('')

    fireEvent.change(officialQuotaApiKey, { target: { value: 'admin-usage-key' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-openai',
      officialQuotaApiKey: 'admin-usage-key',
    })
    expect(saveProviderSettings.mock.calls[0][0]).not.toHaveProperty('apiKey')
  })

  it('clears the typed API key when save fails', async () => {
    const saveProviderSettings = vi.fn().mockRejectedValue(new Error('safe save failure'))
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    const apiKey = within(dialog).getByLabelText('API key')
    const officialQuotaApiKey = within(dialog).getByLabelText('Official quota admin key')
    fireEvent.change(apiKey, { target: { value: 'sk-failed-secret' } })
    fireEvent.change(officialQuotaApiKey, { target: { value: 'admin-failed-secret' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(apiKey).toHaveValue('')
    expect(officialQuotaApiKey).toHaveValue('')
    expect(dialog).not.toHaveTextContent('sk-failed-secret')
    expect(dialog).not.toHaveTextContent('admin-failed-secret')
  })

  it('clears the typed API key when closing without saving', () => {
    const onOpenChange = vi.fn()
    renderDialog({ onOpenChange })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    const apiKey = within(dialog).getByLabelText('API key')
    const officialQuotaApiKey = within(dialog).getByLabelText('Official quota admin key')
    fireEvent.change(apiKey, { target: { value: 'sk-unsaved-secret' } })
    fireEvent.change(officialQuotaApiKey, { target: { value: 'admin-unsaved-secret' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(apiKey).toHaveValue('')
    expect(officialQuotaApiKey).toHaveValue('')
    expect(dialog).not.toHaveTextContent('sk-unsaved-secret')
    expect(dialog).not.toHaveTextContent('admin-unsaved-secret')
  })

  it('switches the model field to the selected provider catalog', () => {
    renderDialog()

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('Provider'), {
      target: { value: 'anthropic' },
    })

    expect(within(dialog).getByLabelText('Model')).toHaveValue('claude-sonnet-4')
  })

  it('saves Anthropic provider defaults from structured controls', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: anthropicProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: anthropicProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    expect(within(dialog).queryByLabelText('Model options')).not.toBeInTheDocument()

    fireEvent.click(within(dialog).getByLabelText('Thinking'))
    fireEvent.change(within(dialog).getByLabelText('Thinking budget'), {
      target: { value: '4096' },
    })
    fireEvent.change(within(dialog).getByLabelText('Service tier'), {
      target: { value: 'auto' },
    })
    fireEvent.change(within(dialog).getByLabelText('Output effort'), {
      target: { value: 'medium' },
    })
    fireEvent.change(within(dialog).getByLabelText('Top P'), {
      target: { value: '0.9' },
    })
    fireEvent.change(within(dialog).getByLabelText('Stop sequences'), {
      target: { value: 'DONE,STOP' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-anthropic',
      providerId: 'anthropic',
      providerDefaults: {
        body: {
          thinking: { type: 'enabled', budget_tokens: 4096 },
          output_config: { effort: 'medium' },
          service_tier: 'auto',
          stop_sequences: ['DONE', 'STOP'],
          top_p: 0.9,
        },
        headers: {},
      },
    })
    expect(saveProviderSettings.mock.calls[0][0]).not.toHaveProperty('modelOptionsJson')
  })

  it('preserves an existing model id that is missing from the current catalog', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: {
        ...existingProfile,
        modelId: 'gpt-old',
      },
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: {
        ...existingProfile,
        modelId: 'gpt-old',
      },
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    expect(within(dialog).getByLabelText('Model')).toHaveValue('gpt-old')

    fireEvent.change(within(dialog).getByLabelText('Configuration name'), {
      target: { value: 'Old OpenAI' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-openai',
      displayName: 'Old OpenAI',
      modelId: 'gpt-old',
      providerId: 'openai',
    })
  })

  it('saves Qwen Chat Completions search and code interpreter as extra body fields', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: qwenProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: qwenProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('API mode'), {
      target: { value: 'chat_completions' },
    })
    fireEvent.click(within(dialog).getByLabelText('web_search'))
    fireEvent.click(within(dialog).getByLabelText('code_interpreter'))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-qwen',
      protocol: 'chat_completions',
      providerDefaults: {
        body: {
          enable_code_interpreter: true,
          enable_search: true,
        },
      },
    })
    expect(saveProviderSettings.mock.calls[0][0].providerDefaults.body).not.toHaveProperty('tools')
    expect(saveProviderSettings.mock.calls[0][0].providerDefaults.body).not.toHaveProperty(
      'search_options',
    )
  })

  it('saves Qwen Chat Completions web extractor only for agent max capable models', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: qwen3MaxProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: qwen3MaxProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('API mode'), {
      target: { value: 'chat_completions' },
    })
    fireEvent.click(within(dialog).getByLabelText('web_extractor'))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-qwen-max',
      protocol: 'chat_completions',
      providerDefaults: {
        body: {
          enable_search: true,
          enable_thinking: true,
          search_options: {
            search_strategy: 'agent_max',
          },
        },
      },
    })
  })

  it('does not save Qwen Chat Completions web extractor fields for non-agent-max models', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: qwenFlashProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: qwenFlashProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('API mode'), {
      target: { value: 'chat_completions' },
    })

    expect(within(dialog).getByLabelText('web_extractor')).toBeDisabled()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0].providerDefaults.body).not.toHaveProperty(
      'search_options',
    )
    expect(saveProviderSettings.mock.calls[0][0].providerDefaults.body).not.toHaveProperty(
      'enable_search',
    )
  })

  it('saves Qwen Responses reasoning effort values from the official enum', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: qwenProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: qwenProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    const reasoningEffort = within(dialog).getByLabelText('Reasoning effort')
    expect(within(reasoningEffort).getByRole('option', { name: 'None' })).toBeInTheDocument()
    expect(within(reasoningEffort).getByRole('option', { name: 'Minimal' })).toBeInTheDocument()
    expect(within(reasoningEffort).queryByRole('option', { name: 'Low' })).not.toBeInTheDocument()

    fireEvent.change(reasoningEffort, { target: { value: 'minimal' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-qwen',
      protocol: 'responses',
      providerDefaults: {
        body: {
          reasoning: { effort: 'minimal' },
        },
      },
    })
  })

  it('exposes all official Qwen API modes', () => {
    renderDialog({ profile: qwenProfile })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    const apiMode = within(dialog).getByLabelText('API mode')

    expect(within(apiMode).getByRole('option', { name: 'Responses' })).toBeInTheDocument()
    expect(within(apiMode).getByRole('option', { name: 'Chat Completions' })).toBeInTheDocument()
    expect(within(apiMode).getByRole('option', { name: 'Messages' })).toBeInTheDocument()
    expect(within(apiMode).getByRole('option', { name: 'DashScope' })).toBeInTheDocument()
  })

  it('saves Qwen Messages thinking as Anthropic-compatible body fields', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: qwenProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: qwenProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('API mode'), {
      target: { value: 'messages' },
    })
    fireEvent.click(within(dialog).getByLabelText('Thinking'))
    fireEvent.change(within(dialog).getByLabelText('Thinking budget'), {
      target: { value: '2048' },
    })
    fireEvent.click(within(dialog).getByLabelText('web_search'))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-qwen',
      protocol: 'messages',
      providerDefaults: {
        body: {
          thinking: { type: 'enabled', budget_tokens: 2048 },
        },
      },
    })
    expect(saveProviderSettings.mock.calls[0][0].providerDefaults.body).not.toHaveProperty(
      'enable_thinking',
    )
    expect(saveProviderSettings.mock.calls[0][0].providerDefaults.body).not.toHaveProperty('tools')
  })

  it('saves Qwen DashScope thinking and tool fields as flat defaults', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: qwenProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: qwenProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('API mode'), {
      target: { value: 'dashscope' },
    })
    fireEvent.click(within(dialog).getByLabelText('Thinking'))
    fireEvent.change(within(dialog).getByLabelText('Thinking budget'), {
      target: { value: '2048' },
    })
    fireEvent.click(within(dialog).getByLabelText('Preserve thinking'))
    fireEvent.click(within(dialog).getByLabelText('web_search'))
    fireEvent.click(within(dialog).getByLabelText('code_interpreter'))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-qwen',
      protocol: 'dashscope',
      providerDefaults: {
        body: {
          enable_thinking: true,
          thinking_budget: 2048,
          preserve_thinking: true,
          enable_search: true,
          enable_code_interpreter: true,
        },
      },
    })
    expect(saveProviderSettings.mock.calls[0][0].providerDefaults.body).not.toHaveProperty('tools')
  })
})

const modelCapability: ConversationModelCapability = {
  inputModalities: ['text'],
  outputModalities: ['text'],
  contextWindow: 128000,
  maxOutputTokens: 8192,
  streaming: true,
  toolCalling: true,
  reasoning: false,
  promptCache: false,
  structuredOutput: true,
}

const gpt41 = {
  protocol: 'responses' as const,
  supportedParameters: [],
  conversationCapability: modelCapability,
  contextWindow: 128000,
  displayName: 'GPT-4.1',
  lifecycle: { kind: 'stable' as const },
  maxOutputTokens: 8192,
  modelId: 'gpt-4.1',
  runtimeStatus: { kind: 'runnable' as const },
}

const qwen37Max = {
  ...gpt41,
  displayName: 'Qwen3.7 Max',
  modelId: 'qwen3.7-max',
}

const qwen3Max = {
  ...gpt41,
  displayName: 'Qwen3 Max',
  modelId: 'qwen3-max',
}

const qwenFlash = {
  ...gpt41,
  displayName: 'Qwen Flash',
  modelId: 'qwen-flash',
}

const catalog: ModelProviderCatalogResponse = {
  providers: [
    {
      defaultBaseUrl: 'https://api.openai.com/v1',
      displayName: 'OpenAI',
      models: [gpt41],
      providerId: 'openai',
      runtimeCapability: {
        authScheme: 'bearer',
        baseUrlRegions: [{ id: 'default', label: 'Default', baseUrl: 'https://api.openai.com/v1' }],
        supportsLiveValidation: true,
        supportsStreamingValidation: true,
        secretRevealSupported: true,
      },
      serviceCapabilities: [],
      sourceUrl: 'https://platform.openai.com/docs',
      verifiedDate: '2026-06-30',
    },
    {
      defaultBaseUrl: 'https://api.anthropic.com/v1',
      displayName: 'Anthropic',
      models: [
        {
          ...gpt41,
          displayName: 'Claude Sonnet 4',
          modelId: 'claude-sonnet-4',
          supportedParameters: [
            'thinking',
            'output_config',
            'service_tier',
            'stop_sequences',
            'top_p',
          ],
        },
      ],
      providerId: 'anthropic',
      runtimeCapability: {
        authScheme: 'bearer',
        baseUrlRegions: [
          { id: 'default', label: 'Default', baseUrl: 'https://api.anthropic.com/v1' },
        ],
        supportsLiveValidation: true,
        supportsStreamingValidation: true,
        secretRevealSupported: true,
      },
      serviceCapabilities: [],
      sourceUrl: 'https://docs.anthropic.com',
      verifiedDate: '2026-06-30',
    },
    {
      defaultBaseUrl: 'https://dashscope-us.aliyuncs.com/compatible-mode/v1',
      displayName: 'Qwen',
      models: [qwen37Max, qwen3Max, qwenFlash],
      providerId: 'qwen',
      runtimeCapability: {
        authScheme: 'bearer',
        baseUrlRegions: [
          {
            id: 'default',
            label: 'Default',
            baseUrl: 'https://dashscope-us.aliyuncs.com/compatible-mode/v1',
          },
        ],
        supportsLiveValidation: true,
        supportsStreamingValidation: true,
        secretRevealSupported: true,
      },
      serviceCapabilities: [],
      sourceUrl: 'https://help.aliyun.com/zh/model-studio/',
      verifiedDate: '2026-07-09',
    },
  ],
}

const existingProfile: ProviderConfig = {
  id: 'cfg-openai',
  providerId: 'openai',
  modelId: 'gpt-4.1',
  displayName: 'Primary OpenAI',
  baseUrl: 'https://api.openai.com/v1',
  hasApiKey: true,
  hasOfficialQuotaApiKey: false,
  isDefault: true,
  protocol: 'responses',
  modelDescriptor: gpt41,
}

const anthropicProfile: ProviderConfig = {
  id: 'cfg-anthropic',
  providerId: 'anthropic',
  modelId: 'claude-sonnet-4',
  displayName: 'Primary Anthropic',
  baseUrl: 'https://api.anthropic.com/v1',
  hasApiKey: true,
  hasOfficialQuotaApiKey: false,
  isDefault: false,
  protocol: 'messages',
  providerDefaults: {
    body: {},
    headers: {},
  },
  modelDescriptor: catalog.providers[1].models[0],
}

const qwenProfile: ProviderConfig = {
  id: 'cfg-qwen',
  providerId: 'qwen',
  modelId: 'qwen3.7-max',
  displayName: 'Primary Qwen',
  baseUrl: 'https://dashscope-us.aliyuncs.com/compatible-mode/v1',
  hasApiKey: true,
  hasOfficialQuotaApiKey: false,
  isDefault: false,
  protocol: 'responses',
  providerDefaults: {
    body: {},
    headers: {},
  },
  modelDescriptor: qwen37Max,
}

const qwen3MaxProfile: ProviderConfig = {
  ...qwenProfile,
  id: 'cfg-qwen-max',
  modelId: 'qwen3-max',
  displayName: 'Primary Qwen Max',
  modelDescriptor: qwen3Max,
}

const qwenFlashProfile: ProviderConfig = {
  ...qwenProfile,
  id: 'cfg-qwen-flash',
  modelId: 'qwen-flash',
  displayName: 'Primary Qwen Flash',
  modelDescriptor: qwenFlash,
}
