import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type {
  CommandClient,
  ConversationModelCapability,
  GetModelUsageSummaryResponse,
  ListOfficialQuotaSnapshotsResponse,
  ListProviderProbeSnapshotsResponse,
  ListProviderSettingsResponse,
  ModelProviderCatalogResponse,
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
  displayName: 'Claude Sonnet',
  modelId: 'claude-sonnet',
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
  configs: [
    {
      id: 'cfg-openai',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      displayName: 'Primary OpenAI',
      hasApiKey: true,
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

  return render(<ModelSettingsPage />, { wrapper: Wrapper })
}

function readyClient(overrides: Parameters<typeof createTestCommandClient>[0] = {}) {
  return createTestCommandClient({
    modelProviderCatalog: catalog,
    providerSettingsList: settings,
    providerProbeSnapshots: probeSnapshots,
    modelUsageSummary: usageSummary,
    officialQuotaSnapshots: quotaSnapshots,
    ...overrides,
  })
}

describe('ModelSettingsPage', () => {
  it('renders loading, empty, and safe error states', async () => {
    const { unmount } = renderModelSettingsPage(readyClient({ delayMs: 50 }))
    expect(screen.getByText('Loading model settings')).toBeInTheDocument()
    unmount()

    renderModelSettingsPage(
      readyClient({ providerSettingsList: { defaultConfigId: null, configs: [] } }),
    )
    expect(await screen.findByText('No configured models')).toBeInTheDocument()
    unmount()

    const errorClient = {
      ...readyClient(),
      listProviderSettings: vi.fn().mockRejectedValue(new Error('Safe backend error')),
    } satisfies CommandClient
    renderModelSettingsPage(errorClient)
    expect(await screen.findByRole('alert')).toHaveTextContent('Safe backend error')
  })

  it('renders summary band and matrix rows with backend usage, connectivity, and quota state', async () => {
    renderModelSettingsPage()

    expect(await screen.findByRole('heading', { name: 'Models' })).toBeInTheDocument()
    expect(await screen.findByRole('row', { name: /Primary OpenAI/ })).toBeInTheDocument()
    expect(screen.getByText('3 configured')).toBeInTheDocument()
    expect(screen.getByText('1 available')).toBeInTheDocument()
    expect(screen.getByText('2 failing')).toBeInTheDocument()
    expect(screen.getAllByText('Today').length).toBeGreaterThan(0)
    expect(screen.getByText('200 tokens')).toBeInTheDocument()
    expect(screen.getByLabelText('Model settings summary')).toHaveTextContent('660 tokens')
    expect(screen.getByLabelText('Model settings summary')).toHaveTextContent('2,100 tokens')

    const primaryRow = within(screen.getByRole('row', { name: /Primary OpenAI/ }))
    expect(primaryRow.getByText('OpenAI')).toBeInTheDocument()
    expect(primaryRow.getByText('Default')).toBeInTheDocument()
    expect(primaryRow.getByText('Online')).toBeInTheDocument()
    expect(primaryRow.getByText('118 ms')).toBeInTheDocument()
    expect(primaryRow.getByText('10,000 ms')).toBeInTheDocument()
    expect(primaryRow.getByText('100 tokens')).toBeInTheDocument()
    expect(primaryRow.getByText('310 tokens')).toBeInTheDocument()
    expect(primaryRow.getByText('1,400 tokens')).toBeInTheDocument()
    expect(primaryRow.getByText('Supported')).toBeInTheDocument()

    expect(screen.getByRole('row', { name: /Research Claude.*Unsupported/ })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: /Backup OpenAI.*Failed/ })).toBeInTheDocument()
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
    const client = {
      ...baseClient,
      probeProviderConfig,
      refreshOfficialQuota,
    } satisfies CommandClient

    renderModelSettingsPage(client)
    const row = within(await screen.findByRole('row', { name: /Primary OpenAI/ }))

    fireEvent.click(row.getByRole('button', { name: 'Probe Primary OpenAI' }))
    expect(await row.findByRole('button', { name: 'Probing Primary OpenAI' })).toBeDisabled()
    fireEvent.click(row.getByRole('button', { name: 'Probing Primary OpenAI' }))
    expect(probeProviderConfig).toHaveBeenCalledTimes(1)
    expect(probeProviderConfig).toHaveBeenCalledWith({ configId: 'cfg-openai', timeoutMs: 10000 })
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

  it('keeps partial probe, usage, and quota failures local to affected metrics', async () => {
    const client = {
      ...readyClient(),
      getModelUsageSummary: vi.fn().mockRejectedValue(new Error('Usage unavailable')),
      listOfficialQuotaSnapshots: vi.fn().mockRejectedValue(new Error('Quota unavailable')),
      listProviderProbeSnapshots: vi.fn().mockRejectedValue(new Error('Probe unavailable')),
    } satisfies CommandClient
    renderModelSettingsPage(client)

    expect(await screen.findByRole('row', { name: /Primary OpenAI/ })).toBeInTheDocument()
    expect(screen.getAllByText('Unavailable').length).toBeGreaterThanOrEqual(3)
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('does not render API keys or raw provider payloads', async () => {
    const { container } = renderModelSettingsPage()

    await screen.findByRole('row', { name: /Primary OpenAI/ })
    expect(container).not.toHaveTextContent('sk-live-secret')
    expect(container).not.toHaveTextContent('Authorization')
    expect(container).not.toHaveTextContent('raw_provider_payload')
  })
})
