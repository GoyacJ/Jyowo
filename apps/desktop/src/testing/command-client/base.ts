import type {
  AppInfo,
  CommandClient,
  CreateAttachmentFromPathResponse,
  DeleteAutomationResponse,
  ExportMemoryItemsResponse,
  ExportSupportBundleResponse,
  GetArtifactMediaPreviewResponse,
  GetAttachmentMediaPreviewResponse,
  GetContextSnapshotResponse,
  GetConversationResponse,
  GetExecutionSettingsResponse,
  GetMcpServerConfigResponse,
  GetMemoryItemResponse,
  GetModelUsageSummaryResponse,
  GetPluginDetailResponse,
  GetProviderConfigApiKeyResponse,
  GetSkillCatalogEntryResponse,
  GetSkillCatalogFileResponse,
  GetSkillDetailResponse,
  GetSkillFileResponse,
  HarnessHealthcheck,
  InstallSkillFromCatalogResponse,
  ListActivityResponse,
  ListArtifactsResponse,
  ListAutomationRunsResponse,
  ListAutomationsResponse,
  ListBackgroundAgentsResponse,
  ListBrowserMcpPresetsResponse,
  ListConversationsResponse,
  ListEvalCasesResponse,
  ListMcpDiagnosticsResponse,
  ListMcpServersResponse,
  ListMemoryItemsResponse,
  ListOfficialQuotaSnapshotsResponse,
  ListPluginsResponse,
  ListProjectConversationGroupsResponse,
  ListProjectsResponse,
  ListProviderCapabilityRouteOptionsResponse,
  ListProviderCapabilityRoutesResponse,
  ListProviderProbeSnapshotsResponse,
  ListProviderSettingsResponse,
  ListReferenceCandidatesResponse,
  ListRuntimeToolsResponse,
  ListSkillCatalogEntriesResponse,
  ListSkillCatalogInstallTasksResponse,
  ListSkillCatalogSourcesResponse,
  ListSkillsResponse,
  ModelProviderCatalogResponse,
  PageConversationTimelineResponse,
  PageConversationWorktreeResponse,
  PluginInstallReport,
  PluginOperationResult,
  ProbeProviderConfigResponse,
  RefreshOfficialQuotaResponse,
  ReplayTimelineResponse,
  RequestProviderConfigApiKeyRevealResponse,
  RunAutomationNowResponse,
  RuntimeExecutionStatus,
  SaveAutomationResponse,
  SaveBrowserMcpPresetResponse,
  SaveMcpServerResponse,
  SaveProviderSettingsResponse,
  SetAutomationEnabledResponse,
  SetExecutionSettingsResponse,
  SetProjectPluginsEnabledResponse,
  SubscribeConversationEventsResponse,
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

export const fixtureHarnessHealthcheck: HarnessHealthcheck = {
  status: 'available',
  sdkCrate: 'jyowo_harness_sdk',
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

type TestCommandResponseOverride<TMethod> = TMethod extends (
  ...args: infer Args
) => Promise<infer Response>
  ? Response | ((...args: Args) => Promise<Response> | Response)
  : never

function isResponseOverrideHandler<Args extends unknown[], Response>(
  override: Response | ((...args: Args) => Promise<Response> | Response) | undefined,
): override is (...args: Args) => Promise<Response> | Response {
  return typeof override === 'function'
}

export async function resolveResponseOverride<Args extends unknown[], Response>(
  override: Response | ((...args: Args) => Promise<Response> | Response) | undefined,
  fallback: Response,
  ...args: Args
): Promise<Response> {
  const response = isResponseOverrideHandler(override)
    ? await override(...args)
    : (override ?? fallback)
  return cloneResponse(response)
}

export interface TestCommandClientOptions {
  appInfo?: AppInfo
  attachmentFromPath?: CreateAttachmentFromPathResponse
  contextSnapshot?: GetContextSnapshotResponse
  conversation?: GetConversationResponse
  conversationCommandOutput?: TestCommandResponseOverride<
    CommandClient['getConversationCommandOutput']
  >
  conversationDiffPatch?: TestCommandResponseOverride<CommandClient['getConversationDiffPatch']>
  conversationEvidenceExport?: TestCommandResponseOverride<
    CommandClient['exportConversationEvidence']
  >
  conversations?: ListConversationsResponse
  projectConversationGroups?: ListProjectConversationGroupsResponse
  executionSettings?: GetExecutionSettingsResponse
  healthcheck?: HarnessHealthcheck
  artifacts?: ListArtifactsResponse
  artifactRevisionContent?: TestCommandResponseOverride<CommandClient['getArtifactRevisionContent']>
  automations?: ListAutomationsResponse
  automationRuns?: ListAutomationRunsResponse
  automationRunNow?: RunAutomationNowResponse
  automationSave?: SaveAutomationResponse
  automationSetEnabled?: SetAutomationEnabledResponse
  automationDelete?: DeleteAutomationResponse
  backgroundAgents?: ListBackgroundAgentsResponse
  artifactMediaPreview?: GetArtifactMediaPreviewResponse
  attachmentMediaPreview?: GetAttachmentMediaPreviewResponse
  listActivity?: ListActivityResponse
  memoryExport?: ExportMemoryItemsResponse
  evalCases?: ListEvalCasesResponse
  browserMcpPresets?: ListBrowserMcpPresetsResponse
  browserMcpPreset?: SaveBrowserMcpPresetResponse
  memoryItem?: GetMemoryItemResponse
  memoryItems?: ListMemoryItemsResponse
  mcpDiagnostics?: ListMcpDiagnosticsResponse
  mcpServerConfig?: GetMcpServerConfigResponse
  mcpServer?: SaveMcpServerResponse
  mcpServers?: ListMcpServersResponse
  modelProviderCatalog?: ModelProviderCatalogResponse
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
  referenceCandidates?: ListReferenceCandidatesResponse
  replayTimeline?: ReplayTimelineResponse
  runtimeExecutionStatus?: RuntimeExecutionStatus
  runtimeTools?: ListRuntimeToolsResponse
  conversationTimelinePage?: PageConversationTimelineResponse
  conversationInspectorItem?: TestCommandResponseOverride<
    CommandClient['getConversationInspectorItem']
  >
  conversationWorktreePage?: PageConversationWorktreeResponse
  subscribeConversationEvents?: SubscribeConversationEventsResponse
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
  supportBundleExport?: ExportSupportBundleResponse
  delayMs?: number
}
