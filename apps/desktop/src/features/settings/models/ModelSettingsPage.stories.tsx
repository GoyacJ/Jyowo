import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'

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
  ModelSettingsPageResponse,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { ModelSettingsPage } from './ModelSettingsPage'

const meta = {
  title: 'Settings/ModelSettingsPage',
  component: ModelSettingsPage,
  parameters: {
    layout: 'fullscreen',
  },
} satisfies Meta<typeof ModelSettingsPage>

export default meta

type Story = StoryObj<typeof meta>

const withClient =
  (
    createClient: () => CommandClient,
    appearance: { locale?: 'en-US' | 'zh-CN'; dark?: boolean } = {},
  ): Decorator =>
  (StoryComponent) => {
    uiStore.getState().setLocale(appearance.locale ?? 'en-US')
    return (
      <StoryFrame dark={appearance.dark}>
        <CommandClientProvider client={createClient()}>
          <QueryClientProvider
            client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
          >
            <AppI18nProvider>
              <StoryComponent />
            </AppI18nProvider>
          </QueryClientProvider>
        </CommandClientProvider>
      </StoryFrame>
    )
  }

export const Loading: Story = {
  decorators: [withClient(() => createTestCommandClient({ delayMs: 120_000 }))],
}

export const Empty: Story = {
  decorators: [
    withClient(() =>
      createTestCommandClient({
        modelProviderCatalog: catalog,
        providerSettingsList: { defaultConfigId: null, selectionScope: 'global', configs: [] },
      }),
    ),
  ],
}

export const Ready: Story = {
  decorators: [withClient(() => readyClient())],
}

export const ReferenceDesign: Story = {
  decorators: [withClient(() => readyClient(), { locale: 'zh-CN', dark: true })],
}

export const PartialData: Story = {
  decorators: [
    withClient(() =>
      readyClient({
        modelSettingsPage: readyModelSettingsPage({
          probeSnapshots: { status: 'error', safeMessage: 'Probe snapshots unavailable.' },
          usageSummary: { status: 'error', safeMessage: 'Usage summary unavailable.' },
          quotaSnapshots: { status: 'error', safeMessage: 'Quota snapshots unavailable.' },
        }),
      }),
    ),
  ],
}

export const ErrorState: Story = {
  name: 'Error',
  decorators: [
    withClient(() => ({
      ...readyClient(),
      getModelSettingsPage: async () => {
        throw new globalThis.Error('Provider settings could not be read safely.')
      },
    })),
  ],
}

export const UnsupportedQuota: Story = {
  decorators: [
    withClient(() =>
      readyClient({
        officialQuotaSnapshots: {
          snapshots: quotaSnapshots.snapshots.map((snapshot) => ({
            ...snapshot,
            status: 'unsupported',
            safeMessage: 'Official quota API is unavailable for this provider.',
          })),
        },
      }),
    ),
  ],
}

export const NarrowLayout: Story = {
  decorators: [withClient(() => readyClient())],
  parameters: {
    viewport: {
      defaultViewport: 'mobile1',
    },
  },
}

function StoryFrame({ children, dark }: { children: ReactNode; dark?: boolean }) {
  return (
    <main className={`${dark ? 'dark ' : ''}min-h-screen bg-background p-4 text-foreground`}>
      {children}
    </main>
  )
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
    capabilityRoutes: { status: 'ready', data: { version: 1, routes: [] } },
    capabilityRouteOptions: { status: 'ready', data: { options: [] } },
    generatedAt: '2026-06-30T12:00:00Z',
    ...overrides,
  }
}

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
  ],
}

const usageSummary: GetModelUsageSummaryResponse = {
  timezoneId: 'UTC',
  timezoneOffsetMinutes: 0,
  today: {
    period: 'today',
    total: usage(930_000_000, 0),
    byModel: [
      {
        key: 'openai/gpt-4.1',
        providerId: 'openai',
        modelId: 'gpt-4.1',
        usage: usage(930_000_000, 0),
      },
    ],
  },
  monthToDate: {
    period: 'month_to_date',
    total: usage(2_140_000_000, 0),
    byModel: [
      {
        key: 'openai/gpt-4.1',
        providerId: 'openai',
        modelId: 'gpt-4.1',
        usage: usage(2_140_000_000, 0),
      },
    ],
  },
  allTime: {
    period: 'all_time',
    total: usage(12_730_000_000, 0),
    byModel: [
      {
        key: 'openai/gpt-4.1',
        providerId: 'openai',
        modelId: 'gpt-4.1',
        usage: usage(12_730_000_000, 0),
      },
    ],
  },
  activity: {
    rangeStart: '2025-08-01',
    rangeEnd: '2026-07-31',
    peakDayTokens: 930_000_000,
    currentStreakDays: 3,
    longestStreakDays: 18,
    longestTaskDurationMs: 59_280_000,
    days: referenceUsageDays(),
  },
  generatedAt: '2026-07-31T12:00:00Z',
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

function referenceUsageDays() {
  const start = Date.UTC(2025, 7, 1)
  return Array.from({ length: 365 }, (_, index) => {
    const date = new Date(start + index * 86_400_000).toISOString().slice(0, 10)
    return { date, usage: usage(referenceDayTokens(date, index), 0) }
  })
}

function referenceDayTokens(date: string, index: number): number {
  if (date === '2026-06-18') {
    return 930_000_000
  }

  const month = Number(date.slice(5, 7))
  const density =
    date < '2026-02-01' ? 0 : ({ 2: 22, 3: 28, 4: 58, 5: 72, 6: 68, 7: 34 }[month] ?? 0)
  const sample = (index * 37 + month * 17) % 100
  if (sample >= density) {
    return 0
  }

  return ((sample % 4) + 1) * 170_000_000
}
