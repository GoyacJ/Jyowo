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

  it('saves OpenAI Responses options as typed model options', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: existingProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: {
        ...existingProfile,
        modelOptions: {
          openaiResponses: {
            reasoning: { effort: 'medium', summary: 'auto' },
            serviceTier: 'auto',
            text: { verbosity: 'low' },
            store: true,
          },
        },
      },
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    expect(within(dialog).getByLabelText('OpenAI reasoning effort')).toHaveValue('medium')
    expect(within(dialog).getByLabelText('OpenAI reasoning summary')).toHaveValue('auto')
    expect(within(dialog).getByLabelText('OpenAI text verbosity')).toHaveValue('low')
    expect(within(dialog).getByLabelText('OpenAI service tier')).toHaveValue('auto')
    expect(within(dialog).getByLabelText('OpenAI store response')).toBeChecked()

    fireEvent.change(within(dialog).getByLabelText('OpenAI prompt cache key'), {
      target: { value: 'tenant:stable-prefix' },
    })
    fireEvent.change(within(dialog).getByLabelText('OpenAI prompt cache retention'), {
      target: { value: '24h' },
    })
    fireEvent.click(within(dialog).getByLabelText('OpenAI parallel tool calls'))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-openai',
      providerId: 'openai',
      modelOptions: {
        openaiResponses: {
          reasoning: { effort: 'medium', summary: 'auto' },
          serviceTier: 'auto',
          text: { verbosity: 'low' },
          promptCacheKey: 'tenant:stable-prefix',
          promptCacheRetention: '24h',
          parallelToolCalls: true,
          store: true,
        },
      },
    })
    expect(saveProviderSettings.mock.calls[0][0]).not.toHaveProperty('providerDefaults')
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

  it('saves DeepSeek API mode and official thinking defaults', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: deepseekProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: deepseekProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    expect(within(dialog).getByLabelText('API mode')).not.toHaveTextContent('FIM Beta')
    fireEvent.change(within(dialog).getByLabelText('API mode'), {
      target: { value: 'messages' },
    })
    fireEvent.change(within(dialog).getByLabelText('Thinking'), {
      target: { value: 'disabled' },
    })
    fireEvent.change(within(dialog).getByLabelText('Reasoning effort'), {
      target: { value: 'max' },
    })
    fireEvent.change(within(dialog).getByLabelText('Top P'), {
      target: { value: '0.8' },
    })
    fireEvent.change(within(dialog).getByLabelText('Stop sequences'), {
      target: { value: 'DONE,STOP' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      baseUrl: 'https://api.deepseek.com/anthropic',
      configId: 'cfg-deepseek',
      protocol: 'messages',
      providerDefaults: {
        body: {
          thinking: { type: 'disabled' },
          output_config: { effort: 'max' },
          top_p: 0.8,
          stop_sequences: ['DONE', 'STOP'],
        },
        headers: {},
      },
    })
    expect(saveProviderSettings.mock.calls[0][0].providerDefaults.body).not.toHaveProperty(
      'chat_prefix',
    )
  })

  it('saves Zhipu official chat provider defaults from structured controls', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: zhipuProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: zhipuProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    const reasoningEffort = within(dialog).getByLabelText('Reasoning effort')
    expect(within(reasoningEffort).getByRole('option', { name: 'Max' })).toBeInTheDocument()
    expect(within(reasoningEffort).getByRole('option', { name: 'None' })).toBeInTheDocument()

    fireEvent.change(within(dialog).getByLabelText('Thinking'), { target: { value: 'enabled' } })
    fireEvent.change(within(dialog).getByLabelText('Clear thinking'), {
      target: { value: 'false' },
    })
    fireEvent.change(reasoningEffort, { target: { value: 'xhigh' } })
    fireEvent.change(within(dialog).getByLabelText('Sample output'), {
      target: { value: 'false' },
    })
    fireEvent.change(within(dialog).getByLabelText('Tool stream'), { target: { value: 'true' } })
    fireEvent.change(within(dialog).getByLabelText('Temperature'), { target: { value: '0.8' } })
    fireEvent.change(within(dialog).getByLabelText('Top P'), { target: { value: '0.7' } })
    fireEvent.change(within(dialog).getByLabelText('Max tokens'), { target: { value: '4096' } })
    fireEvent.change(within(dialog).getByLabelText('Stop sequences'), {
      target: { value: 'DONE,STOP' },
    })
    fireEvent.change(within(dialog).getByLabelText('Response format'), {
      target: { value: 'json_object' },
    })
    fireEvent.change(within(dialog).getByLabelText('User ID'), { target: { value: 'user-1' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-zhipu',
      providerId: 'zhipu',
      providerDefaults: {
        body: {
          thinking: { type: 'enabled', clear_thinking: false },
          reasoning_effort: 'xhigh',
          do_sample: false,
          tool_stream: true,
          temperature: 0.8,
          top_p: 0.7,
          max_tokens: 4096,
          stop: ['DONE', 'STOP'],
          response_format: { type: 'json_object' },
          user_id: 'user-1',
        },
        headers: {},
      },
    })
  })

  it('round trips Zhipu disabled thinking and stop defaults', async () => {
    const profile: ProviderConfig = {
      ...zhipuProfile,
      providerDefaults: {
        body: { thinking: { type: 'disabled', clear_thinking: false }, stop: ['DONE'] },
        headers: {},
      },
    }
    const saveProviderSettings = vi.fn().mockResolvedValue({ config: profile, status: 'saved' })
    renderDialog({
      client: { ...createTestCommandClient(), saveProviderSettings },
      profile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    expect(within(dialog).getByLabelText('Thinking')).toHaveValue('disabled')
    expect(within(dialog).getByLabelText('Clear thinking')).toHaveValue('false')
    expect(within(dialog).getByLabelText('Stop sequences')).toHaveValue('DONE')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      providerDefaults: {
        body: { thinking: { type: 'disabled', clear_thinking: false }, stop: ['DONE'] },
      },
    })
  })

  it('saves Doubao official reasoning and service tier defaults with advanced JSON', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: doubaoProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: doubaoProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('Thinking mode'), {
      target: { value: 'auto' },
    })
    fireEvent.change(within(dialog).getByLabelText('Reasoning effort'), {
      target: { value: 'xhigh' },
    })
    fireEvent.change(within(dialog).getByLabelText('Service tier'), {
      target: { value: 'fast' },
    })
    fireEvent.change(within(dialog).getByLabelText('Advanced request body JSON'), {
      target: { value: '{"max_completion_tokens":1024}' },
    })
    expect(within(dialog).queryByLabelText('Advanced request headers JSON')).not.toBeInTheDocument()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-doubao',
      providerDefaults: {
        body: {
          max_completion_tokens: 1024,
          reasoning_effort: 'xhigh',
          service_tier: 'fast',
          thinking: { type: 'auto' },
        },
        headers: {},
      },
      providerId: 'doubao',
    })
  })

  it('shows invalid advanced body JSON without saving', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: doubaoProfile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: doubaoProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('Advanced request body JSON'), {
      target: { value: '{"response_format":' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(within(dialog).getByRole('alert')).toHaveTextContent(/JSON/i))
    expect(saveProviderSettings).not.toHaveBeenCalled()
  })

  it('drops hidden Doubao reasoning defaults when switching to a non-reasoning model', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: {
        ...doubaoProfile,
        modelId: 'doubao-seed-character-260628',
      },
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile: doubaoProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('Thinking mode'), {
      target: { value: 'auto' },
    })
    fireEvent.change(within(dialog).getByLabelText('Reasoning effort'), {
      target: { value: 'xhigh' },
    })
    fireEvent.change(within(dialog).getByLabelText('Advanced request body JSON'), {
      target: { value: '{"max_completion_tokens":1024}' },
    })
    fireEvent.change(within(dialog).getByLabelText('Model'), {
      target: { value: 'doubao-seed-character-260628' },
    })
    expect(within(dialog).queryByLabelText('Thinking mode')).not.toBeInTheDocument()
    expect(within(dialog).queryByLabelText('Reasoning effort')).not.toBeInTheDocument()

    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-doubao',
      modelId: 'doubao-seed-character-260628',
      providerDefaults: {
        body: {
          max_completion_tokens: 1024,
        },
        headers: {},
      },
      providerId: 'doubao',
    })
    expect(saveProviderSettings.mock.calls[0][0].providerDefaults?.body).not.toHaveProperty(
      'thinking',
    )
    expect(saveProviderSettings.mock.calls[0][0].providerDefaults?.body).not.toHaveProperty(
      'reasoning_effort',
    )
  })

  it('prefills Doubao managed defaults and preserves reasoning effort', async () => {
    const profile: ProviderConfig = {
      ...doubaoProfile,
      providerDefaults: {
        body: {
          reasoning_effort: 'xhigh',
          response_format: { type: 'json_object' },
          service_tier: 'fast',
          thinking: { type: 'auto' },
        },
        headers: {
          'x-ark-beta': 'enabled',
        },
      },
    }
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: profile,
      status: 'saved',
    })
    renderDialog({
      client: {
        ...createTestCommandClient(),
        saveProviderSettings,
      },
      profile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    expect(within(dialog).getByLabelText('Thinking mode')).toHaveValue('auto')
    expect(within(dialog).getByLabelText('Reasoning effort')).toHaveValue('xhigh')
    expect(within(dialog).getByLabelText('Service tier')).toHaveValue('fast')
    expect(within(dialog).getByLabelText('Advanced request body JSON')).toHaveValue(
      '{\n  "response_format": {\n    "type": "json_object"\n  }\n}',
    )
    expect(within(dialog).queryByLabelText('Advanced request headers JSON')).not.toBeInTheDocument()

    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      providerDefaults: {
        body: {
          reasoning_effort: 'xhigh',
          response_format: { type: 'json_object' },
          service_tier: 'fast',
          thinking: { type: 'auto' },
        },
        headers: {},
      },
    })
  })

  it('saves Gemini official provider defaults from structured controls', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: geminiProfile,
      status: 'saved',
    })
    renderDialog({
      client: { ...createTestCommandClient(), saveProviderSettings },
      profile: geminiProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.click(within(dialog).getByLabelText('Thinking'))
    fireEvent.change(within(dialog).getByLabelText('Thinking budget'), {
      target: { value: '2048' },
    })
    fireEvent.change(within(dialog).getByLabelText('Thinking level'), {
      target: { value: 'HIGH' },
    })
    fireEvent.change(within(dialog).getByLabelText('Response JSON schema'), {
      target: { value: '{"type":"object","properties":{"ok":{"type":"boolean"}}}' },
    })
    fireEvent.change(within(dialog).getByLabelText('Tool config'), {
      target: { value: '{"functionCallingConfig":{"mode":"AUTO"}}' },
    })
    fireEvent.change(within(dialog).getByLabelText('Safety settings'), {
      target: {
        value:
          '[{"category":"HARM_CATEGORY_DANGEROUS_CONTENT","threshold":"BLOCK_ONLY_HIGH"}]',
      },
    })
    fireEvent.change(within(dialog).getByLabelText('Cached content'), {
      target: { value: 'cachedContents/demo' },
    })
    fireEvent.change(within(dialog).getByLabelText('Service tier'), {
      target: { value: 'standard' },
    })
    fireEvent.click(within(dialog).getByLabelText('Store provider response'))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-gemini',
      providerId: 'gemini',
      providerDefaults: {
        body: {
          thinkingConfig: {
            includeThoughts: true,
            thinkingBudget: 2048,
            thinkingLevel: 'HIGH',
          },
          responseJsonSchema: {
            type: 'object',
            properties: { ok: { type: 'boolean' } },
          },
          toolConfig: { functionCallingConfig: { mode: 'AUTO' } },
          safetySettings: [
            {
              category: 'HARM_CATEGORY_DANGEROUS_CONTENT',
              threshold: 'BLOCK_ONLY_HIGH',
            },
          ],
          cachedContent: 'cachedContents/demo',
          serviceTier: 'standard',
          store: true,
        },
        headers: {},
      },
    })
  })

  it('rejects invalid Gemini JSON config before saving', async () => {
    const saveProviderSettings = vi.fn()
    renderDialog({
      client: { ...createTestCommandClient(), saveProviderSettings },
      profile: geminiProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('Response JSON schema'), {
      target: { value: '{"type":' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(dialog).toHaveTextContent('responseJsonSchema must be valid JSON.')
    })
    expect(saveProviderSettings).not.toHaveBeenCalled()
  })

  it('saves MiniMax selected API mode without hidden protocol defaults', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: minimaxProfile,
      status: 'saved',
    })
    renderDialog({
      client: { ...createTestCommandClient(), saveProviderSettings },
      profile: minimaxProfile,
    })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    fireEvent.change(within(dialog).getByLabelText('API mode'), {
      target: { value: 'chat_completions' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings.mock.calls[0][0]).toMatchObject({
      configId: 'cfg-minimax',
      protocol: 'chat_completions',
    })
  })

  it('updates MiniMax provider options when API mode changes', async () => {
    renderDialog({ profile: minimaxProfile })

    const dialog = screen.getByRole('dialog', { name: 'Edit model configuration' })
    expect(within(dialog).getByLabelText('Service tier')).toBeInTheDocument()
    expect(within(dialog).queryByLabelText('Stop sequences')).not.toBeInTheDocument()

    fireEvent.change(within(dialog).getByLabelText('API mode'), {
      target: { value: 'messages' },
    })

    expect(within(dialog).getByLabelText('Stop sequences')).toBeInTheDocument()
    expect(within(dialog).queryByLabelText('Service tier')).not.toBeInTheDocument()
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
  supportedProtocols: ['responses' as const],
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

const minimaxText01 = {
  ...gpt41,
  displayName: 'MiniMax-Text-01',
  modelId: 'MiniMax-Text-01',
  providerCapabilityMetadata: {
    provider: 'minimax',
    serviceTiers: ['standard', 'priority'],
    protocolSupportedParameters: {
      responses: ['service_tier'],
      chat_completions: ['thinking'],
      messages: ['stop_sequences'],
    },
  },
  supportedParameters: ['service_tier'],
  supportedProtocols: ['responses' as const, 'chat_completions' as const, 'messages' as const],
}

const deepseekModel = {
  ...gpt41,
  protocol: 'chat_completions' as const,
  displayName: 'DeepSeek V4 Pro',
  modelId: 'deepseek-v4-pro',
  supportedParameters: [
    'thinking',
    'reasoning_effort',
    'top_p',
    'stop',
    'response_format',
    'tool_choice',
  ],
}

const zhipuGlm52 = {
  ...gpt41,
  displayName: 'GLM-5.2',
  modelId: 'glm-5.2',
  protocol: 'chat_completions' as const,
  supportedParameters: [
    'thinking',
    'reasoning_effort',
    'do_sample',
    'temperature',
    'top_p',
    'max_tokens',
    'tool_stream',
    'tools',
    'tool_choice',
    'stop',
    'response_format',
    'user_id',
  ],
}

const doubaoSeed21Pro = {
  ...gpt41,
  displayName: 'Doubao Seed 2.1 Pro',
  modelId: 'doubao-seed-2-1-pro-260628',
  supportedParameters: [
    'thinking',
    'reasoning_effort',
    'service_tier',
    'response_format',
    'top_p',
    'stop',
  ],
}

const doubaoSeedCharacter = {
  ...gpt41,
  displayName: 'Doubao Seed Character',
  modelId: 'doubao-seed-character-260628',
  supportedParameters: ['service_tier', 'max_completion_tokens', 'stop', 'top_p'],
}

const gemini25Flash = {
  ...gpt41,
  displayName: 'Gemini 2.5 Flash',
  modelId: 'gemini-2.5-flash',
  protocol: 'generate_content' as const,
  supportedProtocols: ['generate_content' as const],
  supportedParameters: [
    'thinkingConfig',
    'stopSequences',
    'topP',
    'topK',
    'seed',
    'responseMimeType',
    'responseJsonSchema',
    'toolConfig',
    'safetySettings',
    'cachedContent',
    'serviceTier',
    'store',
  ],
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
      models: [qwen37Max, qwen3Max],
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
    {
      defaultBaseUrl: 'https://api.minimaxi.com/v1',
      displayName: 'MiniMax',
      models: [minimaxText01],
      providerId: 'minimax',
      runtimeCapability: {
        authScheme: 'bearer',
        baseUrlRegions: [{ id: 'global', label: 'Global', baseUrl: 'https://api.minimaxi.com/v1' }],
        supportsLiveValidation: true,
        supportsStreamingValidation: true,
        secretRevealSupported: true,
      },
      serviceCapabilities: [],
      sourceUrl: 'https://platform.minimaxi.com/document',
      verifiedDate: '2026-07-09',
    },
    {
      defaultBaseUrl: 'https://api.deepseek.com',
      displayName: 'DeepSeek',
      models: [deepseekModel],
      providerId: 'deepseek',
      runtimeCapability: {
        authScheme: 'bearer',
        baseUrlRegions: [
          { id: 'default', label: 'Default', baseUrl: 'https://api.deepseek.com' },
          {
            id: 'anthropic',
            label: 'Anthropic',
            baseUrl: 'https://api.deepseek.com/anthropic',
          },
          { id: 'beta', label: 'Beta', baseUrl: 'https://api.deepseek.com/beta' },
        ],
        supportsLiveValidation: true,
        supportsStreamingValidation: true,
        secretRevealSupported: true,
      },
      serviceCapabilities: [],
      sourceUrl: 'https://api-docs.deepseek.com',
      verifiedDate: '2026-07-09',
    },
    {
      defaultBaseUrl: 'https://open.bigmodel.cn/api/paas/v4',
      displayName: 'Zhipu',
      models: [zhipuGlm52],
      providerId: 'zhipu',
      runtimeCapability: {
        authScheme: 'bearer',
        baseUrlRegions: [
          {
            id: 'default',
            label: 'Default',
            baseUrl: 'https://open.bigmodel.cn/api/paas/v4',
          },
        ],
        supportsLiveValidation: true,
        supportsStreamingValidation: true,
        secretRevealSupported: true,
      },
      serviceCapabilities: [],
      sourceUrl: 'https://docs.bigmodel.cn/api-reference/模型-api/对话补全',
      verifiedDate: '2026-07-09',
    },
    {
      defaultBaseUrl: 'https://ark.cn-beijing.volces.com/api/v3',
      displayName: 'Doubao',
      models: [doubaoSeed21Pro, doubaoSeedCharacter],
      providerId: 'doubao',
      runtimeCapability: {
        authScheme: 'bearer',
        baseUrlRegions: [
          {
            id: 'cn-beijing',
            label: 'China Beijing',
            baseUrl: 'https://ark.cn-beijing.volces.com/api/v3',
          },
        ],
        supportsLiveValidation: true,
        supportsStreamingValidation: true,
        secretRevealSupported: true,
      },
      serviceCapabilities: [],
      sourceUrl: 'https://www.volcengine.com/docs/82379/1494384',
      verifiedDate: '2026-07-08',
    },
    {
      defaultBaseUrl: 'https://generativelanguage.googleapis.com',
      displayName: 'Gemini',
      models: [gemini25Flash],
      providerId: 'gemini',
      runtimeCapability: {
        authScheme: 'api_key',
        baseUrlRegions: [
          {
            id: 'default',
            label: 'Default',
            baseUrl: 'https://generativelanguage.googleapis.com',
          },
        ],
        supportsLiveValidation: true,
        supportsStreamingValidation: true,
        secretRevealSupported: true,
      },
      serviceCapabilities: [],
      sourceUrl: 'https://ai.google.dev/gemini-api/docs/models',
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

const deepseekProfile: ProviderConfig = {
  id: 'cfg-deepseek',
  providerId: 'deepseek',
  modelId: 'deepseek-v4-pro',
  displayName: 'Primary DeepSeek',
  baseUrl: 'https://api.deepseek.com',
  hasApiKey: true,
  hasOfficialQuotaApiKey: false,
  isDefault: false,
  protocol: 'chat_completions',
  providerDefaults: {
    body: {},
    headers: {},
  },
  modelDescriptor: deepseekModel,
}

const zhipuProfile: ProviderConfig = {
  id: 'cfg-zhipu',
  providerId: 'zhipu',
  modelId: 'glm-5.2',
  displayName: 'Primary GLM',
  baseUrl: 'https://open.bigmodel.cn/api/paas/v4',
  hasApiKey: true,
  hasOfficialQuotaApiKey: false,
  isDefault: false,
  protocol: 'chat_completions',
  providerDefaults: {
    body: {},
    headers: {},
  },
  modelDescriptor: zhipuGlm52,
}

const doubaoProfile: ProviderConfig = {
  id: 'cfg-doubao',
  providerId: 'doubao',
  modelId: 'doubao-seed-2-1-pro-260628',
  displayName: 'Primary Doubao',
  baseUrl: 'https://ark.cn-beijing.volces.com/api/v3',
  hasApiKey: true,
  hasOfficialQuotaApiKey: false,
  isDefault: false,
  protocol: 'responses',
  providerDefaults: {
    body: {},
    headers: {},
  },
  modelDescriptor: doubaoSeed21Pro,
}

const minimaxProfile: ProviderConfig = {
  id: 'cfg-minimax',
  providerId: 'minimax',
  modelId: 'MiniMax-Text-01',
  displayName: 'Primary MiniMax',
  baseUrl: 'https://api.minimaxi.com/v1',
  hasApiKey: true,
  hasOfficialQuotaApiKey: false,
  isDefault: false,
  protocol: 'responses',
  providerDefaults: { body: {}, headers: {} },
  modelDescriptor: minimaxText01,
}

const geminiProfile: ProviderConfig = {
  id: 'cfg-gemini',
  providerId: 'gemini',
  modelId: 'gemini-2.5-flash',
  displayName: 'Primary Gemini',
  baseUrl: 'https://generativelanguage.googleapis.com',
  hasApiKey: true,
  hasOfficialQuotaApiKey: false,
  isDefault: false,
  protocol: 'generate_content',
  providerDefaults: { body: {}, headers: {} },
  modelDescriptor: gemini25Flash,
}
