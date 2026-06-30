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
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { ModelSettingsPage } from './ModelSettingsPage'

uiStore.getState().setLocale('en-US')

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
  (createClient: () => CommandClient): Decorator =>
  (StoryComponent) => (
    <StoryFrame>
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

export const Loading: Story = {
  decorators: [withClient(() => createTestCommandClient({ delayMs: 120_000 }))],
}

export const Empty: Story = {
  decorators: [
    withClient(() =>
      createTestCommandClient({
        modelProviderCatalog: catalog,
        providerSettingsList: { defaultConfigId: null, configs: [] },
      }),
    ),
  ],
}

export const Ready: Story = {
  decorators: [withClient(() => readyClient())],
}

export const PartialData: Story = {
  decorators: [
    withClient(() =>
      failingNonCriticalClient({
        getModelUsageSummary: true,
        listOfficialQuotaSnapshots: true,
        listProviderProbeSnapshots: true,
      }),
    ),
  ],
}

export const ErrorState: Story = {
  name: 'Error',
  decorators: [
    withClient(() => ({
      ...readyClient(),
      listProviderSettings: async () => {
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

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="min-h-screen bg-background p-4 text-foreground">{children}</main>
}

function failingNonCriticalClient(failures: {
  getModelUsageSummary?: boolean
  listOfficialQuotaSnapshots?: boolean
  listProviderProbeSnapshots?: boolean
}) {
  const client = readyClient()
  return {
    ...client,
    getModelUsageSummary: failures.getModelUsageSummary
      ? async () => {
          throw new globalThis.Error('Usage summary unavailable.')
        }
      : client.getModelUsageSummary,
    listOfficialQuotaSnapshots: failures.listOfficialQuotaSnapshots
      ? async () => {
          throw new globalThis.Error('Quota snapshots unavailable.')
        }
      : client.listOfficialQuotaSnapshots,
    listProviderProbeSnapshots: failures.listProviderProbeSnapshots
      ? async () => {
          throw new globalThis.Error('Probe snapshots unavailable.')
        }
      : client.listProviderProbeSnapshots,
  } satisfies CommandClient
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
