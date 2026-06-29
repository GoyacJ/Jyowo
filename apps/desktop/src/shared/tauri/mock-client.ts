import type { RunEvent } from '@/shared/events/run-event-schema'
import type {
  AppInfo,
  CancelRunResponse,
  ClearMcpDiagnosticsResponse,
  CommandClient,
  ConversationEventBatchPayload,
  ConversationModelCapability,
  CreateAttachmentFromPathResponse,
  CreateConversationResponse,
  DeleteConversationResponse,
  DeleteProjectResponse,
  ExportMemoryItemsResponse,
  ExportSupportBundleResponse,
  GetArtifactMediaPreviewResponse,
  GetContextSnapshotResponse,
  GetConversationResponse,
  GetExecutionSettingsResponse,
  GetMcpServerConfigResponse,
  GetMemoryItemResponse,
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
  ListConversationsResponse,
  ListEvalCasesResponse,
  ListMcpDiagnosticsResponse,
  ListMcpServersResponse,
  ListMemoryItemsResponse,
  ListProjectsResponse,
  ListProviderSettingsResponse,
  ListReferenceCandidatesResponse,
  ListSkillCatalogEntriesResponse,
  ListSkillCatalogInstallTasksResponse,
  ListSkillCatalogSourcesResponse,
  ListSkillsResponse,
  ModelProviderCatalogResponse,
  PageConversationTimelineResponse,
  PageConversationWorktreeResponse,
  ReplayTimelineResponse,
  RequestProviderConfigApiKeyRevealResponse,
  ResolvePermissionResponse,
  RunEvalCaseResponse,
  SaveMcpServerResponse,
  SaveProviderSettingsResponse,
  SetConversationModelConfigResponse,
  SetExecutionSettingsResponse,
  SetMcpServerEnabledResponse,
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
} from './commands'

const mockAppInfo: AppInfo = {
  name: 'Jyowo',
  version: '0.1.0',
  shell: 'tauri2-react',
  harness: {
    sdkCrate: 'jyowo_harness_sdk',
    mode: 'in-process',
  },
}

const mockHarnessHealthcheck: HarnessHealthcheck = {
  status: 'available',
  sdkCrate: 'jyowo_harness_sdk',
}

const timestamp = '2026-06-17T02:22:00.000Z'

const mockListConversations: ListConversationsResponse = {
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

const mockConversation: GetConversationResponse = {
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

const mockListActivity: ListActivityResponse = {
  events: [
    {
      id: 'evt-001',
      conversationSequence: 1,
      payload: { sessionId: 'conversation-001' },
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

const mockListArtifacts: ListArtifactsResponse = {
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

const mockAttachment: CreateAttachmentFromPathResponse = {
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

const mockArtifactMediaPreview: GetArtifactMediaPreviewResponse = {
  dataUrl:
    'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=',
  mimeType: 'image/png',
  sizeBytes: 68,
}

const mockReferenceCandidates: ListReferenceCandidatesResponse = {
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

const mockContextSnapshot: GetContextSnapshotResponse = {
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

const mockValidateProviderSettings: ValidateProviderSettingsResponse = {
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

const mockModelProviderCatalog: ModelProviderCatalogResponse = {
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

const mockProviderSettingsList: ListProviderSettingsResponse = {
  defaultConfigId: null,
  configs: [],
}

const mockExecutionSettings: GetExecutionSettingsResponse = {
  autoModeAvailable: false,
  permissionMode: 'default',
}

const mockSetExecutionSettings: SetExecutionSettingsResponse = {
  autoModeAvailable: false,
  permissionMode: 'default',
}

const mockSaveProviderSettings: SaveProviderSettingsResponse = {
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

const mockListMcpServers: ListMcpServersResponse = {
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

const mockMcpServerConfig: GetMcpServerConfigResponse = {
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

const mockWorkspaceSkill: SkillSummary = {
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

const mockBundledSkill: SkillSummary = {
  description: 'Inspects source changes and returns risks.',
  enabled: true,
  id: 'code-review',
  manageable: false,
  name: 'code-review',
  sourceKind: 'bundled',
  status: 'ready',
  tags: ['review'],
}

const mockListSkills: ListSkillsResponse = {
  skills: [mockWorkspaceSkill, mockBundledSkill],
}

const mockSkillCatalogSources: ListSkillCatalogSourcesResponse = {
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

const mockSkillCatalogEntries: ListSkillCatalogEntriesResponse = {
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

const mockSkillCatalogEntry: GetSkillCatalogEntryResponse = {
  entry: mockSkillCatalogEntries.entries[0],
  files: [{ kind: 'file', path: 'SKILL.md', sizeBytes: 512 }],
  readmePreview: 'Create distinctive frontend interfaces.',
  validation: {
    issues: [],
    status: 'ready',
  },
}

const mockSkillDetail: GetSkillDetailResponse = {
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
    summary: mockWorkspaceSkill,
  },
}

const mockSkillEntryFile: GetSkillFileResponse = {
  file: {
    content: 'Write concise release notes from the current workspace diff.',
    path: 'SKILL.md',
  },
}

const mockSkillCatalogFile: GetSkillCatalogFileResponse = {
  file: {
    content: 'Write concise release notes from the current workspace diff.',
    path: 'SKILL.md',
    truncated: false,
  },
}

const mockSkillCatalogInstallTasks: ListSkillCatalogInstallTasksResponse = {
  tasks: [],
}

const mockSaveMcpServer: SaveMcpServerResponse = {
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

const mockListMcpDiagnostics: ListMcpDiagnosticsResponse = {
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

const mockMemoryItems: ListMemoryItemsResponse = {
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

const mockMemoryItem: GetMemoryItemResponse = {
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

export const mockEmptyProjects: ListProjectsResponse = {
  activePath: null,
  projects: [],
}

export const mockJyowoProject: ListProjectsResponse = {
  activePath: '/Users/goya/Repo/Git/Jyowo',
  projects: [
    {
      lastOpenedAt: timestamp,
      name: 'Jyowo',
      path: '/Users/goya/Repo/Git/Jyowo',
    },
  ],
}

const mockMemoryExport: ExportMemoryItemsResponse = {
  exportedAt: timestamp,
  format: 'json',
  itemCount: 1,
  path: '.jyowo/runtime/exports/memory-20260617T000000.000Z.json',
}

const mockListEvalCases: ListEvalCasesResponse = {
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

const mockReplayTimeline: ReplayTimelineResponse = {
  events: mockListActivity.events,
  replayed: true,
}

const mockConversationWorktreePage: PageConversationWorktreeResponse = {
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
            id: 'segment:tools:tool-mock-read',
            order: 2,
            attempts: [
              {
                id: 'tool:tool-mock-read',
                order: 0,
                toolUseId: 'tool-mock-read',
                toolName: 'read_file',
                status: 'completed',
                permission: {
                  id: 'permission:01HZ0000000000000000000001',
                  requestId: '01HZ0000000000000000000001',
                  toolUseId: 'tool-mock-read',
                  status: 'approved',
                  summary: 'Approved once',
                },
              },
              {
                id: 'tool:tool-mock-verify',
                order: 1,
                toolUseId: 'tool-mock-verify',
                toolName: 'local_verification',
                status: 'waitingPermission',
                permission: {
                  id: 'permission:01HZ0000000000000000000002',
                  requestId: '01HZ0000000000000000000002',
                  toolUseId: 'tool-mock-verify',
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
            id: 'segment:tools:tool-mock-test',
            order: 0,
            attempts: [
              {
                id: 'tool:tool-mock-test',
                order: 0,
                toolUseId: 'tool-mock-test',
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

function worktreePageForMockRun(
  conversationId: string,
  prompt: string,
  clientMessageId: string | undefined,
  status: 'running' | 'complete',
): PageConversationWorktreeResponse {
  const turn: PageConversationWorktreeResponse['turns'][number] = {
    id: 'turn:message-mock-user',
    conversationId,
    position: 0,
    user: {
      id: 'user:message-mock-user',
      messageId: 'message-mock-user',
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
              id: 'process-step:mock-reasoning',
              order: 0,
              kind: 'reasoning',
              status,
              title: '整理实施计划',
              body: 'Drafting the implementation plan.',
            },
            {
              id: 'process-step:mock-read',
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
          id: 'segment:tools:tool-mock-read',
          order: 1,
          attempts: [
            {
              id: 'tool:tool-mock-read',
              order: 0,
              toolUseId: 'tool-mock-read',
              toolName: 'Reading files',
              status: 'completed',
            },
            {
              id: 'tool:tool-mock-verify',
              order: 1,
              toolUseId: 'tool-mock-verify',
              toolName: 'Run local verification',
              status: status === 'running' ? 'waitingPermission' : 'completed',
              permission: {
                id: 'permission:01HZ0000000000000000000001',
                requestId: '01HZ0000000000000000000001',
                toolUseId: 'tool-mock-verify',
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
          id: 'segment:text:message-mock-assistant',
          order: 3,
          messageId: 'message-mock-assistant',
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

const mockSupportBundleExport: ExportSupportBundleResponse = {
  bundlePath: '.jyowo/runtime/exports/support-bundle-20260617T000000.000Z.json',
  eventCount: 1,
  exportedAt: timestamp,
  jsonlPath: '.jyowo/runtime/exports/events-20260617T000000.000Z.jsonl',
  markdownPath: '.jyowo/runtime/exports/support-report-20260617T000000.000Z.md',
  redacted: true,
}

export interface MockCommandClientOptions {
  appInfo?: AppInfo
  attachmentFromPath?: CreateAttachmentFromPathResponse
  contextSnapshot?: GetContextSnapshotResponse
  conversation?: GetConversationResponse
  conversations?: ListConversationsResponse
  executionSettings?: GetExecutionSettingsResponse
  healthcheck?: HarnessHealthcheck
  artifacts?: ListArtifactsResponse
  artifactMediaPreview?: GetArtifactMediaPreviewResponse
  listActivity?: ListActivityResponse
  memoryExport?: ExportMemoryItemsResponse
  evalCases?: ListEvalCasesResponse
  memoryItem?: GetMemoryItemResponse
  memoryItems?: ListMemoryItemsResponse
  mcpDiagnostics?: ListMcpDiagnosticsResponse
  mcpServerConfig?: GetMcpServerConfigResponse
  mcpServer?: SaveMcpServerResponse
  mcpServers?: ListMcpServersResponse
  modelProviderCatalog?: ModelProviderCatalogResponse
  providerConfigApiKey?: GetProviderConfigApiKeyResponse
  providerConfigApiKeyReveal?: RequestProviderConfigApiKeyRevealResponse
  providerSettingsList?: ListProviderSettingsResponse
  projects?: ListProjectsResponse
  providerSettings?: SaveProviderSettingsResponse
  providerValidation?: ValidateProviderSettingsResponse
  setExecutionSettings?: SetExecutionSettingsResponse
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

function mockProviderApiKeyForConfig(configId: string) {
  return ['mock', 'provider', 'revealed', configId].join(':')
}

export function createMockCommandClient(options: MockCommandClientOptions = {}): CommandClient {
  let batchListener: ((batch: ConversationEventBatchPayload) => void) | null = null
  let activeSubscription: SubscribeConversationEventsResponse | null = null
  let subscriptionCounter = 0
  let providerRevealCounter = 0
  let completionBatchFlushed: Promise<void> = Promise.resolve()
  let projects = options.projects ?? mockJyowoProject
  let providerSettings = cloneResponse(options.providerSettingsList ?? mockProviderSettingsList)
  let createdConversationCounter = 0
  let conversations = cloneResponse(options.conversations ?? mockListConversations)
  const providerRevealConfigIdsByToken = new Map<string, string>()
  const conversationDetailsById = new Map<string, GetConversationResponse>()
  conversationDetailsById.set(
    'conversation-001',
    cloneResponse(options.conversation ?? mockConversation),
  )
  const worktreePagesByConversation = new Map<string, PageConversationWorktreeResponse>()
  worktreePagesByConversation.set(
    'conversation-001',
    cloneResponse(options.conversationWorktreePage ?? mockConversationWorktreePage),
  )
  const pendingBatchTimeouts = new Map<number, () => void>()
  const catalogInstallProgressListeners = new Set<
    (progress: SkillCatalogInstallProgressPayload) => void
  >()
  const mockEventState: MockConversationEventState = {
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
      return options.attachmentFromPath ?? mockAttachment
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
    async deleteMcpServer(id) {
      await wait(options.delayMs)
      return { id, status: 'deleted' }
    },
    async deleteMemoryItem(id) {
      await wait(options.delayMs)
      return { id, status: 'deleted' }
    },
    async deleteSkill(id) {
      await wait(options.delayMs)
      return { id, status: 'deleted' }
    },
    async exportMemoryItems() {
      await wait(options.delayMs)
      return options.memoryExport ?? mockMemoryExport
    },
    async exportSupportBundle() {
      await wait(options.delayMs)
      return options.supportBundleExport ?? mockSupportBundleExport
    },
    async getContextSnapshot() {
      await wait(options.delayMs)
      return options.contextSnapshot ?? mockContextSnapshot
    },
    async getExecutionSettings() {
      await wait(options.delayMs)
      return options.executionSettings ?? mockExecutionSettings
    },
    async getConversation(conversationId) {
      await wait(options.delayMs)
      return options.conversation ?? conversationDetailsById.get(conversationId) ?? mockConversation
    },
    async getArtifactMediaPreview() {
      await wait(options.delayMs)
      return options.artifactMediaPreview ?? mockArtifactMediaPreview
    },
    async getAppInfo() {
      await wait(options.delayMs)
      return options.appInfo ?? mockAppInfo
    },
    async getHarnessHealthcheck() {
      await wait(options.delayMs)
      return options.healthcheck ?? mockHarnessHealthcheck
    },
    async getMemoryItem() {
      await wait(options.delayMs)
      return options.memoryItem ?? mockMemoryItem
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
        apiKey: mockProviderApiKeyForConfig(configId),
        configId,
      }
    },
    async getReplayTimeline() {
      await wait(options.delayMs)
      return options.replayTimeline ?? mockReplayTimeline
    },
    async getSkillCatalogEntry() {
      await wait(options.delayMs)
      return options.skillCatalogEntry ?? mockSkillCatalogEntry
    },
    async getSkillCatalogFile() {
      await wait(options.delayMs)
      return options.skillCatalogFile ?? mockSkillCatalogFile
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
        (options.skills ?? mockListSkills).skills.find((skill) => skill.id === id) ??
        mockWorkspaceSkill

      return {
        skill: {
          ...mockSkillDetail.skill,
          summary,
        },
      } satisfies GetSkillDetailResponse
    },
    async getSkillFile(_id, path) {
      await wait(options.delayMs)
      if (options.skillFile) {
        return options.skillFile
      }

      return path === mockSkillEntryFile.file.path
        ? mockSkillEntryFile
        : {
            file: {
              content: `Mock content for ${path}`,
              path,
            },
          }
    },
    async importSkill() {
      await wait(options.delayMs)
      return { skill: mockWorkspaceSkill }
    },
    async installSkillFromCatalog(request) {
      emitCatalogInstallProgress(request, 'preparing', 5)
      await wait(options.delayMs)
      emitCatalogInstallProgress(request, 'completed', 100)
      return (
        options.skillCatalogInstall ?? {
          task: {
            entryId: request.entryId,
            operationId: request.operationId ?? 'catalog-install-mock',
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
      return options.skillCatalogInstallTasks ?? mockSkillCatalogInstallTasks
    },
    async listenSkillCatalogInstallProgress(onProgress) {
      catalogInstallProgressListeners.add(onProgress)
      return () => {
        catalogInstallProgressListeners.delete(onProgress)
      }
    },
    async listActivity() {
      await wait(options.delayMs)
      return options.listActivity ?? mockListActivity
    },
    async listArtifacts(_request) {
      await wait(options.delayMs)
      return options.artifacts ?? mockListArtifacts
    },
    async listConversations() {
      await wait(options.delayMs)
      return conversations
    },
    async listEvalCases() {
      await wait(options.delayMs)
      return options.evalCases ?? mockListEvalCases
    },
    async listModelProviderCatalog() {
      await wait(options.delayMs)
      return options.modelProviderCatalog ?? mockModelProviderCatalog
    },
    async listMcpDiagnostics() {
      await wait(options.delayMs)
      return options.mcpDiagnostics ?? mockListMcpDiagnostics
    },
    async listMcpServers() {
      await wait(options.delayMs)
      return options.mcpServers ?? mockListMcpServers
    },
    async getMcpServerConfig(id) {
      await wait(options.delayMs)
      if (options.mcpServerConfig?.server.id === id) {
        return options.mcpServerConfig
      }
      if (mockMcpServerConfig.server.id === id) {
        return mockMcpServerConfig
      }
      throw new Error(`MCP server not found: ${id}`)
    },
    async listMemoryItems() {
      await wait(options.delayMs)
      return options.memoryItems ?? mockMemoryItems
    },
    async listProviderSettings() {
      await wait(options.delayMs)
      return cloneResponse(providerSettings)
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
      return options.referenceCandidates ?? mockReferenceCandidates
    },
    async listSkillCatalogEntries() {
      await wait(options.delayMs)
      return options.skillCatalogEntries ?? mockSkillCatalogEntries
    },
    async listSkillCatalogSources() {
      await wait(options.delayMs)
      return options.skillCatalogSources ?? mockSkillCatalogSources
    },
    async listSkills() {
      await wait(options.delayMs)
      return options.skills ?? mockListSkills
    },
    async resolvePermission(request) {
      await wait(options.delayMs)
      await completionBatchFlushed
      emitMockConversationBatch(
        mockEventState,
        activeSubscription,
        [
          mockTimelineEvent(
            'permission.resolved',
            {
              decision: request.decision,
              requestId: request.requestId,
            },
            {
              conversationSequence: 10,
              id: 'evt-mock-permission-resolved',
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
          revealToken: `mock-reveal-token-${providerRevealCounter}`,
          status: 'ready',
        }
      providerRevealConfigIdsByToken.set(response.revealToken, configId)
      return {
        ...response,
        configId,
      }
    },
    async runEvalCase(caseId) {
      await wait(options.delayMs)
      const evalCase =
        (options.evalCases ?? mockListEvalCases).cases.find(
          (currentCase) => currentCase.id === caseId,
        ) ?? mockListEvalCases.cases[0]

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
    async saveProviderSettings() {
      await wait(options.delayMs)
      const response = options.providerSettings ?? mockSaveProviderSettings
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
    async setExecutionSettings(request) {
      await wait(options.delayMs)
      return (
        options.setExecutionSettings ?? {
          ...mockSetExecutionSettings,
          permissionMode: request.permissionMode,
        }
      )
    },
    async saveMcpServer() {
      await wait(options.delayMs)
      return options.mcpServer ?? mockSaveMcpServer
    },
    async setMcpServerEnabled(id, enabled) {
      await wait(options.delayMs)
      const server =
        (options.mcpServers ?? mockListMcpServers).servers.find((server) => server.id === id) ??
        mockSaveMcpServer.server
      return {
        server: {
          ...server,
          enabled,
          status: enabled ? server.status : 'disabled',
        },
      } satisfies SetMcpServerEnabledResponse
    },
    async restartMcpServer(id) {
      await wait(options.delayMs)
      const server =
        (options.mcpServers ?? mockListMcpServers).servers.find((server) => server.id === id) ??
        mockSaveMcpServer.server
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
        (options.skills ?? mockListSkills).skills.find((currentSkill) => currentSkill.id === id) ??
        mockWorkspaceSkill

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
        worktreePageForMockRun(
          request.conversationId,
          request.prompt,
          request.clientMessageId,
          'running',
        ),
      )
      emitMockConversationBatch(mockEventState, activeSubscription, [
        mockTimelineEvent(
          'run.started',
          { sessionId: request.conversationId },
          { conversationSequence: 1, id: 'evt-mock-run-started', sequence: 1 },
        ),
        mockTimelineEvent(
          'user.message.appended',
          {
            body: request.prompt,
            clientMessageId: request.clientMessageId,
            messageId: 'message-mock-user',
          },
          {
            conversationSequence: 2,
            id: 'evt-mock-user-message',
            sequence: 2,
            source: 'user',
          },
        ),
        mockTimelineEvent(
          'assistant.delta',
          {
            messageId: 'message-mock-delta',
            text: 'Drafting the implementation plan.',
          },
          {
            conversationSequence: 3,
            id: 'evt-mock-assistant-delta',
            sequence: 3,
            source: 'assistant',
          },
        ),
        mockTimelineEvent(
          'tool.requested',
          {
            argumentsSummary: 'Input withheld from conversation timeline.',
            toolName: 'read_file',
            toolUseId: 'tool-mock-read',
          },
          {
            conversationSequence: 4,
            id: 'evt-mock-tool-requested',
            sequence: 4,
            source: 'tool',
          },
        ),
        mockTimelineEvent(
          'tool.completed',
          {
            durationMs: 42,
            outputSummary: 'Output withheld from conversation timeline.',
            toolUseId: 'tool-mock-read',
          },
          {
            conversationSequence: 5,
            id: 'evt-mock-tool-completed',
            sequence: 5,
            source: 'tool',
          },
        ),
        mockTimelineEvent(
          'permission.requested',
          {
            decisionScope: 'this run',
            exposure: 'workspace',
            operation: 'Run local verification',
            reason: 'Confirm the generated foundation before continuing.',
            requestId: '01HZ0000000000000000000001',
            severity: 'medium',
            target: 'local verification task',
            toolUseId: 'tool-mock-read',
            workspaceBoundary: 'workspace',
          },
          {
            conversationSequence: 6,
            id: 'evt-mock-permission-requested',
            sequence: 6,
            source: 'policy',
          },
        ),
        mockTimelineEvent(
          'artifact.created',
          { artifactId: 'artifact-desktop-foundation', status: 'ready' },
          {
            conversationSequence: 7,
            id: 'evt-mock-artifact-created',
            sequence: 7,
            source: 'engine',
          },
        ),
      ])
      worktreePagesByConversation.set(
        request.conversationId,
        worktreePageForMockRun(
          request.conversationId,
          request.prompt,
          request.clientMessageId,
          'complete',
        ),
      )
      completionBatchFlushed = emitMockConversationBatch(
        mockEventState,
        activeSubscription,
        [
          mockTimelineEvent(
            'assistant.completed',
            {
              body: 'The setup is ready for review.',
              messageId: 'message-mock-assistant',
            },
            {
              conversationSequence: 8,
              id: 'evt-mock-assistant-completed',
              sequence: 8,
              source: 'assistant',
            },
          ),
          mockTimelineEvent(
            'run.ended',
            { reason: 'completed' },
            {
              conversationSequence: 9,
              id: 'evt-mock-run-ended',
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
        subscriptionId: `subscription-mock-${subscriptionCounter}`,
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
        replayEvents: (options.mcpDiagnostics ?? mockListMcpDiagnostics).events,
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
          ...(options.memoryItem ?? mockMemoryItem).item,
          content: request.content,
          id: request.id,
        },
      } satisfies UpdateMemoryItemResponse
    },
    async validateProviderSettings() {
      await wait(options.delayMs)
      return options.providerValidation ?? mockValidateProviderSettings
    },
  }
}

function mockTimelineEvent<TType extends RunEvent['type']>(
  type: TType,
  payload: Extract<RunEvent, { type: TType }>['payload'],
  options: Partial<RunEvent> = {},
): RunEvent {
  return {
    id: options.id ?? `evt-mock-${type}`,
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

type MockConversationEventState = {
  getListener: () => ((batch: ConversationEventBatchPayload) => void) | null
  getSubscription: () => SubscribeConversationEventsResponse | null
  trackTimeout: (timeoutId: number, resolve: () => void) => void
  untrackTimeout: (timeoutId: number) => void
}

function emitMockConversationBatch(
  state: MockConversationEventState,
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

export function createRejectedCommandClient(error: unknown): CommandClient {
  return {
    cancelRun: () => Promise.reject(error),
    createAttachmentFromPath: () => Promise.reject(error),
    createConversation: () => Promise.reject(error),
    deleteConversation: () => Promise.reject(error),
    deleteMcpServer: () => Promise.reject(error),
    deleteMemoryItem: () => Promise.reject(error),
    deleteSkill: () => Promise.reject(error),
    exportMemoryItems: () => Promise.reject(error),
    exportSupportBundle: () => Promise.reject(error),
    getContextSnapshot: () => Promise.reject(error),
    getExecutionSettings: () => Promise.reject(error),
    getConversation: () => Promise.reject(error),
    getArtifactMediaPreview: () => Promise.reject(error),
    getAppInfo: () => Promise.reject(error),
    getHarnessHealthcheck: () => Promise.reject(error),
    getMemoryItem: () => Promise.reject(error),
    getMcpServerConfig: () => Promise.reject(error),
    getProviderConfigApiKey: () => Promise.reject(error),
    getReplayTimeline: () => Promise.reject(error),
    getSkillCatalogEntry: () => Promise.reject(error),
    getSkillCatalogFile: () => Promise.reject(error),
    pageConversationTimeline: () => Promise.reject(error),
    pageConversationWorktree: () => Promise.reject(error),
    getSkillDetail: () => Promise.reject(error),
    getSkillFile: () => Promise.reject(error),
    importSkill: () => Promise.reject(error),
    installSkillFromCatalog: () => Promise.reject(error),
    listSkillCatalogInstallTasks: () => Promise.reject(error),
    listenSkillCatalogInstallProgress: () => Promise.reject(error),
    listActivity: () => Promise.reject(error),
    listArtifacts: () => Promise.reject(error),
    listConversations: () => Promise.reject(error),
    listEvalCases: () => Promise.reject(error),
    listModelProviderCatalog: () => Promise.reject(error),
    listMcpDiagnostics: () => Promise.reject(error),
    listMcpServers: () => Promise.reject(error),
    listMemoryItems: () => Promise.reject(error),
    listProviderSettings: () => Promise.reject(error),
    listProjects: () => Promise.reject(error),
    addProject: () => Promise.reject(error),
    switchProject: () => Promise.reject(error),
    deleteProject: () => Promise.reject(error),
    listReferenceCandidates: () => Promise.reject(error),
    listSkillCatalogEntries: () => Promise.reject(error),
    listSkillCatalogSources: () => Promise.reject(error),
    listSkills: () => Promise.reject(error),
    resolvePermission: () => Promise.reject(error),
    requestProviderConfigApiKeyReveal: () => Promise.reject(error),
    runEvalCase: () => Promise.reject(error),
    saveMcpServer: () => Promise.reject(error),
    setMcpServerEnabled: () => Promise.reject(error),
    restartMcpServer: () => Promise.reject(error),
    clearMcpDiagnostics: () => Promise.reject(error),
    saveProviderSettings: () => Promise.reject(error),
    setExecutionSettings: () => Promise.reject(error),
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
    validateProviderSettings: () => Promise.reject(error),
  }
}
