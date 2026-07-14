import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type {
  CommandClient,
  ConversationModelCapability,
  GetModelUsageSummaryResponse,
  ListOfficialQuotaSnapshotsResponse,
  ListProviderCapabilityRouteOptionsResponse,
  ListProviderCapabilityRoutesResponse,
  ListProviderProbeSnapshotsResponse,
  ListProviderSettingsResponse,
  ModelProviderCatalogResponse,
  ModelSettingsPageResponse,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { ModelSettingsPage } from './ModelSettingsPage'

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

const claude = {
  ...gpt41,
  protocol: 'messages' as const,
  supportedProtocols: ['messages' as const],
  displayName: 'Claude Sonnet',
  modelId: 'claude-sonnet',
}

const minimaxM3 = {
  ...gpt41,
  displayName: 'MiniMax M3',
  modelId: 'MiniMax-M3',
  supportedProtocols: ['responses' as const, 'chat_completions' as const, 'messages' as const],
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
      defaultBaseUrl: 'https://api.anthropic.com',
      displayName: 'Anthropic',
      models: [claude],
      providerId: 'anthropic',
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

const settings: ListProviderSettingsResponse = {
  defaultConfigId: 'cfg-openai',
  selectionScope: 'global',
  configs: [
    {
      id: 'cfg-openai',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      displayName: 'Primary OpenAI',
      hasApiKey: true,
      hasOfficialQuotaApiKey: false,
      isDefault: true,
      protocol: 'responses',
      modelDescriptor: gpt41,
    },
    {
      id: 'cfg-anthropic',
      providerId: 'anthropic',
      modelId: 'claude-sonnet',
      displayName: 'Research Claude',
      hasApiKey: true,
      hasOfficialQuotaApiKey: false,
      isDefault: false,
      protocol: 'messages',
      modelDescriptor: claude,
    },
    {
      id: 'cfg-openai-backup',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      displayName: 'Backup OpenAI',
      hasApiKey: true,
      hasOfficialQuotaApiKey: false,
      isDefault: false,
      protocol: 'responses',
      modelDescriptor: gpt41,
    },
  ],
}

const probeSnapshots: ListProviderProbeSnapshotsResponse = {
  snapshots: [
    {
      configId: 'cfg-openai',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      status: 'online',
      timeoutMs: 10_000,
      latencyMs: 118,
      checkedAt: '2026-06-30T10:00:00Z',
    },
    {
      configId: 'cfg-anthropic',
      providerId: 'anthropic',
      modelId: 'claude-sonnet',
      status: 'failed',
      timeoutMs: 10_000,
      checkedAt: '2026-06-30T10:05:00Z',
      errorKind: 'provider',
      safeMessage: 'Provider returned a safe failure summary.',
    },
    {
      configId: 'cfg-openai-backup',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      status: 'rate_limited',
      timeoutMs: 10_000,
      checkedAt: '2026-06-30T10:10:00Z',
      errorKind: 'rate_limit',
      safeMessage: 'Rate limited.',
    },
  ],
}

const usageSummary: GetModelUsageSummaryResponse = {
  timezoneId: 'UTC',
  timezoneOffsetMinutes: 0,
  today: {
    period: 'today',
    total: usage(120, 80),
    byModel: [
      { key: 'openai/gpt-4.1', providerId: 'openai', modelId: 'gpt-4.1', usage: usage(70, 30) },
    ],
  },
  monthToDate: {
    period: 'month_to_date',
    total: usage(420, 240),
    byModel: [
      { key: 'openai/gpt-4.1', providerId: 'openai', modelId: 'gpt-4.1', usage: usage(220, 90) },
    ],
  },
  allTime: {
    period: 'all_time',
    total: usage(1200, 900),
    byModel: [
      { key: 'openai/gpt-4.1', providerId: 'openai', modelId: 'gpt-4.1', usage: usage(900, 500) },
    ],
  },
  activity: {
    rangeStart: '2026-06-24',
    rangeEnd: '2026-06-30',
    peakDayTokens: 200,
    currentStreakDays: 2,
    longestStreakDays: 3,
    longestTaskDurationMs: 61_000,
    days: [
      { date: '2026-06-24', usage: usage(25, 0) },
      { date: '2026-06-25', usage: usage(0, 0) },
      { date: '2026-06-26', usage: usage(40, 10) },
      { date: '2026-06-27', usage: usage(70, 30) },
      { date: '2026-06-28', usage: usage(0, 0) },
      { date: '2026-06-29', usage: usage(120, 40) },
      { date: '2026-06-30', usage: usage(120, 80) },
    ],
  },
  generatedAt: '2026-06-30T12:00:00Z',
}

const quotaSnapshots: ListOfficialQuotaSnapshotsResponse = {
  snapshots: [
    {
      configId: 'cfg-openai',
      providerId: 'openai',
      scope: 'account',
      status: 'supported',
      sourceUrl: 'https://platform.openai.com/docs/api-reference/usage',
      fetchedAt: '2026-06-30T11:00:00Z',
      expiresAt: '2026-06-30T12:00:00Z',
      isStale: false,
      quotaUsed: 10,
      quotaTotal: 100,
      quotaRemaining: 90,
      unit: 'usd',
    },
    {
      configId: 'cfg-anthropic',
      providerId: 'anthropic',
      scope: 'provider',
      status: 'unsupported',
      sourceUrl: 'https://docs.anthropic.com',
      fetchedAt: '2026-06-30T11:00:00Z',
      expiresAt: '2026-06-30T12:00:00Z',
      isStale: false,
      safeMessage: 'Official quota API is unavailable.',
    },
    {
      configId: 'cfg-openai-backup',
      providerId: 'openai',
      scope: 'account',
      status: 'failed',
      sourceUrl: 'https://platform.openai.com/docs/api-reference/usage',
      fetchedAt: '2026-06-30T11:00:00Z',
      expiresAt: '2026-06-30T12:00:00Z',
      isStale: false,
      safeMessage: 'Official quota refresh failed safely.',
    },
  ],
}

const capabilityRoutes: ListProviderCapabilityRoutesResponse = {
  version: 1,
  routes: [
    {
      kind: 'image_generation',
      configId: 'cfg-openai',
      providerId: 'openai',
      operationIds: ['images.generate'],
      enabled: true,
    },
  ],
}

const capabilityRouteOptions: ListProviderCapabilityRouteOptionsResponse = {
  options: [
    {
      kind: 'image_generation',
      configId: 'cfg-openai',
      providerId: 'openai',
      operationId: 'images.generate',
      outputArtifact: 'image',
      execution: 'sync',
      costRisk: 'medium',
      runtimeSupported: true,
    },
    {
      kind: 'video_generation',
      configId: 'cfg-openai',
      providerId: 'openai',
      operationId: 'videos.generate',
      outputArtifact: 'video',
      execution: 'async_job',
      costRisk: 'high',
      runtimeSupported: true,
    },
    {
      kind: 'speech_to_text',
      configId: 'cfg-anthropic',
      providerId: 'anthropic',
      operationId: 'audio.transcriptions',
      outputArtifact: 'text',
      execution: 'sync',
      costRisk: 'low',
      runtimeSupported: false,
      unavailableReason: 'Backend rejected this route for the selected profile',
    },
    {
      kind: 'text_to_speech',
      configId: 'cfg-openai',
      providerId: 'openai',
      operationId: 'audio.speech',
      outputArtifact: 'audio',
      execution: 'sync',
      costRisk: 'medium',
      runtimeSupported: true,
    },
    {
      kind: 'music_generation',
      configId: 'cfg-openai-backup',
      providerId: 'openai',
      operationId: 'music.generate',
      outputArtifact: 'audio',
      execution: 'async_job',
      costRisk: 'high',
      runtimeSupported: true,
    },
  ],
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

function renderModelSettingsPage(client: CommandClient = readyClient()) {
  uiStore.getState().setLocale('en-US')

  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
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

  return { ...render(<ModelSettingsPage />, { wrapper: Wrapper }), queryClient }
}

function readyClient(overrides: Parameters<typeof createTestCommandClient>[0] = {}) {
  return createTestCommandClient({
    modelProviderCatalog: catalog,
    providerSettingsList: settings,
    providerProbeSnapshots: probeSnapshots,
    modelUsageSummary: usageSummary,
    officialQuotaSnapshots: quotaSnapshots,
    providerCapabilityRoutes: capabilityRoutes,
    providerCapabilityRouteOptions: capabilityRouteOptions,
    ...overrides,
  })
}

function readyModelSettingsPage(
  overrides: Partial<ModelSettingsPageResponse> = {},
): ModelSettingsPageResponse {
  return {
    catalog,
    catalogSnapshot: { source: 'bundled' },
    providerSettings: settings,
    probeSnapshots: { status: 'ready', data: probeSnapshots },
    usageSummary: { status: 'ready', data: usageSummary },
    quotaSnapshots: { status: 'ready', data: quotaSnapshots },
    capabilityRoutes: { status: 'ready', data: capabilityRoutes },
    capabilityRouteOptions: { status: 'ready', data: capabilityRouteOptions },
    generatedAt: '2026-06-30T12:00:00Z',
    ...overrides,
  }
}

describe('ModelSettingsPage', () => {
  it('renders loading, empty, and safe error states', async () => {
    const { unmount } = renderModelSettingsPage(readyClient({ delayMs: 50 }))
    expect(screen.getByText('Loading model settings')).toBeInTheDocument()
    unmount()

    renderModelSettingsPage(
      readyClient({
        providerSettingsList: { defaultConfigId: null, selectionScope: 'global', configs: [] },
      }),
    )
    expect(await screen.findByText('No configured models')).toBeInTheDocument()
    unmount()

    const errorClient = {
      ...readyClient(),
      getModelSettingsPage: vi.fn().mockRejectedValue(new Error('Safe backend error')),
    } satisfies CommandClient
    renderModelSettingsPage(errorClient)
    expect(await screen.findByRole('alert')).toHaveTextContent('Safe backend error')
  })

  it('keeps provider settings labeled as global even if a stale project scope is returned', async () => {
    renderModelSettingsPage(
      readyClient({
        providerSettingsList: {
          ...settings,
          selectionScope: 'project',
        },
      }),
    )

    expect(await screen.findByText('Global defaults')).toBeInTheDocument()
    expect(screen.queryByText('Project overrides')).not.toBeInTheDocument()
  })

  it('renders usage insights panel and matrix rows with backend usage, connectivity, and quota state', async () => {
    renderModelSettingsPage()

    expect(await screen.findByRole('row', { name: /Primary OpenAI/ })).toBeInTheDocument()
    expect(await screen.findByRole('row', { name: /Primary OpenAI/ })).toBeInTheDocument()
    expect(screen.getByLabelText('Token activity')).toHaveTextContent('2.1K')
    expect(screen.getByLabelText('Token activity')).toHaveTextContent('1m 1s')
    expect(screen.getByTestId('usage-day-2026-06-30')).toHaveAttribute('data-level', '4')
    expect(screen.getAllByText('Today').length).toBeGreaterThan(0)
    expect(screen.getByLabelText('Token activity')).toHaveTextContent('200')
    expect(screen.queryByLabelText('Model settings summary')).not.toBeInTheDocument()

    const primaryRow = within(screen.getByRole('row', { name: /Primary OpenAI/ }))
    expect(primaryRow.getByText('OpenAI')).toBeInTheDocument()
    expect(primaryRow.getByText('Default')).toBeInTheDocument()
    expect(primaryRow.getByText('Online')).toBeInTheDocument()
    expect(primaryRow.getByText('118')).toBeInTheDocument()
    expect(primaryRow.getByText('10,000')).toBeInTheDocument()
    expect(primaryRow.getByText('100')).toBeInTheDocument()
    expect(primaryRow.getByText('310')).toBeInTheDocument()
    expect(primaryRow.getByText('1,400')).toBeInTheDocument()
    expect(primaryRow.getByText('Supported')).toBeInTheDocument()
    expect(screen.getByRole('columnheader', { name: 'Latency ms' })).toBeInTheDocument()
    expect(screen.getByRole('columnheader', { name: 'Timeout threshold ms' })).toBeInTheDocument()
    expect(screen.getByRole('columnheader', { name: 'Today tokens' })).toBeInTheDocument()
    expect(screen.getByRole('columnheader', { name: 'Month-to-date tokens' })).toBeInTheDocument()
    expect(screen.getByRole('columnheader', { name: 'Total tokens' })).toBeInTheDocument()
    const matrix = screen.getByLabelText('Model matrix')
    expect(matrix).toHaveClass('model-matrix-layout')
    expect(matrix.querySelector('.model-matrix-table-wrap')).toBeInTheDocument()
    expect(matrix.querySelector('table')).toHaveClass('min-w-[1040px]')
    expect(matrix.querySelector('ul')).toHaveClass('model-matrix-card-list')

    expect(screen.getByRole('row', { name: /Research Claude.*Unsupported/ })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: /Backup OpenAI.*Failed/ })).toBeInTheDocument()
  })

  it('refreshes the provider catalog only from the explicit catalog action', async () => {
    const refreshModelProviderCatalog = vi.fn().mockResolvedValue({
      catalog,
      catalogSnapshot: { source: 'bundled' },
    })
    renderModelSettingsPage({
      ...readyClient(),
      refreshModelProviderCatalog,
    })

    expect(await screen.findByRole('row', { name: /Primary OpenAI/ })).toBeInTheDocument()
    expect(refreshModelProviderCatalog).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Refresh catalog' }))

    await waitFor(() => expect(refreshModelProviderCatalog).toHaveBeenCalledTimes(1))
  })

  it('shows model action failures instead of swallowing rejected mutations', async () => {
    renderModelSettingsPage({
      ...readyClient(),
      refreshModelProviderCatalog: vi.fn().mockRejectedValue(new Error('Catalog refresh failed')),
      probeProviderConfig: vi.fn().mockRejectedValue(new Error('Provider probe failed')),
      refreshOfficialQuota: vi.fn().mockRejectedValue(new Error('Quota refresh failed')),
      saveProviderSettings: vi.fn().mockRejectedValue(new Error('Default update failed')),
    })

    fireEvent.click(await screen.findByRole('button', { name: 'Refresh catalog' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Catalog refresh failed')

    const primaryRow = within(screen.getByRole('row', { name: /Primary OpenAI/ }))
    fireEvent.click(primaryRow.getByRole('button', { name: 'Probe Primary OpenAI' }))
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent('Provider probe failed'),
    )

    fireEvent.click(primaryRow.getByRole('button', { name: 'Refresh quota for Primary OpenAI' }))
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('Quota refresh failed'))

    const researchRow = within(screen.getByRole('row', { name: /Research Claude/ }))
    fireEvent.click(researchRow.getByRole('button', { name: 'Set Research Claude as default' }))
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent('Default update failed'),
    )
  })

  it('does not present metadata validation as a connectivity check', async () => {
    renderModelSettingsPage()

    await screen.findByRole('row', { name: /Primary OpenAI/ })

    expect(screen.queryByRole('button', { name: 'Check' })).not.toBeInTheDocument()
    expect(screen.queryByText('Provider metadata accepted.')).not.toBeInTheDocument()
    expect(screen.queryByText('Check accepted')).not.toBeInTheDocument()
    expect(screen.queryByText('Check failed')).not.toBeInTheDocument()
  })

  it('uses one row entry point and saves configuration inside the details dialog', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: {
        ...settings.configs[0],
        displayName: 'Primary OpenAI Edited',
      },
      status: 'saved',
    })
    const { queryClient } = renderModelSettingsPage({
      ...readyClient(),
      saveProviderSettings,
    })
    const globalTaskQuery = ['provider-settings', 'list', null] as const
    const projectTaskQuery = ['provider-settings', 'list', '/workspace/project'] as const
    queryClient.setQueryData(globalTaskQuery, settings)
    queryClient.setQueryData(projectTaskQuery, settings)

    const row = within(await screen.findByRole('row', { name: /Primary OpenAI/ }))

    expect(row.getByRole('button', { name: 'Configure Primary OpenAI' })).toBeInTheDocument()
    expect(
      row.queryByRole('button', { name: 'View details for Primary OpenAI' }),
    ).not.toBeInTheDocument()
    expect(row.queryByRole('button', { name: 'Edit Primary OpenAI' })).not.toBeInTheDocument()

    fireEvent.click(row.getByRole('button', { name: 'Configure Primary OpenAI' }))

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    expect(within(dialog).queryByRole('tab', { name: 'Configuration' })).not.toBeInTheDocument()
    expect(
      screen.queryByRole('dialog', { name: 'Edit model configuration' }),
    ).not.toBeInTheDocument()

    fireEvent.change(within(dialog).getByLabelText('Configuration name'), {
      target: { value: 'Primary OpenAI Edited' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() =>
      expect(saveProviderSettings).toHaveBeenCalledWith({
        configId: 'cfg-openai',
        displayName: 'Primary OpenAI Edited',
        modelId: 'gpt-4.1',
        modelOptions: {},
        providerId: 'openai',
        setDefault: true,
      }),
    )
    expect(saveProviderSettings.mock.calls[0]?.[0]).not.toHaveProperty('apiKey')
    expect(saveProviderSettings.mock.calls[0]?.[0]).not.toHaveProperty('officialQuotaApiKey')
    expect(queryClient.getQueryState(globalTaskQuery)?.isInvalidated).toBe(true)
    expect(queryClient.getQueryState(projectTaskQuery)?.isInvalidated).toBe(true)
  })

  it('filters by provider, health, default state, failing state, and search', async () => {
    renderModelSettingsPage()
    await screen.findByRole('row', { name: /Primary OpenAI/ })

    fireEvent.change(screen.getByLabelText('Provider'), { target: { value: 'anthropic' } })
    expect(screen.queryByRole('row', { name: /Primary OpenAI/ })).not.toBeInTheDocument()
    expect(screen.getByRole('row', { name: /Research Claude/ })).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Provider'), { target: { value: 'all' } })
    fireEvent.change(screen.getByLabelText('Health'), { target: { value: 'online' } })
    expect(screen.getByRole('row', { name: /Primary OpenAI/ })).toBeInTheDocument()
    expect(screen.queryByRole('row', { name: /Research Claude/ })).not.toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Health'), { target: { value: 'all' } })
    fireEvent.click(screen.getByLabelText('Default only'))
    expect(screen.getByRole('row', { name: /Primary OpenAI/ })).toBeInTheDocument()
    expect(screen.queryByRole('row', { name: /Backup OpenAI/ })).not.toBeInTheDocument()

    fireEvent.click(screen.getByLabelText('Default only'))
    fireEvent.click(screen.getByLabelText('Failing only'))
    expect(screen.queryByRole('row', { name: /Primary OpenAI/ })).not.toBeInTheDocument()
    expect(screen.getByRole('row', { name: /Research Claude/ })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: /Backup OpenAI/ })).toBeInTheDocument()

    fireEvent.click(screen.getByLabelText('Failing only'))
    fireEvent.change(screen.getByLabelText('Search models'), { target: { value: 'backup' } })
    expect(screen.queryByRole('row', { name: /Primary OpenAI/ })).not.toBeInTheDocument()
    expect(screen.getByRole('row', { name: /Backup OpenAI/ })).toBeInTheDocument()
  })

  it('uses configId for row actions and blocks repeated probe and quota clicks while pending', async () => {
    const baseClient = readyClient()
    let resolveProbe: (() => void) | undefined
    let resolveQuota: (() => void) | undefined
    const probeProviderConfig = vi.fn(
      () =>
        new Promise<Awaited<ReturnType<CommandClient['probeProviderConfig']>>>((resolve) => {
          resolveProbe = () =>
            resolve({
              snapshot: {
                checkedAt: '2026-06-30T12:00:00Z',
                configId: 'cfg-openai',
                latencyMs: 101,
                modelId: 'gpt-4.1',
                providerId: 'openai',
                status: 'online',
                timeoutMs: 10_000,
              },
            })
        }),
    )
    const refreshOfficialQuota = vi.fn(
      () =>
        new Promise<Awaited<ReturnType<CommandClient['refreshOfficialQuota']>>>((resolve) => {
          resolveQuota = () =>
            resolve({
              snapshot: {
                configId: 'cfg-openai',
                providerId: 'openai',
                scope: 'account',
                status: 'supported',
                sourceUrl: 'https://platform.openai.com/docs/api-reference/usage',
                fetchedAt: '2026-06-30T12:00:00Z',
                expiresAt: '2026-06-30T12:15:00Z',
                isStale: false,
              },
            })
        }),
    )
    const validateProviderSettings = vi.fn()
    const client = {
      ...baseClient,
      probeProviderConfig,
      refreshOfficialQuota,
      validateProviderSettings,
    } satisfies CommandClient

    renderModelSettingsPage(client)
    const row = within(await screen.findByRole('row', { name: /Primary OpenAI/ }))

    fireEvent.click(row.getByRole('button', { name: 'Probe Primary OpenAI' }))
    expect(await row.findByRole('button', { name: 'Probing Primary OpenAI' })).toBeDisabled()
    fireEvent.click(row.getByRole('button', { name: 'Probing Primary OpenAI' }))
    expect(probeProviderConfig).toHaveBeenCalledTimes(1)
    expect(probeProviderConfig).toHaveBeenCalledWith({ configId: 'cfg-openai', timeoutMs: 10000 })
    expect(validateProviderSettings).not.toHaveBeenCalled()
    resolveProbe?.()

    fireEvent.click(row.getByRole('button', { name: 'Refresh quota for Primary OpenAI' }))
    expect(
      await row.findByRole('button', { name: 'Refreshing quota for Primary OpenAI' }),
    ).toBeDisabled()
    fireEvent.click(row.getByRole('button', { name: 'Refreshing quota for Primary OpenAI' }))
    expect(refreshOfficialQuota).toHaveBeenCalledTimes(1)
    expect(refreshOfficialQuota).toHaveBeenCalledWith({ configId: 'cfg-openai' })
    resolveQuota?.()
  })

  it('sets a non-default row as the default without resubmitting stored keys', async () => {
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: {
        ...settings.configs[1],
        isDefault: true,
      },
      status: 'saved',
    })
    const client = {
      ...readyClient(),
      saveProviderSettings,
    } satisfies CommandClient
    renderModelSettingsPage(client)

    const row = within(await screen.findByRole('row', { name: /Research Claude/ }))
    fireEvent.click(row.getByRole('button', { name: 'Set Research Claude as default' }))

    await waitFor(() =>
      expect(saveProviderSettings).toHaveBeenCalledWith({
        configId: 'cfg-anthropic',
        displayName: 'Research Claude',
        modelId: 'claude-sonnet',
        providerId: 'anthropic',
        setDefault: true,
      }),
    )
    expect(saveProviderSettings.mock.calls[0]?.[0]).not.toHaveProperty('apiKey')
    expect(saveProviderSettings.mock.calls[0]?.[0]).not.toHaveProperty('officialQuotaApiKey')
  })

  it('keeps the selected MiniMax protocol when setting a row as default', async () => {
    const minimaxCatalog: ModelProviderCatalogResponse = {
      providers: [
        ...catalog.providers,
        {
          defaultBaseUrl: 'https://api.minimax.io',
          displayName: 'MiniMax',
          models: [minimaxM3],
          providerId: 'minimax',
          runtimeCapability: {
            authScheme: 'bearer',
            baseUrlRegions: [
              { id: 'default', label: 'Default', baseUrl: 'https://api.minimax.io' },
            ],
            supportsLiveValidation: true,
            supportsStreamingValidation: true,
            secretRevealSupported: true,
          },
          serviceCapabilities: [],
          sourceUrl: 'https://platform.minimax.io/docs',
          verifiedDate: '2026-07-09',
        },
      ],
    }
    const minimaxSettings: ListProviderSettingsResponse = {
      ...settings,
      configs: [
        ...settings.configs,
        {
          id: 'cfg-minimax',
          providerId: 'minimax',
          modelId: 'MiniMax-M3',
          displayName: 'MiniMax Messages',
          hasApiKey: true,
          hasOfficialQuotaApiKey: false,
          isDefault: false,
          protocol: 'messages',
          modelDescriptor: minimaxM3,
        },
      ],
    }
    const saveProviderSettings = vi.fn().mockResolvedValue({
      config: minimaxSettings.configs[3],
      status: 'saved',
    })
    const client = {
      ...readyClient({
        modelProviderCatalog: minimaxCatalog,
        providerSettingsList: minimaxSettings,
      }),
      saveProviderSettings,
    } satisfies CommandClient
    renderModelSettingsPage(client)

    const row = within(await screen.findByRole('row', { name: /MiniMax Messages/ }))
    fireEvent.click(row.getByRole('button', { name: 'Set MiniMax Messages as default' }))

    await waitFor(() =>
      expect(saveProviderSettings).toHaveBeenCalledWith({
        configId: 'cfg-minimax',
        displayName: 'MiniMax Messages',
        modelId: 'MiniMax-M3',
        providerId: 'minimax',
        protocol: 'messages',
        setDefault: true,
      }),
    )
  })

  it('blocks concurrent default writes while a default save is pending', async () => {
    let resolveSave: (() => void) | undefined
    const saveProviderSettings = vi.fn(
      () =>
        new Promise<Awaited<ReturnType<CommandClient['saveProviderSettings']>>>((resolve) => {
          resolveSave = () =>
            resolve({
              config: {
                ...settings.configs[1],
                isDefault: true,
              },
              status: 'saved',
            })
        }),
    )
    renderModelSettingsPage({
      ...readyClient(),
      saveProviderSettings,
    })

    const researchRow = within(await screen.findByRole('row', { name: /Research Claude/ }))
    fireEvent.click(researchRow.getByRole('button', { name: 'Set Research Claude as default' }))
    expect(
      await researchRow.findByRole('button', { name: 'Setting Research Claude as default' }),
    ).toBeDisabled()

    fireEvent.change(screen.getByLabelText('Provider'), { target: { value: 'openai' } })

    const backupRow = within(screen.getByRole('row', { name: /Backup OpenAI/ }))
    const backupDefault = backupRow.getByRole('button', {
      name: 'Set Backup OpenAI as default',
    })
    expect(backupDefault).toBeDisabled()
    fireEvent.click(backupDefault)
    expect(saveProviderSettings).toHaveBeenCalledTimes(1)

    resolveSave?.()
  })

  it('keeps partial probe, usage, and quota failures local to affected metrics', async () => {
    renderModelSettingsPage(
      readyClient({
        modelSettingsPage: readyModelSettingsPage({
          probeSnapshots: { status: 'error', safeMessage: 'Probe unavailable' },
          usageSummary: { status: 'error', safeMessage: 'Usage unavailable' },
          quotaSnapshots: { status: 'error', safeMessage: 'Quota unavailable' },
        }),
      }),
    )

    expect(await screen.findByRole('row', { name: /Primary OpenAI/ })).toBeInTheDocument()
    await waitFor(() => expect(screen.getAllByText('Unavailable').length).toBeGreaterThanOrEqual(3))
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('polls model settings only while the usage slice is rebuilding', async () => {
    vi.useFakeTimers()
    try {
      let page = readyModelSettingsPage({
        usageSummary: { status: 'rebuilding', safeMessage: 'Usage projection rebuilding' },
      })
      const getModelSettingsPage = vi.fn(async () => page)
      renderModelSettingsPage({ ...readyClient(), getModelSettingsPage })

      await act(async () => {
        await vi.advanceTimersByTimeAsync(0)
      })
      expect(getModelSettingsPage).toHaveBeenCalledTimes(1)

      page = readyModelSettingsPage()
      await act(async () => {
        await vi.advanceTimersByTimeAsync(250)
      })
      expect(getModelSettingsPage).toHaveBeenCalledTimes(2)

      await act(async () => {
        await vi.advanceTimersByTimeAsync(5_000)
      })
      expect(getModelSettingsPage).toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('opens the configuration dialog for new provider profiles', async () => {
    renderModelSettingsPage()

    await screen.findByRole('row', { name: /Primary OpenAI/ })
    fireEvent.click(screen.getByRole('button', { name: 'New configuration' }))

    const dialog = screen.getByRole('dialog', { name: 'Create model configuration' })
    expect(within(dialog).getByLabelText('Provider')).toHaveValue('openai')
    expect(within(dialog).getByLabelText('Model')).toHaveValue('gpt-4.1')
    expect(within(dialog).getByLabelText('API key')).toHaveValue('')
  })

  it('does not render API keys or raw provider payloads', async () => {
    const { container } = renderModelSettingsPage()

    await screen.findByRole('row', { name: /Primary OpenAI/ })
    expect(container).not.toHaveTextContent('sk-live-secret')
    expect(container).not.toHaveTextContent('Authorization')
    expect(container).not.toHaveTextContent('raw_provider_payload')
  })

  it('has Models and Capability Routes sub-tabs and keeps routes out of the model matrix', async () => {
    renderModelSettingsPage()

    expect(await screen.findByRole('tab', { name: 'Models' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Capability Routes' })).toBeInTheDocument()
    expect(screen.getByLabelText('Model matrix')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('tab', { name: 'Capability Routes' }))

    expect(screen.getByRole('table', { name: 'Capability route table' })).toBeInTheDocument()
    expect(
      screen.getByRole('row', {
        name: /Image generation.*Primary OpenAI.*Online.*Sync.*Medium/,
      }),
    ).toBeInTheDocument()
    expect(
      screen.getByRole('row', { name: /Video generation.*Not configured/ }),
    ).toBeInTheDocument()
    expect(screen.getByRole('row', { name: /Speech to text.*Not configured/ })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: /Text to speech.*Not configured/ })).toBeInTheDocument()
    expect(
      screen.getByRole('row', { name: /Music generation.*Not configured/ }),
    ).toBeInTheDocument()
    expect(screen.queryByLabelText('Model matrix')).not.toBeInTheDocument()
  })
})
