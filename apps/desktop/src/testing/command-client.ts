import type { RunEvent } from '@/shared/events/run-event-schema'
import type {
  AppInfo,
  AutomationSpec,
  CancelRunResponse,
  ClearMcpDiagnosticsResponse,
  CommandClient,
  ConversationEventBatchPayload,
  ConversationModelCapability,
  CreateAttachmentFromPathResponse,
  CreateConversationResponse,
  DeleteAutomationResponse,
  DeleteConversationResponse,
  DeleteProjectResponse,
  DeleteProviderCapabilityRouteResponse,
  ExportMemoryItemsResponse,
  ExportSupportBundleResponse,
  GetArtifactMediaPreviewResponse,
  GetAttachmentMediaPreviewResponse,
  GetContextSnapshotResponse,
  GetConversationResponse,
  GetExecutionSettingsResponse,
  GetMcpServerConfigResponse,
  GetMemoryItemResponse,
  GetPluginDetailResponse,
  GetProviderConfigApiKeyResponse,
  GetSkillCatalogEntryResponse,
  GetSkillCatalogFileResponse,
  GetSkillDetailResponse,
  GetSkillFileResponse,
  HarnessHealthcheck,
  InstallSkillFromCatalogRequest,
  InstallSkillFromCatalogResponse,
  ListActivityResponse,
  ListArtifactsResponse,
  ListAutomationRunsResponse,
  ListAutomationsResponse,
  ListBrowserMcpPresetsResponse,
  ListConversationsResponse,
  ListEvalCasesResponse,
  ListMcpDiagnosticsResponse,
  ListMcpServersResponse,
  ListMemoryItemsResponse,
  ListPluginsResponse,
  ListProjectsResponse,
  ListProviderCapabilityRouteOptionsResponse,
  ListProviderCapabilityRoutesResponse,
  ListProviderSettingsResponse,
  ListReferenceCandidatesResponse,
  ListSkillCatalogEntriesResponse,
  ListSkillCatalogInstallTasksResponse,
  ListSkillCatalogSourcesResponse,
  ListSkillsResponse,
  ModelProviderCatalogResponse,
  PageConversationTimelineResponse,
  PageConversationWorktreeResponse,
  PluginInstallReport,
  PluginOperationResult,
  ReplayTimelineResponse,
  RequestProviderConfigApiKeyRevealResponse,
  ResolvePermissionResponse,
  RunAutomationNowResponse,
  RunEvalCaseResponse,
  SaveAutomationRequest,
  SaveAutomationResponse,
  SaveBrowserMcpPresetResponse,
  SaveMcpServerResponse,
  SaveProviderCapabilityRouteResponse,
  SaveProviderSettingsResponse,
  SetAutomationEnabledResponse,
  SetConversationModelConfigResponse,
  SetExecutionSettingsResponse,
  SetMcpServerEnabledResponse,
  SetProjectPluginsEnabledResponse,
  SkillCatalogInstallProgressPayload,
  SkillSummary,
  StartRunResponse,
  SubscribeConversationEventsResponse,
  SubscribeMcpDiagnosticsResponse,
  SwitchProjectResponse,
  UnsubscribeConversationEventsResponse,
  UnsubscribeMcpDiagnosticsResponse,
  UpdateMemoryItemResponse,
  ValidateProviderSettingsResponse,
} from '@/shared/tauri/commands'

const fixtureAppInfo: AppInfo = {
  name: 'Jyowo',
  version: '0.1.0',
  shell: 'tauri2-react',
  harness: {
    sdkCrate: 'jyowo_harness_sdk',
    mode: 'in-process',
  },
}

const fixtureHarnessHealthcheck: HarnessHealthcheck = {
  status: 'available',
  sdkCrate: 'jyowo_harness_sdk',
}

const timestamp = '2026-06-17T02:22:00.000Z'

const fixtureListConversations: ListConversationsResponse = {
  conversations: [
    {
      id: 'conversation-001',
      isEmpty: false,
      lastMessagePreview: 'Restore the product shell',
      title: 'Build the desktop foundation',
      updatedAt: timestamp,
    },
  ],
}

const fixtureAutomation = {
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

const fixtureListAutomations: ListAutomationsResponse = {
  automations: [fixtureAutomation],
}

const fixtureAutomationRun = {
  automationId: 'checks',
  completedAt: '2026-06-30T01:01:00Z',
  id: 'automation-run-001',
  message: 'Starting automation runs requires the runtime conversation facade.',
  startedAt: '2026-06-30T01:00:00Z',
  status: 'rejected',
} satisfies ListAutomationRunsResponse['runs'][number]

const fixtureAutomationRuns: ListAutomationRunsResponse = {
  runs: [fixtureAutomationRun],
}

const fixtureConversation: GetConversationResponse = {
  conversation: {
    id: 'conversation-001',
    messages: [
      {
        author: 'user',
        body: [
          "Let's scaffold the desktop app with Electron + React + TypeScript.",
          'Use Vite for the renderer. Keep it minimal and ready for local AI features.',
        ].join('\n'),
        id: 'message-001',
        timestamp: '2026-06-17T02:21:00.000Z',
      },
      {
        author: 'assistant',
        body: [
          'The runtime conversation is connected to the local workspace.',
          'Activity, artifacts, and context now come from command responses instead of embedded UI data.',
          'Continue from the composer to start another runtime-backed turn.',
        ].join('\n'),
        id: 'message-002',
        timestamp,
      },
    ],
    modelConfigId: null,
    title: 'Build the desktop foundation',
    updatedAt: timestamp,
  },
}

const fixtureListActivity: ListActivityResponse = {
  events: [
    {
      id: 'evt-001',
      conversationSequence: 1,
      payload: { permissionMode: 'default', sessionId: 'conversation-001' },
      runId: 'run-001',
      sequence: 1,
      source: 'engine',
      timestamp,
      type: 'run.started',
      visibility: 'public',
    },
    {
      id: 'evt-002',
      conversationSequence: 2,
      payload: { toolUseId: 'start_run' },
      runId: 'run-001',
      sequence: 2,
      source: 'tool',
      timestamp,
      type: 'tool.approved',
      visibility: 'public',
    },
  ],
}

const fixtureListArtifacts: ListArtifactsResponse = {
  artifacts: [
    {
      actionLabel: 'Run app',
      description: 'Tauri + React + TypeScript with Vite',
      id: 'artifact-desktop-foundation',
      kind: 'app',
      preview: 'Tauri command boundary, React renderer shell, and Vite development scripts.',
      status: 'ready',
      title: 'Desktop foundation created',
    },
    {
      actionLabel: 'Inspect',
      description: 'Follow-up verification checklist',
      id: 'artifact-verification-notes',
      kind: 'markdown',
      status: 'pending',
      title: 'Verification notes',
    },
    {
      actionLabel: 'Inspect diff',
      description: 'Runtime shell entrypoint changes',
      id: 'artifact-shell-diff',
      kind: 'diff',
      preview: [
        '--- src/main/main.ts',
        '+++ src/main/main.ts',
        "+ import { app, BrowserWindow, ipcMain } from 'electron'",
        "+ import path from 'path'",
        '+',
        '+ function createWindow () {',
        '+   const win = new BrowserWindow({',
        '+     width: 1200,',
        '+     height: 800,',
        '+     webPreferences: {',
        "+       preload: path.join(__dirname, 'preload.js'),",
        '+       contextIsolation: true,',
        '+     }',
        '+   })',
        "+   win.loadURL(process.env.VITE_DEV_SERVER_URL || 'index.html')",
        '+ }',
      ].join('\n'),
      status: 'ready',
      title: 'src/main/main.ts',
    },
  ],
}

const fixtureAttachment: CreateAttachmentFromPathResponse = {
  attachment: {
    blobRef: {
      contentHash: Array.from({ length: 32 }, () => 1),
      contentType: 'text/plain',
      id: '01J00000000000000000000000',
      size: 128,
    },
    id: 'attachment-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
    mimeType: 'text/plain',
    name: 'notes.txt',
    sizeBytes: 128,
  },
}

const fixtureArtifactMediaPreview: GetArtifactMediaPreviewResponse = {
  dataUrl:
    'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=',
  mimeType: 'image/png',
  sizeBytes: 68,
}

const fixtureReferenceCandidates: ListReferenceCandidatesResponse = {
  artifacts: [{ id: 'artifact-desktop-foundation', label: 'Desktop foundation created' }],
  conversations: [{ id: 'conversation-001', label: 'Build the desktop foundation' }],
  files: [
    {
      label: 'apps/desktop/src/shared/tauri/commands.ts',
      path: 'apps/desktop/src/shared/tauri/commands.ts',
    },
  ],
  memories: [{ id: '01HZ0000000000000000000001', label: 'Prefers concise Chinese responses' }],
  mcpServers: [{ id: 'stdio', label: 'stdio' }],
  skills: [{ id: 'release-notes', label: 'release-notes' }],
  tools: [{ id: 'list_dir', label: 'List directory' }],
}

const fixtureContextSnapshot: GetContextSnapshotResponse = {
  activeArtifact: 'App shell (WIP)',
  decisions: [{ detail: 'When: Before adding AI features', title: 'Choose IPC pattern' }],
  files: [
    { label: 'src/' },
    { label: 'public/' },
    { label: 'package.json' },
    { label: 'main.ts' },
    { label: 'preload.ts' },
    { label: 'vite.config.ts' },
  ],
  nextActions: ['Review changes', 'Continue', 'Open artifact'],
  path: '~/projects/desktop-app',
  project: 'Desktop App',
}

const fixtureValidateProviderSettings: ValidateProviderSettingsResponse = {
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

const fixtureModelProviderCatalog: ModelProviderCatalogResponse = {
  providers: [
    {
      defaultBaseUrl: 'https://api.openai.com',
      displayName: 'OpenAI',
      models: [
        {
          protocol: 'responses',
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

const fixtureProviderSettingsList: ListProviderSettingsResponse = {
  defaultConfigId: null,
  configs: [],
}

const fixtureAgentCapabilities = {
  agentTeamsAvailable: false,
  agentTeamsEnabled: false,
  backgroundAgentsAvailable: false,
  backgroundAgentsEnabled: false,
  subagentsAvailable: false,
  subagentsEnabled: false,
  unavailableReasons: [],
}

const fixtureExecutionSettings: GetExecutionSettingsResponse = {
  agentCapabilities: fixtureAgentCapabilities,
  autoModeAvailable: false,
  contextCompressionTriggerRatio: 0.8,
  permissionMode: 'default',
  toolProfile: 'full',
}

const fixtureSetExecutionSettings: SetExecutionSettingsResponse = {
  agentCapabilities: fixtureAgentCapabilities,
  autoModeAvailable: false,
  contextCompressionTriggerRatio: 0.8,
  permissionMode: 'default',
  toolProfile: 'full',
}

const fixtureSaveProviderSettings: SaveProviderSettingsResponse = {
  config: {
    protocol: 'responses',
    baseUrl: 'https://api.openai.com',
    displayName: 'OpenAI',
    hasApiKey: true,
    id: 'openai',
    isDefault: true,
    modelId: 'gpt-4o-mini',
    modelDescriptor: {
      protocol: 'responses',
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

const fixtureListMcpServers: ListMcpServersResponse = {
  servers: [
    {
      displayName: 'Workspace GitHub',
      enabled: true,
      exposedToolCount: 2,
      id: 'github',
      manageable: true,
      origin: 'workspace',
      scope: 'global',
      status: 'ready',
      transport: 'stdio',
    },
  ],
}

const fixtureListBrowserMcpPresets: ListBrowserMcpPresetsResponse = {
  presets: [
    {
      description: 'Browser automation through Playwright MCP.',
      displayName: 'Playwright Browser',
      enabled: false,
      id: 'playwright',
      serverId: 'browser-playwright',
    },
    {
      description: 'Browser inspection through Chrome DevTools MCP.',
      displayName: 'Chrome DevTools Browser',
      enabled: false,
      id: 'chrome-devtools',
      serverId: 'browser-chrome-devtools',
    },
  ],
}

const fixtureMcpServerConfig: GetMcpServerConfigResponse = {
  server: {
    displayName: 'Workspace GitHub',
    enabled: true,
    id: 'github',
    scope: 'global',
    transport: {
      args: ['mcp-server'],
      command: 'node',
      env: [{ key: 'LOG_LEVEL', value: 'info' }],
      inheritEnv: ['GITHUB_TOKEN'],
      kind: 'stdio',
    },
  },
}

const fixtureWorkspaceSkill: SkillSummary = {
  description: 'Creates release notes from recent changes.',
  enabled: true,
  id: 'skill-001',
  importedAt: '2026-06-21T00:00:00.000Z',
  manageable: true,
  name: 'release-notes',
  sourceKind: 'workspace',
  status: 'ready',
  tags: ['writing'],
  updatedAt: '2026-06-21T00:00:00.000Z',
}

const fixtureBundledSkill: SkillSummary = {
  description: 'Inspects source changes and returns risks.',
  enabled: true,
  id: 'code-review',
  manageable: false,
  name: 'code-review',
  sourceKind: 'bundled',
  status: 'ready',
  tags: ['review'],
}

const fixtureListSkills: ListSkillsResponse = {
  skills: [fixtureWorkspaceSkill, fixtureBundledSkill],
}

const fixtureListPlugins: ListPluginsResponse = {
  allowProjectPlugins: false,
  plugins: [
    {
      id: 'formatter@1.0.0',
      name: 'formatter',
      version: '1.0.0',
      description: 'Formats workspace files.',
      source: 'user',
      trustLevel: 'user_controlled',
      enabled: true,
      state: 'activated',
      capabilities: [
        {
          kind: 'tool',
          name: 'format_file',
          destructive: false,
          registered: true,
        },
      ],
      warnings: [],
    },
  ],
}

const fixturePluginInstallReport: PluginInstallReport = {
  sourcePath: '/tmp/formatter-plugin',
  valid: true,
  summary: fixtureListPlugins.plugins[0],
  warnings: [],
}

const fixturePluginDetail: GetPluginDetailResponse = {
  plugin: {
    summary: fixtureListPlugins.plugins[0],
    manifestOrigin: {
      file: {
        path: '/tmp/formatter-plugin/plugin.json',
      },
    },
    manifestHash: Array.from({ length: 32 }, () => 7),
    manifest: {
      name: 'formatter',
      version: '1.0.0',
    },
    configurationSchema: {
      type: 'object',
      properties: {
        lineWidth: {
          type: 'number',
        },
        formatOnSave: {
          type: 'boolean',
        },
        apiToken: {
          type: 'string',
          secret: true,
        },
      },
    },
    config: {
      lineWidth: 100,
      formatOnSave: true,
    },
    registeredCapabilities: fixtureListPlugins.plugins[0].capabilities,
    recentEvents: ['loaded'],
  },
}

const fixturePluginOperation: PluginOperationResult = {
  pluginId: fixtureListPlugins.plugins[0].id,
  status: 'installed',
  summary: fixtureListPlugins.plugins[0],
  report: fixturePluginInstallReport,
}

const fixtureSkillCatalogSources: ListSkillCatalogSourcesResponse = {
  sources: [
    {
      description: 'Official Anthropic skills repository.',
      id: 'anthropic',
      installable: true,
      label: 'Anthropic Skills',
      trustLevel: 'official',
    },
    {
      description: 'Validation standard for portable agent skills.',
      id: 'agent-skills-spec',
      installable: false,
      label: 'Agent Skills spec',
      trustLevel: 'standard',
    },
    {
      description: 'Curated community index of agent skill repositories.',
      id: 'awesome-agent-skills',
      installable: true,
      label: 'Awesome Agent Skills',
      trustLevel: 'curated',
    },
    {
      description: 'Public ClawHub registry with security scan metadata.',
      id: 'clawhub',
      installable: true,
      label: 'ClawHub',
      trustLevel: 'community',
    },
  ],
}

const fixtureSkillCatalogEntries: ListSkillCatalogEntriesResponse = {
  entries: [
    {
      description: 'Create distinctive frontend interfaces.',
      entryId: 'anthropic:frontend-design',
      homepageUrl: 'https://github.com/anthropics/skills/tree/main/frontend-design',
      installable: true,
      installed: false,
      name: 'frontend-design',
      sourceId: 'anthropic',
      sourceLabel: 'Anthropic Skills',
      tags: ['frontend'],
      trustLevel: 'official',
      version: 'main',
    },
  ],
}

const fixtureSkillCatalogEntry: GetSkillCatalogEntryResponse = {
  entry: fixtureSkillCatalogEntries.entries[0],
  files: [{ kind: 'file', path: 'SKILL.md', sizeBytes: 512 }],
  readmePreview: 'Create distinctive frontend interfaces.',
  validation: {
    issues: [],
    status: 'ready',
  },
}

const fixtureSkillDetail: GetSkillDetailResponse = {
  skill: {
    bodyPreview: 'Write concise release notes from the current workspace diff.',
    configKeys: ['CHANGELOG_TOKEN'],
    files: [
      {
        depth: 0,
        kind: 'file',
        name: 'SKILL.md',
        path: 'SKILL.md',
        sizeBytes: 96,
      },
      {
        depth: 0,
        kind: 'directory',
        name: 'references',
        path: 'references',
      },
      {
        depth: 1,
        kind: 'file',
        name: 'style.md',
        path: 'references/style.md',
        sizeBytes: 42,
      },
    ],
    parameters: [
      {
        description: 'Target release version.',
        name: 'version',
        paramType: 'string',
        required: true,
      },
    ],
    summary: fixtureWorkspaceSkill,
  },
}

const fixtureSkillEntryFile: GetSkillFileResponse = {
  file: {
    content: 'Write concise release notes from the current workspace diff.',
    path: 'SKILL.md',
  },
}

const fixtureSkillCatalogFile: GetSkillCatalogFileResponse = {
  file: {
    content: 'Write concise release notes from the current workspace diff.',
    path: 'SKILL.md',
    truncated: false,
  },
}

const fixtureSkillCatalogInstallTasks: ListSkillCatalogInstallTasksResponse = {
  tasks: [],
}

const fixtureSaveMcpServer: SaveMcpServerResponse = {
  server: {
    displayName: 'Workspace GitHub',
    enabled: true,
    exposedToolCount: 0,
    id: 'github',
    manageable: true,
    origin: 'workspace',
    scope: 'global',
    status: 'configured',
    transport: 'stdio',
  },
}

const fixtureSaveBrowserMcpPreset: SaveBrowserMcpPresetResponse = {
  preset: fixtureListBrowserMcpPresets.presets[0],
  server: {
    displayName: 'Playwright Browser',
    enabled: false,
    exposedToolCount: 0,
    id: 'browser-playwright',
    manageable: true,
    origin: 'workspace',
    scope: 'global',
    status: 'disabled',
    transport: 'stdio',
  },
}

const fixtureListMcpDiagnostics: ListMcpDiagnosticsResponse = {
  events: [
    {
      eventType: 'connection_recovered',
      id: 'mcp-diagnostic-001',
      serverId: 'github',
      severity: 'info',
      summary: 'MCP server connection recovered.',
      timestamp,
    },
  ],
}

const fixtureMemoryItems: ListMemoryItemsResponse = {
  items: [
    {
      contentPreview: 'Prefers concise Chinese responses',
      id: '01HZ0000000000000000000001',
      kind: 'user_preference',
      source: 'user_input',
      tags: ['tone'],
      updatedAt: timestamp,
      visibility: 'tenant',
    },
  ],
}

const fixtureMemoryItem: GetMemoryItemResponse = {
  item: {
    accessCount: 0,
    confidence: 1,
    content: 'Prefers concise Chinese responses',
    createdAt: timestamp,
    id: '01HZ0000000000000000000001',
    kind: 'user_preference',
    source: 'user_input',
    tags: ['tone'],
    updatedAt: timestamp,
    visibility: 'tenant',
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

const fixtureMemoryExport: ExportMemoryItemsResponse = {
  exportedAt: timestamp,
  format: 'json',
  itemCount: 1,
  path: '.jyowo/runtime/exports/memory-20260617T000000.000Z.json',
}

const fixtureListEvalCases: ListEvalCasesResponse = {
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

const fixtureReplayTimeline: ReplayTimelineResponse = {
  events: fixtureListActivity.events,
  replayed: true,
}

const fixtureConversationWorktreePage: PageConversationWorktreeResponse = {
  turns: [
    {
      id: 'turn:message-001',
      conversationId: 'conversation-001',
      position: 0,
      user: {
        id: 'user:message-001',
        messageId: 'message-001',
        body: 'Restore the product shell',
        timestamp: '2026-06-17T02:21:00.000Z',
      },
      assistant: {
        id: 'assistant:run-001',
        runId: 'run-001',
        status: 'running',
        segments: [
          {
            kind: 'process',
            id: 'segment:process:run-001',
            order: 0,
            status: 'running',
            summary: '正在处理请求',
            steps: [
              {
                id: 'process-step:run-001:reasoning',
                order: 0,
                kind: 'reasoning',
                status: 'running',
                title: '分析工作区状态',
                body: '正在检查本地项目上下文。',
              },
              {
                id: 'process-step:run-001:file-read',
                order: 1,
                kind: 'fileRead',
                status: 'complete',
                title: '读取项目文件',
                detail: {
                  type: 'activity',
                  summary: '已读取 1 个文件',
                  itemCount: 1,
                },
              },
            ],
          },
          {
            kind: 'text',
            id: 'segment:text:message-002',
            order: 1,
            messageId: 'message-002',
            body: 'I am checking the workspace state.',
          },
          {
            kind: 'toolGroup',
            id: 'segment:tools:tool-fixture-read',
            order: 2,
            attempts: [
              {
                id: 'tool:tool-fixture-read',
                order: 0,
                toolUseId: 'tool-fixture-read',
                toolName: 'read_file',
                status: 'completed',
                permission: {
                  id: 'permission:01HZ0000000000000000000001',
                  requestId: '01HZ0000000000000000000001',
                  toolUseId: 'tool-fixture-read',
                  status: 'approved',
                  summary: 'Approved once',
                },
              },
              {
                id: 'tool:tool-fixture-verify',
                order: 1,
                toolUseId: 'tool-fixture-verify',
                toolName: 'local_verification',
                status: 'waitingPermission',
                permission: {
                  id: 'permission:01HZ0000000000000000000002',
                  requestId: '01HZ0000000000000000000002',
                  toolUseId: 'tool-fixture-verify',
                  status: 'pending',
                  summary: 'Awaiting approval',
                },
              },
            ],
          },
        ],
      },
    },
    {
      id: 'turn:message-003',
      conversationId: 'conversation-001',
      position: 1,
      user: {
        id: 'user:message-003',
        messageId: 'message-003',
        body: 'Run the checks',
        timestamp: '2026-06-17T02:22:00.000Z',
      },
      assistant: {
        id: 'assistant:run-002',
        runId: 'run-002',
        status: 'complete',
        segments: [
          {
            kind: 'toolGroup',
            id: 'segment:tools:tool-fixture-test',
            order: 0,
            attempts: [
              {
                id: 'tool:tool-fixture-test',
                order: 0,
                toolUseId: 'tool-fixture-test',
                toolName: 'pnpm test',
                status: 'failed',
                failureSummary: '工具执行失败。可在详情中查看。',
              },
            ],
          },
          {
            kind: 'text',
            id: 'segment:text:message-004',
            order: 1,
            messageId: 'message-004',
            body: 'The checks need follow-up.',
          },
        ],
      },
    },
  ],
  pageCursor: {
    turnId: 'turn:message-003',
    position: 1,
  },
  eventCursor: {
    eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
    conversationSequence: 9,
  },
  hasMoreBefore: false,
  hasMoreAfter: false,
  gap: false,
}

function cloneResponse<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

function emptyWorktreePage(): PageConversationWorktreeResponse {
  return {
    turns: [],
    pageCursor: undefined,
    eventCursor: undefined,
    hasMoreBefore: false,
    hasMoreAfter: false,
    gap: false,
  }
}

function worktreePageForFixtureRun(
  conversationId: string,
  prompt: string,
  clientMessageId: string | undefined,
  status: 'running' | 'complete',
): PageConversationWorktreeResponse {
  const turn: PageConversationWorktreeResponse['turns'][number] = {
    id: 'turn:message-fixture-user',
    conversationId,
    position: 0,
    user: {
      id: 'user:message-fixture-user',
      messageId: 'message-fixture-user',
      clientMessageId,
      body: prompt,
      timestamp,
    },
    assistant: {
      id: 'assistant:run-001',
      runId: 'run-001',
      status,
      segments: [
        {
          kind: 'process',
          id: 'segment:process:run-001',
          order: 0,
          status,
          summary: status === 'running' ? '正在处理请求' : '已完成工作过程',
          steps: [
            {
              id: 'process-step:fixture-reasoning',
              order: 0,
              kind: 'reasoning',
              status,
              title: '整理实施计划',
              body: 'Drafting the implementation plan.',
            },
            {
              id: 'process-step:fixture-read',
              order: 1,
              kind: 'fileRead',
              status: 'complete',
              title: 'Reading files',
              detail: {
                type: 'activity',
                summary: 'Read project files',
                itemCount: 1,
              },
            },
          ],
        },
        {
          kind: 'toolGroup',
          id: 'segment:tools:tool-fixture-read',
          order: 1,
          attempts: [
            {
              id: 'tool:tool-fixture-read',
              order: 0,
              toolUseId: 'tool-fixture-read',
              toolName: 'Reading files',
              status: 'completed',
            },
            {
              id: 'tool:tool-fixture-verify',
              order: 1,
              toolUseId: 'tool-fixture-verify',
              toolName: 'Run local verification',
              status: status === 'running' ? 'waitingPermission' : 'completed',
              permission: {
                id: 'permission:01HZ0000000000000000000001',
                requestId: '01HZ0000000000000000000001',
                toolUseId: 'tool-fixture-verify',
                status: status === 'running' ? 'pending' : 'approved',
                summary:
                  status === 'running' ? 'Awaiting approval' : 'Approved for this verification run',
              },
            },
          ],
        },
        {
          kind: 'artifact',
          id: 'segment:artifact:artifact-desktop-foundation',
          order: 2,
          artifactId: 'artifact-desktop-foundation',
          title: 'Desktop foundation created',
        },
        {
          kind: 'text',
          id: 'segment:text:message-fixture-assistant',
          order: 3,
          messageId: 'message-fixture-assistant',
          body: 'The setup is ready for review.',
        },
      ],
    },
  }

  return {
    turns: [turn],
    pageCursor: {
      turnId: turn.id,
      position: turn.position,
    },
    eventCursor: {
      eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
      conversationSequence: status === 'running' ? 7 : 9,
    },
    hasMoreBefore: false,
    hasMoreAfter: false,
    gap: false,
  }
}

const fixtureSupportBundleExport: ExportSupportBundleResponse = {
  bundlePath: '.jyowo/runtime/exports/support-bundle-20260617T000000.000Z.json',
  eventCount: 1,
  exportedAt: timestamp,
  jsonlPath: '.jyowo/runtime/exports/events-20260617T000000.000Z.jsonl',
  markdownPath: '.jyowo/runtime/exports/support-report-20260617T000000.000Z.md',
  redacted: true,
}

export interface TestCommandClientOptions {
  appInfo?: AppInfo
  attachmentFromPath?: CreateAttachmentFromPathResponse
  contextSnapshot?: GetContextSnapshotResponse
  conversation?: GetConversationResponse
  conversations?: ListConversationsResponse
  executionSettings?: GetExecutionSettingsResponse
  healthcheck?: HarnessHealthcheck
  artifacts?: ListArtifactsResponse
  automations?: ListAutomationsResponse
  automationRuns?: ListAutomationRunsResponse
  automationRunNow?: RunAutomationNowResponse
  automationSave?: SaveAutomationResponse
  automationSetEnabled?: SetAutomationEnabledResponse
  automationDelete?: DeleteAutomationResponse
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
  projects?: ListProjectsResponse
  providerSettings?: SaveProviderSettingsResponse
  providerValidation?: ValidateProviderSettingsResponse
  setExecutionSettings?: SetExecutionSettingsResponse
  setProjectPluginsEnabled?: SetProjectPluginsEnabledResponse
  referenceCandidates?: ListReferenceCandidatesResponse
  replayTimeline?: ReplayTimelineResponse
  conversationTimelinePage?: PageConversationTimelineResponse
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

function wait(delayMs: number | undefined) {
  if (!delayMs) {
    return Promise.resolve()
  }

  return new Promise<void>((resolve) => {
    window.setTimeout(resolve, delayMs)
  })
}

function fixtureProviderApiKeyForConfig(configId: string) {
  return ['fixture', 'provider', 'revealed', configId].join(':')
}

function normalizeAutomationSpec(automation: SaveAutomationRequest['automation']): AutomationSpec {
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

export function createTestCommandClient(options: TestCommandClientOptions = {}): CommandClient {
  let batchListener: ((batch: ConversationEventBatchPayload) => void) | null = null
  let activeSubscription: SubscribeConversationEventsResponse | null = null
  let subscriptionCounter = 0
  let providerRevealCounter = 0
  let completionBatchFlushed: Promise<void> = Promise.resolve()
  let projects = options.projects ?? testJyowoProject
  let providerSettings = cloneResponse(options.providerSettingsList ?? fixtureProviderSettingsList)
  let providerCapabilityRoutes = cloneResponse(
    options.providerCapabilityRoutes ?? {
      version: 1,
      routes: [],
    },
  )
  const providerCapabilityRouteOptions = cloneResponse(
    options.providerCapabilityRouteOptions ?? {
      options: [],
    },
  )
  let createdConversationCounter = 0
  let conversations = cloneResponse(options.conversations ?? fixtureListConversations)
  let automations = cloneResponse(options.automations ?? fixtureListAutomations)
  let automationRuns = cloneResponse(options.automationRuns ?? fixtureAutomationRuns)
  const providerRevealConfigIdsByToken = new Map<string, string>()
  const conversationDetailsById = new Map<string, GetConversationResponse>()
  conversationDetailsById.set(
    'conversation-001',
    cloneResponse(options.conversation ?? fixtureConversation),
  )
  const worktreePagesByConversation = new Map<string, PageConversationWorktreeResponse>()
  worktreePagesByConversation.set(
    'conversation-001',
    cloneResponse(options.conversationWorktreePage ?? fixtureConversationWorktreePage),
  )
  const pendingBatchTimeouts = new Map<number, () => void>()
  const catalogInstallProgressListeners = new Set<
    (progress: SkillCatalogInstallProgressPayload) => void
  >()
  const fixtureEventState: FixtureConversationEventState = {
    getListener: () => batchListener,
    getSubscription: () => activeSubscription,
    trackTimeout: (timeoutId, resolve) => {
      pendingBatchTimeouts.set(timeoutId, resolve)
    },
    untrackTimeout: (timeoutId) => {
      pendingBatchTimeouts.delete(timeoutId)
    },
  }
  const clearPendingBatches = () => {
    for (const [timeoutId, resolve] of pendingBatchTimeouts) {
      window.clearTimeout(timeoutId)
      resolve()
    }
    pendingBatchTimeouts.clear()
  }
  const emitCatalogInstallProgress = (
    request: InstallSkillFromCatalogRequest,
    stage: SkillCatalogInstallProgressPayload['stage'],
    percent: number,
  ) => {
    if (!request.operationId) {
      return
    }

    const payload = {
      entryId: request.entryId,
      operationId: request.operationId,
      percent,
      sourceId: request.sourceId,
      stage,
      version: request.version,
    } satisfies SkillCatalogInstallProgressPayload
    for (const listener of catalogInstallProgressListeners) {
      listener(payload)
    }
  }

  return {
    async cancelRun(runId) {
      await wait(options.delayMs)
      return { runId, status: 'cancelled' } satisfies CancelRunResponse
    },
    async createAttachmentFromPath() {
      await wait(options.delayMs)
      return options.attachmentFromPath ?? fixtureAttachment
    },
    async createConversation() {
      await wait(options.delayMs)
      createdConversationCounter += 1
      const conversationId = `conversation-created-${String(createdConversationCounter).padStart(3, '0')}`
      const conversation = {
        id: conversationId,
        isEmpty: true,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'New conversation',
        updatedAt: new Date().toISOString(),
      } satisfies CreateConversationResponse['conversation']
      conversations = {
        conversations: [
          conversation,
          ...conversations.conversations.filter((current) => current.id !== conversationId),
        ],
      }
      conversationDetailsById.set(conversationId, {
        conversation: {
          id: conversationId,
          messages: [],
          modelConfigId: null,
          title: conversation.title,
          updatedAt: conversation.updatedAt,
        },
      })
      worktreePagesByConversation.set(conversationId, emptyWorktreePage())

      return {
        conversation,
      } satisfies CreateConversationResponse
    },
    async deleteConversation(conversationId) {
      await wait(options.delayMs)
      return {
        conversationId,
        status: 'deleted',
      } satisfies DeleteConversationResponse
    },
    async deleteAutomation(id) {
      await wait(options.delayMs)
      automations = {
        automations: automations.automations.filter((automation) => automation.id !== id),
      }
      return (
        options.automationDelete ?? {
          id,
          status: 'deleted',
        }
      )
    },
    async deleteMcpServer(id) {
      await wait(options.delayMs)
      return { id, status: 'deleted' }
    },
    async deleteMemoryItem(id) {
      await wait(options.delayMs)
      return { id, status: 'deleted' }
    },
    async uninstallPlugin(pluginId) {
      await wait(options.delayMs)
      return {
        pluginId,
        status: 'uninstalled',
      } satisfies PluginOperationResult
    },
    async deleteSkill(id) {
      await wait(options.delayMs)
      return { id, status: 'deleted' }
    },
    async exportMemoryItems() {
      await wait(options.delayMs)
      return options.memoryExport ?? fixtureMemoryExport
    },
    async exportSupportBundle() {
      await wait(options.delayMs)
      return options.supportBundleExport ?? fixtureSupportBundleExport
    },
    async getContextSnapshot() {
      await wait(options.delayMs)
      return options.contextSnapshot ?? fixtureContextSnapshot
    },
    async getExecutionSettings(_request) {
      await wait(options.delayMs)
      return options.executionSettings ?? fixtureExecutionSettings
    },
    async getConversation(conversationId) {
      await wait(options.delayMs)
      return (
        options.conversation ?? conversationDetailsById.get(conversationId) ?? fixtureConversation
      )
    },
    async getArtifactMediaPreview() {
      await wait(options.delayMs)
      return options.artifactMediaPreview ?? fixtureArtifactMediaPreview
    },
    async getAttachmentMediaPreview() {
      await wait(options.delayMs)
      if (options.attachmentMediaPreview) {
        return options.attachmentMediaPreview
      }
      throw new Error('attachment media preview is unavailable')
    },
    async getAppInfo() {
      await wait(options.delayMs)
      return options.appInfo ?? fixtureAppInfo
    },
    async getHarnessHealthcheck() {
      await wait(options.delayMs)
      return options.healthcheck ?? fixtureHarnessHealthcheck
    },
    async getMemoryItem() {
      await wait(options.delayMs)
      return options.memoryItem ?? fixtureMemoryItem
    },
    async getPluginDetail(pluginId) {
      await wait(options.delayMs)
      if (options.pluginDetail?.plugin.summary.id === pluginId) {
        return options.pluginDetail
      }
      if (fixturePluginDetail.plugin.summary.id === pluginId) {
        return fixturePluginDetail
      }
      throw new Error(`Plugin not found: ${pluginId}`)
    },
    async getProviderConfigApiKey(configId, revealToken) {
      await wait(options.delayMs)
      const tokenConfigId = providerRevealConfigIdsByToken.get(revealToken)
      providerRevealConfigIdsByToken.delete(revealToken)
      if (tokenConfigId !== configId) {
        throw new Error('provider API key reveal token is invalid or expired')
      }
      if (options.providerConfigApiKey) {
        return {
          ...options.providerConfigApiKey,
          configId,
        }
      }
      return {
        apiKey: fixtureProviderApiKeyForConfig(configId),
        configId,
      }
    },
    async getReplayTimeline() {
      await wait(options.delayMs)
      return options.replayTimeline ?? fixtureReplayTimeline
    },
    async getSkillCatalogEntry() {
      await wait(options.delayMs)
      return options.skillCatalogEntry ?? fixtureSkillCatalogEntry
    },
    async getSkillCatalogFile() {
      await wait(options.delayMs)
      return options.skillCatalogFile ?? fixtureSkillCatalogFile
    },
    async pageConversationTimeline(request) {
      await wait(options.delayMs)
      const page = options.conversationTimelinePage ?? {
        events: [],
        cursor: undefined,
        gap: false,
      }
      if (!request.afterCursor) {
        return page
      }

      const afterSequence = request.afterCursor.conversationSequence
      return {
        ...page,
        events: page.events.filter((event) => event.conversationSequence > afterSequence),
      }
    },
    async pageConversationWorktree(request) {
      await wait(options.delayMs)
      const page =
        options.conversationWorktreePage ??
        worktreePagesByConversation.get(request.conversationId) ??
        emptyWorktreePage()
      if (!request.pageCursor) {
        return page
      }

      const pageCursor = request.pageCursor
      return {
        ...page,
        turns: page.turns.filter((turn) =>
          request.direction === 'before'
            ? turn.position < pageCursor.position
            : turn.position > pageCursor.position,
        ),
      }
    },
    async getSkillDetail(id) {
      await wait(options.delayMs)
      if (options.skillDetail) {
        return options.skillDetail
      }

      const summary =
        (options.skills ?? fixtureListSkills).skills.find((skill) => skill.id === id) ??
        fixtureWorkspaceSkill

      return {
        skill: {
          ...fixtureSkillDetail.skill,
          summary,
        },
      } satisfies GetSkillDetailResponse
    },
    async getSkillFile(_id, path) {
      await wait(options.delayMs)
      if (options.skillFile) {
        return options.skillFile
      }

      return path === fixtureSkillEntryFile.file.path
        ? fixtureSkillEntryFile
        : {
            file: {
              content: `Fixture content for ${path}`,
              path,
            },
          }
    },
    async importSkill() {
      await wait(options.delayMs)
      return { skill: fixtureWorkspaceSkill }
    },
    async installPluginFromPath() {
      await wait(options.delayMs)
      return options.pluginOperation ?? fixturePluginOperation
    },
    async installSkillFromCatalog(request) {
      emitCatalogInstallProgress(request, 'preparing', 5)
      await wait(options.delayMs)
      emitCatalogInstallProgress(request, 'completed', 100)
      return (
        options.skillCatalogInstall ?? {
          task: {
            entryId: request.entryId,
            operationId: request.operationId ?? 'catalog-install-fixture',
            percent: 5,
            sourceId: request.sourceId,
            stage: 'preparing',
            startedAt: '2026-06-28T00:00:00Z',
            status: 'running',
            updatedAt: '2026-06-28T00:00:00Z',
            version: request.version,
          },
        }
      )
    },
    async listSkillCatalogInstallTasks() {
      await wait(options.delayMs)
      return options.skillCatalogInstallTasks ?? fixtureSkillCatalogInstallTasks
    },
    async listenSkillCatalogInstallProgress(onProgress) {
      catalogInstallProgressListeners.add(onProgress)
      return () => {
        catalogInstallProgressListeners.delete(onProgress)
      }
    },
    async listActivity() {
      await wait(options.delayMs)
      return options.listActivity ?? fixtureListActivity
    },
    async listArtifacts(_request) {
      await wait(options.delayMs)
      return options.artifacts ?? fixtureListArtifacts
    },
    async listConversations() {
      await wait(options.delayMs)
      return conversations
    },
    async listAutomations() {
      await wait(options.delayMs)
      return cloneResponse(automations)
    },
    async listAutomationRuns(automationId) {
      await wait(options.delayMs)
      const runs =
        automationId === undefined
          ? automationRuns.runs
          : automationRuns.runs.filter((run) => run.automationId === automationId)
      return {
        runs: cloneResponse(runs),
      }
    },
    async listEvalCases() {
      await wait(options.delayMs)
      return options.evalCases ?? fixtureListEvalCases
    },
    async listBrowserMcpPresets() {
      await wait(options.delayMs)
      return options.browserMcpPresets ?? fixtureListBrowserMcpPresets
    },
    async listModelProviderCatalog() {
      await wait(options.delayMs)
      return options.modelProviderCatalog ?? fixtureModelProviderCatalog
    },
    async listMcpDiagnostics() {
      await wait(options.delayMs)
      return options.mcpDiagnostics ?? fixtureListMcpDiagnostics
    },
    async listMcpServers() {
      await wait(options.delayMs)
      return options.mcpServers ?? fixtureListMcpServers
    },
    async getMcpServerConfig(id) {
      await wait(options.delayMs)
      if (options.mcpServerConfig?.server.id === id) {
        return options.mcpServerConfig
      }
      if (fixtureMcpServerConfig.server.id === id) {
        return fixtureMcpServerConfig
      }
      throw new Error(`MCP server not found: ${id}`)
    },
    async listMemoryItems() {
      await wait(options.delayMs)
      return options.memoryItems ?? fixtureMemoryItems
    },
    async listPlugins() {
      await wait(options.delayMs)
      return options.plugins ?? fixtureListPlugins
    },
    async listProviderSettings() {
      await wait(options.delayMs)
      return cloneResponse(providerSettings)
    },
    async listProviderCapabilityRoutes() {
      await wait(options.delayMs)
      return cloneResponse(providerCapabilityRoutes)
    },
    async listProviderCapabilityRouteOptions() {
      await wait(options.delayMs)
      return cloneResponse(providerCapabilityRouteOptions)
    },
    async listProjects() {
      await wait(options.delayMs)
      return projects
    },
    async addProject(path) {
      await wait(options.delayMs)
      const name = path.split(/[\\/]/).filter(Boolean).at(-1) ?? 'Project'
      const project = {
        lastOpenedAt: new Date().toISOString(),
        name,
        path,
      } satisfies SwitchProjectResponse['project']
      projects = {
        activePath: path,
        projects: [project, ...projects.projects.filter((entry) => entry.path !== path)],
      }
      return { project }
    },
    async switchProject(path) {
      await wait(options.delayMs)
      const project = projects.projects.find((entry) => entry.path === path)
      if (!project) {
        throw new Error(`Project not found: ${path}`)
      }
      projects = {
        ...projects,
        activePath: path,
      }
      return { project }
    },
    async deleteProject(path) {
      await wait(options.delayMs)
      const removed = projects.projects.find((entry) => entry.path === path)
      if (!removed) {
        throw new Error(`Project not found: ${path}`)
      }
      const activePath = projects.activePath === path ? null : projects.activePath
      projects = {
        activePath,
        projects: projects.projects.filter((entry) => entry.path !== path),
      }
      return {
        activePath,
        path,
        status: 'deleted',
      } satisfies DeleteProjectResponse
    },
    async listReferenceCandidates(_request) {
      await wait(options.delayMs)
      return options.referenceCandidates ?? fixtureReferenceCandidates
    },
    async listSkillCatalogEntries() {
      await wait(options.delayMs)
      return options.skillCatalogEntries ?? fixtureSkillCatalogEntries
    },
    async listSkillCatalogSources() {
      await wait(options.delayMs)
      return options.skillCatalogSources ?? fixtureSkillCatalogSources
    },
    async listSkills() {
      await wait(options.delayMs)
      return options.skills ?? fixtureListSkills
    },
    async resolvePermission(request) {
      await wait(options.delayMs)
      await completionBatchFlushed
      emitFixtureConversationBatch(
        fixtureEventState,
        activeSubscription,
        [
          fixtureTimelineEvent(
            'permission.resolved',
            {
              decision: request.decision,
              requestId: request.requestId,
            },
            {
              conversationSequence: 10,
              id: 'evt-fixture-permission-resolved',
              sequence: 10,
              source: 'policy',
            },
          ),
        ],
        120,
      )
      return {
        ...request,
        status: 'resolved',
      } satisfies ResolvePermissionResponse
    },
    async requestProviderConfigApiKeyReveal(configId) {
      await wait(options.delayMs)
      const config = providerSettings.configs.find((currentConfig) => currentConfig.id === configId)
      if (!config?.hasApiKey) {
        throw new Error(`provider config API key is not configured: ${configId}`)
      }
      providerRevealCounter += 1
      const response: RequestProviderConfigApiKeyRevealResponse =
        options.providerConfigApiKeyReveal ?? {
          configId,
          expiresInSeconds: 60,
          revealToken: `fixture-reveal-token-${providerRevealCounter}`,
          status: 'ready',
        }
      providerRevealConfigIdsByToken.set(response.revealToken, configId)
      return {
        ...response,
        configId,
      }
    },
    async reloadPlugin(pluginId) {
      await wait(options.delayMs)
      const summary =
        (options.plugins ?? fixtureListPlugins).plugins.find((plugin) => plugin.id === pluginId) ??
        fixtureListPlugins.plugins[0]
      return {
        pluginId,
        status: 'reloaded',
        summary,
      } satisfies PluginOperationResult
    },
    async runEvalCase(caseId) {
      await wait(options.delayMs)
      const evalCase =
        (options.evalCases ?? fixtureListEvalCases).cases.find(
          (currentCase) => currentCase.id === caseId,
        ) ?? fixtureListEvalCases.cases[0]

      return {
        case: {
          ...evalCase,
          lastRun: {
            completedAt: timestamp,
            failed: 0,
            passed: (evalCase.lastRun?.passed ?? 0) + 1,
            status: 'passed',
          },
        },
        status: 'completed',
      } satisfies RunEvalCaseResponse
    },
    async runAutomationNow(id) {
      await wait(options.delayMs)
      const record = {
        ...fixtureAutomationRun,
        automationId: id,
      } satisfies ListAutomationRunsResponse['runs'][number]
      automationRuns = {
        runs: [record, ...automationRuns.runs.filter((run) => run.id !== record.id)],
      }
      return (
        options.automationRunNow ?? {
          record,
        }
      )
    },
    async saveAutomation(request) {
      await wait(options.delayMs)
      const automation = normalizeAutomationSpec(request.automation)
      automations = {
        automations: [
          automation,
          ...automations.automations.filter(
            (automation) => automation.id !== request.automation.id,
          ),
        ],
      }
      return (
        options.automationSave ?? {
          automation,
          status: 'saved',
        }
      )
    },
    async saveProviderSettings() {
      await wait(options.delayMs)
      const response = options.providerSettings ?? fixtureSaveProviderSettings
      providerSettings = {
        defaultConfigId: response.config.isDefault
          ? response.config.id
          : providerSettings.defaultConfigId,
        configs: [
          ...providerSettings.configs.filter((config) => config.id !== response.config.id),
          response.config,
        ]
          .map((config) =>
            response.config.isDefault
              ? {
                  ...config,
                  isDefault: config.id === response.config.id,
                }
              : config,
          )
          .sort((left, right) => left.id.localeCompare(right.id)),
      }
      return response
    },
    async saveProviderCapabilityRoute(request) {
      await wait(options.delayMs)
      const nextRoutes = providerCapabilityRoutes.routes.filter(
        (route) =>
          !(
            route.kind === request.route.kind &&
            route.configId === request.route.configId &&
            route.providerId === request.route.providerId
          ),
      )
      if (request.route.enabled) {
        nextRoutes.push(request.route)
      }
      providerCapabilityRoutes = {
        version: providerCapabilityRoutes.version,
        routes: nextRoutes.sort((left, right) =>
          `${left.kind}:${left.configId}`.localeCompare(`${right.kind}:${right.configId}`),
        ),
      }
      return {
        version: providerCapabilityRoutes.version,
        routes: cloneResponse(providerCapabilityRoutes.routes),
        status: 'saved',
      } satisfies SaveProviderCapabilityRouteResponse
    },
    async deleteProviderCapabilityRoute(request) {
      await wait(options.delayMs)
      providerCapabilityRoutes = {
        version: providerCapabilityRoutes.version,
        routes: providerCapabilityRoutes.routes.filter(
          (route) =>
            !(
              route.kind === request.kind &&
              route.configId === request.configId &&
              route.providerId === request.providerId
            ),
        ),
      }
      return {
        version: providerCapabilityRoutes.version,
        routes: cloneResponse(providerCapabilityRoutes.routes),
        status: 'deleted',
      } satisfies DeleteProviderCapabilityRouteResponse
    },
    async setExecutionSettings(request) {
      await wait(options.delayMs)
      return (
        options.setExecutionSettings ?? {
          ...fixtureSetExecutionSettings,
          agentCapabilities: {
            ...fixtureSetExecutionSettings.agentCapabilities,
            agentTeamsEnabled: request.agentTeamsEnabled,
            backgroundAgentsEnabled: request.backgroundAgentsEnabled,
            subagentsEnabled: request.subagentsEnabled,
          },
          contextCompressionTriggerRatio: request.contextCompressionTriggerRatio,
          permissionMode: request.permissionMode,
          toolProfile: request.toolProfile,
        }
      )
    },
    async setAutomationEnabled(id, enabled) {
      await wait(options.delayMs)
      const automation =
        automations.automations.find((automation) => automation.id === id) ?? fixtureAutomation
      const updated = {
        ...automation,
        enabled,
        updatedAt: new Date().toISOString(),
      } satisfies ListAutomationsResponse['automations'][number]
      automations = {
        automations: [
          updated,
          ...automations.automations.filter((automation) => automation.id !== id),
        ],
      }
      return (
        options.automationSetEnabled ?? {
          automation: updated,
          status: 'saved',
        }
      )
    },
    async saveBrowserMcpPreset(request) {
      await wait(options.delayMs)
      const preset =
        (options.browserMcpPresets ?? fixtureListBrowserMcpPresets).presets.find(
          (preset) => preset.id === request.presetId,
        ) ?? fixtureListBrowserMcpPresets.presets[0]
      return (options.browserMcpPreset ?? {
        preset: {
          ...preset,
          enabled: request.enabled ?? false,
        },
        server: {
          ...fixtureSaveBrowserMcpPreset.server,
          displayName: preset.displayName,
          enabled: request.enabled ?? false,
          id: preset.serverId,
          status: request.enabled ? 'configured' : 'disabled',
        },
      }) satisfies SaveBrowserMcpPresetResponse
    },
    async saveMcpServer() {
      await wait(options.delayMs)
      return options.mcpServer ?? fixtureSaveMcpServer
    },
    async setMcpServerEnabled(id, enabled) {
      await wait(options.delayMs)
      const server =
        (options.mcpServers ?? fixtureListMcpServers).servers.find((server) => server.id === id) ??
        fixtureSaveMcpServer.server
      return {
        server: {
          ...server,
          enabled,
          status: enabled ? server.status : 'disabled',
        },
      } satisfies SetMcpServerEnabledResponse
    },
    async setPluginEnabled(pluginId, enabled) {
      await wait(options.delayMs)
      const summary =
        (options.plugins ?? fixtureListPlugins).plugins.find((plugin) => plugin.id === pluginId) ??
        fixtureListPlugins.plugins[0]
      return {
        pluginId,
        status: enabled ? 'enabled' : 'disabled',
        summary: {
          ...summary,
          enabled,
          state: enabled ? 'activated' : { disabled: { last_state: 'activated' } },
        },
      } satisfies PluginOperationResult
    },
    async setProjectPluginsEnabled(enabled) {
      await wait(options.delayMs)
      return (
        options.setProjectPluginsEnabled ?? {
          allowProjectPlugins: enabled,
        }
      )
    },
    async restartMcpServer(id) {
      await wait(options.delayMs)
      const server =
        (options.mcpServers ?? fixtureListMcpServers).servers.find((server) => server.id === id) ??
        fixtureSaveMcpServer.server
      return {
        server,
      }
    },
    async clearMcpDiagnostics() {
      await wait(options.delayMs)
      return { status: 'cleared' } satisfies ClearMcpDiagnosticsResponse
    },
    async setConversationModelConfig(conversationId, modelConfigId) {
      await wait(options.delayMs)
      return {
        conversationId,
        modelConfigId,
        status: 'saved',
      } satisfies SetConversationModelConfigResponse
    },
    async setSkillEnabled(id, enabled) {
      await wait(options.delayMs)
      const skill =
        (options.skills ?? fixtureListSkills).skills.find(
          (currentSkill) => currentSkill.id === id,
        ) ?? fixtureWorkspaceSkill

      return {
        skill: {
          ...skill,
          enabled,
          status: enabled ? 'ready' : 'disabled',
        },
      }
    },
    async startRun(request) {
      await wait(options.delayMs)
      worktreePagesByConversation.set(
        request.conversationId,
        worktreePageForFixtureRun(
          request.conversationId,
          request.prompt,
          request.clientMessageId,
          'running',
        ),
      )
      emitFixtureConversationBatch(fixtureEventState, activeSubscription, [
        fixtureTimelineEvent(
          'run.started',
          {
            permissionMode: request.permissionMode ?? 'default',
            sessionId: request.conversationId,
          },
          { conversationSequence: 1, id: 'evt-fixture-run-started', sequence: 1 },
        ),
        fixtureTimelineEvent(
          'user.message.appended',
          {
            body: request.prompt,
            clientMessageId: request.clientMessageId,
            messageId: 'message-fixture-user',
          },
          {
            conversationSequence: 2,
            id: 'evt-fixture-user-message',
            sequence: 2,
            source: 'user',
          },
        ),
        fixtureTimelineEvent(
          'assistant.delta',
          {
            messageId: 'message-fixture-delta',
            text: 'Drafting the implementation plan.',
          },
          {
            conversationSequence: 3,
            id: 'evt-fixture-assistant-delta',
            sequence: 3,
            source: 'assistant',
          },
        ),
        fixtureTimelineEvent(
          'tool.requested',
          {
            argumentsSummary: 'Input withheld from conversation timeline.',
            toolName: 'read_file',
            toolUseId: 'tool-fixture-read',
          },
          {
            conversationSequence: 4,
            id: 'evt-fixture-tool-requested',
            sequence: 4,
            source: 'tool',
          },
        ),
        fixtureTimelineEvent(
          'tool.completed',
          {
            durationMs: 42,
            outputSummary: 'Output withheld from conversation timeline.',
            toolUseId: 'tool-fixture-read',
          },
          {
            conversationSequence: 5,
            id: 'evt-fixture-tool-completed',
            sequence: 5,
            source: 'tool',
          },
        ),
        fixtureTimelineEvent(
          'permission.requested',
          {
            autoResolved: false,
            decisionScope: 'this run',
            exposure: 'workspace',
            operation: 'Run local verification',
            reason: 'Confirm the generated foundation before continuing.',
            requestId: '01HZ0000000000000000000001',
            severity: 'medium',
            target: 'local verification task',
            toolUseId: 'tool-fixture-read',
            workspaceBoundary: 'workspace',
          },
          {
            conversationSequence: 6,
            id: 'evt-fixture-permission-requested',
            sequence: 6,
            source: 'policy',
          },
        ),
        fixtureTimelineEvent(
          'artifact.created',
          { artifactId: 'artifact-desktop-foundation', status: 'ready' },
          {
            conversationSequence: 7,
            id: 'evt-fixture-artifact-created',
            sequence: 7,
            source: 'engine',
          },
        ),
      ])
      worktreePagesByConversation.set(
        request.conversationId,
        worktreePageForFixtureRun(
          request.conversationId,
          request.prompt,
          request.clientMessageId,
          'complete',
        ),
      )
      completionBatchFlushed = emitFixtureConversationBatch(
        fixtureEventState,
        activeSubscription,
        [
          fixtureTimelineEvent(
            'assistant.completed',
            {
              body: 'The setup is ready for review.',
              messageId: 'message-fixture-assistant',
            },
            {
              conversationSequence: 8,
              id: 'evt-fixture-assistant-completed',
              sequence: 8,
              source: 'assistant',
            },
          ),
          fixtureTimelineEvent(
            'run.ended',
            { reason: 'completed' },
            {
              conversationSequence: 9,
              id: 'evt-fixture-run-ended',
              sequence: 9,
            },
          ),
        ],
        100,
      )
      return { runId: 'run-001', status: 'started' } satisfies StartRunResponse
    },
    async subscribeConversationEvents(request) {
      await wait(options.delayMs)
      subscriptionCounter += 1
      activeSubscription = options.subscribeConversationEvents ?? {
        subscriptionId: `subscription-fixture-${subscriptionCounter}`,
        conversationId: request.conversationId,
        replayEvents: [],
        gap: false,
      }
      return activeSubscription
    },
    async listenConversationEventBatches(onBatch) {
      await wait(options.delayMs)
      batchListener = onBatch
      return () => {
        if (batchListener === onBatch) {
          batchListener = null
          clearPendingBatches()
        }
      }
    },
    async subscribeMcpDiagnostics() {
      await wait(options.delayMs)
      return (options.subscribeMcpDiagnostics ?? {
        replayEvents: (options.mcpDiagnostics ?? fixtureListMcpDiagnostics).events,
        subscriptionId: 'mcp-diagnostic-subscription-001',
      }) satisfies SubscribeMcpDiagnosticsResponse
    },
    async listenMcpDiagnosticBatches() {
      await wait(options.delayMs)
      return () => undefined
    },
    async unsubscribeMcpDiagnostics(subscriptionId) {
      await wait(options.delayMs)
      return {
        status: 'unsubscribed',
        subscriptionId,
      } satisfies UnsubscribeMcpDiagnosticsResponse
    },
    async unsubscribeConversationEvents(subscriptionId) {
      await wait(options.delayMs)
      if (activeSubscription?.subscriptionId === subscriptionId) {
        activeSubscription = null
        clearPendingBatches()
      }
      return {
        subscriptionId,
        status: 'unsubscribed',
      } satisfies UnsubscribeConversationEventsResponse
    },
    async updateMemoryItem(request) {
      await wait(options.delayMs)
      return {
        item: {
          ...(options.memoryItem ?? fixtureMemoryItem).item,
          content: request.content,
          id: request.id,
        },
      } satisfies UpdateMemoryItemResponse
    },
    async updatePluginConfig(pluginId) {
      await wait(options.delayMs)
      const summary =
        (options.plugins ?? fixtureListPlugins).plugins.find((plugin) => plugin.id === pluginId) ??
        fixtureListPlugins.plugins[0]
      return {
        pluginId,
        status: 'configured',
        summary,
      } satisfies PluginOperationResult
    },
    async validatePluginFromPath() {
      await wait(options.delayMs)
      return options.pluginInstallReport ?? fixturePluginInstallReport
    },
    async validateProviderSettings() {
      await wait(options.delayMs)
      return options.providerValidation ?? fixtureValidateProviderSettings
    },
  }
}

function fixtureTimelineEvent<TType extends RunEvent['type']>(
  type: TType,
  payload: Extract<RunEvent, { type: TType }>['payload'],
  options: Partial<RunEvent> = {},
): RunEvent {
  return {
    id: options.id ?? `evt-fixture-${type}`,
    conversationSequence: options.conversationSequence ?? 1,
    runId: options.runId ?? 'run-001',
    sequence: options.sequence ?? 1,
    source: options.source ?? 'engine',
    timestamp,
    type,
    visibility: options.visibility ?? 'public',
    payload,
  } as RunEvent
}

type FixtureConversationEventState = {
  getListener: () => ((batch: ConversationEventBatchPayload) => void) | null
  getSubscription: () => SubscribeConversationEventsResponse | null
  trackTimeout: (timeoutId: number, resolve: () => void) => void
  untrackTimeout: (timeoutId: number) => void
}

function emitFixtureConversationBatch(
  state: FixtureConversationEventState,
  subscription: SubscribeConversationEventsResponse | null,
  events: RunEvent[],
  delayMs = 0,
): Promise<void> {
  if (!state.getListener() || !subscription || events.length === 0) {
    return Promise.resolve()
  }

  return new Promise<void>((resolve) => {
    const timeoutId = window.setTimeout(() => {
      state.untrackTimeout(timeoutId)
      const listener = state.getListener()
      const currentSubscription = state.getSubscription()

      if (
        !listener ||
        currentSubscription !== subscription ||
        currentSubscription.subscriptionId !== subscription.subscriptionId ||
        currentSubscription.conversationId !== subscription.conversationId
      ) {
        resolve()
        return
      }

      listener({
        subscriptionId: currentSubscription.subscriptionId,
        conversationId: currentSubscription.conversationId,
        events,
        cursor: events.at(-1)
          ? {
              eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
              conversationSequence: events.at(-1)?.conversationSequence ?? 0,
            }
          : currentSubscription.cursor,
        gap: false,
        phase: 'live',
      })
      resolve()
    }, delayMs)
    state.trackTimeout(timeoutId, resolve)
  })
}

export function createRejectedTestCommandClient(error: unknown): CommandClient {
  return {
    cancelRun: () => Promise.reject(error),
    createAttachmentFromPath: () => Promise.reject(error),
    createConversation: () => Promise.reject(error),
    deleteAutomation: () => Promise.reject(error),
    deleteConversation: () => Promise.reject(error),
    deleteMcpServer: () => Promise.reject(error),
    deleteMemoryItem: () => Promise.reject(error),
    uninstallPlugin: () => Promise.reject(error),
    deleteSkill: () => Promise.reject(error),
    exportMemoryItems: () => Promise.reject(error),
    exportSupportBundle: () => Promise.reject(error),
    getContextSnapshot: () => Promise.reject(error),
    getExecutionSettings: () => Promise.reject(error),
    getConversation: () => Promise.reject(error),
    getArtifactMediaPreview: () => Promise.reject(error),
    getAttachmentMediaPreview: () => Promise.reject(error),
    getAppInfo: () => Promise.reject(error),
    getHarnessHealthcheck: () => Promise.reject(error),
    getMemoryItem: () => Promise.reject(error),
    getMcpServerConfig: () => Promise.reject(error),
    getPluginDetail: () => Promise.reject(error),
    getProviderConfigApiKey: () => Promise.reject(error),
    getReplayTimeline: () => Promise.reject(error),
    getSkillCatalogEntry: () => Promise.reject(error),
    getSkillCatalogFile: () => Promise.reject(error),
    pageConversationTimeline: () => Promise.reject(error),
    pageConversationWorktree: () => Promise.reject(error),
    getSkillDetail: () => Promise.reject(error),
    getSkillFile: () => Promise.reject(error),
    importSkill: () => Promise.reject(error),
    installPluginFromPath: () => Promise.reject(error),
    installSkillFromCatalog: () => Promise.reject(error),
    listSkillCatalogInstallTasks: () => Promise.reject(error),
    listenSkillCatalogInstallProgress: () => Promise.reject(error),
    listActivity: () => Promise.reject(error),
    listArtifacts: () => Promise.reject(error),
    listAutomationRuns: () => Promise.reject(error),
    listAutomations: () => Promise.reject(error),
    listConversations: () => Promise.reject(error),
    listEvalCases: () => Promise.reject(error),
    listModelProviderCatalog: () => Promise.reject(error),
    listMcpDiagnostics: () => Promise.reject(error),
    listMcpServers: () => Promise.reject(error),
    listBrowserMcpPresets: () => Promise.reject(error),
    listMemoryItems: () => Promise.reject(error),
    listPlugins: () => Promise.reject(error),
    listProviderSettings: () => Promise.reject(error),
    listProviderCapabilityRoutes: () => Promise.reject(error),
    listProviderCapabilityRouteOptions: () => Promise.reject(error),
    listProjects: () => Promise.reject(error),
    addProject: () => Promise.reject(error),
    switchProject: () => Promise.reject(error),
    deleteProject: () => Promise.reject(error),
    listReferenceCandidates: () => Promise.reject(error),
    listSkillCatalogEntries: () => Promise.reject(error),
    listSkillCatalogSources: () => Promise.reject(error),
    listSkills: () => Promise.reject(error),
    resolvePermission: () => Promise.reject(error),
    reloadPlugin: () => Promise.reject(error),
    requestProviderConfigApiKeyReveal: () => Promise.reject(error),
    runAutomationNow: () => Promise.reject(error),
    runEvalCase: () => Promise.reject(error),
    saveAutomation: () => Promise.reject(error),
    saveBrowserMcpPreset: () => Promise.reject(error),
    saveMcpServer: () => Promise.reject(error),
    setMcpServerEnabled: () => Promise.reject(error),
    setPluginEnabled: () => Promise.reject(error),
    setProjectPluginsEnabled: () => Promise.reject(error),
    restartMcpServer: () => Promise.reject(error),
    clearMcpDiagnostics: () => Promise.reject(error),
    saveProviderSettings: () => Promise.reject(error),
    saveProviderCapabilityRoute: () => Promise.reject(error),
    deleteProviderCapabilityRoute: () => Promise.reject(error),
    setExecutionSettings: () => Promise.reject(error),
    setAutomationEnabled: () => Promise.reject(error),
    setConversationModelConfig: () => Promise.reject(error),
    setSkillEnabled: () => Promise.reject(error),
    startRun: () => Promise.reject(error),
    subscribeConversationEvents: () => Promise.reject(error),
    listenConversationEventBatches: () => Promise.reject(error),
    subscribeMcpDiagnostics: () => Promise.reject(error),
    listenMcpDiagnosticBatches: () => Promise.reject(error),
    unsubscribeMcpDiagnostics: () => Promise.reject(error),
    unsubscribeConversationEvents: () => Promise.reject(error),
    updateMemoryItem: () => Promise.reject(error),
    updatePluginConfig: () => Promise.reject(error),
    validatePluginFromPath: () => Promise.reject(error),
    validateProviderSettings: () => Promise.reject(error),
  }
}
