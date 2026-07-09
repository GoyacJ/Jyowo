import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient, ModelProviderCatalogResponse } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { ModelDetailsDrawer } from './ModelDetailsDrawer'
import type { ModelAssetRow } from './model-settings-view-model'

function renderDrawer({
  client = createTestCommandClient(),
  onSaved,
  open = true,
  row,
}: {
  client?: CommandClient
  onSaved?: () => void
  open?: boolean
  row: ModelAssetRow
}) {
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
    <ModelDetailsDrawer
      catalog={catalog}
      onOpenChange={vi.fn()}
      onSaved={onSaved}
      open={open}
      row={row}
    />,
    { wrapper: Wrapper },
  )
}

describe('ModelDetailsDrawer', () => {
  it('uses three tabs and folds connectivity, usage, and configuration into overview', () => {
    renderDrawer({ row: failingRow })

    const dialog = screen.getByRole('dialog', { name: 'Research Claude' })
    expect(within(dialog).getByRole('tab', { name: 'Overview' })).toBeInTheDocument()
    expect(within(dialog).getByRole('tab', { name: 'Official quota' })).toBeInTheDocument()
    expect(within(dialog).getByRole('tab', { name: 'Capabilities' })).toBeInTheDocument()
    expect(within(dialog).queryByRole('tab', { name: 'Connectivity' })).not.toBeInTheDocument()
    expect(within(dialog).queryByRole('tab', { name: 'Usage' })).not.toBeInTheDocument()
    expect(within(dialog).queryByRole('tab', { name: 'Configuration' })).not.toBeInTheDocument()

    expect(dialog).toHaveTextContent('Failed')
    expect(dialog).toHaveTextContent('10,000 ms')
    expect(dialog).toHaveTextContent('Unavailable')
    expect(dialog).toHaveTextContent('2026-06-30T10:05:00Z')
    expect(dialog).toHaveTextContent('Provider returned a safe failure summary.')
    expect(dialog).toHaveTextContent('Model-level usage')
    expect(within(dialog).getByLabelText('Configuration name')).toHaveValue('Research Claude')
    expect(within(dialog).getByLabelText('Provider')).toHaveValue('anthropic')
    expect(within(dialog).getByLabelText('Model')).toHaveValue('claude-sonnet')
    expect(within(dialog).getByLabelText('API key')).toHaveAttribute('type', 'password')
  })

  it('labels usage as model-level and shows shared usage when profiles share provider/model', () => {
    renderDrawer({ row: sharedUsageRow })

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    expect(dialog).toHaveTextContent('Model-level usage')
    expect(dialog).toHaveTextContent('Shared model usage')
    expect(dialog).toHaveTextContent('100 tokens')
    expect(dialog).toHaveTextContent('310 tokens')
    expect(dialog).toHaveTextContent('1,400 tokens')
  })

  it('renders compact overview status cards and single inline key state badges', () => {
    renderDrawer({ row: sharedUsageRow })

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    expect(dialog).toHaveTextContent('Provider')
    expect(dialog).toHaveTextContent('Model')
    expect(dialog).toHaveTextContent('Default')
    expect(dialog).toHaveTextContent('Connectivity')
    expect(dialog).toHaveTextContent('Online')
    expect(dialog.querySelectorAll('[data-icon]').length).toBeGreaterThanOrEqual(6)
    expect(within(dialog).getAllByText('API key saved')).toHaveLength(1)
    expect(within(dialog).getAllByText('Not saved')).toHaveLength(1)
  })

  it('uses semantic status token classes instead of hardcoded product colors', () => {
    renderDrawer({ row: sharedUsageRow })

    const classNames = Array.from(document.body.querySelectorAll('[class]'))
      .map((element) => element.getAttribute('class') ?? '')
      .join(' ')

    expect(classNames).toContain('text-success')
    expect(classNames).not.toMatch(/\b(?:emerald|amber)-/)
  })

  it('renders official quota states with scope labels', () => {
    for (const [status, scope, label] of [
      ['supported', 'account', 'Account quota'],
      ['unsupported', 'provider', 'Provider quota'],
      ['failed', 'project', 'Project quota'],
      ['authRequired', 'model', 'Model quota'],
      ['notConfigured', 'account', 'Account quota'],
    ] as const) {
      const { unmount } = renderDrawer({
        row: {
          ...sharedUsageRow,
          quota: {
            status,
            scope,
            scopeLabel: scope,
            sourceUrl: 'https://provider.example/docs/usage',
            fetchedAt: '2026-06-30T11:00:00Z',
            expiresAt: '2026-06-30T12:00:00Z',
            isStale: false,
            safeMessage:
              status === 'supported' || status === 'notConfigured' ? undefined : 'Safe state.',
          },
        },
      })
      const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
      fireEvent.click(within(dialog).getByRole('tab', { name: 'Official quota' }))
      expect(dialog).toHaveTextContent(label)
      expect(dialog).toHaveTextContent(quotaStatusLabel[status])
      unmount()
    }
  })

  it('shows API key presence and reveals raw key only through the explicit reveal flow', async () => {
    const rawKey = 'sk-test-revealed-key'
    const requestProviderConfigApiKeyReveal = vi.fn().mockResolvedValue({
      configId: 'cfg-openai',
      expiresInSeconds: 60,
      revealToken: 'reveal-token',
      status: 'ready',
    })
    const getProviderConfigApiKey = vi.fn().mockResolvedValue({
      apiKey: rawKey,
      configId: 'cfg-openai',
    })
    renderDrawer({
      row: { ...sharedUsageRow, hasApiKey: true },
      client: {
        ...createTestCommandClient(),
        requestProviderConfigApiKeyReveal,
        getProviderConfigApiKey,
      },
    })

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    expect(dialog).toHaveTextContent('API key saved')
    expect(dialog).not.toHaveTextContent(rawKey)

    fireEvent.click(within(dialog).getByRole('button', { name: 'View key' }))
    expect(await within(dialog).findByText(rawKey)).toBeInTheDocument()
    expect(requestProviderConfigApiKeyReveal).toHaveBeenCalledWith('cfg-openai')
    expect(getProviderConfigApiKey).toHaveBeenCalledWith('cfg-openai', 'reveal-token')
  })

  it('does not apply a late reveal response after switching rows', async () => {
    const rawKey = 'sk-late-revealed-key'
    const reveal = deferred<{
      configId: string
      expiresInSeconds: number
      revealToken: string
      status: 'ready'
    }>()
    const key = deferred<{ apiKey: string; configId: string }>()
    const requestProviderConfigApiKeyReveal = vi.fn().mockReturnValue(reveal.promise)
    const getProviderConfigApiKey = vi.fn().mockReturnValue(key.promise)
    const { rerender } = renderDrawer({
      row: { ...sharedUsageRow, hasApiKey: true },
      client: {
        ...createTestCommandClient(),
        requestProviderConfigApiKeyReveal,
        getProviderConfigApiKey,
      },
    })

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('button', { name: 'View key' }))

    await act(async () => {
      reveal.resolve({
        configId: 'cfg-openai',
        expiresInSeconds: 60,
        revealToken: 'reveal-token',
        status: 'ready',
      })
    })
    await waitFor(() =>
      expect(getProviderConfigApiKey).toHaveBeenCalledWith('cfg-openai', 'reveal-token'),
    )

    rerender(
      <ModelDetailsDrawer
        catalog={catalog}
        onOpenChange={vi.fn()}
        open
        row={{ ...failingRow, hasApiKey: true }}
      />,
    )

    await act(async () => {
      key.resolve({ apiKey: rawKey, configId: 'cfg-openai' })
    })

    expect(screen.getByRole('dialog', { name: 'Research Claude' })).not.toHaveTextContent(rawKey)
  })

  it('does not fetch the raw key when the row changes before reveal token returns', async () => {
    const reveal = deferred<{
      configId: string
      expiresInSeconds: number
      revealToken: string
      status: 'ready'
    }>()
    const requestProviderConfigApiKeyReveal = vi.fn().mockReturnValue(reveal.promise)
    const getProviderConfigApiKey = vi.fn()
    const { rerender } = renderDrawer({
      row: { ...sharedUsageRow, hasApiKey: true },
      client: {
        ...createTestCommandClient(),
        requestProviderConfigApiKeyReveal,
        getProviderConfigApiKey,
      },
    })

    fireEvent.click(
      within(screen.getByRole('dialog', { name: 'Primary OpenAI' })).getByRole('button', {
        name: 'View key',
      }),
    )

    rerender(
      <ModelDetailsDrawer
        catalog={catalog}
        onOpenChange={vi.fn()}
        open
        row={{ ...failingRow, hasApiKey: true }}
      />,
    )

    await act(async () => {
      reveal.resolve({
        configId: 'cfg-openai',
        expiresInSeconds: 60,
        revealToken: 'reveal-token',
        status: 'ready',
      })
    })

    expect(getProviderConfigApiKey).not.toHaveBeenCalled()
  })

  it('uses the shared centered dialog placement', () => {
    renderDrawer({ row: sharedUsageRow })

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    expect(dialog).not.toHaveClass('right-4')
    expect(dialog).not.toHaveClass('top-4')
    expect(dialog).not.toHaveClass('translate-x-0')
    expect(dialog).not.toHaveClass('translate-y-0')
  })

  it('renders backend catalog model capabilities when available', () => {
    renderDrawer({ row: sharedUsageRow })

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Capabilities' }))

    expect(dialog).toHaveTextContent('Streaming')
    expect(dialog).toHaveTextContent('Tools')
    expect(dialog).toHaveTextContent('Structured output')
    expect(dialog).toHaveTextContent('128,000')
    expect(dialog).toHaveTextContent('8,192')
  })

  it('shows read-only route bindings and shortcuts without the full route editor table', () => {
    const onUseForRoute = vi.fn()
    render(
      <ModelDetailsDrawer
        catalog={catalog}
        onOpenChange={vi.fn()}
        onUseForRoute={onUseForRoute}
        open
        row={{
          ...sharedUsageRow,
          routeBindings: {
            status: 'ready',
            data: [{ kind: 'image_generation', operationIds: ['images.generate'] }],
          },
        }}
      />,
      { wrapper: DrawerWrapper },
    )

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Capabilities' }))

    expect(dialog).toHaveTextContent('Image generation')
    expect(dialog).toHaveTextContent('images.generate')
    expect(
      within(dialog).queryByRole('table', { name: 'Capability route table' }),
    ).not.toBeInTheDocument()

    fireEvent.click(within(dialog).getByRole('button', { name: 'Use for image generation' }))
    expect(onUseForRoute).toHaveBeenCalledWith('image_generation', sharedUsageRow.configId)
  })

  it('shows route binding loading state instead of an empty binding fallback', () => {
    render(
      <ModelDetailsDrawer
        catalog={catalog}
        onOpenChange={vi.fn()}
        open
        row={{ ...sharedUsageRow, routeBindings: { status: 'loading' } }}
      />,
      { wrapper: DrawerWrapper },
    )

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Capabilities' }))

    expect(dialog).toHaveTextContent('Loading')
    expect(dialog).not.toHaveTextContent('No capability routes target this profile.')
  })

  it('saves overview configuration inline without opening a nested edit dialog', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: {
        id: 'cfg-openai',
        baseUrl: 'https://api.openai.com/v1',
        displayName: 'Primary OpenAI Edited',
        providerId: 'openai',
        modelId: 'gpt-4.1',
        isDefault: true,
        hasApiKey: true,
        hasOfficialQuotaApiKey: false,
        modelDescriptor,
      },
      status: 'saved',
    })
    const onSaved = vi.fn()
    renderDrawer({
      row: sharedUsageRow,
      onSaved,
      client: { ...createTestCommandClient(), saveProviderSettings },
    })

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    expect(
      screen.queryByRole('dialog', { name: 'Edit model configuration' }),
    ).not.toBeInTheDocument()
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
    expect(onSaved).toHaveBeenCalled()
  })

  it('does not clear typed secrets when the same row or catalog refreshes', () => {
    const { rerender } = renderDrawer({ row: sharedUsageRow })

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    const apiKey = within(dialog).getByLabelText('API key')
    const officialQuotaApiKey = within(dialog).getByLabelText('Official quota admin key')
    fireEvent.change(apiKey, { target: { value: 'sk-unsaved-secret' } })
    fireEvent.change(officialQuotaApiKey, { target: { value: 'quota-unsaved-secret' } })

    rerender(
      <ModelDetailsDrawer
        catalog={{ providers: [...catalog.providers] }}
        onOpenChange={vi.fn()}
        open
        row={{
          ...sharedUsageRow,
          connectivity: {
            status: 'online',
            checkedAt: '2026-06-30T10:10:00Z',
            latencyMs: 120,
            timeoutMs: 10_000,
          },
        }}
      />,
    )

    expect(
      within(screen.getByRole('dialog', { name: 'Primary OpenAI' })).getByLabelText('API key'),
    ).toHaveValue('sk-unsaved-secret')
    expect(
      within(screen.getByRole('dialog', { name: 'Primary OpenAI' })).getByLabelText(
        'Official quota admin key',
      ),
    ).toHaveValue('quota-unsaved-secret')
  })
})

function DrawerWrapper({ children }: { children: ReactNode }) {
  uiStore.getState().setLocale('en-US')
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })

  return (
    <CommandClientProvider client={createTestCommandClient()}>
      <QueryClientProvider client={queryClient}>
        <AppI18nProvider>{children}</AppI18nProvider>
      </QueryClientProvider>
    </CommandClientProvider>
  )
}

const modelDescriptor = {
  protocol: 'responses',
  conversationCapability: {
    inputModalities: ['text'],
    outputModalities: ['text'],
    contextWindow: 128000,
    maxOutputTokens: 8192,
    streaming: true,
    toolCalling: true,
    reasoning: false,
    promptCache: false,
    structuredOutput: true,
  },
  contextWindow: 128000,
  displayName: 'GPT-4.1',
  lifecycle: { kind: 'stable' },
  maxOutputTokens: 8192,
  modelId: 'gpt-4.1',
  runtimeStatus: { kind: 'runnable' },
} satisfies NonNullable<ModelAssetRow['modelDescriptor']>

const catalog: ModelProviderCatalogResponse = {
  providers: [
    {
      providerId: 'openai',
      displayName: 'OpenAI',
      defaultBaseUrl: 'https://api.openai.com/v1',
      models: [modelDescriptor],
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
      providerId: 'anthropic',
      displayName: 'Anthropic',
      defaultBaseUrl: 'https://api.anthropic.com',
      models: [
        {
          ...modelDescriptor,
          displayName: 'Claude Sonnet',
          modelId: 'claude-sonnet',
        },
      ],
      runtimeCapability: {
        authScheme: 'bearer',
        baseUrlRegions: [{ id: 'default', label: 'Default', baseUrl: 'https://api.anthropic.com' }],
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

const sharedUsageRow: ModelAssetRow = {
  configId: 'cfg-openai',
  providerId: 'openai',
  modelId: 'gpt-4.1',
  modelDescriptor,
  displayName: 'Primary OpenAI',
  providerDisplayName: 'OpenAI',
  isDefault: true,
  hasApiKey: true,
  hasOfficialQuotaApiKey: false,
  baseUrl: 'https://api.openai.com/v1',
  connectivity: {
    status: 'online',
    checkedAt: '2026-06-30T10:00:00Z',
    latencyMs: 118,
    timeoutMs: 10_000,
  },
  usage: {
    status: 'ready',
    sharedModelUsage: true,
    today: usage(70, 30),
    monthToDate: usage(220, 90),
    allTime: usage(900, 500),
  },
  quota: {
    status: 'supported',
    scope: 'account',
    scopeLabel: 'account',
    sourceUrl: 'https://platform.openai.com/docs/api-reference/usage',
    fetchedAt: '2026-06-30T11:00:00Z',
    expiresAt: '2026-06-30T12:00:00Z',
    isStale: false,
    quotaUsed: 10,
    quotaTotal: 100,
    quotaRemaining: 90,
    unit: 'usd',
  },
  routeBindings: { status: 'ready', data: [] },
}

const failingRow: ModelAssetRow = {
  ...sharedUsageRow,
  configId: 'cfg-anthropic',
  providerId: 'anthropic',
  modelId: 'claude-sonnet',
  displayName: 'Research Claude',
  providerDisplayName: 'Anthropic',
  isDefault: false,
  baseUrl: 'https://api.anthropic.com',
  connectivity: {
    status: 'failed',
    checkedAt: '2026-06-30T10:05:00Z',
    timeoutMs: 10_000,
    errorKind: 'provider',
    safeMessage: 'Provider returned a safe failure summary.',
  },
}

const quotaStatusLabel = {
  supported: 'Supported',
  unsupported: 'Unsupported',
  failed: 'Failed',
  authRequired: 'Auth required',
  notConfigured: 'Not configured',
}

function usage(inputTokens: number, outputTokens: number) {
  return {
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    costMicros: 0,
    inputTokens,
    outputTokens,
    toolCalls: 0,
  }
}

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}
