import type {
  AutomationSpec,
  ConversationModelCapability,
  GetModelUsageSummaryResponse,
  ListAutomationRunsResponse,
  ListAutomationsResponse,
  ListEvalCasesResponse,
  ListOfficialQuotaSnapshotsResponse,
  ListProjectsResponse,
  ListProviderProbeSnapshotsResponse,
  ListProviderSettingsResponse,
  ModelProviderCatalogResponse,
  ProbeProviderConfigResponse,
  RefreshOfficialQuotaResponse,
  SaveAutomationRequest,
  SaveProviderSettingsResponse,
  ValidateProviderSettingsResponse,
} from '@/shared/tauri/commands'

import { timestamp } from './base'

export const fixtureAutomation = {
  id: 'checks',
  enabled: false,
  prompt: 'Run checks',
  schedule: { intervalMinutes: 30 },
  toolProfile: 'coding',
  permissionMode: 'default',
  sandboxMode: 'none',
  workspaceScope: 'current_workspace',
  workspaceAccess: 'read_only',
  missedRunPolicy: 'skip',
  createdAt: '2026-06-30T01:00:00Z',
  updatedAt: '2026-06-30T01:00:00Z',
} satisfies ListAutomationsResponse['automations'][number]

export const fixtureListAutomations: ListAutomationsResponse = {
  automations: [fixtureAutomation],
}

export const fixtureAutomationRun = {
  automationId: 'checks',
  completedAt: '2026-06-30T01:01:00Z',
  id: 'automation-run-001',
  message: 'Starting automation runs requires the runtime conversation facade.',
  startedAt: '2026-06-30T01:00:00Z',
  status: 'rejected',
} satisfies ListAutomationRunsResponse['runs'][number]

export const fixtureAutomationRuns: ListAutomationRunsResponse = {
  runs: [fixtureAutomationRun],
}

export const fixtureValidateProviderSettings: ValidateProviderSettingsResponse = {
  modelId: 'gpt-4o-mini',
  providerId: 'openai',
  status: 'accepted',
}

const textCapability: ConversationModelCapability = {
  inputModalities: ['text'],
  outputModalities: ['text'],
  contextWindow: 128000,
  maxOutputTokens: 8192,
  streaming: true,
  toolCalling: false,
  reasoning: false,
  promptCache: false,
  structuredOutput: false,
}

export const fixtureModelProviderCatalog: ModelProviderCatalogResponse = {
  providers: [
    {
      defaultBaseUrl: 'https://api.openai.com',
      displayName: 'OpenAI',
      models: [
        {
          protocol: 'responses',
          supportedParameters: [],
          conversationCapability: {
            ...textCapability,
            inputModalities: ['text', 'image'],
            maxOutputTokens: 16384,
            toolCalling: true,
            structuredOutput: true,
          },
          contextWindow: 128000,
          displayName: 'GPT-4o mini',
          lifecycle: { kind: 'stable' },
          maxOutputTokens: 16384,
          modelId: 'gpt-4o-mini',
          runtimeStatus: { kind: 'runnable' },
        },
      ],
      providerId: 'openai',
      runtimeCapability: {
        authScheme: 'bearer',
        baseUrlRegions: [{ id: 'default', label: 'Default', baseUrl: 'https://api.openai.com' }],
        supportsLiveValidation: false,
        supportsStreamingValidation: true,
        secretRevealSupported: true,
      },
      serviceCapabilities: [],
      sourceUrl: 'https://platform.openai.com/docs/models',
      verifiedDate: '2026-06-21',
    },
    {
      defaultBaseUrl: 'http://localhost:11434',
      displayName: 'Local Llama',
      models: [
        {
          protocol: 'messages',
          supportedParameters: [],
          conversationCapability: textCapability,
          contextWindow: 128000,
          displayName: 'Llama 3.1',
          lifecycle: { kind: 'stable' },
          maxOutputTokens: 8192,
          modelId: 'llama3.1',
          runtimeStatus: { kind: 'runnable' },
        },
      ],
      providerId: 'local-llama',
      runtimeCapability: {
        authScheme: 'none',
        baseUrlRegions: [{ id: 'default', label: 'Default', baseUrl: 'http://localhost:11434' }],
        supportsLiveValidation: false,
        supportsStreamingValidation: false,
        secretRevealSupported: false,
      },
      serviceCapabilities: [],
      sourceUrl: 'https://ollama.com/library/llama3.1',
      verifiedDate: '2026-06-21',
    },
  ],
}

export const fixtureProviderSettingsList: ListProviderSettingsResponse = {
  defaultConfigId: 'provider-config-001',
  selectionScope: 'global',
  configs: [
    {
      protocol: 'responses',
      displayName: 'OpenAI',
      hasApiKey: true,
      hasOfficialQuotaApiKey: false,
      id: 'provider-config-001',
      isDefault: true,
      modelDescriptor: {
        protocol: 'responses',
        supportedParameters: [],
        conversationCapability: {
          ...textCapability,
          inputModalities: ['text', 'image'],
          maxOutputTokens: 16384,
          structuredOutput: true,
          toolCalling: true,
        },
        contextWindow: 128000,
        displayName: 'GPT-4o mini',
        lifecycle: { kind: 'stable' },
        maxOutputTokens: 16384,
        modelId: 'gpt-4o-mini',
        runtimeStatus: { kind: 'runnable' },
      },
      modelId: 'gpt-4o-mini',
      providerId: 'openai',
    },
  ],
}

export const fixtureSaveProviderSettings: SaveProviderSettingsResponse = {
  config: {
    protocol: 'responses',
    baseUrl: 'https://api.openai.com',
    displayName: 'OpenAI',
    hasApiKey: true,
    hasOfficialQuotaApiKey: false,
    id: 'openai',
    isDefault: true,
    modelId: 'gpt-4o-mini',
    modelDescriptor: {
      protocol: 'responses',
      supportedParameters: [],
      conversationCapability: {
        ...textCapability,
        inputModalities: ['text', 'image'],
        maxOutputTokens: 16384,
        toolCalling: true,
        structuredOutput: true,
      },
      contextWindow: 128000,
      displayName: 'GPT-4o mini',
      lifecycle: { kind: 'stable' },
      maxOutputTokens: 16384,
      modelId: 'gpt-4o-mini',
      runtimeStatus: { kind: 'runnable' },
    },
    providerId: 'openai',
  },
  status: 'saved',
}

export const fixtureListProviderProbeSnapshots: ListProviderProbeSnapshotsResponse = {
  snapshots: [],
}

const emptyUsageWindow = {
  period: 'today' as const,
  total: {
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    costMicros: 0,
    inputTokens: 0,
    outputTokens: 0,
    toolCalls: 0,
  },
  byModel: [],
}

export const fixtureGetModelUsageSummary: GetModelUsageSummaryResponse = {
  timezoneId: 'UTC',
  timezoneOffsetMinutes: 0,
  today: emptyUsageWindow,
  monthToDate: { ...emptyUsageWindow, period: 'month_to_date' },
  allTime: { ...emptyUsageWindow, period: 'all_time' },
  generatedAt: '2026-06-30T12:00:00+00:00',
}

export const fixtureListOfficialQuotaSnapshots: ListOfficialQuotaSnapshotsResponse = {
  snapshots: [],
}

export const fixtureRefreshOfficialQuota: RefreshOfficialQuotaResponse = {
  snapshot: {
    configId: 'openrouter-work',
    expiresAt: '2026-06-30T12:15:00+00:00',
    fetchedAt: '2026-06-30T12:00:00+00:00',
    isStale: false,
    providerId: 'openrouter',
    scope: 'account',
    sourceUrl: 'https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key',
    status: 'supported',
  },
}

export const fixtureProbeProviderConfig: ProbeProviderConfigResponse = {
  snapshot: {
    checkedAt: '2026-06-30T12:00:00+00:00',
    configId: 'openai-work',
    latencyMs: 120,
    modelId: 'gpt-4o-mini',
    providerId: 'openai',
    status: 'online',
    timeoutMs: 10_000,
  },
}

export const testJyowoProject: ListProjectsResponse = {
  activePath: '/Users/goya/Repo/Git/Jyowo',
  projects: [
    {
      lastOpenedAt: timestamp,
      name: 'Jyowo',
      path: '/Users/goya/Repo/Git/Jyowo',
    },
  ],
}

export const fixtureListEvalCases: ListEvalCasesResponse = {
  cases: [
    {
      id: 'regression-smoke',
      lastRun: {
        completedAt: timestamp,
        failed: 0,
        passed: 3,
        status: 'passed',
      },
      title: 'Regression smoke',
    },
  ],
}

export function fixtureProviderApiKeyForConfig(configId: string) {
  return ['fixture', 'provider', 'revealed', configId].join(':')
}

export function normalizeAutomationSpec(
  automation: SaveAutomationRequest['automation'],
): AutomationSpec {
  return {
    ...automation,
    enabled: automation.enabled ?? false,
    missedRunPolicy: automation.missedRunPolicy ?? 'skip',
    workspaceAccess: normalizeAutomationWorkspaceAccess(automation.workspaceAccess),
  }
}

function normalizeAutomationWorkspaceAccess(
  workspaceAccess: SaveAutomationRequest['automation']['workspaceAccess'],
): AutomationSpec['workspaceAccess'] {
  if (typeof workspaceAccess === 'object' && 'read_write' in workspaceAccess) {
    return {
      read_write: {
        allowed_writable_subpaths: workspaceAccess.read_write.allowed_writable_subpaths ?? [],
      },
    }
  }

  return workspaceAccess
}
