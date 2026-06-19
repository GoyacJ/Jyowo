import type {
  AppInfo,
  CancelRunResponse,
  CommandClient,
  ExportMemoryItemsResponse,
  ExportSupportBundleResponse,
  GetContextSnapshotResponse,
  GetConversationResponse,
  GetMemoryItemResponse,
  HarnessHealthcheck,
  ListActivityResponse,
  ListArtifactsResponse,
  ListConversationsResponse,
  ListEvalCasesResponse,
  ListMcpServersResponse,
  ListMemoryItemsResponse,
  ReplayTimelineResponse,
  ResolvePermissionResponse,
  RunEvalCaseResponse,
  SaveMcpServerResponse,
  SaveProviderSettingsResponse,
  StartRunResponse,
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
          "I'll set up the foundation with a clean project structure, dev scripts, and base app shell.",
          '<!-- jyowo-plan: Initialize project & dependencies -->',
          '<!-- jyowo-plan: Configure Electron main process -->',
          '<!-- jyowo-plan: Set up React + TypeScript (Vite) -->',
          '<!-- jyowo-plan: Add base app shell & IPC bridge -->',
          '<!-- jyowo-plan: Add scripts, README, and .gitignore -->',
        ].join('\n'),
        id: 'message-002',
        timestamp,
      },
    ],
    title: 'Build the desktop foundation',
    updatedAt: timestamp,
  },
}

const mockListActivity: ListActivityResponse = {
  events: [
    {
      id: 'evt-001',
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
      sourceMessageId: 'message-002',
      sourceRunId: 'run-001',
      status: 'ready',
      title: 'Desktop foundation created',
    },
    {
      actionLabel: 'Inspect',
      description: 'Follow-up verification checklist',
      id: 'artifact-verification-notes',
      kind: 'markdown',
      sourceMessageId: 'message-002',
      sourceRunId: 'run-001',
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
      sourceMessageId: 'message-002',
      sourceRunId: 'run-001',
      status: 'ready',
      title: 'src/main/main.ts',
    },
  ],
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

const mockSaveProviderSettings: SaveProviderSettingsResponse = {
  modelId: 'gpt-4o-mini',
  providerId: 'openai',
  secretRef: 'provider/workspace-local/openai/default',
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
  contextSnapshot?: GetContextSnapshotResponse
  conversation?: GetConversationResponse
  conversations?: ListConversationsResponse
  healthcheck?: HarnessHealthcheck
  artifacts?: ListArtifactsResponse
  listActivity?: ListActivityResponse
  memoryExport?: ExportMemoryItemsResponse
  evalCases?: ListEvalCasesResponse
  memoryItem?: GetMemoryItemResponse
  memoryItems?: ListMemoryItemsResponse
  mcpServer?: SaveMcpServerResponse
  mcpServers?: ListMcpServersResponse
  providerSettings?: SaveProviderSettingsResponse
  providerValidation?: ValidateProviderSettingsResponse
  replayTimeline?: ReplayTimelineResponse
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
  return {
    async cancelRun(runId) {
      await wait(options.delayMs)
      return { runId, status: 'cancelled' } satisfies CancelRunResponse
    },
    async deleteMcpServer(id) {
      await wait(options.delayMs)
      return { id, status: 'deleted' }
    },
    async deleteMemoryItem(id) {
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
    async getReplayTimeline() {
      await wait(options.delayMs)
      return options.replayTimeline ?? mockReplayTimeline
    },
    async listActivity() {
      await wait(options.delayMs)
      return options.listActivity ?? mockListActivity
    },
    async listArtifacts() {
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
    async listMcpServers() {
      await wait(options.delayMs)
      return options.mcpServers ?? mockListMcpServers
    },
    async listMemoryItems() {
      await wait(options.delayMs)
      return options.memoryItems ?? mockMemoryItems
    },
    async resolvePermission(request) {
      await wait(options.delayMs)
      return {
        ...request,
        status: 'resolved',
      } satisfies ResolvePermissionResponse
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
    async saveMcpServer() {
      await wait(options.delayMs)
      return options.mcpServer ?? mockSaveMcpServer
    },
    async startRun() {
      await wait(options.delayMs)
      return { runId: 'run-001', status: 'started' } satisfies StartRunResponse
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

export function createRejectedCommandClient(error: Error): CommandClient {
  return {
    cancelRun: () => Promise.reject(error),
    deleteMcpServer: () => Promise.reject(error),
    deleteMemoryItem: () => Promise.reject(error),
    exportMemoryItems: () => Promise.reject(error),
    exportSupportBundle: () => Promise.reject(error),
    getContextSnapshot: () => Promise.reject(error),
    getConversation: () => Promise.reject(error),
    getAppInfo: () => Promise.reject(error),
    getHarnessHealthcheck: () => Promise.reject(error),
    getMemoryItem: () => Promise.reject(error),
    getReplayTimeline: () => Promise.reject(error),
    listActivity: () => Promise.reject(error),
    listArtifacts: () => Promise.reject(error),
    listConversations: () => Promise.reject(error),
    listEvalCases: () => Promise.reject(error),
    listMcpServers: () => Promise.reject(error),
    listMemoryItems: () => Promise.reject(error),
    resolvePermission: () => Promise.reject(error),
    runEvalCase: () => Promise.reject(error),
    saveMcpServer: () => Promise.reject(error),
    saveProviderSettings: () => Promise.reject(error),
    startRun: () => Promise.reject(error),
    updateMemoryItem: () => Promise.reject(error),
    validateProviderSettings: () => Promise.reject(error),
  }
}
