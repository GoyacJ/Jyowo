import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { onProjectWorkspaceChanged } from '@/features/workspace/reset-workspace-scope'
import type {
  CommandClient,
  ConversationModelCapability,
  ListProviderSettingsResponse,
  ModelProviderCatalogResponse,
} from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'
import { ProviderSettingsForm } from './ProviderSettingsForm'

type ModelCatalogEntry = ModelProviderCatalogResponse['providers'][number]['models'][number]

function renderProviderSettingsForm(commandClient: CommandClient = createMockCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { gcTime: 0, retry: false },
      mutations: { retry: false },
    },
  })

  return {
    queryClient,
    ...render(
      <QueryClientProvider client={queryClient}>
        <CommandClientProvider client={commandClient}>
          <ProviderSettingsForm />
        </CommandClientProvider>
      </QueryClientProvider>,
    ),
  }
}

function getQueryClientCacheSnapshot(queryClient: QueryClient) {
  return JSON.stringify({
    mutations: queryClient
      .getMutationCache()
      .getAll()
      .map((mutation) => mutation.state),
    queries: queryClient
      .getQueryCache()
      .getAll()
      .map((query) => query.state.data),
  })
}

function createDeferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, reject, resolve }
}

async function findReadyNewConfigurationButton() {
  const button = await screen.findByRole('button', { name: 'New configuration' })
  await waitFor(() => expect(button).toBeEnabled())
  return button
}

async function openCreateDialog() {
  fireEvent.click(await findReadyNewConfigurationButton())
  return screen.findByRole('dialog', { name: 'Create model configuration' })
}

async function findSavedConfigurations() {
  return screen.findByRole('region', { name: 'Saved configurations' })
}

async function findProfileDetails(profileName: RegExp | string) {
  const detail = await screen.findByRole('region', { name: 'Model configuration details' })
  await within(detail).findByRole('heading', { name: profileName })
  return detail
}

const textConversationCapability: ConversationModelCapability = {
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

const providerRuntimeCapability = {
  authScheme: 'bearer',
  baseUrlRegions: [{ id: 'default', label: 'Default', baseUrl: 'https://api.example.com' }],
  supportsLiveValidation: false,
  supportsStreamingValidation: true,
  secretRevealSupported: true,
}

const openAiModelDescriptor: ModelCatalogEntry = {
  protocol: 'responses',
  conversationCapability: {
    ...textConversationCapability,
    inputModalities: ['text', 'image'],
    maxOutputTokens: 16384,
    toolCalling: true,
    structuredOutput: true,
  },
  contextWindow: 128000,
  displayName: 'GPT-5.4 mini',
  lifecycle: { kind: 'stable' },
  maxOutputTokens: 16384,
  modelId: 'gpt-5.4-mini',
  runtimeStatus: { kind: 'runnable' },
}

const localModelDescriptor: ModelCatalogEntry = {
  protocol: 'messages',
  conversationCapability: textConversationCapability,
  contextWindow: 128000,
  displayName: 'Llama 3.1',
  lifecycle: { kind: 'stable' },
  maxOutputTokens: 8192,
  modelId: 'llama3.1',
  runtimeStatus: { kind: 'runnable' },
}

describe('ProviderSettingsForm', () => {
  it('lets Minimax choose domestic or international base URL presets', async () => {
    const client = {
      ...createMockCommandClient(),
      listModelProviderCatalog: vi.fn().mockResolvedValue({
        providers: [
          {
            defaultBaseUrl: 'https://api.minimax.io',
            displayName: 'Minimax',
            models: [
              {
                protocol: 'chat_completions',
                conversationCapability: {
                  ...textConversationCapability,
                  contextWindow: 1000000,
                  reasoning: true,
                },
                contextWindow: 1000000,
                displayName: 'MiniMax M1',
                lifecycle: { kind: 'stable' },
                maxOutputTokens: 8192,
                modelId: 'MiniMax-M1',
                runtimeStatus: { kind: 'runnable' },
              },
            ],
            providerId: 'minimax',
            runtimeCapability: {
              ...providerRuntimeCapability,
              baseUrlRegions: [
                { id: 'global', label: 'Global', baseUrl: 'https://api.minimax.io' },
                { id: 'cn', label: 'China', baseUrl: 'https://api.minimaxi.com' },
              ],
            },
            serviceCapabilities: [],
            sourceUrl: 'https://api.minimax.io',
            verifiedDate: '2026-06-21',
          },
        ],
      }),
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: null,
        configs: [],
      }),
    }

    renderProviderSettingsForm(client)

    const dialog = await openCreateDialog()

    expect(within(dialog).getByText('Base URL region')).toBeInTheDocument()
    expect(within(dialog).getByLabelText('Base URL')).toHaveValue('https://api.minimax.io')

    fireEvent.click(within(dialog).getByRole('button', { name: 'China' }))
    expect(within(dialog).getByLabelText('Base URL')).toHaveValue('https://api.minimaxi.com')

    fireEvent.click(within(dialog).getByRole('button', { name: 'International' }))
    expect(within(dialog).getByLabelText('Base URL')).toHaveValue('https://api.minimax.io')
  })

  it('shows provider service capabilities separately from model capabilities', async () => {
    const client = {
      ...createMockCommandClient(),
      listModelProviderCatalog: vi.fn().mockResolvedValue({
        providers: [
          {
            defaultBaseUrl: 'https://api.minimax.io',
            displayName: 'Minimax',
            models: [
              {
                protocol: 'chat_completions',
                conversationCapability: textConversationCapability,
                contextWindow: 1000000,
                displayName: 'MiniMax M3',
                lifecycle: { kind: 'stable' },
                maxOutputTokens: 8192,
                modelId: 'MiniMax-M3',
                runtimeStatus: { kind: 'runnable' },
              },
            ],
            providerId: 'minimax',
            runtimeCapability: providerRuntimeCapability,
            serviceCapabilities: [
              {
                operationId: 'minimax.image_generation',
                category: 'image',
                inputModalities: ['text', 'image'],
                outputArtifact: 'image',
                execution: 'sync',
                requiresPolling: false,
                permissionSubject: 'network:minimax',
                costRisk: 'high',
              },
            ],
            sourceUrl: 'https://api.minimax.io',
            verifiedDate: '2026-06-21',
          },
        ],
      }),
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: 'minimax',
        configs: [
          {
            protocol: 'chat_completions',
            baseUrl: 'https://api.minimax.io',
            displayName: 'Minimax',
            hasApiKey: true,
            id: 'minimax',
            isDefault: true,
            modelDescriptor: {
              protocol: 'chat_completions',
              conversationCapability: textConversationCapability,
              contextWindow: 1000000,
              displayName: 'MiniMax M3',
              lifecycle: { kind: 'stable' },
              maxOutputTokens: 8192,
              modelId: 'MiniMax-M3',
              runtimeStatus: { kind: 'runnable' },
            },
            modelId: 'MiniMax-M3',
            providerId: 'minimax',
          },
        ],
      }),
    }

    renderProviderSettingsForm(client)

    const detail = await findProfileDetails('Minimax')

    expect(within(detail).getByText('Provider services')).toBeInTheDocument()
    expect(within(detail).getByText('minimax.image_generation')).toBeInTheDocument()
    expect(within(detail).getByText('Image')).toBeInTheDocument()
  })

  it('keeps creation in a dialog and shows selected profile details', async () => {
    const client = createMockCommandClient({
      providerSettingsList: {
        defaultConfigId: 'openai',
        configs: [
          {
            protocol: 'responses',
            baseUrl: 'https://api.openai.com',
            displayName: 'OpenAI',
            hasApiKey: true,
            id: 'openai',
            isDefault: true,
            modelDescriptor: openAiModelDescriptor,
            modelId: 'gpt-5.4-mini',
            providerId: 'openai',
          },
        ],
      },
    })

    renderProviderSettingsForm(client)

    const profileList = await findSavedConfigurations()
    const openAiProfileButton = await within(profileList).findByRole('button', { name: /OpenAI/ })
    const detail = await findProfileDetails('OpenAI')

    expect(profileList).toHaveClass('rounded-md')
    expect(profileList).toHaveClass('border')
    expect(detail).toHaveClass('rounded-md')
    expect(detail).toHaveClass('border')
    expect(profileList).toContainElement(openAiProfileButton)
    expect(within(detail).getAllByText('OpenAI').length).toBeGreaterThan(0)
    expect(within(detail).getByRole('heading', { name: 'OpenAI' })).toBeInTheDocument()
    expect(
      within(detail).queryByRole('heading', { name: 'Model configuration' }),
    ).not.toBeInTheDocument()
    expect(within(detail).getByRole('combobox', { name: 'Provider' })).toHaveValue('openai')
    await waitFor(() =>
      expect(within(detail).getByRole('combobox', { name: 'Model' })).toHaveValue('gpt-5.4-mini'),
    )
    expect(within(detail).getByRole('button', { name: 'Save' })).toBeEnabled()
    expect(
      screen.queryByRole('dialog', { name: 'Create model configuration' }),
    ).not.toBeInTheDocument()

    fireEvent.click(within(profileList).getByRole('button', { name: 'New configuration' }))

    const dialog = await screen.findByRole('dialog', { name: 'Create model configuration' })
    expect(within(dialog).getByLabelText('Provider')).toBeInTheDocument()
    expect(within(dialog).getByRole('button', { name: 'Save' })).toBeInTheDocument()
  })

  it('saves edits from the selected profile details without resubmitting a saved key', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: {
        protocol: 'responses',
        baseUrl: 'https://gateway.example.com',
        displayName: 'OpenAI gateway',
        hasApiKey: true,
        id: 'openai',
        isDefault: true,
        modelDescriptor: openAiModelDescriptor,
        modelId: 'gpt-5.4-mini',
        providerId: 'openai',
      },
      status: 'saved',
    })
    const client = {
      ...createMockCommandClient(),
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: 'openai',
        configs: [
          {
            protocol: 'responses',
            baseUrl: 'https://api.openai.com',
            displayName: 'OpenAI',
            hasApiKey: true,
            id: 'openai',
            isDefault: true,
            modelDescriptor: openAiModelDescriptor,
            modelId: 'gpt-5.4-mini',
            providerId: 'openai',
          },
        ],
      }),
      saveProviderSettings,
    }

    renderProviderSettingsForm(client)

    const detail = await findProfileDetails('OpenAI')
    const saveButton = within(detail).getByRole('button', { name: 'Save' })
    await waitFor(() => expect(saveButton).toBeEnabled())
    await waitFor(() =>
      expect(within(detail).getByRole('combobox', { name: 'Model' })).toHaveValue('gpt-5.4-mini'),
    )
    fireEvent.change(within(detail).getByRole('textbox', { name: 'Configuration name' }), {
      target: { value: 'OpenAI gateway' },
    })
    fireEvent.change(within(detail).getByRole('textbox', { name: 'Base URL' }), {
      target: { value: 'https://gateway.example.com' },
    })
    fireEvent.click(saveButton)

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings).toHaveBeenCalledWith({
      baseUrl: 'https://gateway.example.com',
      configId: 'openai',
      displayName: 'OpenAI gateway',
      modelId: 'gpt-5.4-mini',
      providerId: 'openai',
      setDefault: true,
    })
    expect(saveProviderSettings.mock.calls[0][0]).not.toHaveProperty('apiKey')
    expect(screen.queryByText('Provider saved.')).not.toBeInTheDocument()
  })

  it('does not write a completed save into provider settings cache after workspace reset', async () => {
    const initialSettings: ListProviderSettingsResponse = {
      defaultConfigId: 'openai',
      configs: [
        {
          protocol: 'responses',
          baseUrl: 'https://api.openai.com',
          displayName: 'OpenAI',
          hasApiKey: true,
          id: 'openai',
          isDefault: true,
          modelDescriptor: openAiModelDescriptor,
          modelId: 'gpt-5.4-mini',
          providerId: 'openai',
        },
      ],
    }
    const resetSettings: ListProviderSettingsResponse = {
      defaultConfigId: null,
      configs: [],
    }
    const saveDeferred =
      createDeferred<Awaited<ReturnType<CommandClient['saveProviderSettings']>>>()
    const saveProviderSettings = vi.fn(() => saveDeferred.promise)
    const client = {
      ...createMockCommandClient(),
      listProviderSettings: vi
        .fn()
        .mockResolvedValueOnce(initialSettings)
        .mockResolvedValue(resetSettings),
      saveProviderSettings,
    }

    const { queryClient } = renderProviderSettingsForm(client)

    const detail = await findProfileDetails('OpenAI')
    const saveButton = within(detail).getByRole('button', { name: 'Save' })
    await waitFor(() => expect(saveButton).toBeEnabled())
    fireEvent.change(within(detail).getByRole('textbox', { name: 'Configuration name' }), {
      target: { value: 'OpenAI from old workspace' },
    })
    fireEvent.click(saveButton)

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    await onProjectWorkspaceChanged(queryClient, async () => undefined)
    queryClient.setQueryData<ListProviderSettingsResponse>(['provider-settings'], resetSettings)
    expect(queryClient.getQueryData<ListProviderSettingsResponse>(['provider-settings'])).toEqual(
      resetSettings,
    )

    saveDeferred.resolve({
      config: {
        protocol: 'responses',
        baseUrl: 'https://api.openai.com',
        displayName: 'OpenAI from old workspace',
        hasApiKey: true,
        id: 'openai',
        isDefault: true,
        modelDescriptor: openAiModelDescriptor,
        modelId: 'gpt-5.4-mini',
        providerId: 'openai',
      },
      status: 'saved',
    })

    await waitFor(() => expect(queryClient.isMutating()).toBe(0))
    expect(
      queryClient
        .getQueryData<ListProviderSettingsResponse>(['provider-settings'])
        ?.configs.some((profile) => profile.displayName === 'OpenAI from old workspace') ?? false,
    ).toBe(false)
  })

  it('renders provider models from the backend catalog', async () => {
    const client = {
      ...createMockCommandClient(),
      listModelProviderCatalog: vi.fn().mockResolvedValue({
        providers: [
          {
            defaultBaseUrl: 'https://api.openai.com',
            displayName: 'OpenAI',
            models: [
              {
                protocol: 'responses',
                conversationCapability: {
                  ...textConversationCapability,
                  maxOutputTokens: 16384,
                },
                contextWindow: 128000,
                displayName: 'GPT-5.4 mini',
                lifecycle: { kind: 'stable' },
                maxOutputTokens: 16384,
                modelId: 'gpt-5.4-mini',
                runtimeStatus: { kind: 'runnable' },
              },
            ],
            providerId: 'openai',
            runtimeCapability: providerRuntimeCapability,
            serviceCapabilities: [],
            sourceUrl: 'https://platform.openai.com/docs/models',
            verifiedDate: '2026-06-21',
          },
        ],
      }),
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: null,
        configs: [],
      }),
    }

    renderProviderSettingsForm(client)

    const dialog = await openCreateDialog()

    expect(within(dialog).getByRole('option', { name: 'OpenAI' })).toBeInTheDocument()
    expect(within(dialog).getByRole('option', { name: 'GPT-5.4 mini' })).toBeInTheDocument()
    expect(within(dialog).getByLabelText('Base URL')).toHaveValue('https://api.openai.com')
  })

  it('rejects invalid input before calling the backend', async () => {
    const saveProviderSettings = vi.fn()
    const client = {
      ...createMockCommandClient(),
      listModelProviderCatalog: vi.fn().mockResolvedValue({
        providers: [
          {
            defaultBaseUrl: 'https://api.openai.com',
            displayName: 'OpenAI',
            models: [],
            providerId: 'openai',
            runtimeCapability: providerRuntimeCapability,
            serviceCapabilities: [],
            sourceUrl: 'https://platform.openai.com/docs/models',
            verifiedDate: '2026-06-21',
          },
        ],
      }),
      saveProviderSettings,
    }

    renderProviderSettingsForm(client)

    const dialog = await openCreateDialog()
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'Save' })).toBeEnabled())
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('Model is required.')).toBeInTheDocument()
    expect(screen.getByText('API key is required.')).toBeInTheDocument()
    expect(saveProviderSettings).not.toHaveBeenCalled()
  })

  it('shows a saved dynamic OpenRouter descriptor in details when catalog omits it', async () => {
    const dynamicDescriptor = {
      protocol: 'chat_completions',
      conversationCapability: textConversationCapability,
      contextWindow: 128000,
      displayName: 'Dynamic OpenRouter model',
      lifecycle: { kind: 'stable' },
      maxOutputTokens: 8192,
      modelId: 'dynamic/provider-model',
      runtimeStatus: { kind: 'runnable' },
    } as const
    const client = {
      ...createMockCommandClient(),
      listModelProviderCatalog: vi.fn().mockResolvedValue({
        providers: [
          {
            defaultBaseUrl: 'https://openrouter.ai/api',
            displayName: 'OpenRouter',
            models: [],
            providerId: 'openrouter',
            runtimeCapability: providerRuntimeCapability,
            serviceCapabilities: [],
            sourceUrl: 'https://openrouter.ai/docs/api-reference/list-available-models',
            verifiedDate: '2026-06-21',
          },
        ],
      }),
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: 'openrouter',
        configs: [
          {
            protocol: 'chat_completions',
            displayName: 'OpenRouter dynamic',
            id: 'openrouter',
            isDefault: true,
            modelDescriptor: dynamicDescriptor,
            modelId: 'dynamic/provider-model',
            providerId: 'openrouter',
          },
        ],
      }),
    }

    renderProviderSettingsForm(client)

    const detail = await findProfileDetails('OpenRouter dynamic')

    expect(within(detail).getAllByText('OpenRouter dynamic').length).toBeGreaterThan(0)
    await waitFor(() =>
      expect(within(detail).getByRole('combobox', { name: 'Provider' })).toHaveValue('openrouter'),
    )
    await waitFor(() =>
      expect(within(detail).getByRole('combobox', { name: 'Model' })).toHaveValue(
        'dynamic/provider-model',
      ),
    )
    expect(within(detail).getByText('Dynamic OpenRouter model')).toBeInTheDocument()
  })

  it('shows model capabilities in selected profile details', async () => {
    const client = {
      ...createMockCommandClient(),
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: 'openai',
        configs: [
          {
            protocol: 'responses',
            baseUrl: 'https://api.openai.com',
            displayName: 'OpenAI',
            hasApiKey: true,
            id: 'openai',
            isDefault: true,
            modelDescriptor: openAiModelDescriptor,
            modelId: 'gpt-4o-mini',
            providerId: 'openai',
          },
        ],
      }),
    }

    renderProviderSettingsForm(client)

    const detail = await findProfileDetails('OpenAI')

    expect(within(detail).getByText('Capabilities')).toBeInTheDocument()
    expect(within(detail).getByText('Tools')).toBeInTheDocument()
    expect(within(detail).getByText('Vision')).toBeInTheDocument()
    expect(within(detail).getByText('Thinking')).toBeInTheDocument()
    expect(within(detail).getByText('Video input')).toBeInTheDocument()
    expect(within(detail).getByText('Prompt cache')).toBeInTheDocument()
    expect(within(detail).getAllByText('Supported').length).toBeGreaterThan(0)
    expect(within(detail).getAllByText('Unsupported').length).toBeGreaterThan(0)
  })

  it('sets a selected saved configuration as the default without resubmitting its key', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: {
        protocol: 'responses',
        baseUrl: 'https://api.openai.com',
        displayName: 'OpenAI',
        hasApiKey: true,
        id: 'openai',
        isDefault: true,
        modelDescriptor: openAiModelDescriptor,
        modelId: 'gpt-4o-mini',
        providerId: 'openai',
      },
      status: 'saved',
    })
    const client = {
      ...createMockCommandClient(),
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: 'local',
        configs: [
          {
            protocol: 'responses',
            baseUrl: 'https://api.openai.com',
            displayName: 'OpenAI',
            hasApiKey: true,
            id: 'openai',
            isDefault: false,
            modelDescriptor: openAiModelDescriptor,
            modelId: 'gpt-4o-mini',
            providerId: 'openai',
          },
          {
            protocol: 'messages',
            baseUrl: 'http://localhost:11434',
            displayName: 'Local',
            hasApiKey: false,
            id: 'local',
            isDefault: true,
            modelDescriptor: localModelDescriptor,
            modelId: 'llama3.1',
            providerId: 'local-llama',
          },
        ],
      }),
      saveProviderSettings,
    }

    renderProviderSettingsForm(client)

    const profileList = await findSavedConfigurations()
    fireEvent.click(await within(profileList).findByRole('button', { name: /OpenAI/ }))
    const detail = await findProfileDetails('OpenAI')
    const setDefaultButton = within(detail).getByRole('button', { name: 'Set as default' })
    const checkButton = within(detail).getByRole('button', { name: 'Check' })
    expect(setDefaultButton.compareDocumentPosition(checkButton)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    )

    fireEvent.click(setDefaultButton)

    await waitFor(() => expect(saveProviderSettings).toHaveBeenCalledTimes(1))
    expect(saveProviderSettings).toHaveBeenCalledWith({
      baseUrl: 'https://api.openai.com',
      configId: 'openai',
      displayName: 'OpenAI',
      modelId: 'gpt-4o-mini',
      providerId: 'openai',
      setDefault: true,
    })
    expect(saveProviderSettings.mock.calls[0][0]).not.toHaveProperty('apiKey')
    await waitFor(() => {
      expect(within(profileList).getByRole('button', { name: /OpenAI/ })).toHaveTextContent(
        'Default',
      )
    })
  })

  it('disables submit while backend save is pending', async () => {
    const saveProviderSettings = vi.fn(
      () =>
        new Promise<Awaited<ReturnType<CommandClient['saveProviderSettings']>>>((resolve) => {
          window.setTimeout(
            () =>
              resolve({
                config: {
                  protocol: 'responses',
                  baseUrl: 'https://api.openai.com',
                  displayName: 'OpenAI',
                  hasApiKey: true,
                  id: 'openai',
                  isDefault: true,
                  modelDescriptor: openAiModelDescriptor,
                  modelId: 'gpt-5.4-mini',
                  providerId: 'openai',
                },
                status: 'saved',
              }),
            25,
          )
        }),
    )
    const client = {
      ...createMockCommandClient(),
      saveProviderSettings,
    }

    renderProviderSettingsForm(client)

    const dialog = await openCreateDialog()
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'Save' })).toBeEnabled())
    fireEvent.change(within(dialog).getByLabelText('API key'), {
      target: { value: 'provider-test-token' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    expect(screen.getByRole('button', { name: 'Saving' })).toBeDisabled()
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'Save' })).toBeEnabled())
  })

  it('surfaces backend errors without keeping the submitted key visible', async () => {
    const client = {
      ...createMockCommandClient(),
      saveProviderSettings: vi.fn().mockRejectedValue({
        code: 'INVALID_PAYLOAD',
        message: 'modelId must be supported by the selected provider',
      }),
    }

    renderProviderSettingsForm(client)

    const dialog = await openCreateDialog()
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'Save' })).toBeEnabled())
    fireEvent.change(within(dialog).getByLabelText('API key'), {
      target: { value: 'provider-test-token' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    expect(
      await screen.findByText('modelId must be supported by the selected provider'),
    ).toBeInTheDocument()
    expect(screen.getByLabelText('API key')).toHaveValue('')
    expect(screen.queryByText('provider-test-token')).not.toBeInTheDocument()
  })

  it('shows saved secret reference and masks the raw key after save', async () => {
    const rawKey = 'provider-test-token'
    const client = {
      ...createMockCommandClient(),
      saveProviderSettings: vi.fn().mockResolvedValue({
        config: {
          protocol: 'responses',
          baseUrl: 'https://api.openai.com',
          displayName: 'OpenAI',
          hasApiKey: true,
          id: 'openai',
          isDefault: true,
          modelDescriptor: openAiModelDescriptor,
          modelId: 'gpt-5.4-mini',
          providerId: 'openai',
        },
        status: 'saved',
      }),
    }

    const { queryClient } = renderProviderSettingsForm(client)

    const dialog = await openCreateDialog()
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'Save' })).toBeEnabled())
    fireEvent.change(within(dialog).getByLabelText('API key'), {
      target: { value: rawKey },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'Create model configuration' }),
      ).not.toBeInTheDocument(),
    )
    expect(screen.queryByText('Provider saved.')).not.toBeInTheDocument()
    expect(
      screen.queryByText('The API key is saved in the workspace config file and can be viewed.'),
    ).not.toBeInTheDocument()
    expect(
      screen.queryByRole('dialog', { name: 'Create model configuration' }),
    ).not.toBeInTheDocument()
    expect(screen.queryByDisplayValue(rawKey)).not.toBeInTheDocument()
    expect(screen.queryByText(rawKey)).not.toBeInTheDocument()
    expect(getQueryClientCacheSnapshot(queryClient)).not.toContain(rawKey)
  })

  it('keeps saved keys masked and out of editable state', async () => {
    const requestProviderConfigApiKeyReveal = vi.fn()
    const getProviderConfigApiKey = vi.fn()
    const client = {
      ...createMockCommandClient(),
      getProviderConfigApiKey,
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: 'openai',
        configs: [
          {
            protocol: 'responses',
            baseUrl: 'https://api.openai.com',
            displayName: 'OpenAI',
            hasApiKey: true,
            id: 'openai',
            isDefault: true,
            modelDescriptor: openAiModelDescriptor,
            modelId: 'gpt-5.4-mini',
            providerId: 'openai',
          },
        ],
      }),
      requestProviderConfigApiKeyReveal,
    }

    renderProviderSettingsForm(client)

    const detail = await findProfileDetails('OpenAI')
    const apiKeyInput = within(detail).getByLabelText('API key')
    const savedKeyMask = '\u2022'.repeat(32)

    expect(apiKeyInput).toHaveValue('')
    expect(apiKeyInput).toHaveAttribute('type', 'password')
    expect(screen.getByText(savedKeyMask)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'View key' })).toBeInTheDocument()
    expect(requestProviderConfigApiKeyReveal).not.toHaveBeenCalled()
    expect(getProviderConfigApiKey).not.toHaveBeenCalled()
  })

  it('reveals a saved key only after explicit request and clears it when the profile changes', async () => {
    const rawKey = 'provider-test-token'
    const requestProviderConfigApiKeyReveal = vi.fn().mockResolvedValue({
      configId: 'openai',
      expiresInSeconds: 60,
      revealToken: 'reveal-token-openai',
      status: 'ready',
    })
    const getProviderConfigApiKey = vi.fn().mockResolvedValue({
      apiKey: rawKey,
      configId: 'openai',
    })
    const client = {
      ...createMockCommandClient(),
      getProviderConfigApiKey,
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: 'openai',
        configs: [
          {
            protocol: 'responses',
            baseUrl: 'https://api.openai.com',
            displayName: 'OpenAI',
            hasApiKey: true,
            id: 'openai',
            isDefault: true,
            modelDescriptor: openAiModelDescriptor,
            modelId: 'gpt-5.4-mini',
            providerId: 'openai',
          },
          {
            protocol: 'messages',
            baseUrl: 'http://localhost:11434',
            displayName: 'Local',
            hasApiKey: false,
            id: 'local',
            isDefault: false,
            modelDescriptor: localModelDescriptor,
            modelId: 'llama3.1',
            providerId: 'local-llama',
          },
        ],
      }),
      requestProviderConfigApiKeyReveal,
    }

    const { queryClient } = renderProviderSettingsForm(client)

    const profileList = await findSavedConfigurations()
    const detail = await findProfileDetails('OpenAI')
    expect(screen.queryByText(rawKey)).not.toBeInTheDocument()

    fireEvent.click(within(detail).getByRole('button', { name: 'View key' }))

    await waitFor(() => expect(requestProviderConfigApiKeyReveal).toHaveBeenCalledWith('openai'))
    expect(getProviderConfigApiKey).toHaveBeenCalledWith('openai', 'reveal-token-openai')
    expect(await screen.findByText(rawKey)).toBeInTheDocument()
    expect(getQueryClientCacheSnapshot(queryClient)).not.toContain(rawKey)

    fireEvent.click(within(profileList).getByRole('button', { name: /Local/ }))

    expect(screen.queryByText(rawKey)).not.toBeInTheDocument()
  })

  it('checks selected provider metadata without implying network latency', async () => {
    const validateProviderSettings = vi.fn().mockResolvedValue({
      modelId: 'gpt-4o-mini',
      providerId: 'openai',
      status: 'accepted',
    })
    const client = {
      ...createMockCommandClient(),
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: 'openai',
        configs: [
          {
            protocol: 'responses',
            baseUrl: 'https://api.openai.com',
            displayName: 'OpenAI',
            hasApiKey: true,
            id: 'openai',
            isDefault: true,
            modelDescriptor: openAiModelDescriptor,
            modelId: 'gpt-4o-mini',
            providerId: 'openai',
          },
        ],
      }),
      validateProviderSettings,
    }

    renderProviderSettingsForm(client)

    const detail = await findProfileDetails('OpenAI')
    fireEvent.click(within(detail).getByRole('button', { name: 'Check' }))

    await waitFor(() =>
      expect(validateProviderSettings).toHaveBeenCalledWith({
        modelId: 'gpt-4o-mini',
        providerId: 'openai',
      }),
    )
    expect(await screen.findByRole('status')).toHaveTextContent('Check accepted')
    expect(screen.getByRole('status')).toHaveTextContent('Provider metadata accepted.')
    expect(screen.getByRole('status')).not.toHaveTextContent(/\d+ ms/)
    expect(within(detail).queryByText(/\d+ ms/)).not.toBeInTheDocument()

    const dialog = await openCreateDialog()
    expect(within(dialog).queryByRole('button', { name: 'Check' })).not.toBeInTheDocument()
  })

  it('does not auto-check selected profiles or show latency in the profile list', async () => {
    const validateProviderSettings = vi.fn().mockResolvedValue({
      modelId: 'gpt-4o-mini',
      providerId: 'openai',
      status: 'accepted',
    })
    const client = {
      ...createMockCommandClient(),
      listProviderSettings: vi.fn().mockResolvedValue({
        defaultConfigId: 'openai',
        configs: [
          {
            protocol: 'responses',
            baseUrl: 'https://api.openai.com',
            displayName: 'OpenAI',
            hasApiKey: true,
            id: 'openai',
            isDefault: true,
            modelDescriptor: openAiModelDescriptor,
            modelId: 'gpt-4o-mini',
            providerId: 'openai',
          },
          {
            protocol: 'messages',
            baseUrl: 'http://localhost:11434',
            displayName: 'Local',
            hasApiKey: false,
            id: 'local',
            isDefault: false,
            modelDescriptor: localModelDescriptor,
            modelId: 'llama3.1',
            providerId: 'local-llama',
          },
        ],
      }),
      validateProviderSettings,
    }

    renderProviderSettingsForm(client)

    const profileList = await findSavedConfigurations()
    const openAiProfile = await within(profileList).findByRole('button', { name: /OpenAI/ })

    expect(openAiProfile).not.toHaveTextContent(/\d+ ms/)
    expect(validateProviderSettings).not.toHaveBeenCalled()
    expect(screen.queryByRole('status')).not.toBeInTheDocument()

    fireEvent.click(within(profileList).getByRole('button', { name: /Local/ }))

    expect(validateProviderSettings).not.toHaveBeenCalled()
    expect(within(profileList).getByRole('button', { name: /Local/ })).not.toHaveTextContent(
      /\d+ ms/,
    )
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
  })
})
