import type { RunEvent } from '@/shared/events/run-event-schema'
import type {
  AppInfo,
  CancelRunResponse,
  CommandClient,
  ConversationEventBatchPayload,
  ConversationModelCapability,
  CreateAttachmentFromPathResponse,
  CreateConversationResponse,
  DeleteConversationResponse,
  ExportMemoryItemsResponse,
  ExportSupportBundleResponse,
  GetContextSnapshotResponse,
  GetConversationResponse,
  GetExecutionSettingsResponse,
  GetMemoryItemResponse,
  GetProviderConfigApiKeyResponse,
  GetSkillDetailResponse,
  GetSkillFileResponse,
  HarnessHealthcheck,
  ListActivityResponse,
  ListArtifactsResponse,
  ListConversationsResponse,
  ListEvalCasesResponse,
  ListMcpServersResponse,
  ListMemoryItemsResponse,
  ListProjectsResponse,
  ListProviderSettingsResponse,
  ListReferenceCandidatesResponse,
  ListSkillsResponse,
  ModelProviderCatalogResponse,
  PageConversationTimelineResponse,
  ReplayTimelineResponse,
  RequestProviderConfigApiKeyRevealResponse,
  ResolvePermissionResponse,
  RunEvalCaseResponse,
  SaveMcpServerResponse,
  SaveProviderSettingsResponse,
  SetConversationModelConfigResponse,
  SetExecutionSettingsResponse,
  SkillSummary,
  StartRunResponse,
  SubscribeConversationEventsResponse,
  SwitchProjectResponse,
  UnsubscribeConversationEventsResponse,
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
        supportsLiveValidation: true,
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
      exposedToolCount: 2,
      id: 'github',
      origin: 'workspace',
      scope: 'global',
      status: 'ready',
      transport: 'stdio',
    },
  ],
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

const mockSaveMcpServer: SaveMcpServerResponse = {
  server: {
    displayName: 'Workspace GitHub',
    exposedToolCount: 0,
    id: 'github',
    origin: 'workspace',
    scope: 'global',
    status: 'configured',
    transport: 'stdio',
  },
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
  listActivity?: ListActivityResponse
  memoryExport?: ExportMemoryItemsResponse
  evalCases?: ListEvalCasesResponse
  memoryItem?: GetMemoryItemResponse
  memoryItems?: ListMemoryItemsResponse
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
  subscribeConversationEvents?: SubscribeConversationEventsResponse
  skillDetail?: GetSkillDetailResponse
  skillFile?: GetSkillFileResponse
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

export function createMockCommandClient(options: MockCommandClientOptions = {}): CommandClient {
  let batchListener: ((batch: ConversationEventBatchPayload) => void) | null = null
  let activeSubscription: SubscribeConversationEventsResponse | null = null
  let subscriptionCounter = 0
  let completionBatchFlushed: Promise<void> = Promise.resolve()
  let projects = options.projects ?? mockJyowoProject
  const pendingBatchTimeouts = new Map<number, () => void>()
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
      return {
        conversation: (options.conversations ?? mockListConversations).conversations[0],
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
    async getConversation() {
      await wait(options.delayMs)
      return options.conversation ?? mockConversation
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
    async getProviderConfigApiKey() {
      await wait(options.delayMs)
      if (options.providerConfigApiKey) {
        return options.providerConfigApiKey
      }
      throw new Error('provider API key reveal is disabled')
    },
    async getReplayTimeline() {
      await wait(options.delayMs)
      return options.replayTimeline ?? mockReplayTimeline
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
      return options.conversations ?? mockListConversations
    },
    async listEvalCases() {
      await wait(options.delayMs)
      return options.evalCases ?? mockListEvalCases
    },
    async listModelProviderCatalog() {
      await wait(options.delayMs)
      return options.modelProviderCatalog ?? mockModelProviderCatalog
    },
    async listMcpServers() {
      await wait(options.delayMs)
      return options.mcpServers ?? mockListMcpServers
    },
    async listMemoryItems() {
      await wait(options.delayMs)
      return options.memoryItems ?? mockMemoryItems
    },
    async listProviderSettings() {
      await wait(options.delayMs)
      return options.providerSettingsList ?? mockProviderSettingsList
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
    async listReferenceCandidates(_request) {
      await wait(options.delayMs)
      return options.referenceCandidates ?? mockReferenceCandidates
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
    async requestProviderConfigApiKeyReveal() {
      await wait(options.delayMs)
      if (options.providerConfigApiKeyReveal) {
        return options.providerConfigApiKeyReveal
      }
      throw new Error('provider API key reveal is disabled')
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
      return options.providerSettings ?? mockSaveProviderSettings
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
          { text: 'Drafting the implementation plan.' },
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
    getAppInfo: () => Promise.reject(error),
    getHarnessHealthcheck: () => Promise.reject(error),
    getMemoryItem: () => Promise.reject(error),
    getProviderConfigApiKey: () => Promise.reject(error),
    getReplayTimeline: () => Promise.reject(error),
    pageConversationTimeline: () => Promise.reject(error),
    getSkillDetail: () => Promise.reject(error),
    getSkillFile: () => Promise.reject(error),
    importSkill: () => Promise.reject(error),
    listActivity: () => Promise.reject(error),
    listArtifacts: () => Promise.reject(error),
    listConversations: () => Promise.reject(error),
    listEvalCases: () => Promise.reject(error),
    listModelProviderCatalog: () => Promise.reject(error),
    listMcpServers: () => Promise.reject(error),
    listMemoryItems: () => Promise.reject(error),
    listProviderSettings: () => Promise.reject(error),
    listProjects: () => Promise.reject(error),
    addProject: () => Promise.reject(error),
    switchProject: () => Promise.reject(error),
    listReferenceCandidates: () => Promise.reject(error),
    listSkills: () => Promise.reject(error),
    resolvePermission: () => Promise.reject(error),
    requestProviderConfigApiKeyReveal: () => Promise.reject(error),
    runEvalCase: () => Promise.reject(error),
    saveMcpServer: () => Promise.reject(error),
    saveProviderSettings: () => Promise.reject(error),
    setExecutionSettings: () => Promise.reject(error),
    setConversationModelConfig: () => Promise.reject(error),
    setSkillEnabled: () => Promise.reject(error),
    startRun: () => Promise.reject(error),
    subscribeConversationEvents: () => Promise.reject(error),
    listenConversationEventBatches: () => Promise.reject(error),
    unsubscribeConversationEvents: () => Promise.reject(error),
    updateMemoryItem: () => Promise.reject(error),
    validateProviderSettings: () => Promise.reject(error),
  }
}
