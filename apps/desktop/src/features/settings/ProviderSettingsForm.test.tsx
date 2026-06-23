import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type {
  CommandClient,
  ConversationModelCapability,
  ModelProviderCatalogResponse,
} from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { ProviderSettingsForm } from './ProviderSettingsForm'

type ModelCatalogEntry = ModelProviderCatalogResponse['providers'][number]['models'][number]

function renderProviderSettingsForm(commandClient: CommandClient = createMockCommandClient()) {
  return render(
    <CommandClientProvider client={commandClient}>
      <ProviderSettingsForm />
    </CommandClientProvider>,
  )
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
  supportsLiveValidation: true,
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

    fireEvent.click(await screen.findByRole('button', { name: 'New configuration' }))
    const dialog = await screen.findByRole('dialog', { name: 'Create model configuration' })

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

    const detail = await screen.findByRole('region', { name: 'Model configuration details' })

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

    const profileList = await screen.findByRole('region', { name: 'Saved configurations' })
    const openAiProfileButton = await screen.findByRole('button', { name: /OpenAI/ })
    const detail = screen.getByRole('region', { name: 'Model configuration details' })

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

    const detail = await screen.findByRole('region', { name: 'Model configuration details' })
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

    fireEvent.click(await screen.findByRole('button', { name: 'New configuration' }))
    const dialog = await screen.findByRole('dialog', { name: 'Create model configuration' })

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

    fireEvent.click(await screen.findByRole('button', { name: 'New configuration' }))
    const dialog = await screen.findByRole('dialog', { name: 'Create model configuration' })
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

    const detail = await screen.findByRole('region', { name: 'Model configuration details' })

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

    const detail = await screen.findByRole('region', { name: 'Model configuration details' })

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

    const profileList = await screen.findByRole('region', { name: 'Saved configurations' })
    fireEvent.click(within(profileList).getByRole('button', { name: /OpenAI/ }))
    const detail = await screen.findByRole('region', { name: 'Model configuration details' })
    const setDefaultButton = within(detail).getByRole('button', { name: 'Set as default' })
    const testButton = within(detail).getByRole('button', { name: 'Test' })
    expect(setDefaultButton.compareDocumentPosition(testButton)).toBe(
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

    fireEvent.click(await screen.findByRole('button', { name: 'New configuration' }))
    const dialog = await screen.findByRole('dialog', { name: 'Create model configuration' })
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

    fireEvent.click(await screen.findByRole('button', { name: 'New configuration' }))
    const dialog = await screen.findByRole('dialog', { name: 'Create model configuration' })
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

    renderProviderSettingsForm(client)

    fireEvent.click(await screen.findByRole('button', { name: 'New configuration' }))
    const dialog = await screen.findByRole('dialog', { name: 'Create model configuration' })
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

    await screen.findByRole('region', { name: 'Model configuration details' })
    const apiKeyInput = screen.getByLabelText('API key')
    const savedKeyMask = '\u2022'.repeat(32)

    expect(apiKeyInput).toHaveValue('')
    expect(apiKeyInput).toHaveAttribute('type', 'password')
    expect(screen.getByText(savedKeyMask)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'View key' })).not.toBeInTheDocument()
    expect(requestProviderConfigApiKeyReveal).not.toHaveBeenCalled()
    expect(getProviderConfigApiKey).not.toHaveBeenCalled()
  })

  it('tests the selected provider and model from selected profile details', async () => {
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

    const detail = await screen.findByRole('region', { name: 'Model configuration details' })
    fireEvent.click(within(detail).getByRole('button', { name: 'Test' }))

    await waitFor(() =>
      expect(validateProviderSettings).toHaveBeenCalledWith({
        modelId: 'gpt-4o-mini',
        providerId: 'openai',
      }),
    )
    expect(await screen.findByRole('status')).toHaveTextContent('Test passed')
    expect(screen.getByRole('status')).toHaveTextContent(/Latency \d+ ms\./)
    expect(await within(detail).findByText(/\d+ ms/)).toBeInTheDocument()
    expect(within(detail).queryByText('Provider settings accepted.')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'New configuration' }))
    const dialog = await screen.findByRole('dialog', { name: 'Create model configuration' })
    expect(within(dialog).queryByRole('button', { name: 'Test' })).not.toBeInTheDocument()
  })

  it('auto-tests the selected profile and shows latency in the profile list without a toast', async () => {
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

    const profileList = await screen.findByRole('region', { name: 'Saved configurations' })
    const openAiProfile = within(profileList).getByRole('button', { name: /OpenAI/ })

    await waitFor(() =>
      expect(validateProviderSettings).toHaveBeenCalledWith({
        modelId: 'gpt-4o-mini',
        providerId: 'openai',
      }),
    )
    expect(openAiProfile).toHaveTextContent(/\d+ ms/)
    expect(screen.queryByRole('status')).not.toBeInTheDocument()

    fireEvent.click(within(profileList).getByRole('button', { name: /Local/ }))

    await waitFor(() =>
      expect(validateProviderSettings).toHaveBeenCalledWith({
        modelId: 'llama3.1',
        providerId: 'local-llama',
      }),
    )
    expect(within(profileList).getByRole('button', { name: /Local/ })).toHaveTextContent(/\d+ ms/)
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
  })
})
