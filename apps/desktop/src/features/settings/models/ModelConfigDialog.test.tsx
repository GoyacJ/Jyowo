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
  conversationCapability: modelCapability,
  contextWindow: 128000,
  displayName: 'GPT-4.1',
  lifecycle: { kind: 'stable' as const },
  maxOutputTokens: 8192,
  modelId: 'gpt-4.1',
  runtimeStatus: { kind: 'runnable' as const },
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
