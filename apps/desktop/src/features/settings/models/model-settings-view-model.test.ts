import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import { createElement, type ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type {
  GetModelUsageSummaryResponse,
  ListOfficialQuotaSnapshotsResponse,
  ListProviderCapabilityRouteOptionsResponse,
  ListProviderCapabilityRoutesResponse,
  ListProviderProbeSnapshotsResponse,
  ListProviderSettingsResponse,
  ModelProviderCatalogResponse,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import {
  ModelSettingsMutationBlockedError,
  useModelSettingsViewModel,
  useProbeProviderConfig,
  useRefreshOfficialQuota,
} from './model-settings-queries'
import {
  buildModelSettingsPageState,
  buildModelSettingsViewModel,
  emptyUsageSummary,
  isFailingConnectivity,
  isModelScopedQuota,
  type ModelSettingsQueryInputs,
  modelUsageKey,
} from './model-settings-view-model'

const conversationCapability = {
  inputModalities: ['text'] as ('text' | 'image' | 'audio' | 'video' | 'file' | 'embedding')[],
  outputModalities: ['text'] as ('text' | 'image' | 'audio' | 'video' | 'file' | 'embedding')[],
  contextWindow: 128000,
  maxOutputTokens: 8192,
  streaming: true,
  toolCalling: true,
  reasoning: false,
  promptCache: false,
  structuredOutput: true,
}

const modelDescriptor = {
  protocol: 'chat_completions' as const,
  supportedParameters: [],
  conversationCapability,
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
      models: [modelDescriptor],
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
  ],
}

const settings: ListProviderSettingsResponse = {
  defaultConfigId: 'cfg-primary',
  selectionScope: 'global',
  configs: [
    {
      id: 'cfg-primary',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      displayName: 'Primary',
      hasApiKey: true,
      hasOfficialQuotaApiKey: false,
      isDefault: true,
      protocol: 'chat_completions',
      modelDescriptor,
    },
    {
      id: 'cfg-backup',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      displayName: 'Backup',
      hasApiKey: true,
      hasOfficialQuotaApiKey: false,
      isDefault: false,
      protocol: 'chat_completions',
      modelDescriptor,
    },
  ],
}

const usageBucket = {
  key: 'openai/gpt-4.1',
  providerId: 'openai' as const,
  modelId: 'gpt-4.1',
  usage: {
    cacheReadTokens: 1,
    cacheWriteTokens: 2,
    costMicros: 100,
    inputTokens: 10,
    outputTokens: 5,
    toolCalls: 1,
  },
  lastUsedAt: '2026-06-30T10:00:00Z',
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

const usageSummary: GetModelUsageSummaryResponse = {
  timezoneId: 'UTC',
  timezoneOffsetMinutes: 0,
  today: { period: 'today', total: usageBucket.usage, byModel: [usageBucket] },
  monthToDate: {
    period: 'month_to_date',
    total: { ...usageBucket.usage, inputTokens: 20 },
    byModel: [{ ...usageBucket, usage: { ...usageBucket.usage, inputTokens: 20 } }],
  },
  allTime: {
    period: 'all_time',
    total: { ...usageBucket.usage, inputTokens: 100 },
    byModel: [{ ...usageBucket, usage: { ...usageBucket.usage, inputTokens: 100 } }],
  },
  activity: {
    rangeStart: '2026-06-24',
    rangeEnd: '2026-06-30',
    peakDayTokens: 40,
    currentStreakDays: 2,
    longestStreakDays: 3,
    longestTaskDurationMs: 61000,
    days: [
      { date: '2026-06-24', usage: usage(5, 0) },
      { date: '2026-06-25', usage: usage(0, 0) },
      { date: '2026-06-26', usage: usage(7, 3) },
      { date: '2026-06-27', usage: usage(10, 10) },
      { date: '2026-06-28', usage: usage(0, 0) },
      { date: '2026-06-29', usage: usage(20, 10) },
      { date: '2026-06-30', usage: usage(30, 10) },
    ],
  },
  generatedAt: '2026-06-30T12:00:00Z',
}

const probeSnapshots: ListProviderProbeSnapshotsResponse = {
  snapshots: [
    {
      configId: 'cfg-primary',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      status: 'online',
      timeoutMs: 10_000,
      latencyMs: 120,
      checkedAt: '2026-06-30T11:00:00Z',
    },
    {
      configId: 'cfg-backup',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      status: 'timeout',
      timeoutMs: 10_000,
      checkedAt: '2026-06-30T11:05:00Z',
      errorKind: 'timeout',
      safeMessage: 'Probe timed out',
    },
  ],
}

const quotaSnapshots: ListOfficialQuotaSnapshotsResponse = {
  snapshots: [
    {
      configId: 'cfg-primary',
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
      configId: 'cfg-backup',
      providerId: 'openai',
      scope: 'provider',
      status: 'unsupported',
      sourceUrl: 'https://platform.openai.com/docs/api-reference/usage',
      fetchedAt: '2026-06-30T11:00:00Z',
      expiresAt: '2026-06-30T12:00:00Z',
      isStale: false,
      safeMessage: 'Official quota API is unavailable for this provider profile.',
    },
  ],
}

const routes: ListProviderCapabilityRoutesResponse = {
  version: 1,
  routes: [
    {
      kind: 'image_generation',
      configId: 'cfg-primary',
      providerId: 'openai',
      operationIds: ['images.generate'],
      enabled: true,
    },
  ],
}

const routeOptions: ListProviderCapabilityRouteOptionsResponse = {
  options: [
    {
      kind: 'image_generation',
      configId: 'cfg-primary',
      providerId: 'openai',
      operationId: 'images.generate',
      outputArtifact: 'image',
      execution: 'sync',
      costRisk: 'medium',
      runtimeSupported: true,
    },
    {
      kind: 'image_generation',
      configId: 'cfg-backup',
      providerId: 'openai',
      operationId: 'images.generate',
      outputArtifact: 'image',
      execution: 'sync',
      costRisk: 'medium',
      runtimeSupported: false,
      unavailableReason: 'Missing image capability on selected model',
    },
    {
      kind: 'video_generation',
      configId: 'cfg-primary',
      providerId: 'openai',
      operationId: 'videos.generate',
      outputArtifact: 'video',
      execution: 'async_job',
      costRisk: 'high',
      runtimeSupported: true,
    },
    {
      kind: 'speech_to_text',
      configId: 'cfg-backup',
      providerId: 'openai',
      operationId: 'audio.transcriptions',
      outputArtifact: 'text',
      execution: 'sync',
      costRisk: 'low',
      runtimeSupported: false,
      unavailableReason: 'Backend route option rejected this profile',
    },
    {
      kind: 'text_to_speech',
      configId: 'cfg-primary',
      providerId: 'openai',
      operationId: 'audio.speech',
      outputArtifact: 'audio',
      execution: 'sync',
      costRisk: 'medium',
      runtimeSupported: true,
    },
    {
      kind: 'music_generation',
      configId: 'cfg-primary',
      providerId: 'openai',
      operationId: 'music.generate',
      outputArtifact: 'audio',
      execution: 'async_job',
      costRisk: 'high',
      runtimeSupported: true,
    },
  ],
}

function ready<T>(data: T) {
  return { status: 'ready' as const, data }
}

function errorSlice(message: string) {
  return { status: 'error' as const, safeMessage: message }
}

function baseInput(overrides: Partial<ModelSettingsQueryInputs> = {}): ModelSettingsQueryInputs {
  return {
    catalog: ready(catalog),
    providerSettings: ready(settings),
    probeSnapshots: ready(probeSnapshots),
    usageSummary: ready(usageSummary),
    quotaSnapshots: ready(quotaSnapshots),
    routes: ready(routes),
    routeOptions: ready(routeOptions),
    ...overrides,
  }
}

function createQueryWrapper(commandClient = createTestCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { gcTime: 0, retry: false },
      mutations: { retry: false },
    },
  })

  return {
    queryClient,
    wrapper: ({ children }: { children: ReactNode }) =>
      createElement(
        QueryClientProvider,
        { client: queryClient },
        createElement(CommandClientProvider, { client: commandClient, children }),
      ),
  }
}

describe('model-settings-view-model', () => {
  it('merges provider settings, catalog, probes, usage, quota, and routes into model asset rows', () => {
    const viewModel = buildModelSettingsViewModel(baseInput())

    expect(viewModel.rows).toHaveLength(2)
    expect(viewModel.rows[0]).toMatchObject({
      configId: 'cfg-primary',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      providerDisplayName: 'OpenAI',
      isDefault: true,
      connectivity: { status: 'online', latencyMs: 120 },
      usage: {
        status: 'ready',
        today: usageBucket.usage,
        monthToDate: { ...usageBucket.usage, inputTokens: 20 },
        allTime: { ...usageBucket.usage, inputTokens: 100 },
      },
      quota: {
        status: 'supported',
        scope: 'account',
        scopeLabel: 'account',
      },
    })
  })

  it('keys probe display by config id and usage display by provider/model', () => {
    const viewModel = buildModelSettingsViewModel(baseInput())
    const primary = viewModel.rows.find((row) => row.configId === 'cfg-primary')
    const backup = viewModel.rows.find((row) => row.configId === 'cfg-backup')

    expect(primary?.connectivity).toMatchObject({ status: 'online' })
    expect(backup?.connectivity).toMatchObject({ status: 'timeout' })
    expect(primary?.usage.status).toBe('ready')
    if (primary?.usage.status === 'ready' && backup?.usage.status === 'ready') {
      expect(primary.usage.today).toEqual(backup.usage.today)
    }
    expect(modelUsageKey('openai', 'gpt-4.1')).toBe('openai/gpt-4.1')
  })

  it('never derives today or month usage from all-time totals in the view model', () => {
    const viewModel = buildModelSettingsViewModel(baseInput())
    const row = viewModel.rows[0]

    expect(row.usage.status).toBe('ready')
    if (row.usage.status !== 'ready') {
      return
    }

    expect(row.usage.today.inputTokens).toBe(10)
    expect(row.usage.monthToDate.inputTokens).toBe(20)
    expect(row.usage.allTime.inputTokens).toBe(100)
  })

  it('derives token activity insights from the usage summary activity range', () => {
    const viewModel = buildModelSettingsViewModel(baseInput())

    expect(viewModel.usageInsights.status).toBe('ready')
    if (viewModel.usageInsights.status !== 'ready') {
      return
    }

    expect(viewModel.usageInsights.data.metrics).toEqual({
      totalTokens: 108,
      peakDayTokens: 40,
      longestTaskDurationMs: 61000,
      currentStreakDays: 2,
      longestStreakDays: 3,
    })
    expect(viewModel.usageInsights.data.daily.map((day) => [day.date, day.tokens])).toEqual([
      ['2026-06-24', 5],
      ['2026-06-25', 0],
      ['2026-06-26', 10],
      ['2026-06-27', 20],
      ['2026-06-28', 0],
      ['2026-06-29', 30],
      ['2026-06-30', 40],
    ])
    expect(viewModel.usageInsights.data.daily.map((day) => day.level)).toEqual([
      1, 0, 1, 2, 0, 3, 4,
    ])
    expect(viewModel.usageInsights.data.weekly).toEqual([
      { weekStart: '2026-06-22', weekEnd: '2026-06-28', tokens: 35 },
      { weekStart: '2026-06-29', weekEnd: '2026-07-05', tokens: 70 },
    ])
    expect(viewModel.usageInsights.data.cumulative).toEqual([
      { date: '2026-06-24', tokens: 5 },
      { date: '2026-06-25', tokens: 5 },
      { date: '2026-06-26', tokens: 15 },
      { date: '2026-06-27', tokens: 35 },
      { date: '2026-06-28', tokens: 35 },
      { date: '2026-06-29', tokens: 65 },
      { date: '2026-06-30', tokens: 105 },
    ])
    expect(viewModel.usageInsights.data.monthLabels).toEqual([{ date: '2026-06-24', label: 'Jun' }])
  })

  it('shares model-level usage across duplicate provider/model profiles', () => {
    const viewModel = buildModelSettingsViewModel(baseInput())

    for (const row of viewModel.rows) {
      expect(row.usage.status).toBe('ready')
      if (row.usage.status !== 'ready') {
        continue
      }
      expect(row.usage.sharedModelUsage).toBe(true)
      expect(row.usage.today.inputTokens).toBe(10)
    }
  })

  it('keys quota display by config id and preserves scope labels', () => {
    const viewModel = buildModelSettingsViewModel(baseInput())
    const primary = viewModel.rows.find((row) => row.configId === 'cfg-primary')
    const backup = viewModel.rows.find((row) => row.configId === 'cfg-backup')

    expect(primary?.quota).toMatchObject({ scope: 'account', scopeLabel: 'account' })
    expect(backup?.quota).toMatchObject({
      status: 'unsupported',
      scope: 'provider',
      scopeLabel: 'provider',
      safeMessage: expect.stringContaining('unavailable'),
    })
    expect(isModelScopedQuota('account')).toBe(false)
    expect(isModelScopedQuota('model')).toBe(true)
  })

  it('maps missing probe snapshots to never_checked', () => {
    const viewModel = buildModelSettingsViewModel(
      baseInput({
        probeSnapshots: ready({ snapshots: probeSnapshots.snapshots.slice(0, 1) }),
      }),
    )
    const backup = viewModel.rows.find((row) => row.configId === 'cfg-backup')

    expect(backup?.connectivity).toEqual({ status: 'never_checked' })
  })

  it('derives default model from backend isDefault instead of frontend guesswork', () => {
    const viewModel = buildModelSettingsViewModel(baseInput())

    expect(viewModel.summary.defaultModel).toEqual({
      status: 'ready',
      data: {
        configId: 'cfg-primary',
        displayName: 'Primary',
        providerDisplayName: 'OpenAI',
      },
    })
    expect(viewModel.rows.find((row) => row.isDefault)?.configId).toBe('cfg-primary')
  })

  it('groups capability routes by kind and surfaces unavailable backend reasons', () => {
    const viewModel = buildModelSettingsViewModel(baseInput())

    expect(viewModel.capabilityRoutes.status).toBe('ready')
    if (viewModel.capabilityRoutes.status !== 'ready') {
      return
    }

    const imageRoute = viewModel.capabilityRoutes.data.find(
      (row) => row.kind === 'image_generation',
    )
    expect(imageRoute?.savedRoute?.configId).toBe('cfg-primary')
    expect(imageRoute?.selectedTarget).toMatchObject({
      configId: 'cfg-primary',
      displayName: 'Primary',
      execution: 'sync',
      costRisk: 'medium',
      health: { status: 'online' },
    })
    expect(imageRoute?.eligibleTargets).toEqual([
      expect.objectContaining({
        configId: 'cfg-primary',
        displayName: 'Primary',
        operationIds: ['images.generate'],
      }),
    ])
    expect(imageRoute?.unavailableTargets).toEqual([
      {
        configId: 'cfg-backup',
        displayName: 'Backup',
        providerId: 'openai',
        modelId: 'gpt-4.1',
        operationId: 'images.generate',
        reason: 'Missing image capability on selected model',
      },
    ])
    expect(viewModel.capabilityRoutes.data.map((row) => row.kind)).toEqual([
      'image_generation',
      'video_generation',
      'speech_to_text',
      'text_to_speech',
      'music_generation',
    ])
    expect(viewModel.rows[0]?.routeBindings).toEqual({
      status: 'ready',
      data: [
        expect.objectContaining({ kind: 'image_generation', operationIds: ['images.generate'] }),
      ],
    })
  })

  it('keeps an existing saved route visible when its target becomes unavailable', () => {
    const viewModel = buildModelSettingsViewModel(
      baseInput({
        routes: ready({
          version: 1,
          routes: [
            {
              kind: 'image_generation',
              configId: 'cfg-backup',
              providerId: 'openai',
              operationIds: ['images.generate'],
              enabled: true,
            },
          ],
        }),
      }),
    )

    expect(viewModel.capabilityRoutes.status).toBe('ready')
    if (viewModel.capabilityRoutes.status !== 'ready') {
      return
    }

    const imageRoute = viewModel.capabilityRoutes.data.find(
      (row) => row.kind === 'image_generation',
    )
    expect(imageRoute?.savedRoute?.configId).toBe('cfg-backup')
    expect(imageRoute?.selectedTarget).toMatchObject({
      configId: 'cfg-backup',
      displayName: 'Backup',
      execution: 'sync',
      costRisk: 'medium',
      health: { status: 'timeout' },
    })
    expect(imageRoute?.eligibleTargets).toEqual([
      expect.objectContaining({ configId: 'cfg-primary' }),
    ])
    expect(imageRoute?.unavailableTargets).toEqual([
      expect.objectContaining({
        configId: 'cfg-backup',
        reason: 'Missing image capability on selected model',
      }),
    ])
  })

  it('returns page-blocking error when provider settings or catalog fail', () => {
    expect(
      buildModelSettingsPageState(
        baseInput({ providerSettings: errorSlice('Provider settings unavailable') }),
      ),
    ).toEqual({
      kind: 'error',
      safeMessage: 'Provider settings unavailable',
    })

    expect(
      buildModelSettingsPageState(baseInput({ catalog: errorSlice('Catalog unavailable') })),
    ).toEqual({
      kind: 'error',
      safeMessage: 'Catalog unavailable',
    })
  })

  it('returns partial unavailable sections when usage, probe, or quota queries fail', () => {
    const viewModel = buildModelSettingsViewModel(
      baseInput({
        usageSummary: errorSlice('Usage unavailable'),
        probeSnapshots: errorSlice('Probe unavailable'),
        quotaSnapshots: errorSlice('Quota unavailable'),
      }),
    )

    expect(viewModel.summary.localUsage).toEqual({ status: 'unavailable' })
    expect(viewModel.usageInsights).toEqual({ status: 'unavailable' })
    expect(viewModel.summary.officialQuota).toEqual({ status: 'unavailable' })
    expect(viewModel.rows.every((row) => row.usage.status === 'unavailable')).toBe(true)
    expect(viewModel.rows.every((row) => row.connectivity.status === 'unavailable')).toBe(true)
    expect(viewModel.rows.every((row) => row.quota.status === 'unavailable')).toBe(true)
  })

  it('keeps secondary query pending states as loading after critical data is ready', () => {
    const state = buildModelSettingsPageState(
      baseInput({
        probeSnapshots: { status: 'loading' },
        usageSummary: { status: 'loading' },
        quotaSnapshots: { status: 'loading' },
        routes: { status: 'loading' },
        routeOptions: { status: 'loading' },
      }),
    )

    expect(state.kind).toBe('ready')
    if (state.kind !== 'ready') {
      return
    }

    expect(state.viewModel.summary.configuredModels).toEqual({ status: 'loading' })
    expect(state.viewModel.summary.localUsage).toEqual({ status: 'loading' })
    expect(state.viewModel.summary.officialQuota).toEqual({ status: 'loading' })
    expect(state.viewModel.rows.every((row) => row.connectivity.status === 'loading')).toBe(true)
    expect(state.viewModel.rows.every((row) => row.usage.status === 'loading')).toBe(true)
    expect(state.viewModel.rows.every((row) => row.quota.status === 'loading')).toBe(true)
    expect(state.viewModel.rows.every((row) => row.routeBindings.status === 'loading')).toBe(true)
    expect(state.viewModel.capabilityRoutes).toEqual({ status: 'loading' })
  })

  it('returns distinct route loading and error states for the capability route surface', () => {
    expect(
      buildModelSettingsViewModel(baseInput({ routeOptions: { status: 'loading' } }))
        .capabilityRoutes,
    ).toEqual({ status: 'loading' })

    expect(
      buildModelSettingsViewModel(
        baseInput({ routeOptions: errorSlice('Route options unavailable') }),
      ).capabilityRoutes,
    ).toEqual({ status: 'error', safeMessage: 'Route options unavailable' })
  })

  it('returns empty rows and explicit empty summary for empty backend state', () => {
    const emptySettings: ListProviderSettingsResponse = {
      defaultConfigId: null,
      selectionScope: 'global',
      configs: [],
    }
    const viewModel = buildModelSettingsViewModel(
      baseInput({
        providerSettings: ready(emptySettings),
        usageSummary: ready(emptyUsageSummary()),
        probeSnapshots: ready({ snapshots: [] }),
        quotaSnapshots: ready({ snapshots: [] }),
        routes: ready({ version: 0, routes: [] }),
        routeOptions: ready({ options: [] }),
      }),
    )

    expect(viewModel.rows).toEqual([])
    expect(viewModel.summary.defaultModel).toEqual({ status: 'unavailable' })
    expect(viewModel.summary.configuredModels).toEqual({
      status: 'ready',
      data: { total: 0, available: 0, failing: 0 },
    })
    expect(viewModel.summary.localUsage).toEqual({
      status: 'ready',
      data: {
        today: emptyUsageSummary().today.total,
        monthToDate: emptyUsageSummary().monthToDate.total,
        allTime: emptyUsageSummary().allTime.total,
      },
    })
  })

  it('counts failing configured models from probe snapshots', () => {
    const viewModel = buildModelSettingsViewModel(baseInput())

    expect(viewModel.summary.configuredModels).toEqual({
      status: 'ready',
      data: { total: 2, available: 1, failing: 1 },
    })
    expect(
      isFailingConnectivity(viewModel.rows[1]?.connectivity ?? { status: 'never_checked' }),
    ).toBe(true)
  })
})

describe('model-settings-queries', () => {
  it('blocks duplicate probe mutations for the same config id while pending', async () => {
    const deferred = createDeferred<{
      snapshot: ListProviderProbeSnapshotsResponse['snapshots'][number]
    }>()
    const baseClient = createTestCommandClient()
    const commandClient = {
      ...baseClient,
      probeProviderConfig: vi.fn(() => deferred.promise),
    }
    const { wrapper } = createQueryWrapper(commandClient)
    const { result } = renderHook(() => useProbeProviderConfig(), { wrapper })

    const first = result.current.probeConfig('cfg-primary')
    await waitFor(() => expect(result.current.isPendingForConfig('cfg-primary')).toBe(true))

    await expect(result.current.probeConfig('cfg-primary')).rejects.toBeInstanceOf(
      ModelSettingsMutationBlockedError,
    )

    const primaryProbeSnapshot = probeSnapshots.snapshots[0]
    if (!primaryProbeSnapshot) {
      throw new Error('missing primary probe snapshot fixture')
    }

    deferred.resolve({
      snapshot: primaryProbeSnapshot,
    })
    await expect(first).resolves.toBeDefined()
    await waitFor(() => expect(result.current.isPendingForConfig('cfg-primary')).toBe(false))
  })

  it('blocks duplicate quota refresh mutations for the same config id while pending', async () => {
    const deferred = createDeferred<ListOfficialQuotaSnapshotsResponse['snapshots'][number]>()
    const baseClient = createTestCommandClient()
    const commandClient = {
      ...baseClient,
      refreshOfficialQuota: vi.fn(async () => ({ snapshot: await deferred.promise })),
    }
    const { wrapper } = createQueryWrapper(commandClient)
    const { result } = renderHook(() => useRefreshOfficialQuota(), { wrapper })

    const first = result.current.refreshQuota('cfg-primary')
    await waitFor(() => expect(result.current.isPendingForConfig('cfg-primary')).toBe(true))

    await expect(result.current.refreshQuota('cfg-primary')).rejects.toBeInstanceOf(
      ModelSettingsMutationBlockedError,
    )

    const primaryQuotaSnapshot = quotaSnapshots.snapshots[0]
    if (!primaryQuotaSnapshot) {
      throw new Error('missing primary quota snapshot fixture')
    }

    deferred.resolve(primaryQuotaSnapshot)
    await expect(first).resolves.toBeDefined()
    await waitFor(() => expect(result.current.isPendingForConfig('cfg-primary')).toBe(false))
  })

  it('composes backend queries into a ready page view model', async () => {
    const commandClient = createTestCommandClient({
      providerSettingsList: settings,
      modelProviderCatalog: catalog,
      providerProbeSnapshots: probeSnapshots,
      modelUsageSummary: usageSummary,
      officialQuotaSnapshots: quotaSnapshots,
      providerCapabilityRoutes: routes,
      providerCapabilityRouteOptions: routeOptions,
    })
    const { wrapper } = createQueryWrapper(commandClient)
    const { result } = renderHook(() => useModelSettingsViewModel(), { wrapper })

    await waitFor(() => expect(result.current.pageState.kind).toBe('ready'))
    if (result.current.pageState.kind !== 'ready') {
      return
    }

    expect(result.current.pageState.viewModel.rows).toHaveLength(2)
    expect(result.current.isProbePending('cfg-primary')).toBe(false)
  })
})

function createDeferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}
