import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { listen as tauriListen } from '@tauri-apps/api/event'
import { z } from 'zod'

import { runEventsSchema } from '@/shared/events/run-event-schema'

const uuidV4Pattern = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
const unredactedSecretPatterns = [
  /\bAuthorization:?\s*Bearer\s+\S+/i,
  /\bAuthorization:?\s*Basic\s+\S+/i,
  /\bBasic\s+[A-Za-z0-9+/=]{8,}\b/,
  /\b(?:api[_-]?key|token|secret|password)\b\s*(?:=|\s+)\s*\S+/i,
  /\b--(?:api-key|token|secret|password)\b\s+\S+/i,
  /\b[A-Za-z0-9_]*(?:API_KEY|TOKEN|SECRET|PASSWORD|ACCESS_KEY)[A-Za-z0-9_]*\s*=\s*\S+/i,
  /\b[A-Za-z0-9_]*(?:API_KEY|TOKEN|SECRET|PASSWORD|ACCESS_KEY)[A-Za-z0-9_]*\s+\S+/i,
  /\bsk-[A-Za-z0-9]{12,}/i,
  /\bgh[pousr]_[A-Za-z0-9_]{20,}/i,
  /\bAKIA[0-9A-Z]{16}\b/,
  /\bAIza[0-9A-Za-z_-]{30,}\b/,
  /\bxox[baprs]-[0-9A-Za-z-]{20,}\b/,
  /\b(?:rk|sk)_(?:live|test)_[0-9A-Za-z]{12,}\b/i,
  /\bnpm_[0-9A-Za-z]{20,}\b/,
  /\blin_api_[0-9A-Za-z]{20,}\b/,
  /\bsecret_[0-9A-Za-z]{20,}\b/,
  /\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{6,}\b/,
]

function hasObviousUnredactedSecret(value: string): boolean {
  return unredactedSecretPatterns.some((pattern) => pattern.test(value))
}

function hasPrivateAbsolutePath(value: string): boolean {
  return /(?:\/Users\/|\/home\/|\/private\/var\/|[A-Za-z]:\\)/.test(value)
}

const conversationDisplayTextSchema = z
  .string()
  .refine((value) => !hasObviousUnredactedSecret(value), {
    message: 'conversation message body must not contain obvious unredacted secrets',
  })
  .refine((value) => !hasPrivateAbsolutePath(value), {
    message: 'conversation message body must not contain private absolute paths',
  })

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
    isEmpty: z.boolean(),
    lastMessagePreview: conversationDisplayTextSchema.optional(),
    title: conversationDisplayTextSchema.min(1),
    updatedAt: z.string().datetime({ offset: true }),
  })
  .strict()

const conversationMessageSchema = z
  .object({
    author: z.enum(['assistant', 'user']),
    body: conversationDisplayTextSchema,
    clientMessageId: z.uuid().regex(uuidV4Pattern).optional(),
    id: z.string().min(1),
    timestamp: z.string().datetime({ offset: true }),
  })
  .strict()

const conversationSchema = z
  .object({
    id: z.string().min(1),
    messages: z.array(conversationMessageSchema),
    modelConfigId: z.string().min(1).nullable(),
    title: conversationDisplayTextSchema.min(1),
    updatedAt: z.string().datetime({ offset: true }),
  })
  .strict()

const listConversationsResponseSchema = z
  .object({
    conversations: z.array(conversationSummarySchema),
  })
  .strict()

const createConversationResponseSchema = z
  .object({
    conversation: conversationSummarySchema,
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

const deleteConversationRequestSchema = z
  .object({
    conversationId: z.string().min(1),
  })
  .strict()

const deleteConversationResponseSchema = z
  .object({
    conversationId: z.string().min(1),
    status: z.literal('deleted'),
  })
  .strict()

const contextReferenceSchema = z.discriminatedUnion('kind', [
  z
    .object({
      kind: z.literal('workspace_file'),
      label: z.string().trim().min(1),
      path: z.string().trim().min(1),
    })
    .strict(),
  z
    .object({
      id: z.string().trim().min(1),
      kind: z.literal('artifact'),
      label: z.string().trim().min(1),
    })
    .strict(),
  z
    .object({
      id: z.string().trim().min(1),
      kind: z.literal('conversation'),
      label: z.string().trim().min(1),
    })
    .strict(),
  z
    .object({
      id: z.string().trim().min(1),
      kind: z.literal('memory'),
      label: z.string().trim().min(1),
    })
    .strict(),
  z
    .object({
      id: z.string().trim().min(1),
      kind: z.literal('skill'),
      label: z.string().trim().min(1),
    })
    .strict(),
  z
    .object({
      id: z.string().trim().min(1),
      kind: z.literal('tool'),
      label: z.string().trim().min(1),
    })
    .strict(),
  z
    .object({
      id: z.string().trim().min(1),
      kind: z.literal('mcp_server'),
      label: z.string().trim().min(1),
    })
    .strict(),
])

const attachmentReferenceSchema = z
  .object({
    blobRef: z
      .object({
        contentHash: z.array(z.number().int().min(0).max(255)).length(32),
        contentType: z.string().trim().min(1).nullable().optional(),
        id: z.string().trim().min(1),
        size: z.number().int().min(0),
      })
      .strict(),
    id: z
      .string()
      .trim()
      .regex(/^attachment-[0-9a-fA-F]{64}$/),
    mimeType: z.string().trim().min(1),
    name: z.string().trim().min(1),
    sizeBytes: z.number().int().min(0),
  })
  .strict()

const startRunRequestSchema = z
  .object({
    attachments: z.array(attachmentReferenceSchema).optional(),
    clientMessageId: z.uuid().regex(uuidV4Pattern).optional(),
    conversationId: z.string().min(1),
    contextReferences: z.array(contextReferenceSchema).optional(),
    prompt: z.string().min(1),
  })
  .strict()

const startRunResponseSchema = z
  .object({
    runId: z.string().min(1),
    status: z.literal('started'),
  })
  .strict()

const createAttachmentFromPathRequestSchema = z
  .object({
    path: z.string().trim().min(1),
  })
  .strict()

const createAttachmentFromPathResponseSchema = z
  .object({
    attachment: attachmentReferenceSchema,
  })
  .strict()

const referenceCandidateSchema = z
  .object({
    id: z.string().min(1).optional(),
    label: z.string().min(1),
    path: z.string().min(1).optional(),
  })
  .strict()

const listReferenceCandidatesResponseSchema = z
  .object({
    artifacts: z.array(referenceCandidateSchema),
    conversations: z.array(referenceCandidateSchema),
    files: z.array(referenceCandidateSchema),
    memories: z.array(referenceCandidateSchema),
    mcpServers: z.array(referenceCandidateSchema),
    skills: z.array(referenceCandidateSchema),
    tools: z.array(referenceCandidateSchema),
  })
  .strict()

const listReferenceCandidatesRequestSchema = z
  .object({
    conversationId: z.string().min(1),
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
    conversationId: z.string().min(1),
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

const conversationCursorSchema = z
  .object({
    eventId: z.string().regex(/^[0-9A-HJKMNP-TV-Z]{26}$/),
    conversationSequence: z.number().int().nonnegative(),
  })
  .strict()

const pageConversationTimelineRequestSchema = z
  .object({
    afterCursor: conversationCursorSchema.optional(),
    conversationId: z.string().min(1),
    limit: z.number().int().positive().max(200).optional(),
  })
  .strict()

const pageConversationTimelineResponseSchema = z
  .object({
    events: runEventsSchema,
    cursor: conversationCursorSchema.optional(),
    gap: z.boolean(),
  })
  .strict()

const subscribeConversationEventsRequestSchema = z
  .object({
    afterCursor: conversationCursorSchema.optional(),
    conversationId: z.string().min(1),
  })
  .strict()

const subscribeConversationEventsResponseSchema = z
  .object({
    subscriptionId: z.string().min(1),
    conversationId: z.string().min(1),
    replayEvents: runEventsSchema,
    cursor: conversationCursorSchema.optional(),
    gap: z.boolean(),
  })
  .strict()

const unsubscribeConversationEventsRequestSchema = z
  .object({
    subscriptionId: z.string().min(1),
  })
  .strict()

const unsubscribeConversationEventsResponseSchema = z
  .object({
    subscriptionId: z.string().min(1),
    status: z.enum(['unsubscribed', 'alreadyClosed']),
  })
  .strict()

const conversationEventBatchPayloadSchema = z
  .object({
    subscriptionId: z.string().min(1),
    conversationId: z.string().min(1),
    events: runEventsSchema,
    cursor: conversationCursorSchema.optional(),
    gap: z.boolean(),
    phase: z.enum(['replay', 'live']),
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
    status: artifactStatusSchema,
    title: z.string().min(1),
  })
  .strict()

const listArtifactsRequestSchema = z
  .object({
    conversationId: z.string().min(1),
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
    requestId: z
      .string()
      .regex(/^[0-9A-HJKMNP-TV-Z]{26}$/)
      .optional(),
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

const providerIdSchema = z.string().trim().min(1)

const modelProtocolSchema = z.enum([
  'chat_completions',
  'responses',
  'messages',
  'generate_content',
])

const modelLifecycleSchema = z
  .object({
    kind: z.enum(['stable', 'preview', 'deprecated']),
    retirementDate: z.string().min(1).optional(),
  })
  .strict()

const modelModalitySchema = z.enum(['text', 'image', 'audio', 'video', 'file', 'embedding'])

const modelRuntimeStatusSchema = z
  .object({
    kind: z.enum(['runnable', 'unsupported']),
    reason: z.string().min(1).optional(),
  })
  .strict()

const conversationModelCapabilitySchema = z
  .object({
    inputModalities: z.array(modelModalitySchema),
    outputModalities: z.array(modelModalitySchema),
    contextWindow: z.number().int().nonnegative(),
    maxOutputTokens: z.number().int().nonnegative(),
    streaming: z.boolean(),
    toolCalling: z.boolean(),
    reasoning: z.boolean(),
    promptCache: z.boolean(),
    structuredOutput: z.boolean(),
  })
  .strict()

const providerRuntimeCapabilitySchema = z
  .object({
    authScheme: z.enum(['none', 'bearer', 'api_key', 'x_api_key']),
    baseUrlRegions: z.array(
      z
        .object({
          id: z.string().min(1),
          label: z.string().min(1),
          baseUrl: z.string().min(1),
        })
        .strict(),
    ),
    supportsLiveValidation: z.boolean(),
    supportsStreamingValidation: z.boolean(),
    secretRevealSupported: z.boolean(),
  })
  .strict()

const providerServiceCapabilitySchema = z
  .object({
    operationId: z.string().min(1),
    category: z.enum(['conversation', 'image', 'video', 'audio', 'music', 'file', 'model']),
    inputModalities: z.array(modelModalitySchema),
    outputArtifact: z.enum(['text', 'image', 'audio', 'video', 'file', 'embedding']),
    execution: z.enum(['sync', 'async_job', 'websocket']),
    requiresPolling: z.boolean(),
    permissionSubject: z.string().min(1),
    costRisk: z.enum(['low', 'medium', 'high']),
  })
  .strict()

const providerSettingsRequestSchema = z
  .object({
    apiKey: z.string().trim().min(1).optional(),
    baseUrl: z.string().trim().min(1).optional(),
    configId: z.string().trim().min(1).optional(),
    displayName: z.string().trim().min(1).optional(),
    modelId: z.string().trim().min(1),
    providerId: providerIdSchema,
    setDefault: z.boolean().optional(),
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

const modelCatalogEntrySchema = z
  .object({
    protocol: modelProtocolSchema,
    conversationCapability: conversationModelCapabilitySchema,
    contextWindow: z.number().int().nonnegative(),
    displayName: z.string().min(1),
    lifecycle: modelLifecycleSchema,
    maxOutputTokens: z.number().int().nonnegative(),
    modelId: z.string().min(1),
    runtimeStatus: modelRuntimeStatusSchema,
  })
  .strict()

const modelProviderCatalogEntrySchema = z
  .object({
    defaultBaseUrl: z.string().min(1),
    displayName: z.string().min(1),
    models: z.array(modelCatalogEntrySchema),
    providerId: providerIdSchema,
    runtimeCapability: providerRuntimeCapabilitySchema,
    serviceCapabilities: z.array(providerServiceCapabilitySchema),
    sourceUrl: z.string().min(1),
    verifiedDate: z.string().min(1),
  })
  .strict()

const modelProviderCatalogResponseSchema = z
  .object({
    providers: z.array(modelProviderCatalogEntrySchema),
  })
  .strict()

const providerConfigSchema = z
  .object({
    protocol: modelProtocolSchema,
    baseUrl: z.string().min(1).optional(),
    displayName: z.string().min(1),
    hasApiKey: z.boolean(),
    id: z.string().min(1),
    isDefault: z.boolean(),
    modelId: z.string().min(1),
    modelDescriptor: modelCatalogEntrySchema,
    providerId: providerIdSchema,
  })
  .strict()

const listProviderSettingsResponseSchema = z
  .object({
    defaultConfigId: z.string().min(1).nullable(),
    configs: z.array(providerConfigSchema),
  })
  .strict()

const saveProviderSettingsResponseSchema = z
  .object({
    config: providerConfigSchema,
    status: z.literal('saved'),
  })
  .strict()

const permissionModeSchema = z.enum(['default', 'auto', 'bypass_permissions'])

const getExecutionSettingsResponseSchema = z
  .object({
    autoModeAvailable: z.boolean(),
    permissionMode: permissionModeSchema,
  })
  .strict()

const setExecutionSettingsRequestSchema = z
  .object({
    permissionMode: permissionModeSchema,
  })
  .strict()

const setExecutionSettingsResponseSchema = z
  .object({
    autoModeAvailable: z.boolean(),
    permissionMode: permissionModeSchema,
  })
  .strict()

const getProviderConfigApiKeyRequestSchema = z
  .object({
    configId: z.string().trim().min(1),
    revealToken: z.string().trim().min(1),
  })
  .strict()

const requestProviderConfigApiKeyRevealRequestSchema = z
  .object({
    configId: z.string().trim().min(1),
  })
  .strict()

const requestProviderConfigApiKeyRevealResponseSchema = z
  .object({
    configId: z.string().min(1),
    expiresInSeconds: z.number().int().positive(),
    revealToken: z.string().min(1),
    status: z.literal('ready'),
  })
  .strict()

const getProviderConfigApiKeyResponseSchema = z
  .object({
    apiKey: z.string(),
    configId: z.string().min(1),
  })
  .strict()

const setConversationModelConfigRequestSchema = z
  .object({
    conversationId: z.string().min(1),
    modelConfigId: z.string().min(1),
  })
  .strict()

const setConversationModelConfigResponseSchema = z
  .object({
    conversationId: z.string().min(1),
    modelConfigId: z.string().min(1),
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

const skillIdSchema = z.string().trim().min(1)
const skillSourceKindSchema = z.enum(['workspace', 'user', 'bundled', 'plugin', 'mcp'])
const skillStatusSchema = z.enum(['ready', 'prerequisite_missing', 'disabled', 'rejected'])
const skillParamTypeSchema = z.enum(['string', 'number', 'boolean', 'path', 'url'])
const skillFileKindSchema = z.enum(['directory', 'file'])

const skillSummarySchema = z
  .object({
    category: z.string().min(1).optional(),
    description: z.string(),
    enabled: z.boolean(),
    id: skillIdSchema,
    importedAt: z.string().datetime({ offset: true }).optional(),
    manageable: z.boolean(),
    name: z.string().min(1),
    sourceKind: skillSourceKindSchema,
    status: skillStatusSchema,
    tags: z.array(z.string()),
    updatedAt: z.string().datetime({ offset: true }).optional(),
  })
  .strict()

const skillParameterSchema = z
  .object({
    default: z.unknown().optional(),
    description: z.string().optional(),
    name: z.string().min(1),
    paramType: skillParamTypeSchema,
    required: z.boolean(),
  })
  .strict()

const skillFileSchema = z
  .object({
    depth: z.number().int().nonnegative(),
    kind: skillFileKindSchema,
    name: z.string().min(1),
    path: z.string().min(1),
    sizeBytes: z.number().int().nonnegative().optional(),
  })
  .strict()

const skillFileContentSchema = z
  .object({
    content: z.string(),
    path: z.string().min(1),
  })
  .strict()

const skillDetailSchema = z
  .object({
    bodyPreview: z.string(),
    configKeys: z.array(z.string().min(1)),
    files: z.array(skillFileSchema),
    parameters: z.array(skillParameterSchema),
    summary: skillSummarySchema,
    validationError: z.string().optional(),
  })
  .strict()

const listSkillsResponseSchema = z
  .object({
    skills: z.array(skillSummarySchema),
  })
  .strict()

const getSkillDetailRequestSchema = z
  .object({
    id: skillIdSchema,
  })
  .strict()

const getSkillDetailResponseSchema = z
  .object({
    skill: skillDetailSchema,
  })
  .strict()

const getSkillFileRequestSchema = z
  .object({
    id: skillIdSchema,
    path: z.string().trim().min(1),
  })
  .strict()

const getSkillFileResponseSchema = z
  .object({
    file: skillFileContentSchema,
  })
  .strict()

const importSkillRequestSchema = z
  .object({
    sourcePath: z.string().trim().min(1),
  })
  .strict()

const importSkillResponseSchema = z
  .object({
    skill: skillSummarySchema,
  })
  .strict()

const setSkillEnabledRequestSchema = z
  .object({
    enabled: z.boolean(),
    id: skillIdSchema,
  })
  .strict()

const setSkillEnabledResponseSchema = z
  .object({
    skill: skillSummarySchema,
  })
  .strict()

const deleteSkillRequestSchema = z
  .object({
    id: skillIdSchema,
  })
  .strict()

const deleteSkillResponseSchema = z
  .object({
    id: skillIdSchema,
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

const projectRecordSchema = z
  .object({
    path: z.string().min(1),
    name: z.string().min(1),
    lastOpenedAt: z.string().min(1),
  })
  .strict()

const listProjectsResponseSchema = z
  .object({
    projects: z.array(projectRecordSchema),
    activePath: z.string().min(1).nullable(),
  })
  .strict()

const switchProjectRequestSchema = z
  .object({
    path: z.string().min(1),
  })
  .strict()

const switchProjectResponseSchema = z
  .object({
    project: projectRecordSchema,
  })
  .strict()

export type AppInfo = z.infer<typeof appInfoSchema>
export type HarnessHealthcheck = z.infer<typeof harnessHealthcheckSchema>
export type ListConversationsResponse = z.infer<typeof listConversationsResponseSchema>
export type ListProjectsResponse = z.infer<typeof listProjectsResponseSchema>
export type SwitchProjectResponse = z.infer<typeof switchProjectResponseSchema>
export type CreateConversationResponse = z.infer<typeof createConversationResponseSchema>
export type GetConversationResponse = z.infer<typeof getConversationResponseSchema>
export type DeleteConversationResponse = z.infer<typeof deleteConversationResponseSchema>
export type ContextReference = z.infer<typeof contextReferenceSchema>
export type AttachmentReference = z.infer<typeof attachmentReferenceSchema>
export type AttachmentInputModality = Extract<
  z.infer<typeof modelModalitySchema>,
  'image' | 'video' | 'file'
>
export type ConversationModelCapability = z.infer<typeof conversationModelCapabilitySchema>
export type StartRunRequest = z.infer<typeof startRunRequestSchema>
export type StartRunResponse = z.infer<typeof startRunResponseSchema>
export type CreateAttachmentFromPathResponse = z.infer<
  typeof createAttachmentFromPathResponseSchema
>
export type ListReferenceCandidatesResponse = z.infer<typeof listReferenceCandidatesResponseSchema>
export type CancelRunResponse = z.infer<typeof cancelRunResponseSchema>
export type ResolvePermissionRequest = z.infer<typeof resolvePermissionRequestSchema>
export type ResolvePermissionResponse = z.infer<typeof resolvePermissionResponseSchema>
export type ListActivityRequest = z.infer<typeof listActivityRequestSchema>
export type ListActivityResponse = z.infer<typeof listActivityResponseSchema>
export type ReplayTimelineRequest = z.infer<typeof replayTimelineRequestSchema>
export type ReplayTimelineResponse = z.infer<typeof replayTimelineResponseSchema>
export type ConversationCursor = z.infer<typeof conversationCursorSchema>
type PageConversationTimelineRequest = z.infer<typeof pageConversationTimelineRequestSchema>
export type PageConversationTimelineResponse = z.infer<
  typeof pageConversationTimelineResponseSchema
>
type SubscribeConversationEventsRequest = z.infer<typeof subscribeConversationEventsRequestSchema>
export type SubscribeConversationEventsResponse = z.infer<
  typeof subscribeConversationEventsResponseSchema
>
export type UnsubscribeConversationEventsResponse = z.infer<
  typeof unsubscribeConversationEventsResponseSchema
>
export type ConversationEventBatchPayload = z.infer<typeof conversationEventBatchPayloadSchema>
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
export type ModelProviderCatalogResponse = z.infer<typeof modelProviderCatalogResponseSchema>
export type ProviderConfig = z.infer<typeof providerConfigSchema>
export type ListProviderSettingsResponse = z.infer<typeof listProviderSettingsResponseSchema>
export type SaveProviderSettingsResponse = z.infer<typeof saveProviderSettingsResponseSchema>
export type PermissionMode = z.infer<typeof permissionModeSchema>
export type GetExecutionSettingsResponse = z.infer<typeof getExecutionSettingsResponseSchema>
export type SetExecutionSettingsRequest = z.infer<typeof setExecutionSettingsRequestSchema>
export type SetExecutionSettingsResponse = z.infer<typeof setExecutionSettingsResponseSchema>
export type RequestProviderConfigApiKeyRevealResponse = z.infer<
  typeof requestProviderConfigApiKeyRevealResponseSchema
>
export type GetProviderConfigApiKeyResponse = z.infer<typeof getProviderConfigApiKeyResponseSchema>
export type SetConversationModelConfigResponse = z.infer<
  typeof setConversationModelConfigResponseSchema
>
export type McpServerSummary = z.infer<typeof mcpServerSummarySchema>
export type ListMcpServersResponse = z.infer<typeof listMcpServersResponseSchema>
export type SaveMcpServerRequest = z.infer<typeof saveMcpServerRequestSchema>
export type SaveMcpServerResponse = z.infer<typeof saveMcpServerResponseSchema>
export type DeleteMcpServerResponse = z.infer<typeof deleteMcpServerResponseSchema>
export type SkillSummary = z.infer<typeof skillSummarySchema>
export type SkillFile = z.infer<typeof skillFileSchema>
export type ListSkillsResponse = z.infer<typeof listSkillsResponseSchema>
export type GetSkillDetailResponse = z.infer<typeof getSkillDetailResponseSchema>
export type GetSkillFileResponse = z.infer<typeof getSkillFileResponseSchema>
export type ImportSkillResponse = z.infer<typeof importSkillResponseSchema>
export type SetSkillEnabledResponse = z.infer<typeof setSkillEnabledResponseSchema>
export type DeleteSkillResponse = z.infer<typeof deleteSkillResponseSchema>
export type MemoryItemSummary = z.infer<typeof memoryItemSummarySchema>
export type ListMemoryItemsResponse = z.infer<typeof listMemoryItemsResponseSchema>
export type GetMemoryItemResponse = z.infer<typeof getMemoryItemResponseSchema>
export type UpdateMemoryItemRequest = z.infer<typeof updateMemoryItemRequestSchema>
export type UpdateMemoryItemResponse = z.infer<typeof updateMemoryItemResponseSchema>
export type DeleteMemoryItemResponse = z.infer<typeof deleteMemoryItemResponseSchema>
export type ExportMemoryItemsResponse = z.infer<typeof exportMemoryItemsResponseSchema>
export type ListEvalCasesResponse = z.infer<typeof listEvalCasesResponseSchema>
export type RunEvalCaseResponse = z.infer<typeof runEvalCaseResponseSchema>
export type ListArtifactsRequest = z.infer<typeof listArtifactsRequestSchema>
export type ListReferenceCandidatesRequest = z.infer<typeof listReferenceCandidatesRequestSchema>

export interface CommandClient {
  cancelRun: (runId: string) => Promise<CancelRunResponse>
  createAttachmentFromPath: (path: string) => Promise<CreateAttachmentFromPathResponse>
  createConversation: () => Promise<CreateConversationResponse>
  deleteConversation: (conversationId: string) => Promise<DeleteConversationResponse>
  deleteMcpServer: (id: string) => Promise<DeleteMcpServerResponse>
  deleteMemoryItem: (id: string) => Promise<DeleteMemoryItemResponse>
  deleteSkill: (id: string) => Promise<DeleteSkillResponse>
  getContextSnapshot: (request: GetContextSnapshotRequest) => Promise<GetContextSnapshotResponse>
  getConversation: (conversationId: string) => Promise<GetConversationResponse>
  getAppInfo: () => Promise<AppInfo>
  getHarnessHealthcheck: () => Promise<HarnessHealthcheck>
  getMemoryItem: (id: string) => Promise<GetMemoryItemResponse>
  getProviderConfigApiKey: (
    configId: string,
    revealToken: string,
  ) => Promise<GetProviderConfigApiKeyResponse>
  getReplayTimeline: (request: ReplayTimelineRequest) => Promise<ReplayTimelineResponse>
  getSkillDetail: (id: string) => Promise<GetSkillDetailResponse>
  getSkillFile: (id: string, path: string) => Promise<GetSkillFileResponse>
  importSkill: (sourcePath: string) => Promise<ImportSkillResponse>
  exportMemoryItems: () => Promise<ExportMemoryItemsResponse>
  exportSupportBundle: (request: ExportSupportBundleRequest) => Promise<ExportSupportBundleResponse>
  getExecutionSettings: () => Promise<GetExecutionSettingsResponse>
  listActivity: (request: ListActivityRequest) => Promise<ListActivityResponse>
  listArtifacts: (request: ListArtifactsRequest) => Promise<ListArtifactsResponse>
  listConversations: () => Promise<ListConversationsResponse>
  listEvalCases: () => Promise<ListEvalCasesResponse>
  listModelProviderCatalog: () => Promise<ModelProviderCatalogResponse>
  listMcpServers: () => Promise<ListMcpServersResponse>
  listMemoryItems: () => Promise<ListMemoryItemsResponse>
  listProviderSettings: () => Promise<ListProviderSettingsResponse>
  listProjects: () => Promise<ListProjectsResponse>
  addProject: (path: string) => Promise<SwitchProjectResponse>
  switchProject: (path: string) => Promise<SwitchProjectResponse>
  pageConversationTimeline: (
    request: PageConversationTimelineRequest,
  ) => Promise<PageConversationTimelineResponse>
  listReferenceCandidates: (
    request: ListReferenceCandidatesRequest,
  ) => Promise<ListReferenceCandidatesResponse>
  listSkills: () => Promise<ListSkillsResponse>
  resolvePermission: (request: ResolvePermissionRequest) => Promise<ResolvePermissionResponse>
  requestProviderConfigApiKeyReveal: (
    configId: string,
  ) => Promise<RequestProviderConfigApiKeyRevealResponse>
  runEvalCase: (caseId: string) => Promise<RunEvalCaseResponse>
  saveMcpServer: (request: SaveMcpServerRequest) => Promise<SaveMcpServerResponse>
  saveProviderSettings: (request: ProviderSettingsRequest) => Promise<SaveProviderSettingsResponse>
  setExecutionSettings: (
    request: SetExecutionSettingsRequest,
  ) => Promise<SetExecutionSettingsResponse>
  setConversationModelConfig: (
    conversationId: string,
    modelConfigId: string,
  ) => Promise<SetConversationModelConfigResponse>
  setSkillEnabled: (id: string, enabled: boolean) => Promise<SetSkillEnabledResponse>
  startRun: (request: StartRunRequest) => Promise<StartRunResponse>
  subscribeConversationEvents: (
    request: SubscribeConversationEventsRequest,
  ) => Promise<SubscribeConversationEventsResponse>
  listenConversationEventBatches: (
    onBatch: (batch: ConversationEventBatchPayload) => void,
  ) => Promise<() => void>
  unsubscribeConversationEvents: (
    subscriptionId: string,
  ) => Promise<UnsubscribeConversationEventsResponse>
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
    async createAttachmentFromPath(path) {
      const command = 'create_attachment_from_path'
      const args = parseArgs(command, createAttachmentFromPathRequestSchema, {
        path,
      })
      return parsePayload(
        command,
        createAttachmentFromPathResponseSchema,
        await invoke(command, args),
      )
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
    async deleteSkill(id) {
      const command = 'delete_skill'
      const args = parseArgs(command, deleteSkillRequestSchema, { id })
      return parsePayload(command, deleteSkillResponseSchema, await invoke(command, args))
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
    async getExecutionSettings() {
      const command = 'get_execution_settings'
      return parsePayload(command, getExecutionSettingsResponseSchema, await invoke(command))
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
    async getProviderConfigApiKey(configId, revealToken) {
      const command = 'get_provider_config_api_key'
      const args = parseArgs(command, getProviderConfigApiKeyRequestSchema, {
        configId,
        revealToken,
      })
      return parsePayload(
        command,
        getProviderConfigApiKeyResponseSchema,
        await invoke(command, args),
      )
    },
    async getReplayTimeline(request) {
      const command = 'get_replay_timeline'
      const args = parseArgs(command, replayTimelineRequestSchema, request)
      return parsePayload(command, replayTimelineResponseSchema, await invoke(command, args))
    },
    async pageConversationTimeline(request) {
      const command = 'page_conversation_timeline'
      const args = parseArgs(command, pageConversationTimelineRequestSchema, request)
      return parsePayload(
        command,
        pageConversationTimelineResponseSchema,
        await invoke(command, args),
      )
    },
    async getSkillDetail(id) {
      const command = 'get_skill_detail'
      const args = parseArgs(command, getSkillDetailRequestSchema, {
        id,
      })
      return parsePayload(command, getSkillDetailResponseSchema, await invoke(command, args))
    },
    async getSkillFile(id, path) {
      const command = 'get_skill_file'
      const args = parseArgs(command, getSkillFileRequestSchema, {
        id,
        path,
      })
      return parsePayload(command, getSkillFileResponseSchema, await invoke(command, args))
    },
    async importSkill(sourcePath) {
      const command = 'import_skill'
      const args = parseArgs(command, importSkillRequestSchema, { sourcePath })
      return parsePayload(command, importSkillResponseSchema, await invoke(command, args))
    },
    async listActivity(request) {
      const command = 'list_activity'
      const args = parseArgs(command, listActivityRequestSchema, request)
      return parsePayload(command, listActivityResponseSchema, await invoke(command, args))
    },
    async listArtifacts(request) {
      const command = 'list_artifacts'
      const args = parseArgs(command, listArtifactsRequestSchema, request)
      return parsePayload(command, listArtifactsResponseSchema, await invoke(command, args))
    },
    async listConversations() {
      const command = 'list_conversations'
      return parsePayload(command, listConversationsResponseSchema, await invoke(command))
    },
    async createConversation() {
      const command = 'create_conversation'
      return parsePayload(command, createConversationResponseSchema, await invoke(command))
    },
    async deleteConversation(conversationId) {
      const command = 'delete_conversation'
      const args = parseArgs(command, deleteConversationRequestSchema, {
        conversationId,
      })
      return parsePayload(command, deleteConversationResponseSchema, await invoke(command, args))
    },
    async listEvalCases() {
      const command = 'list_eval_cases'
      return parsePayload(command, listEvalCasesResponseSchema, await invoke(command))
    },
    async listModelProviderCatalog() {
      const command = 'list_model_provider_catalog'
      return parsePayload(command, modelProviderCatalogResponseSchema, await invoke(command))
    },
    async listMcpServers() {
      const command = 'list_mcp_servers'
      return parsePayload(command, listMcpServersResponseSchema, await invoke(command))
    },
    async listMemoryItems() {
      const command = 'list_memory_items'
      return parsePayload(command, listMemoryItemsResponseSchema, await invoke(command))
    },
    async listReferenceCandidates(request) {
      const command = 'list_reference_candidates'
      const args = parseArgs(command, listReferenceCandidatesRequestSchema, request)
      return parsePayload(
        command,
        listReferenceCandidatesResponseSchema,
        await invoke(command, args),
      )
    },
    async listProviderSettings() {
      const command = 'list_provider_settings'
      return parsePayload(command, listProviderSettingsResponseSchema, await invoke(command))
    },
    async listProjects() {
      const command = 'list_projects'
      return parsePayload(command, listProjectsResponseSchema, await invoke(command))
    },
    async addProject(path) {
      const command = 'add_project'
      const args = parseArgs(command, switchProjectRequestSchema, { path })
      return parsePayload(command, switchProjectResponseSchema, await invoke(command, args))
    },
    async switchProject(path) {
      const command = 'switch_project'
      const args = parseArgs(command, switchProjectRequestSchema, { path })
      return parsePayload(command, switchProjectResponseSchema, await invoke(command, args))
    },
    async listSkills() {
      const command = 'list_skills'
      return parsePayload(command, listSkillsResponseSchema, await invoke(command))
    },
    async resolvePermission(request) {
      const command = 'resolve_permission'
      const args = parseArgs(command, resolvePermissionRequestSchema, request)
      return parsePayload(command, resolvePermissionResponseSchema, await invoke(command, args))
    },
    async requestProviderConfigApiKeyReveal(configId) {
      const command = 'request_provider_config_api_key_reveal'
      const args = parseArgs(command, requestProviderConfigApiKeyRevealRequestSchema, {
        configId,
      })
      return parsePayload(
        command,
        requestProviderConfigApiKeyRevealResponseSchema,
        await invoke(command, args),
      )
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
    async setExecutionSettings(request) {
      const command = 'set_execution_settings'
      const args = parseArgs(command, setExecutionSettingsRequestSchema, request)
      return parsePayload(command, setExecutionSettingsResponseSchema, await invoke(command, args))
    },
    async saveMcpServer(request) {
      const command = 'save_mcp_server'
      const args = parseArgs(command, saveMcpServerRequestSchema, request)
      return parsePayload(command, saveMcpServerResponseSchema, await invoke(command, args))
    },
    async setConversationModelConfig(conversationId, modelConfigId) {
      const command = 'set_conversation_model_config'
      const args = parseArgs(command, setConversationModelConfigRequestSchema, {
        conversationId,
        modelConfigId,
      })
      return parsePayload(
        command,
        setConversationModelConfigResponseSchema,
        await invoke(command, args),
      )
    },
    async setSkillEnabled(id, enabled) {
      const command = 'set_skill_enabled'
      const args = parseArgs(command, setSkillEnabledRequestSchema, {
        enabled,
        id,
      })
      return parsePayload(command, setSkillEnabledResponseSchema, await invoke(command, args))
    },
    async startRun(request) {
      const command = 'start_run'
      const args = parseArgs(command, startRunRequestSchema, request)
      return parsePayload(command, startRunResponseSchema, await invoke(command, args))
    },
    async subscribeConversationEvents(request) {
      const command = 'subscribe_conversation_events'
      const args = parseArgs(command, subscribeConversationEventsRequestSchema, request)
      return parsePayload(
        command,
        subscribeConversationEventsResponseSchema,
        await invoke(command, args),
      )
    },
    async listenConversationEventBatches(onBatch) {
      const unlisten = await tauriListen<unknown>('conversation_event_batch', (event) => {
        onBatch(
          parsePayload(
            'conversation_event_batch',
            conversationEventBatchPayloadSchema,
            event.payload,
          ),
        )
      })

      return unlisten
    },
    async unsubscribeConversationEvents(subscriptionId) {
      const command = 'unsubscribe_conversation_events'
      const args = parseArgs(command, unsubscribeConversationEventsRequestSchema, {
        subscriptionId,
      })
      return parsePayload(
        command,
        unsubscribeConversationEventsResponseSchema,
        await invoke(command, args),
      )
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

export function createConversation(
  client: CommandClient = tauriCommandClient,
): Promise<CreateConversationResponse> {
  return client.createConversation()
}

export function listEvalCases(
  client: CommandClient = tauriCommandClient,
): Promise<ListEvalCasesResponse> {
  return client.listEvalCases()
}

export function listModelProviderCatalog(
  client: CommandClient = tauriCommandClient,
): Promise<ModelProviderCatalogResponse> {
  return client.listModelProviderCatalog()
}

export function listArtifacts(
  request: ListArtifactsRequest,
  client: CommandClient = tauriCommandClient,
): Promise<ListArtifactsResponse> {
  return client.listArtifacts(request)
}

export function getConversation(
  conversationId: string,
  client: CommandClient = tauriCommandClient,
): Promise<GetConversationResponse> {
  return client.getConversation(conversationId)
}

export function deleteConversation(
  conversationId: string,
  client: CommandClient = tauriCommandClient,
): Promise<DeleteConversationResponse> {
  return client.deleteConversation(conversationId)
}

export function startRun(
  request: StartRunRequest,
  client: CommandClient = tauriCommandClient,
): Promise<StartRunResponse> {
  return client.startRun(request)
}

export function createAttachmentFromPath(
  path: string,
  client: CommandClient = tauriCommandClient,
): Promise<CreateAttachmentFromPathResponse> {
  return client.createAttachmentFromPath(path)
}

export function listReferenceCandidates(
  request: ListReferenceCandidatesRequest,
  client: CommandClient = tauriCommandClient,
): Promise<ListReferenceCandidatesResponse> {
  return client.listReferenceCandidates(request)
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

export function listSkills(
  client: CommandClient = tauriCommandClient,
): Promise<ListSkillsResponse> {
  return client.listSkills()
}

export function getSkillDetail(
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<GetSkillDetailResponse> {
  return client.getSkillDetail(id)
}

export function getSkillFile(
  id: string,
  path: string,
  client: CommandClient = tauriCommandClient,
): Promise<GetSkillFileResponse> {
  return client.getSkillFile(id, path)
}

export function importSkill(
  sourcePath: string,
  client: CommandClient = tauriCommandClient,
): Promise<ImportSkillResponse> {
  return client.importSkill(sourcePath)
}

export function setSkillEnabled(
  id: string,
  enabled: boolean,
  client: CommandClient = tauriCommandClient,
): Promise<SetSkillEnabledResponse> {
  return client.setSkillEnabled(id, enabled)
}

export function deleteSkill(
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<DeleteSkillResponse> {
  return client.deleteSkill(id)
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

export function listProviderSettings(
  client: CommandClient = tauriCommandClient,
): Promise<ListProviderSettingsResponse> {
  return client.listProviderSettings()
}

export function listProjects(
  client: CommandClient = tauriCommandClient,
): Promise<ListProjectsResponse> {
  return client.listProjects()
}

export function addProject(
  path: string,
  client: CommandClient = tauriCommandClient,
): Promise<SwitchProjectResponse> {
  return client.addProject(path)
}

export function switchProject(
  path: string,
  client: CommandClient = tauriCommandClient,
): Promise<SwitchProjectResponse> {
  return client.switchProject(path)
}

export function getExecutionSettings(
  client: CommandClient = tauriCommandClient,
): Promise<GetExecutionSettingsResponse> {
  return client.getExecutionSettings()
}

export function setExecutionSettings(
  request: SetExecutionSettingsRequest,
  client: CommandClient = tauriCommandClient,
): Promise<SetExecutionSettingsResponse> {
  return client.setExecutionSettings(request)
}

export function requestProviderConfigApiKeyReveal(
  configId: string,
  client: CommandClient = tauriCommandClient,
): Promise<RequestProviderConfigApiKeyRevealResponse> {
  return client.requestProviderConfigApiKeyReveal(configId)
}

export function getProviderConfigApiKey(
  configId: string,
  revealToken: string,
  client: CommandClient = tauriCommandClient,
): Promise<GetProviderConfigApiKeyResponse> {
  return client.getProviderConfigApiKey(configId, revealToken)
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
