import type {
  AppInfo,
  GetExecutionSettingsResponse,
  GetMcpServerConfigResponse,
  GetModelUsageSummaryResponse,
  GetPluginDetailResponse,
  GetProviderConfigApiKeyResponse,
  GetSkillCatalogEntryResponse,
  GetSkillCatalogFileResponse,
  GetSkillDetailResponse,
  GetSkillFileResponse,
  InstallSkillFromCatalogResponse,
  ListBrowserMcpPresetsResponse,
  ListMcpDiagnosticsResponse,
  ListMcpServersResponse,
  ListOfficialQuotaSnapshotsResponse,
  ListPluginsResponse,
  ListProjectsResponse,
  ListProviderCapabilityRouteOptionsResponse,
  ListProviderCapabilityRoutesResponse,
  ListProviderProbeSnapshotsResponse,
  ListProviderSettingsResponse,
  ListRuntimeToolsResponse,
  ListSkillCatalogEntriesResponse,
  ListSkillCatalogInstallTasksResponse,
  ListSkillCatalogSourcesResponse,
  ListSkillsResponse,
  ModelProviderCatalogResponse,
  ModelSettingsPageResponse,
  PluginInstallReport,
  PluginOperationResult,
  ProbeProviderConfigResponse,
  RefreshModelProviderCatalogResponse,
  RefreshOfficialQuotaResponse,
  RequestProviderConfigApiKeyRevealResponse,
  RuntimeExecutionStatus,
  SaveBrowserMcpPresetResponse,
  SaveMcpServerResponse,
  SaveProviderSettingsResponse,
  SetExecutionSettingsResponse,
  SetProjectPluginsEnabledResponse,
  SubscribeMcpDiagnosticsResponse,
  ValidateProviderSettingsResponse,
} from '@/shared/tauri/commands'

export const timestamp = '2026-06-17T02:22:00.000Z'

export const fixtureAppInfo: AppInfo = {
  name: 'Jyowo',
  version: '0.1.0',
  shell: 'tauri2-react',
  harness: {
    sdkCrate: 'jyowo_harness_sdk',
    mode: 'in-process',
  },
}

export const fixtureRuntimeExecutionStatus: RuntimeExecutionStatus = {
  processSandbox: {
    backendId: 'routing',
    candidateIds: ['local-process'],
    availableNetworkPolicies: ['none'],
    availableWorkspacePolicies: ['read_only'],
    unavailableReasons: [],
  },
  httpBroker: {
    available: false,
    deniedReasons: ['network broker is not registered in the capability registry'],
  },
  tools: [
    {
      toolName: 'Bash',
      available: true,
      unavailableReason: null,
    },
    {
      toolName: 'WebFetch',
      available: false,
      unavailableReason: 'HTTP broker is not registered',
    },
  ],
}

export const fixtureRuntimeTools: ListRuntimeToolsResponse = {
  generation: 1,
  tools: [
    {
      name: 'FileRead',
      displayName: 'File read',
      description: 'Read a UTF-8 workspace file.',
      category: 'builtin',
      group: 'fileSystem',
      groupLabel: 'File system',
      originKind: 'builtin',
      originId: null,
      access: 'readOnly',
      executionChannel: 'directAuthorizedRust',
      requiredCapabilities: [],
      deferPolicy: 'alwaysLoad',
      longRunning: false,
      serviceBinding: null,
    },
    {
      name: 'Bash',
      displayName: 'Bash',
      description: 'Execute a shell command through the configured sandbox.',
      category: 'builtin',
      group: 'shell',
      groupLabel: 'Shell',
      originKind: 'builtin',
      originId: null,
      access: 'destructive',
      executionChannel: 'processSandbox',
      requiredCapabilities: [],
      deferPolicy: 'alwaysLoad',
      longRunning: true,
      serviceBinding: null,
    },
  ],
}

export function cloneResponse<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

export function wait(delayMs: number | undefined) {
  if (!delayMs) {
    return Promise.resolve()
  }

  return new Promise<void>((resolve) => {
    window.setTimeout(resolve, delayMs)
  })
}

export interface TestCommandClientOptions {
  appInfo?: AppInfo
  executionSettings?: GetExecutionSettingsResponse
  browserMcpPresets?: ListBrowserMcpPresetsResponse
  browserMcpPreset?: SaveBrowserMcpPresetResponse
  mcpDiagnostics?: ListMcpDiagnosticsResponse
  mcpServerConfig?: GetMcpServerConfigResponse
  mcpServer?: SaveMcpServerResponse
  mcpServers?: ListMcpServersResponse
  modelProviderCatalog?: ModelProviderCatalogResponse
  modelSettingsPage?: ModelSettingsPageResponse
  modelProviderCatalogRefresh?: RefreshModelProviderCatalogResponse
  pluginDetail?: GetPluginDetailResponse
  pluginInstallReport?: PluginInstallReport
  pluginOperation?: PluginOperationResult
  plugins?: ListPluginsResponse
  providerConfigApiKey?: GetProviderConfigApiKeyResponse
  providerConfigApiKeyReveal?: RequestProviderConfigApiKeyRevealResponse
  providerSettingsList?: ListProviderSettingsResponse
  providerCapabilityRoutes?: ListProviderCapabilityRoutesResponse
  providerCapabilityRouteOptions?: ListProviderCapabilityRouteOptionsResponse
  providerProbeSnapshots?: ListProviderProbeSnapshotsResponse
  modelUsageSummary?: GetModelUsageSummaryResponse
  officialQuotaSnapshots?: ListOfficialQuotaSnapshotsResponse
  officialQuotaRefresh?: RefreshOfficialQuotaResponse
  providerProbe?: ProbeProviderConfigResponse
  projects?: ListProjectsResponse
  providerSettings?: SaveProviderSettingsResponse
  providerValidation?: ValidateProviderSettingsResponse
  setExecutionSettings?: SetExecutionSettingsResponse
  setProjectPluginsEnabled?: SetProjectPluginsEnabledResponse
  runtimeExecutionStatus?: RuntimeExecutionStatus
  runtimeTools?: ListRuntimeToolsResponse
  subscribeMcpDiagnostics?: SubscribeMcpDiagnosticsResponse
  skillDetail?: GetSkillDetailResponse
  skillFile?: GetSkillFileResponse
  skillCatalogEntry?: GetSkillCatalogEntryResponse
  skillCatalogFile?: GetSkillCatalogFileResponse
  skillCatalogEntries?: ListSkillCatalogEntriesResponse
  skillCatalogInstallTasks?: ListSkillCatalogInstallTasksResponse
  skillCatalogSources?: ListSkillCatalogSourcesResponse
  skillCatalogInstall?: InstallSkillFromCatalogResponse
  skills?: ListSkillsResponse
  delayMs?: number
}
