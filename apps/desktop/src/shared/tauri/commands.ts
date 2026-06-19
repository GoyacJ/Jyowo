import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { z } from 'zod'

import { runEventsSchema } from '@/shared/events/run-event-schema'

const appInfoSchema = z
  .object({
    name: z.literal('Jyowo'),
    version: z.string().min(1),
    shell: z.literal('tauri2-react'),
    harness: z
      .object({
        sdkCrate: z.literal('jyowo_harness_sdk'),
        mode: z.literal('in-process'),
      })
      .strict(),
  })
  .strict()

const harnessHealthcheckSchema = z
  .object({
    status: z.literal('available'),
    sdkCrate: z.literal('jyowo_harness_sdk'),
  })
  .strict()

const conversationSummarySchema = z
  .object({
    id: z.string().min(1),
    lastMessagePreview: z.string().optional(),
    title: z.string().min(1),
    updatedAt: z.string().datetime({ offset: true }),
  })
  .strict()

const conversationMessageSchema = z
  .object({
    author: z.enum(['assistant', 'user']),
    body: z.string(),
    id: z.string().min(1),
    timestamp: z.string().datetime({ offset: true }),
  })
  .strict()

const conversationSchema = z
  .object({
    id: z.string().min(1),
    messages: z.array(conversationMessageSchema),
    title: z.string().min(1),
    updatedAt: z.string().datetime({ offset: true }),
  })
  .strict()

const listConversationsResponseSchema = z
  .object({
    conversations: z.array(conversationSummarySchema),
  })
  .strict()

const getConversationRequestSchema = z
  .object({
    conversationId: z.string().min(1),
  })
  .strict()

const getConversationResponseSchema = z
  .object({
    conversation: conversationSchema,
  })
  .strict()

const startRunRequestSchema = z
  .object({
    contextReferences: z.array(z.string().min(1)).optional(),
    conversationId: z.string().min(1),
    prompt: z.string().min(1),
  })
  .strict()

const startRunResponseSchema = z
  .object({
    runId: z.string().min(1),
    status: z.literal('started'),
  })
  .strict()

const cancelRunRequestSchema = z
  .object({
    runId: z.string().min(1),
  })
  .strict()

const cancelRunResponseSchema = z
  .object({
    runId: z.string().min(1),
    status: z.literal('cancelled'),
  })
  .strict()

const resolvePermissionRequestSchema = z
  .object({
    decision: z.enum(['approve', 'deny']),
    requestId: z.string().regex(/^[0-9A-HJKMNP-TV-Z]{26}$/),
  })
  .strict()

const resolvePermissionResponseSchema = z
  .object({
    decision: z.enum(['approve', 'deny']),
    requestId: z.string().min(1),
    status: z.literal('resolved'),
  })
  .strict()

const listActivityRequestSchema = z
  .object({
    conversationId: z.string().min(1),
    runId: z.string().min(1).optional(),
  })
  .strict()

const listActivityResponseSchema = z
  .object({
    events: runEventsSchema,
  })
  .strict()

const replayTimelineRequestSchema = z
  .object({
    conversationId: z.string().min(1),
    runId: z.string().min(1).optional(),
  })
  .strict()

const replayTimelineResponseSchema = z
  .object({
    events: runEventsSchema,
    replayed: z.literal(true),
  })
  .strict()

const exportSupportBundleRequestSchema = replayTimelineRequestSchema
const exportPathSchema = (extension: 'json' | 'jsonl' | 'md') =>
  z.string().regex(new RegExp(`^\\.jyowo/runtime/exports/[^/]+\\.${extension}$`), {
    message: `export path must be a workspace-relative .${extension} file`,
  })

const exportSupportBundleResponseSchema = z
  .object({
    bundlePath: exportPathSchema('json'),
    eventCount: z.number().int().min(0),
    exportedAt: z.string().datetime({ offset: true }),
    jsonlPath: exportPathSchema('jsonl'),
    markdownPath: exportPathSchema('md'),
    redacted: z.literal(true),
  })
  .strict()

const artifactStatusSchema = z.enum(['failed', 'pending', 'ready', 'running'])
const maxArtifactPreviewBytes = 16 * 1024
const artifactPreviewSchema = z
  .string()
  .refine((value) => new TextEncoder().encode(value).byteLength <= maxArtifactPreviewBytes, {
    message: `Artifact preview must be at most ${maxArtifactPreviewBytes} UTF-8 bytes`,
  })

const artifactSummarySchema = z
  .object({
    actionLabel: z.string().min(1),
    description: z.string(),
    id: z.string().min(1),
    kind: z.string().min(1),
    preview: artifactPreviewSchema.optional(),
    sourceMessageId: z.string().min(1).optional(),
    sourceRunId: z.string().min(1),
    status: artifactStatusSchema,
    title: z.string().min(1),
  })
  .strict()

const listArtifactsResponseSchema = z
  .object({
    artifacts: z.array(artifactSummarySchema),
  })
  .strict()

const contextDecisionSchema = z
  .object({
    detail: z.string(),
    title: z.string().min(1),
  })
  .strict()

const contextFileSchema = z
  .object({
    label: z.string().min(1),
    state: z.enum(['missing', 'ready', 'stale']).optional(),
  })
  .strict()

const getContextSnapshotRequestSchema = z
  .object({
    conversationId: z.string().min(1).optional(),
    runId: z.string().min(1).optional(),
  })
  .strict()

const getContextSnapshotResponseSchema = z
  .object({
    activeArtifact: z.string().nullable(),
    decisions: z.array(contextDecisionSchema),
    files: z.array(contextFileSchema),
    nextActions: z.array(z.string().min(1)),
    path: z.string().min(1),
    project: z.string().min(1),
  })
  .strict()

const providerIdSchema = z.enum([
  'anthropic',
  'codex',
  'deepseek',
  'doubao',
  'gemini',
  'local-llama',
  'minimax',
  'openai',
  'openrouter',
  'qwen',
  'zhipu',
])

const providerSettingsRequestSchema = z
  .object({
    apiKey: z.string().trim().min(1),
    modelId: z.string().trim().min(1),
    providerId: providerIdSchema,
  })
  .strict()

const validateProviderSettingsRequestSchema = z
  .object({
    modelId: z.string().trim().min(1),
    providerId: providerIdSchema,
  })
  .strict()

const validateProviderSettingsResponseSchema = z
  .object({
    modelId: z.string().min(1),
    providerId: providerIdSchema,
    status: z.literal('accepted'),
  })
  .strict()

const saveProviderSettingsResponseSchema = z
  .object({
    modelId: z.string().min(1),
    providerId: providerIdSchema,
    secretRef: z.string().min(1),
    status: z.literal('saved'),
  })
  .strict()

const mcpServerIdSchema = z
  .string()
  .trim()
  .regex(/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/)

const mcpServerScopeSchema = z.enum(['agent', 'global', 'session'])

const mcpServerTransportKindSchema = z.enum(['http', 'inProcess', 'sse', 'stdio', 'websocket'])

const mcpServerStatusSchema = z.enum([
  'closed',
  'configured',
  'connecting',
  'failed',
  'ready',
  'reconnecting',
])

const mcpServerOriginSchema = z.enum(['managed', 'plugin', 'policy', 'user', 'workspace'])

const mcpServerSummarySchema = z
  .object({
    displayName: z.string().min(1),
    exposedToolCount: z.number().int().min(0),
    id: mcpServerIdSchema,
    lastError: z.string().min(1).optional(),
    origin: mcpServerOriginSchema,
    scope: mcpServerScopeSchema,
    status: mcpServerStatusSchema,
    transport: mcpServerTransportKindSchema,
  })
  .strict()

const listMcpServersResponseSchema = z
  .object({
    servers: z.array(mcpServerSummarySchema),
  })
  .strict()

const mcpServerTransportRequestSchema = z
  .object({
    args: z.array(z.string()).max(64),
    command: z.string().trim().min(1),
    kind: z.literal('stdio'),
  })
  .strict()

const saveMcpServerRequestSchema = z
  .object({
    displayName: z.string().trim().min(1),
    id: mcpServerIdSchema,
    scope: mcpServerScopeSchema,
    transport: mcpServerTransportRequestSchema,
  })
  .strict()

const saveMcpServerResponseSchema = z
  .object({
    server: mcpServerSummarySchema,
  })
  .strict()

const deleteMcpServerRequestSchema = z
  .object({
    id: mcpServerIdSchema,
  })
  .strict()

const deleteMcpServerResponseSchema = z
  .object({
    id: mcpServerIdSchema,
    status: z.literal('deleted'),
  })
  .strict()

const memoryItemIdSchema = z.string().regex(/^[0-9A-HJKMNP-TV-Z]{26}$/)

const memoryKindSchema = z.enum([
  'agent_self_note',
  'custom',
  'feedback',
  'project_fact',
  'reference',
  'user_preference',
])

const memoryVisibilitySchema = z.enum(['private', 'team', 'tenant', 'user'])

const memorySourceSchema = z.enum([
  'agent_derived',
  'consolidated',
  'external_retrieval',
  'imported',
  'subagent_derived',
  'user_input',
])

const memoryItemSummarySchema = z
  .object({
    contentPreview: z.string(),
    id: memoryItemIdSchema,
    kind: memoryKindSchema,
    source: memorySourceSchema,
    tags: z.array(z.string()),
    updatedAt: z.string().datetime({ offset: true }),
    visibility: memoryVisibilitySchema,
  })
  .strict()

const memoryItemSchema = z
  .object({
    accessCount: z.number().int().min(0),
    confidence: z.number().min(0).max(1),
    content: z.string(),
    createdAt: z.string().datetime({ offset: true }),
    id: memoryItemIdSchema,
    kind: memoryKindSchema,
    source: memorySourceSchema,
    tags: z.array(z.string()),
    updatedAt: z.string().datetime({ offset: true }),
    visibility: memoryVisibilitySchema,
  })
  .strict()

const listMemoryItemsResponseSchema = z
  .object({
    items: z.array(memoryItemSummarySchema),
  })
  .strict()

const getMemoryItemRequestSchema = z
  .object({
    id: memoryItemIdSchema,
  })
  .strict()

const getMemoryItemResponseSchema = z
  .object({
    item: memoryItemSchema,
  })
  .strict()

const updateMemoryItemRequestSchema = z
  .object({
    content: z
      .string()
      .max(64 * 1024)
      .refine((value) => value.trim().length > 0),
    id: memoryItemIdSchema,
  })
  .strict()

const updateMemoryItemResponseSchema = z
  .object({
    item: memoryItemSchema,
  })
  .strict()

const deleteMemoryItemRequestSchema = z
  .object({
    id: memoryItemIdSchema,
  })
  .strict()

const deleteMemoryItemResponseSchema = z
  .object({
    id: memoryItemIdSchema,
    status: z.literal('deleted'),
  })
  .strict()

const exportMemoryItemsResponseSchema = z
  .object({
    exportedAt: z.string().datetime({ offset: true }),
    format: z.literal('json'),
    itemCount: z.number().int().min(0),
    path: z.string().min(1),
  })
  .strict()

const evalRunStatusSchema = z.enum(['failed', 'passed', 'running', 'unavailable'])

const evalCaseIdSchema = z
  .string()
  .trim()
  .regex(/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/)

const evalLastRunSchema = z
  .object({
    completedAt: z.string().datetime({ offset: true }).optional(),
    failed: z.number().int().min(0),
    passed: z.number().int().min(0),
    status: evalRunStatusSchema,
  })
  .strict()

const evalCaseSchema = z
  .object({
    id: evalCaseIdSchema,
    lastRun: evalLastRunSchema.optional(),
    title: z.string().min(1),
  })
  .strict()

const listEvalCasesResponseSchema = z
  .object({
    cases: z.array(evalCaseSchema),
  })
  .strict()

const runEvalCaseRequestSchema = z
  .object({
    caseId: evalCaseIdSchema,
  })
  .strict()

const runEvalCaseResponseSchema = z
  .object({
    case: evalCaseSchema,
    status: z.literal('completed'),
  })
  .strict()

export type AppInfo = z.infer<typeof appInfoSchema>
export type HarnessHealthcheck = z.infer<typeof harnessHealthcheckSchema>
export type ListConversationsResponse = z.infer<typeof listConversationsResponseSchema>
export type GetConversationResponse = z.infer<typeof getConversationResponseSchema>
export type StartRunRequest = z.infer<typeof startRunRequestSchema>
export type StartRunResponse = z.infer<typeof startRunResponseSchema>
export type CancelRunResponse = z.infer<typeof cancelRunResponseSchema>
export type ResolvePermissionRequest = z.infer<typeof resolvePermissionRequestSchema>
export type ResolvePermissionResponse = z.infer<typeof resolvePermissionResponseSchema>
export type ListActivityRequest = z.infer<typeof listActivityRequestSchema>
export type ListActivityResponse = z.infer<typeof listActivityResponseSchema>
export type ReplayTimelineRequest = z.infer<typeof replayTimelineRequestSchema>
export type ReplayTimelineResponse = z.infer<typeof replayTimelineResponseSchema>
export type ExportSupportBundleRequest = z.infer<typeof exportSupportBundleRequestSchema>
export type ExportSupportBundleResponse = z.infer<typeof exportSupportBundleResponseSchema>
export type ListArtifactsResponse = z.infer<typeof listArtifactsResponseSchema>
export type GetContextSnapshotRequest = z.infer<typeof getContextSnapshotRequestSchema>
export type GetContextSnapshotResponse = z.infer<typeof getContextSnapshotResponseSchema>
export type ProviderSettingsRequest = z.infer<typeof providerSettingsRequestSchema>
export type ValidateProviderSettingsRequest = z.infer<typeof validateProviderSettingsRequestSchema>
export type ValidateProviderSettingsResponse = z.infer<
  typeof validateProviderSettingsResponseSchema
>
export type SaveProviderSettingsResponse = z.infer<typeof saveProviderSettingsResponseSchema>
export type McpServerSummary = z.infer<typeof mcpServerSummarySchema>
export type ListMcpServersResponse = z.infer<typeof listMcpServersResponseSchema>
export type SaveMcpServerRequest = z.infer<typeof saveMcpServerRequestSchema>
export type SaveMcpServerResponse = z.infer<typeof saveMcpServerResponseSchema>
export type DeleteMcpServerResponse = z.infer<typeof deleteMcpServerResponseSchema>
export type MemoryItemSummary = z.infer<typeof memoryItemSummarySchema>
export type ListMemoryItemsResponse = z.infer<typeof listMemoryItemsResponseSchema>
export type GetMemoryItemResponse = z.infer<typeof getMemoryItemResponseSchema>
export type UpdateMemoryItemRequest = z.infer<typeof updateMemoryItemRequestSchema>
export type UpdateMemoryItemResponse = z.infer<typeof updateMemoryItemResponseSchema>
export type DeleteMemoryItemResponse = z.infer<typeof deleteMemoryItemResponseSchema>
export type ExportMemoryItemsResponse = z.infer<typeof exportMemoryItemsResponseSchema>
export type ListEvalCasesResponse = z.infer<typeof listEvalCasesResponseSchema>
export type RunEvalCaseResponse = z.infer<typeof runEvalCaseResponseSchema>

export interface CommandClient {
  cancelRun: (runId: string) => Promise<CancelRunResponse>
  deleteMcpServer: (id: string) => Promise<DeleteMcpServerResponse>
  deleteMemoryItem: (id: string) => Promise<DeleteMemoryItemResponse>
  getContextSnapshot: (request: GetContextSnapshotRequest) => Promise<GetContextSnapshotResponse>
  getConversation: (conversationId: string) => Promise<GetConversationResponse>
  getAppInfo: () => Promise<AppInfo>
  getHarnessHealthcheck: () => Promise<HarnessHealthcheck>
  getMemoryItem: (id: string) => Promise<GetMemoryItemResponse>
  getReplayTimeline: (request: ReplayTimelineRequest) => Promise<ReplayTimelineResponse>
  exportMemoryItems: () => Promise<ExportMemoryItemsResponse>
  exportSupportBundle: (request: ExportSupportBundleRequest) => Promise<ExportSupportBundleResponse>
  listActivity: (request: ListActivityRequest) => Promise<ListActivityResponse>
  listArtifacts: () => Promise<ListArtifactsResponse>
  listConversations: () => Promise<ListConversationsResponse>
  listEvalCases: () => Promise<ListEvalCasesResponse>
  listMcpServers: () => Promise<ListMcpServersResponse>
  listMemoryItems: () => Promise<ListMemoryItemsResponse>
  resolvePermission: (request: ResolvePermissionRequest) => Promise<ResolvePermissionResponse>
  runEvalCase: (caseId: string) => Promise<RunEvalCaseResponse>
  saveMcpServer: (request: SaveMcpServerRequest) => Promise<SaveMcpServerResponse>
  saveProviderSettings: (request: ProviderSettingsRequest) => Promise<SaveProviderSettingsResponse>
  startRun: (request: StartRunRequest) => Promise<StartRunResponse>
  updateMemoryItem: (request: UpdateMemoryItemRequest) => Promise<UpdateMemoryItemResponse>
  validateProviderSettings: (
    request: ValidateProviderSettingsRequest,
  ) => Promise<ValidateProviderSettingsResponse>
}

export type InvokeCommand = (command: string, args?: Record<string, unknown>) => Promise<unknown>

export class TauriCommandPayloadError extends Error {
  readonly command: string

  constructor(command: string, cause: unknown) {
    super(`Invalid Tauri command payload: ${command}`, { cause })
    this.name = 'TauriCommandPayloadError'
    this.command = command
  }
}

function parsePayload<T>(command: string, schema: z.ZodType<T>, payload: unknown): T {
  const result = schema.safeParse(payload)

  if (!result.success) {
    throw new TauriCommandPayloadError(command, result.error)
  }

  return result.data
}

function parseArgs<T>(command: string, schema: z.ZodType<T>, args: unknown): T {
  return parsePayload(`${command} args`, schema, args)
}

export function createInvokeCommandClient(invoke: InvokeCommand = tauriInvoke): CommandClient {
  return {
    async cancelRun(runId) {
      const command = 'cancel_run'
      const args = parseArgs(command, cancelRunRequestSchema, { runId })
      return parsePayload(command, cancelRunResponseSchema, await invoke(command, args))
    },
    async deleteMcpServer(id) {
      const command = 'delete_mcp_server'
      const args = parseArgs(command, deleteMcpServerRequestSchema, { id })
      return parsePayload(command, deleteMcpServerResponseSchema, await invoke(command, args))
    },
    async deleteMemoryItem(id) {
      const command = 'delete_memory_item'
      const args = parseArgs(command, deleteMemoryItemRequestSchema, { id })
      return parsePayload(command, deleteMemoryItemResponseSchema, await invoke(command, args))
    },
    async exportMemoryItems() {
      const command = 'export_memory_items'
      return parsePayload(command, exportMemoryItemsResponseSchema, await invoke(command))
    },
    async exportSupportBundle(request) {
      const command = 'export_support_bundle'
      const args = parseArgs(command, exportSupportBundleRequestSchema, request)
      return parsePayload(command, exportSupportBundleResponseSchema, await invoke(command, args))
    },
    async getContextSnapshot(request) {
      const command = 'get_context_snapshot'
      const args = parseArgs(command, getContextSnapshotRequestSchema, request)
      return parsePayload(command, getContextSnapshotResponseSchema, await invoke(command, args))
    },
    async getConversation(conversationId) {
      const command = 'get_conversation'
      const args = parseArgs(command, getConversationRequestSchema, {
        conversationId,
      })
      return parsePayload(command, getConversationResponseSchema, await invoke(command, args))
    },
    async getAppInfo() {
      const command = 'get_app_info'
      return parsePayload(command, appInfoSchema, await invoke(command))
    },
    async getHarnessHealthcheck() {
      const command = 'harness_healthcheck'
      return parsePayload(command, harnessHealthcheckSchema, await invoke(command))
    },
    async getMemoryItem(id) {
      const command = 'get_memory_item'
      const args = parseArgs(command, getMemoryItemRequestSchema, { id })
      return parsePayload(command, getMemoryItemResponseSchema, await invoke(command, args))
    },
    async getReplayTimeline(request) {
      const command = 'get_replay_timeline'
      const args = parseArgs(command, replayTimelineRequestSchema, request)
      return parsePayload(command, replayTimelineResponseSchema, await invoke(command, args))
    },
    async listActivity(request) {
      const command = 'list_activity'
      const args = parseArgs(command, listActivityRequestSchema, request)
      return parsePayload(command, listActivityResponseSchema, await invoke(command, args))
    },
    async listArtifacts() {
      const command = 'list_artifacts'
      return parsePayload(command, listArtifactsResponseSchema, await invoke(command))
    },
    async listConversations() {
      const command = 'list_conversations'
      return parsePayload(command, listConversationsResponseSchema, await invoke(command))
    },
    async listEvalCases() {
      const command = 'list_eval_cases'
      return parsePayload(command, listEvalCasesResponseSchema, await invoke(command))
    },
    async listMcpServers() {
      const command = 'list_mcp_servers'
      return parsePayload(command, listMcpServersResponseSchema, await invoke(command))
    },
    async listMemoryItems() {
      const command = 'list_memory_items'
      return parsePayload(command, listMemoryItemsResponseSchema, await invoke(command))
    },
    async resolvePermission(request) {
      const command = 'resolve_permission'
      const args = parseArgs(command, resolvePermissionRequestSchema, request)
      return parsePayload(command, resolvePermissionResponseSchema, await invoke(command, args))
    },
    async runEvalCase(caseId) {
      const command = 'run_eval_case'
      const args = parseArgs(command, runEvalCaseRequestSchema, { caseId })
      return parsePayload(command, runEvalCaseResponseSchema, await invoke(command, args))
    },
    async saveProviderSettings(request) {
      const command = 'save_provider_settings'
      const args = parseArgs(command, providerSettingsRequestSchema, request)
      return parsePayload(command, saveProviderSettingsResponseSchema, await invoke(command, args))
    },
    async saveMcpServer(request) {
      const command = 'save_mcp_server'
      const args = parseArgs(command, saveMcpServerRequestSchema, request)
      return parsePayload(command, saveMcpServerResponseSchema, await invoke(command, args))
    },
    async startRun(request) {
      const command = 'start_run'
      const args = parseArgs(command, startRunRequestSchema, request)
      return parsePayload(command, startRunResponseSchema, await invoke(command, args))
    },
    async updateMemoryItem(request) {
      const command = 'update_memory_item'
      const args = parseArgs(command, updateMemoryItemRequestSchema, request)
      return parsePayload(command, updateMemoryItemResponseSchema, await invoke(command, args))
    },
    async validateProviderSettings(request) {
      const command = 'validate_provider_settings'
      const args = parseArgs(command, validateProviderSettingsRequestSchema, request)
      return parsePayload(
        command,
        validateProviderSettingsResponseSchema,
        await invoke(command, args),
      )
    },
  }
}

export const tauriCommandClient = createInvokeCommandClient()

export function getAppInfo(client: CommandClient = tauriCommandClient): Promise<AppInfo> {
  return client.getAppInfo()
}

export function getHarnessHealthcheck(
  client: CommandClient = tauriCommandClient,
): Promise<HarnessHealthcheck> {
  return client.getHarnessHealthcheck()
}

export function listConversations(
  client: CommandClient = tauriCommandClient,
): Promise<ListConversationsResponse> {
  return client.listConversations()
}

export function listEvalCases(
  client: CommandClient = tauriCommandClient,
): Promise<ListEvalCasesResponse> {
  return client.listEvalCases()
}

export function listArtifacts(
  client: CommandClient = tauriCommandClient,
): Promise<ListArtifactsResponse> {
  return client.listArtifacts()
}

export function getConversation(
  conversationId: string,
  client: CommandClient = tauriCommandClient,
): Promise<GetConversationResponse> {
  return client.getConversation(conversationId)
}

export function startRun(
  request: StartRunRequest,
  client: CommandClient = tauriCommandClient,
): Promise<StartRunResponse> {
  return client.startRun(request)
}

export function cancelRun(
  runId: string,
  client: CommandClient = tauriCommandClient,
): Promise<CancelRunResponse> {
  return client.cancelRun(runId)
}

export function resolvePermission(
  request: ResolvePermissionRequest,
  client: CommandClient = tauriCommandClient,
): Promise<ResolvePermissionResponse> {
  return client.resolvePermission(request)
}

export function runEvalCase(
  caseId: string,
  client: CommandClient = tauriCommandClient,
): Promise<RunEvalCaseResponse> {
  return client.runEvalCase(caseId)
}

export function listMcpServers(
  client: CommandClient = tauriCommandClient,
): Promise<ListMcpServersResponse> {
  return client.listMcpServers()
}

export function saveMcpServer(
  request: SaveMcpServerRequest,
  client: CommandClient = tauriCommandClient,
): Promise<SaveMcpServerResponse> {
  return client.saveMcpServer(request)
}

export function deleteMcpServer(
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<DeleteMcpServerResponse> {
  return client.deleteMcpServer(id)
}

export function listMemoryItems(
  client: CommandClient = tauriCommandClient,
): Promise<ListMemoryItemsResponse> {
  return client.listMemoryItems()
}

export function getMemoryItem(
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<GetMemoryItemResponse> {
  return client.getMemoryItem(id)
}

export function updateMemoryItem(
  request: UpdateMemoryItemRequest,
  client: CommandClient = tauriCommandClient,
): Promise<UpdateMemoryItemResponse> {
  return client.updateMemoryItem(request)
}

export function deleteMemoryItem(
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<DeleteMemoryItemResponse> {
  return client.deleteMemoryItem(id)
}

export function exportMemoryItems(
  client: CommandClient = tauriCommandClient,
): Promise<ExportMemoryItemsResponse> {
  return client.exportMemoryItems()
}

export function exportSupportBundle(
  request: ExportSupportBundleRequest,
  client: CommandClient = tauriCommandClient,
): Promise<ExportSupportBundleResponse> {
  return client.exportSupportBundle(request)
}

export function saveProviderSettings(
  request: ProviderSettingsRequest,
  client: CommandClient = tauriCommandClient,
): Promise<SaveProviderSettingsResponse> {
  return client.saveProviderSettings(request)
}

export function validateProviderSettings(
  request: ValidateProviderSettingsRequest,
  client: CommandClient = tauriCommandClient,
): Promise<ValidateProviderSettingsResponse> {
  return client.validateProviderSettings(request)
}

export function listActivity(
  request: ListActivityRequest,
  client: CommandClient = tauriCommandClient,
): Promise<ListActivityResponse> {
  return client.listActivity(request)
}

export function getReplayTimeline(
  request: ReplayTimelineRequest,
  client: CommandClient = tauriCommandClient,
): Promise<ReplayTimelineResponse> {
  return client.getReplayTimeline(request)
}

export function getContextSnapshot(
  request: GetContextSnapshotRequest,
  client: CommandClient = tauriCommandClient,
): Promise<GetContextSnapshotResponse> {
  return client.getContextSnapshot(request)
}
