import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { listen as tauriListen } from '@tauri-apps/api/event'
import { z } from 'zod'

const uuidV4Pattern = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
const permissionModeSchema = z.enum(['default', 'auto', 'bypass_permissions'])
const toolGroupSchema = z.union([
  z.enum([
    'file_system',
    'search',
    'network',
    'shell',
    'agent',
    'coordinator',
    'memory',
    'clarification',
    'meta',
  ]),
  z.object({ custom: z.string().min(1) }).strict(),
])
const toolProfileSchema = z.union([
  z.enum(['minimal', 'coding', 'full']),
  z
    .object({
      custom: z
        .object({
          allowlist: z.array(z.string().min(1)),
          denylist: z.array(z.string().min(1)),
          group_allowlist: z.array(toolGroupSchema),
          group_denylist: z.array(toolGroupSchema),
          mcp_included: z.boolean(),
          plugin_included: z.boolean(),
        })
        .strict(),
    })
    .strict(),
])
const unredactedSecretPatterns = [
  /\bAuthorization:?\s*Bearer\s+\S+/i,
  /\bAuthorization:?\s*Basic\s+\S+/i,
  /\bBasic\s+[A-Za-z0-9+/=]{8,}\b/,
  /\b(?:api[_-]?key|token|secret|password)\b\s*[:=]\s*(?=[A-Za-z0-9._~+/=-]{6,}\b)(?=[A-Za-z0-9._~+/=-]*[0-9_~+/=-])[A-Za-z0-9._~+/=-]+\b/i,
  /\b(?:api[_-]?key|token|secret|password)\b\s+(?=[A-Za-z0-9._~+/=-]{12,}\b)(?=[A-Za-z0-9._~+/=-]*[0-9_~+/=-])[A-Za-z0-9._~+/=-]+\b/i,
  /\b--(?:api-key|token|secret|password)\b\s+\S+/i,
  /\b[A-Za-z0-9_]*(?:API_KEY|TOKEN|SECRET|PASSWORD|ACCESS_KEY)[A-Za-z0-9_]*\s*=\s*\S+/i,
  /\b[A-Z0-9_]*(?:API_KEY|TOKEN|SECRET|PASSWORD|ACCESS_KEY)[A-Z0-9_]*\s+[A-Za-z0-9._~+/=-]{8,}\b/,
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

export function hasObviousUnredactedSecret(value: string): boolean {
  return unredactedSecretPatterns.some((pattern) => pattern.test(value))
}

function hasPrivateAbsolutePath(value: string): boolean {
  return /(?:\/Users\/|\/home\/|\/private\/var\/|[A-Za-z]:[\\/])/.test(value)
}

function hasUnsafeUrl(value: string): boolean {
  const schemeUrlPattern = /([A-Za-z][A-Za-z0-9+.-]*):\/\//g
  let match = schemeUrlPattern.exec(value)
  while (match !== null) {
    if (match[1]?.toLowerCase() !== 'workspace') {
      return true
    }
    match = schemeUrlPattern.exec(value)
  }

  return /(?:^|[^A-Za-z0-9_])(?:blob|data|file|javascript|mailto):/i.test(value)
}

function hasUnsafeDisplayReference(value: string): boolean {
  return (
    hasUnsafeUrl(value) ||
    /(?:~[\\/]|\.jyowo[\\/])/i.test(value) ||
    /(?:^|[^A-Za-z0-9_])(?:[A-Za-z]:[\\/])/.test(value) ||
    /(?:^|[^A-Za-z0-9_.-])\/(?:Applications|Library|System|Users|Volumes|dev|etc|home|media|mnt|opt|private|root|run|tmp|usr|var)(?:[\\/]|$)/.test(
      value,
    )
  )
}

const maxConversationDisplayTextChars = 70_000

const conversationDisplayTextSchema = z.string().superRefine((value, ctx) => {
  if (value.length > maxConversationDisplayTextChars) {
    ctx.addIssue({
      code: z.ZodIssueCode.custom,
      message: `conversation display text must be at most ${maxConversationDisplayTextChars} characters`,
      fatal: true,
    })
    return z.NEVER
  }
  if (hasObviousUnredactedSecret(value)) {
    ctx.addIssue({
      code: z.ZodIssueCode.custom,
      message: 'conversation message body must not contain obvious unredacted secrets',
    })
  }
  if (hasPrivateAbsolutePath(value)) {
    ctx.addIssue({
      code: z.ZodIssueCode.custom,
      message: 'conversation message body must not contain private absolute paths',
    })
  }
  if (hasUnsafeDisplayReference(value)) {
    ctx.addIssue({
      code: z.ZodIssueCode.custom,
      message: 'conversation display text must not contain unsafe display references',
    })
  }
})

const mimeTypeMetadataSchema = z
  .string()
  .trim()
  .min(1)
  .refine((value) => !hasObviousUnredactedSecret(value), {
    message: 'attachment MIME metadata must not contain obvious unredacted secrets',
  })
  .refine((value) => !hasUnsafeDisplayReference(value), {
    message: 'attachment MIME metadata must not contain unsafe display references',
  })
  .refine((value) => /^[A-Za-z0-9!#$&^_.+-]+\/[A-Za-z0-9!#$&^_.+-]+$/.test(value), {
    message: 'attachment MIME metadata must be a MIME type',
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

const settingsScopeSchema = z.enum(['global', 'project'])

const processSandboxStatusSchema = z
  .object({
    backendId: z.string().min(1),
    candidateIds: z.array(z.string()),
    availableNetworkPolicies: z.array(z.string()),
    availableWorkspacePolicies: z.array(z.string()),
    unavailableReasons: z.array(z.string()),
  })
  .strict()

const brokerStatusSchema = z
  .object({
    available: z.boolean(),
    deniedReasons: z.array(z.string()),
  })
  .strict()

const toolRuntimeStatusSchema = z
  .object({
    toolName: z.string().min(1),
    available: z.boolean(),
    unavailableReason: z.string().nullable(),
  })
  .strict()

export const runtimeExecutionStatusSchema = z
  .object({
    processSandbox: processSandboxStatusSchema,
    httpBroker: brokerStatusSchema,
    tools: z.array(toolRuntimeStatusSchema),
  })
  .strict()

const runtimeToolAccessSchema = z.enum(['readOnly', 'mutating', 'destructive'])
const runtimeToolExecutionChannelSchema = z.enum([
  'directAuthorizedRust',
  'processSandbox',
  'httpBroker',
  'externalCapability',
])
const runtimeToolServiceBindingSchema = z
  .object({
    providerId: z.string().min(1),
    operationId: z.string().min(1),
    routeKind: z.string().min(1),
  })
  .strict()
const runtimeToolSummarySchema = z
  .object({
    name: z.string().min(1),
    displayName: z.string().min(1),
    description: z.string(),
    category: z.string(),
    group: z.string().min(1),
    groupLabel: z.string().min(1),
    originKind: z.enum(['builtin', 'plugin', 'mcp', 'skill', 'custom']),
    originId: z.string().min(1).nullable(),
    access: runtimeToolAccessSchema,
    executionChannel: runtimeToolExecutionChannelSchema,
    requiredCapabilities: z.array(z.string().min(1)),
    deferPolicy: z.enum(['alwaysLoad', 'autoDefer', 'forceDefer']),
    longRunning: z.boolean(),
    serviceBinding: runtimeToolServiceBindingSchema.nullable(),
    configuredEnabled: z.boolean(),
    available: z.boolean(),
    unavailableReason: z.string().nullable(),
  })
  .strict()
const listRuntimeToolsResponseSchema = z
  .object({
    generation: z.number().int().nonnegative(),
    scope: settingsScopeSchema,
    customized: z.boolean(),
    tools: z.array(runtimeToolSummarySchema),
  })
  .strict()
const setRuntimeToolEnabledRequestSchema = z
  .object({
    name: z.string().min(1),
    enabled: z.boolean(),
  })
  .strict()

const skillReferenceSourceSchema = z.union([
  z.enum(['bundled', 'workspace', 'user']),
  z.object({ plugin: z.string().trim().min(1) }).strict(),
  z.object({ mcp: z.string().trim().min(1) }).strict(),
])

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
      kind: z.literal('skill'),
      label: z.string().trim().min(1),
      parameters: z.record(z.string(), z.unknown()).default({}),
      skillId: z.string().trim().min(1),
      source: skillReferenceSourceSchema.nullable().optional(),
      version: z.literal(1).default(1),
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

const attachmentReferenceCamelSchema = z
  .object({
    blobRef: z
      .object({
        contentHash: z.array(z.number().int().min(0).max(255)).length(32),
        contentType: mimeTypeMetadataSchema.nullable().optional(),
        id: z.string().trim().min(1),
        size: z.number().int().min(0),
      })
      .strict(),
    id: z
      .string()
      .trim()
      .regex(/^attachment-[0-9a-fA-F]{64}$/),
    mimeType: mimeTypeMetadataSchema,
    name: conversationDisplayTextSchema.pipe(z.string().trim().min(1)),
    sizeBytes: z.number().int().min(0),
  })
  .strict()
const attachmentReferenceSnakeSchema = z
  .object({
    blob_ref: z
      .object({
        content_hash: z.array(z.number().int().min(0).max(255)).length(32),
        content_type: mimeTypeMetadataSchema.nullable().optional(),
        id: z.string().trim().min(1),
        size: z.number().int().min(0),
      })
      .strict(),
    id: z
      .string()
      .trim()
      .regex(/^attachment-[0-9a-fA-F]{64}$/),
    mime_type: mimeTypeMetadataSchema,
    name: conversationDisplayTextSchema.pipe(z.string().trim().min(1)),
    size_bytes: z.number().int().min(0),
  })
  .strict()
  .transform((attachment) => ({
    blobRef: {
      contentHash: attachment.blob_ref.content_hash,
      contentType: attachment.blob_ref.content_type,
      id: attachment.blob_ref.id,
      size: attachment.blob_ref.size,
    },
    id: attachment.id,
    mimeType: attachment.mime_type,
    name: attachment.name,
    sizeBytes: attachment.size_bytes,
  }))
const attachmentReferenceSchema = z.union([
  attachmentReferenceCamelSchema,
  attachmentReferenceSnakeSchema,
])

const referenceCandidateSchema = z
  .object({
    id: z.string().min(1).optional(),
    label: z.string().min(1),
    path: z.string().min(1).optional(),
  })
  .strict()

const skillReferenceCandidateSchema = z
  .object({
    id: z.string().trim().min(1),
    label: z.string().trim().min(1),
    source: skillReferenceSourceSchema,
  })
  .strict()

const listReferenceCandidatesResponseSchema = z
  .object({
    artifacts: z.array(referenceCandidateSchema),
    conversations: z.array(referenceCandidateSchema),
    files: z.array(referenceCandidateSchema),
    memories: z.array(referenceCandidateSchema),
    mcpServers: z.array(referenceCandidateSchema),
    skills: z.array(skillReferenceCandidateSchema),
    tools: z.array(referenceCandidateSchema),
  })
  .strict()

const providerIdSchema = z.string().trim().min(1)

const modelLifecycleSchema = z
  .object({
    kind: z.enum(['stable', 'preview', 'retiring']),
    retirementDate: z.string().min(1).optional(),
  })
  .strict()

const modelRuntimeStatusSchema = z
  .object({
    kind: z.enum(['runnable', 'unsupported']),
    reason: z.string().min(1).optional(),
  })
  .strict()

const modelProtocolSchema = z.enum([
  'chat_completions',
  'completions',
  'responses',
  'messages',
  'dashscope',
  'generate_content',
])

const modelModalitySchema = z.enum(['text', 'image', 'audio', 'video', 'file', 'embedding'])

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
    category: z.enum([
      'conversation',
      'image',
      'video',
      'three_d',
      'embedding',
      'audio',
      'music',
      'file',
      'model',
      'moderation',
      'vector_store',
      'batch',
      'fine_tuning',
      'eval',
      'grader',
      'container',
      'upload',
      'realtime',
      'admin',
      'webhook',
    ]),
    inputModalities: z.array(modelModalitySchema),
    outputArtifact: z.enum(['text', 'image', 'audio', 'video', 'file', 'embedding']),
    execution: z.enum(['sync', 'async_job', 'websocket']),
    requiresPolling: z.boolean(),
    permissionSubject: z.string().min(1),
    costRisk: z.enum(['low', 'medium', 'high']),
  })
  .strict()

const providerDefaultsSchema = z
  .object({
    body: z.record(z.string(), z.unknown()).optional(),
    headers: z.record(z.string(), z.string()).optional(),
  })
  .strict()

const openAiTextFormatSchema = z
  .object({
    type: z.literal('json_schema'),
    name: z.string().min(1),
    schema: z.unknown(),
    strict: z.boolean().optional(),
  })
  .strict()

const openAiResponsesOptionsSchema = z
  .object({
    background: z.boolean().optional(),
    conversation: z.unknown().optional(),
    include: z.array(z.string().min(1)).optional(),
    instructions: z.string().min(1).optional(),
    maxToolCalls: z.number().int().positive().optional(),
    prompt: z.unknown().optional(),
    promptCacheKey: z.string().min(1).optional(),
    promptCacheRetention: z.string().min(1).optional(),
    reasoning: z
      .object({
        effort: z.string().min(1).optional(),
        summary: z.string().min(1).optional(),
        context: z.string().min(1).optional(),
      })
      .strict()
      .optional(),
    safetyIdentifier: z.string().min(1).optional(),
    serviceTier: z.string().min(1).optional(),
    text: z
      .object({
        verbosity: z.string().min(1).optional(),
        format: openAiTextFormatSchema.optional(),
      })
      .strict()
      .optional(),
    topLogprobs: z.number().int().nonnegative().optional(),
    topP: z.unknown().optional(),
    toolChoice: z.unknown().optional(),
    parallelToolCalls: z.boolean().optional(),
    truncation: z.string().min(1).optional(),
    store: z.boolean().optional(),
    metadata: z.record(z.string(), z.string()).optional(),
    user: z.string().min(1).optional(),
    strictToolSchemas: z.boolean().optional(),
  })
  .strict()

const modelRequestOptionsSchema = z
  .object({
    kimiChat: z
      .object({
        partialAssistant: z
          .object({
            content: z.string(),
            name: z.string().min(1).optional(),
          })
          .strict()
          .optional(),
      })
      .strict()
      .optional(),
    openaiResponses: openAiResponsesOptionsSchema.optional(),
  })
  .strict()

const providerSettingsRequestSchema = z
  .object({
    apiKey: z.string().trim().min(1).optional(),
    baseUrl: z.string().trim().min(1).optional(),
    configId: z.string().trim().min(1).optional(),
    displayName: z.string().trim().min(1).optional(),
    modelId: z.string().trim().min(1),
    modelOptions: modelRequestOptionsSchema.optional(),
    officialQuotaApiKey: z.string().trim().min(1).optional(),
    providerId: providerIdSchema,
    protocol: modelProtocolSchema.optional(),
    providerDefaults: providerDefaultsSchema.optional(),
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
    supportedProtocols: z.array(modelProtocolSchema),
    supportedParameters: z.array(z.string().min(1)).optional(),
    providerCapabilityMetadata: z.unknown().optional(),
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
    providerDefaults: providerDefaultsSchema.optional(),
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
    hasOfficialQuotaApiKey: z.boolean(),
    id: z.string().min(1),
    isDefault: z.boolean(),
    modelId: z.string().min(1),
    modelOptions: modelRequestOptionsSchema.optional(),
    modelDescriptor: modelCatalogEntrySchema,
    providerDefaults: providerDefaultsSchema.optional(),
    providerId: providerIdSchema,
  })
  .strict()

const listProviderSettingsResponseSchema = z
  .object({
    defaultConfigId: z.string().min(1).nullable(),
    selectionScope: settingsScopeSchema,
    configs: z.array(providerConfigSchema),
  })
  .strict()

const saveProviderSettingsResponseSchema = z
  .object({
    config: providerConfigSchema,
    status: z.literal('saved'),
  })
  .strict()

const contextCompressionTriggerRatioSchema = z.number().min(0.5).max(0.95)
const agentCapabilityKindSchema = z.enum(['subagents', 'agentTeams', 'backgroundAgents'])
const agentCapabilityUnavailableReasonSchema = z
  .object({
    capability: agentCapabilityKindSchema,
    message: z.string(),
    type: z.literal('daemonUnavailable'),
  })
  .strict()
const agentProfileScopeSchema = z.enum(['builtin', 'user', 'project'])
const agentProfileSandboxInheritanceSchema = z.enum(['inherit_parent', 'narrow_only'])
const agentProfileMemoryScopeSchema = z.enum(['none', 'read_only', 'read_write'])
const agentProfileContextModeSchema = z.enum(['minimal', 'focused', 'full_workspace'])
const agentWorkspaceIsolationModeSchema = z.enum(['read_only', 'patch_only', 'git_worktree'])
const agentUsePolicySchema = z.enum(['off', 'allowed'])
const agentTeamTopologySchema = z.enum(['coordinator_worker', 'peer_to_peer', 'role_routed'])
const agentTeamSharedMemoryPolicySchema = z.enum(['none', 'summaries_only', 'redacted_mailbox'])
const agentProfileIdSchema = z
  .string()
  .trim()
  .min(1)
  .regex(/^[a-z0-9_-]+$/)
const agentProfileModelOverrideSchema = z
  .object({
    modelId: z.string().nullable().optional(),
    providerConfigId: z.string().nullable().optional(),
  })
  .strict()
const agentProfileSchema = z
  .object({
    contextMode: agentProfileContextModeSchema,
    defaultWorkspaceIsolation: agentWorkspaceIsolationModeSchema,
    description: z.string(),
    id: agentProfileIdSchema,
    maxDepth: z.number().int().min(0).max(8),
    maxTurns: z.number().int().min(1),
    memoryScope: agentProfileMemoryScopeSchema,
    modelConfigOverride: agentProfileModelOverrideSchema.nullable().optional(),
    role: z.string().trim().min(1),
    sandboxInheritance: agentProfileSandboxInheritanceSchema,
    scope: agentProfileScopeSchema,
    toolAllowlist: z.array(z.string()).nullable().optional(),
    toolBlocklist: z.array(z.string()),
  })
  .strict()
const agentTeamRunConfigSchema = z
  .object({
    leadProfileId: agentProfileIdSchema,
    maxTurnsPerGoal: z.number().int().min(1),
    memberProfileIds: z.array(agentProfileIdSchema).min(1),
    sharedMemoryPolicy: agentTeamSharedMemoryPolicySchema,
    topology: agentTeamTopologySchema,
  })
  .strict()
const agentToolPolicySchema = z
  .object({
    agentTeam: agentUsePolicySchema,
    backgroundAgents: agentUsePolicySchema,
    maxConcurrentSubagents: z.number().int().min(1),
    maxDepth: z.number().int().min(0).max(8),
    maxTeamMembers: z.number().int().min(1),
    subagents: agentUsePolicySchema,
    teamConfig: agentTeamRunConfigSchema.nullable().optional(),
    workspaceIsolation: agentWorkspaceIsolationModeSchema,
  })
  .strict()
  .superRefine((value, ctx) => {
    if (value.agentTeam === 'off' && value.teamConfig) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: 'teamConfig must be null when agentTeam is off',
        path: ['teamConfig'],
      })
    }
  })

const startRunRequestSchema = z
  .object({
    attachments: z.array(attachmentReferenceSchema).optional(),
    clientMessageId: z.uuid().regex(uuidV4Pattern).optional(),
    conversationId: z.string().min(1),
    contextReferences: z.array(contextReferenceSchema).optional(),
    modelConfigId: z.string().min(1).optional(),
    permissionMode: permissionModeSchema.optional(),
    prompt: z.string().min(1),
  })
  .strict()

const isoDateTimeSchema = z.string().datetime({ offset: true })
const localDateSchema = z.string().regex(/^\d{4}-\d{2}-\d{2}$/)

const listAgentProfilesResponseSchema = z
  .object({
    profiles: z.array(agentProfileSchema),
  })
  .strict()
const saveAgentProfileResponseSchema = z
  .object({
    profile: agentProfileSchema,
    status: z.literal('saved'),
  })
  .strict()
const deleteAgentProfileRequestSchema = z
  .object({
    id: agentProfileIdSchema,
  })
  .strict()
const deleteAgentProfileResponseSchema = z
  .object({
    id: agentProfileIdSchema,
    status: z.literal('deleted'),
  })
  .strict()
const agentCapabilitiesSchema = z
  .object({
    agentTeamsAvailable: z.boolean(),
    agentTeamsEnabled: z.boolean(),
    backgroundAgentsAvailable: z.boolean(),
    backgroundAgentsEnabled: z.boolean(),
    subagentsAvailable: z.boolean(),
    subagentsEnabled: z.boolean(),
    unavailableReasons: z.array(agentCapabilityUnavailableReasonSchema),
  })
  .strict()
const getExecutionSettingsResponseSchema = z
  .object({
    agentCapabilities: agentCapabilitiesSchema,
    autoModeAvailable: z.boolean(),
    contextCompressionTriggerRatio: contextCompressionTriggerRatioSchema,
    permissionMode: permissionModeSchema,
    scope: settingsScopeSchema,
    toolProfile: toolProfileSchema,
  })
  .strict()

const getExecutionSettingsRequestSchema = z
  .object({
    workspacePath: z.string().trim().min(1).optional(),
  })
  .strict()

const setExecutionSettingsRequestSchema = z
  .object({
    agentTeamsEnabled: z.boolean(),
    backgroundAgentsEnabled: z.boolean(),
    contextCompressionTriggerRatio: contextCompressionTriggerRatioSchema,
    permissionMode: permissionModeSchema,
    subagentsEnabled: z.boolean(),
    toolProfile: toolProfileSchema,
  })
  .strict()

const setExecutionSettingsResponseSchema = z
  .object({
    agentCapabilities: agentCapabilitiesSchema,
    autoModeAvailable: z.boolean(),
    contextCompressionTriggerRatio: contextCompressionTriggerRatioSchema,
    permissionMode: permissionModeSchema,
    scope: settingsScopeSchema,
    toolProfile: toolProfileSchema,
  })
  .strict()

const providerProbeStatusSchema = z.enum([
  'online',
  'timeout',
  'unauthenticated',
  'rate_limited',
  'unsupported',
  'failed',
])

const providerProbeErrorKindSchema = z.enum([
  'timeout',
  'auth',
  'rate_limit',
  'network',
  'provider',
  'unsupported',
  'invalid_config',
  'unknown',
])

const usageSnapshotSchema = z
  .object({
    cacheReadTokens: z.number().int().nonnegative(),
    cacheWriteTokens: z.number().int().nonnegative(),
    costMicros: z.number().int().nonnegative(),
    inputTokens: z.number().int().nonnegative(),
    outputTokens: z.number().int().nonnegative(),
    toolCalls: z.number().int().nonnegative(),
  })
  .strict()

const providerProbeSnapshotSchema = z
  .object({
    checkedAt: isoDateTimeSchema,
    configId: z.string().min(1),
    errorKind: providerProbeErrorKindSchema.optional(),
    latencyMs: z.number().int().nonnegative().optional(),
    modelId: z.string().min(1),
    providerId: providerIdSchema,
    safeMessage: z.string().min(1).optional(),
    status: providerProbeStatusSchema,
    timeoutMs: z.number().int().positive(),
  })
  .strict()

const probeProviderConfigRequestSchema = z
  .object({
    configId: z.string().trim().min(1),
    timeoutMs: z.number().int().positive().optional(),
  })
  .strict()

const probeProviderConfigResponseSchema = z
  .object({
    diagnosticUsage: usageSnapshotSchema.optional(),
    snapshot: providerProbeSnapshotSchema,
  })
  .strict()

const listProviderProbeSnapshotsResponseSchema = z
  .object({
    snapshots: z.array(providerProbeSnapshotSchema),
  })
  .strict()

const modelUsagePeriodSchema = z.enum(['today', 'month_to_date', 'all_time'])

const modelUsageBucketSchema = z
  .object({
    key: z.string().min(1),
    providerId: providerIdSchema,
    modelId: z.string().min(1),
    usage: usageSnapshotSchema,
    lastUsedAt: isoDateTimeSchema.optional(),
  })
  .strict()

const modelUsageWindowSchema = z
  .object({
    period: modelUsagePeriodSchema,
    periodStart: isoDateTimeSchema.optional(),
    periodEnd: isoDateTimeSchema.optional(),
    total: usageSnapshotSchema,
    byModel: z.array(modelUsageBucketSchema),
  })
  .strict()

const modelUsageActivityDaySchema = z
  .object({
    date: localDateSchema,
    usage: usageSnapshotSchema,
  })
  .strict()

const modelUsageActivitySchema = z
  .object({
    rangeStart: localDateSchema,
    rangeEnd: localDateSchema,
    days: z.array(modelUsageActivityDaySchema),
    peakDayTokens: z.number().int().nonnegative(),
    currentStreakDays: z.number().int().nonnegative(),
    longestStreakDays: z.number().int().nonnegative(),
    longestTaskDurationMs: z.number().int().nonnegative(),
  })
  .strict()

const getModelUsageSummaryResponseSchema = z
  .object({
    timezoneId: z.string().min(1).optional(),
    timezoneOffsetMinutes: z.number().int(),
    today: modelUsageWindowSchema,
    monthToDate: modelUsageWindowSchema,
    allTime: modelUsageWindowSchema,
    activity: modelUsageActivitySchema,
    generatedAt: isoDateTimeSchema,
  })
  .strict()

const officialQuotaScopeSchema = z.enum(['account', 'project', 'provider', 'model'])

const officialQuotaStatusSchema = z.enum([
  'supported',
  'unsupported',
  'notConfigured',
  'authRequired',
  'failed',
])

const officialQuotaSnapshotSchema = z
  .object({
    billingLabel: z.string().min(1).optional(),
    configId: z.string().min(1),
    expiresAt: isoDateTimeSchema,
    fetchedAt: isoDateTimeSchema,
    isStale: z.boolean(),
    modelId: z.string().min(1).optional(),
    periodEnd: isoDateTimeSchema.optional(),
    periodStart: isoDateTimeSchema.optional(),
    providerId: providerIdSchema,
    quotaRemaining: z.number().int().nonnegative().optional(),
    quotaTotal: z.number().int().nonnegative().optional(),
    quotaUsed: z.number().int().nonnegative().optional(),
    safeMessage: z.string().min(1).optional(),
    scope: officialQuotaScopeSchema,
    sourceUrl: z.string(),
    status: officialQuotaStatusSchema,
    unit: z.string().min(1).optional(),
  })
  .strict()
  .superRefine((value, ctx) => {
    if (value.status !== 'notConfigured' && value.sourceUrl.trim().length === 0) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: 'sourceUrl is required unless status is notConfigured',
        path: ['sourceUrl'],
      })
    }
    if (
      (value.status === 'unsupported' ||
        value.status === 'authRequired' ||
        value.status === 'failed') &&
      !value.safeMessage
    ) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: 'safeMessage is required for unsupported, authRequired, and failed statuses',
        path: ['safeMessage'],
      })
    }
  })

const refreshOfficialQuotaRequestSchema = z
  .object({
    configId: z.string().trim().min(1),
  })
  .strict()

const refreshOfficialQuotaResponseSchema = z
  .object({
    snapshot: officialQuotaSnapshotSchema,
  })
  .strict()

const listOfficialQuotaSnapshotsResponseSchema = z
  .object({
    snapshots: z.array(officialQuotaSnapshotSchema),
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

const capabilityRouteKindSchema = z.enum([
  'image_generation',
  'video_generation',
  'three_d_generation',
  'embedding_generation',
  'file_operation',
  'text_to_speech',
  'speech_to_text',
  'music_generation',
  'moderation',
  'file_management',
  'vector_store_management',
  'batch_job',
  'fine_tuning_job',
  'eval_run',
  'container_session',
  'realtime_session',
  'admin_operation',
  'webhook_verification',
])

const providerCapabilityRouteSchema = z
  .object({
    kind: capabilityRouteKindSchema,
    configId: z.string().min(1),
    providerId: providerIdSchema,
    operationIds: z.array(z.string().min(1)).min(1),
    enabled: z.boolean(),
  })
  .strict()

const listProviderCapabilityRoutesResponseSchema = z
  .object({
    version: z.number().int().nonnegative(),
    routes: z.array(providerCapabilityRouteSchema),
  })
  .strict()

const providerCapabilityRouteOptionSchema = z
  .object({
    kind: capabilityRouteKindSchema,
    configId: z.string().min(1),
    providerId: providerIdSchema,
    operationId: z.string().min(1),
    outputArtifact: modelModalitySchema,
    execution: z.enum(['sync', 'async_job', 'websocket']),
    costRisk: z.enum(['low', 'medium', 'high']),
    runtimeSupported: z.boolean(),
    unavailableReason: z.string().min(1).optional(),
  })
  .strict()

const listProviderCapabilityRouteOptionsResponseSchema = z
  .object({
    options: z.array(providerCapabilityRouteOptionSchema),
  })
  .strict()

const modelSettingsCatalogSnapshotSchema = z
  .object({
    source: z.enum(['bundled', 'snapshot']),
    lastSuccessfulRefreshAt: isoDateTimeSchema.optional(),
    lastAttemptAt: isoDateTimeSchema.optional(),
  })
  .strict()

function modelSettingsPageSliceSchema<T extends z.ZodTypeAny>(dataSchema: T) {
  return z.discriminatedUnion('status', [
    z
      .object({
        status: z.literal('ready'),
        data: dataSchema,
      })
      .strict(),
    z
      .object({
        status: z.literal('rebuilding'),
        safeMessage: z.string().min(1),
      })
      .strict(),
    z
      .object({
        status: z.literal('error'),
        safeMessage: z.string().min(1),
      })
      .strict(),
  ])
}

const modelSettingsPageResponseSchema = z
  .object({
    catalog: modelProviderCatalogResponseSchema,
    catalogSnapshot: modelSettingsCatalogSnapshotSchema,
    providerSettings: listProviderSettingsResponseSchema,
    probeSnapshots: modelSettingsPageSliceSchema(listProviderProbeSnapshotsResponseSchema),
    usageSummary: modelSettingsPageSliceSchema(getModelUsageSummaryResponseSchema),
    quotaSnapshots: modelSettingsPageSliceSchema(listOfficialQuotaSnapshotsResponseSchema),
    capabilityRoutes: modelSettingsPageSliceSchema(listProviderCapabilityRoutesResponseSchema),
    capabilityRouteOptions: modelSettingsPageSliceSchema(
      listProviderCapabilityRouteOptionsResponseSchema,
    ),
    generatedAt: isoDateTimeSchema,
  })
  .strict()

const refreshModelProviderCatalogResponseSchema = z
  .object({
    catalog: modelProviderCatalogResponseSchema,
    catalogSnapshot: modelSettingsCatalogSnapshotSchema,
  })
  .strict()

const saveProviderCapabilityRouteRequestSchema = z
  .object({
    route: providerCapabilityRouteSchema,
  })
  .strict()

const saveProviderCapabilityRouteResponseSchema = z
  .object({
    version: z.number().int().nonnegative(),
    routes: z.array(providerCapabilityRouteSchema),
    status: z.literal('saved'),
  })
  .strict()

const deleteProviderCapabilityRouteRequestSchema = z
  .object({
    kind: capabilityRouteKindSchema,
    configId: z.string().min(1),
    providerId: providerIdSchema,
  })
  .strict()

const deleteProviderCapabilityRouteResponseSchema = z
  .object({
    version: z.number().int().nonnegative(),
    routes: z.array(providerCapabilityRouteSchema),
    status: z.literal('deleted'),
  })
  .strict()

const mcpServerIdSchema = z
  .string()
  .trim()
  .regex(/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/)

const mcpServerScopeSchema = z.enum(['agent', 'global', 'session'])

const mcpConfigLayerSchema = z.enum(['global', 'project'])

const mcpProjectPathSchema = z.string().trim().min(1)

function requireMcpProjectIdentity(
  value: { configLayer: z.infer<typeof mcpConfigLayerSchema>; projectPath?: string | null },
  context: z.RefinementCtx,
): void {
  if (value.configLayer === 'project' && !value.projectPath) {
    context.addIssue({
      code: 'custom',
      message: 'MCP project mutations require projectPath',
      path: ['projectPath'],
    })
  }
  if (value.configLayer === 'global' && value.projectPath != null) {
    context.addIssue({
      code: 'custom',
      message: 'MCP global mutations must not include projectPath',
      path: ['projectPath'],
    })
  }
}

const mcpServerTransportKindSchema = z.enum(['http', 'inProcess', 'sse', 'stdio', 'websocket'])

const mcpServerStatusSchema = z.enum([
  'closed',
  'configured',
  'connecting',
  'disabled',
  'failed',
  'ready',
  'reconnecting',
])

const mcpServerOriginSchema = z.enum([
  'managed',
  'plugin',
  'policy',
  'project',
  'user',
  'workspace',
])

const mcpStatusSourceSchema = z.literal('settings')

const mcpDiagnosticSeveritySchema = z.enum(['info', 'warning', 'error'])

const mcpDiagnosticPlaneSchema = z.enum(['settings', 'task'])

const utf8Encoder = new TextEncoder()

function hasMaxUtf8Bytes(value: string, maxBytes: number): boolean {
  return utf8Encoder.encode(value).byteLength <= maxBytes
}

const mcpDiagnosticSummarySchema = z
  .string()
  .min(1)
  .refine((value) => !hasObviousUnredactedSecret(value), {
    message: 'MCP diagnostic summary must not contain obvious unredacted secrets',
  })
  .refine((value) => !hasPrivateAbsolutePath(value), {
    message: 'MCP diagnostic summary must not contain private absolute paths',
  })

const mcpDiagnosticRecordSchema = z
  .object({
    eventType: z.string().min(1),
    id: z.string().min(1),
    plane: mcpDiagnosticPlaneSchema.default('settings'),
    runId: z.string().min(1).optional(),
    runSegmentId: z.string().min(1).optional(),
    serverId: mcpServerIdSchema,
    sessionId: z.string().min(1).optional(),
    severity: mcpDiagnosticSeveritySchema,
    summary: mcpDiagnosticSummarySchema,
    taskId: z.string().min(1).optional(),
    timestamp: z.string().min(1),
  })
  .strict()

const mcpServerSummarySchema = z
  .object({
    configLayer: mcpConfigLayerSchema,
    displayName: z
      .string()
      .min(1)
      .max(256)
      .refine((value) => hasMaxUtf8Bytes(value, 256)),
    effective: z.boolean(),
    enabled: z.boolean(),
    exposedToolCount: z.number().int().min(0),
    id: mcpServerIdSchema,
    lastDiagnostic: mcpDiagnosticSummarySchema.optional(),
    lastDiagnosticAt: z.string().min(1).optional(),
    lastDiagnosticSeverity: mcpDiagnosticSeveritySchema.optional(),
    lastError: z.string().min(1).optional(),
    manageable: z.boolean(),
    overridesGlobal: z.boolean(),
    origin: mcpServerOriginSchema,
    required: z.boolean(),
    scope: mcpServerScopeSchema,
    sourcePluginId: z.string().min(1).optional(),
    status: mcpServerStatusSchema,
    statusSource: mcpStatusSourceSchema,
    transport: mcpServerTransportKindSchema,
  })
  .strict()

const listMcpServersResponseSchema = z
  .object({
    configLayer: mcpConfigLayerSchema,
    servers: z.array(mcpServerSummarySchema),
  })
  .strict()

const browserMcpPresetIdSchema = z.enum(['playwright', 'chrome-devtools'])

const browserMcpPresetSchema = z
  .object({
    description: z.string().min(1),
    displayName: z.string().min(1),
    enabled: z.boolean(),
    id: browserMcpPresetIdSchema,
    serverId: mcpServerIdSchema,
    version: z.string().trim().min(1),
  })
  .strict()

const listBrowserMcpPresetsResponseSchema = z
  .object({
    presets: z.array(browserMcpPresetSchema),
  })
  .strict()

const saveBrowserMcpPresetRequestSchema = z
  .object({
    enabled: z.boolean().default(false),
    presetId: browserMcpPresetIdSchema,
  })
  .strict()

const saveBrowserMcpPresetResponseSchema = z
  .object({
    preset: browserMcpPresetSchema,
    server: mcpServerSummarySchema,
  })
  .strict()

const mcpEnvVarNameSchema = z
  .string()
  .trim()
  .regex(/^[A-Za-z_][A-Za-z0-9_]*$/)

function isSensitiveEnvName(value: string): boolean {
  const normalized = value.toLowerCase().replaceAll('-', '_')
  return [
    'auth',
    'api_key',
    'apikey',
    'authorization',
    'bearer',
    'password',
    'secret',
    'token',
  ].some((marker) => normalized.includes(marker))
}

function isSensitiveHeaderName(value: string): boolean {
  return /^(?:authorization|cookie|set-cookie|proxy-authorization)$/i.test(value.trim())
}

function hasNul(value: string): boolean {
  return value.includes('\0')
}

function isValidMcpHeaderName(value: string): boolean {
  return /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/.test(value)
}

function isValidMcpHeaderValue(value: string): boolean {
  return [...value].every((character) => {
    const codePoint = character.codePointAt(0) ?? 0
    return codePoint === 9 || (codePoint >= 32 && codePoint !== 127)
  })
}

function isSecretBearingMcpHeaderValue(value: string): boolean {
  const normalized = value.trim().toLowerCase()
  return (
    normalized.startsWith('bearer ') ||
    normalized.startsWith('oauth ') ||
    normalized.includes(' token') ||
    normalized.includes('secret') ||
    normalized.includes('password')
  )
}

function looksLikeRawMcpSecret(value: string): boolean {
  const trimmed = value.trim()
  const lower = trimmed.toLowerCase()
  const knownPrefix = ['ghp_', 'github_pat_', 'glpat-', 'sk-', 'xoxb-', 'xoxp-', 'xoxa-'].some(
    (prefix) => lower.startsWith(prefix),
  )
  return knownPrefix || (trimmed.length >= 32 && /^[A-Za-z0-9_.=/+-]+$/.test(trimmed))
}

function isSafeMcpHttpUrl(value: string): boolean {
  try {
    const parsed = new URL(value)
    if (
      !['http:', 'https:'].includes(parsed.protocol) ||
      parsed.hostname.length === 0 ||
      parsed.username.length > 0 ||
      parsed.password.length > 0
    ) {
      return false
    }
    for (const [key, queryValue] of parsed.searchParams) {
      if (
        isSensitiveEnvName(key) ||
        hasObviousUnredactedSecret(queryValue) ||
        looksLikeRawMcpSecret(queryValue)
      ) {
        return false
      }
    }
    return true
  } catch {
    return false
  }
}

const mcpNameValueConfigSchema = z
  .object({
    hasValue: z.boolean(),
    key: z.string().trim().min(1),
    value: z
      .string()
      .max(8192)
      .refine((value) => !hasNul(value))
      .optional(),
  })
  .strict()

const mcpStdioEnvConfigSchema = mcpNameValueConfigSchema
  .refine((record) => mcpEnvVarNameSchema.safeParse(record.key).success, {
    message: 'MCP stdio env key must be an environment variable name',
  })
  .refine((record) => !isSensitiveEnvName(record.key), {
    message: 'MCP stdio inline env must not contain secret-bearing keys',
  })
  .refine((record) => record.value == null || !hasObviousUnredactedSecret(record.value), {
    message: 'MCP stdio inline env must not contain obvious unredacted secrets',
  })
  .refine((record) => record.value == null || !looksLikeRawMcpSecret(record.value), {
    message: 'MCP stdio inline env must not contain secret-bearing values',
  })
  .refine((record) => record.value == null || hasMaxUtf8Bytes(record.value, 4096), {
    message: 'MCP stdio inline env values must contain at most 4096 bytes',
  })

const mcpHttpHeaderConfigSchema = mcpNameValueConfigSchema
  .refine((record) => isValidMcpHeaderName(record.key.trim()), {
    message: 'MCP static header names must use RFC field-name characters',
  })
  .refine((record) => !isSensitiveHeaderName(record.key), {
    message: 'MCP static headers must not contain sensitive header names',
  })
  .refine((record) => record.value == null || !hasObviousUnredactedSecret(record.value), {
    message: 'MCP static headers must not contain obvious unredacted secrets',
  })
  .refine((record) => record.value == null || !looksLikeRawMcpSecret(record.value), {
    message: 'MCP static headers must not contain secret-bearing values',
  })
  .refine((record) => record.value == null || hasMaxUtf8Bytes(record.value, 8192), {
    message: 'MCP static header values must contain at most 8192 bytes',
  })
  .refine((record) => record.value == null || isValidMcpHeaderValue(record.value), {
    message: 'MCP static header values must not contain control characters',
  })
  .refine((record) => record.value == null || !isSecretBearingMcpHeaderValue(record.value), {
    message: 'MCP static header values must not contain secret-bearing content',
  })

const mcpNameValueSaveRecordSchema = z
  .object({
    key: z.string().trim().min(1),
    preserveExisting: z.boolean().optional(),
    value: z
      .string()
      .max(8192)
      .refine((value) => !hasNul(value))
      .optional(),
  })
  .strict()
  .superRefine((record, context) => {
    const hasValue = typeof record.value === 'string'
    if (record.preserveExisting && hasValue) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        message: 'MCP preserveExisting records must not include a replacement value',
        path: ['value'],
      })
    }
    if (!record.preserveExisting && !hasValue) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        message: 'MCP records must include value or preserveExisting',
        path: ['value'],
      })
    }
    if (hasValue && record.value?.trim().length === 0) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        message: 'MCP record value must not be empty',
        path: ['value'],
      })
    }
  })

const mcpStdioEnvRecordSchema = mcpNameValueSaveRecordSchema
  .refine((record) => mcpEnvVarNameSchema.safeParse(record.key).success, {
    message: 'MCP stdio env key must be an environment variable name',
  })
  .refine((record) => !isSensitiveEnvName(record.key), {
    message: 'MCP stdio inline env must not contain secret-bearing keys',
  })
  .refine((record) => record.value == null || !hasObviousUnredactedSecret(record.value), {
    message: 'MCP stdio inline env must not contain obvious unredacted secrets',
  })
  .refine((record) => record.value == null || !looksLikeRawMcpSecret(record.value), {
    message: 'MCP stdio inline env must not contain secret-bearing values',
  })
  .refine((record) => record.value == null || hasMaxUtf8Bytes(record.value, 4096), {
    message: 'MCP stdio inline env values must contain at most 4096 bytes',
  })

const mcpHttpHeaderRecordSchema = mcpNameValueSaveRecordSchema
  .refine((record) => isValidMcpHeaderName(record.key.trim()), {
    message: 'MCP static header names must use RFC field-name characters',
  })
  .refine((record) => !isSensitiveHeaderName(record.key), {
    message: 'MCP static headers must not contain sensitive header names',
  })
  .refine((record) => record.value == null || !hasObviousUnredactedSecret(record.value), {
    message: 'MCP static headers must not contain obvious unredacted secrets',
  })
  .refine((record) => record.value == null || !looksLikeRawMcpSecret(record.value), {
    message: 'MCP static headers must not contain secret-bearing values',
  })
  .refine((record) => record.value == null || hasMaxUtf8Bytes(record.value, 8192), {
    message: 'MCP static header values must contain at most 8192 bytes',
  })
  .refine((record) => record.value == null || isValidMcpHeaderValue(record.value), {
    message: 'MCP static header values must not contain control characters',
  })
  .refine((record) => record.value == null || !isSecretBearingMcpHeaderValue(record.value), {
    message: 'MCP static header values must not contain secret-bearing content',
  })

const mcpHeaderEnvRecordSchema = z
  .object({
    envVar: mcpEnvVarNameSchema,
    key: z.string().trim().min(1),
  })
  .strict()
  .refine((record) => !isSensitiveHeaderName(record.key), {
    message: 'MCP headers from env must not contain sensitive header names',
  })
  .refine((record) => isValidMcpHeaderName(record.key.trim()), {
    message: 'MCP headers from env must use RFC field-name characters',
  })

const mcpStdioTransportRequestSchema = z
  .object({
    args: z
      .array(
        z
          .string()
          .min(1)
          .max(4096)
          .refine((value) => hasMaxUtf8Bytes(value, 4096))
          .refine((value) => value.trim().length > 0 && !hasNul(value)),
      )
      .max(64)
      .default([]),
    command: z
      .string()
      .trim()
      .min(1)
      .max(4096)
      .refine((value) => hasMaxUtf8Bytes(value, 4096))
      .refine((value) => !hasNul(value)),
    env: z.array(mcpStdioEnvRecordSchema).max(64).default([]),
    inheritEnv: z
      .array(
        mcpEnvVarNameSchema.refine((value) => !isSensitiveEnvName(value), {
          message: 'MCP inherited env must not contain secret-bearing names',
        }),
      )
      .max(128)
      .default([]),
    kind: z.literal('stdio'),
    workingDir: z
      .string()
      .trim()
      .min(1)
      .max(4096)
      .refine((value) => hasMaxUtf8Bytes(value, 4096))
      .refine((value) => !hasNul(value))
      .optional(),
  })
  .strict()

const mcpHttpTransportRequestSchema = z
  .object({
    bearerTokenEnvVar: mcpEnvVarNameSchema.optional(),
    headers: z.array(mcpHttpHeaderRecordSchema).max(64).default([]),
    headersFromEnv: z.array(mcpHeaderEnvRecordSchema).max(64).default([]),
    kind: z.literal('http'),
    url: z.string().trim().url().refine(isSafeMcpHttpUrl, {
      message: 'MCP HTTP URL must be a safe http or https URL',
    }),
  })
  .strict()

const mcpServerTransportRequestSchema = z.discriminatedUnion('kind', [
  mcpStdioTransportRequestSchema,
  mcpHttpTransportRequestSchema,
])

const saveMcpServerRequestSchema = z
  .object({
    configLayer: mcpConfigLayerSchema,
    displayName: z
      .string()
      .trim()
      .min(1)
      .max(256)
      .refine((value) => hasMaxUtf8Bytes(value, 256))
      .refine((value) => !hasNul(value)),
    enabled: z.boolean().default(true),
    id: mcpServerIdSchema,
    projectPath: mcpProjectPathSchema.nullable().optional(),
    required: z.boolean().default(false),
    scope: mcpServerScopeSchema,
    transport: mcpServerTransportRequestSchema,
  })
  .strict()
  .superRefine(requireMcpProjectIdentity)

const saveMcpServerResponseSchema = z
  .object({
    server: mcpServerSummarySchema,
  })
  .strict()

const mcpServerConfigSchema = z
  .object({
    configLayer: mcpConfigLayerSchema,
    displayName: z
      .string()
      .trim()
      .min(1)
      .max(256)
      .refine((value) => hasMaxUtf8Bytes(value, 256)),
    effective: z.boolean(),
    enabled: z.boolean(),
    id: mcpServerIdSchema,
    manageable: z.boolean(),
    overridesGlobal: z.boolean(),
    required: z.boolean(),
    scope: mcpServerScopeSchema,
    transport: z.discriminatedUnion('kind', [
      mcpStdioTransportRequestSchema.extend({
        env: z.array(mcpStdioEnvConfigSchema).max(64).default([]),
      }),
      mcpHttpTransportRequestSchema.extend({
        headers: z.array(mcpHttpHeaderConfigSchema).max(64).default([]),
      }),
      z.object({ kind: z.literal('inProcess') }).strict(),
    ]),
  })
  .strict()

const getMcpServerConfigRequestSchema = z
  .object({
    configLayer: mcpConfigLayerSchema,
    id: mcpServerIdSchema,
  })
  .strict()

const getMcpServerConfigResponseSchema = z
  .object({
    server: mcpServerConfigSchema,
  })
  .strict()

const deleteMcpServerRequestSchema = z
  .object({
    configLayer: mcpConfigLayerSchema,
    id: mcpServerIdSchema,
    projectPath: mcpProjectPathSchema.nullable().optional(),
  })
  .strict()
  .superRefine(requireMcpProjectIdentity)

const deleteMcpServerResponseSchema = z
  .object({
    configLayer: mcpConfigLayerSchema,
    id: mcpServerIdSchema,
    status: z.literal('deleted'),
  })
  .strict()

const setMcpServerEnabledRequestSchema = z
  .object({
    configLayer: mcpConfigLayerSchema,
    enabled: z.boolean(),
    id: mcpServerIdSchema,
    projectPath: mcpProjectPathSchema.nullable().optional(),
  })
  .strict()
  .superRefine(requireMcpProjectIdentity)

const setMcpServerEnabledResponseSchema = z
  .object({
    server: mcpServerSummarySchema,
  })
  .strict()

const restartMcpServerRequestSchema = z
  .object({
    configLayer: mcpConfigLayerSchema,
    id: mcpServerIdSchema,
    projectPath: mcpProjectPathSchema.nullable().optional(),
  })
  .strict()
  .superRefine(requireMcpProjectIdentity)

const restartMcpServerResponseSchema = z
  .object({
    server: mcpServerSummarySchema,
  })
  .strict()

const listMcpDiagnosticsRequestSchema = z
  .object({
    serverId: mcpServerIdSchema.optional(),
  })
  .strict()

const listMcpDiagnosticsResponseSchema = z
  .object({
    events: z.array(mcpDiagnosticRecordSchema),
  })
  .strict()

const clearMcpDiagnosticsRequestSchema = listMcpDiagnosticsRequestSchema

const clearMcpDiagnosticsResponseSchema = z
  .object({
    status: z.literal('cleared'),
  })
  .strict()

const subscribeMcpDiagnosticsRequestSchema = listMcpDiagnosticsRequestSchema

const subscribeMcpDiagnosticsResponseSchema = z
  .object({
    replayEvents: z.array(mcpDiagnosticRecordSchema),
    serverId: mcpServerIdSchema.optional(),
    subscriptionId: z.string().min(1),
  })
  .strict()

const unsubscribeMcpDiagnosticsRequestSchema = z
  .object({
    subscriptionId: z.string().min(1),
  })
  .strict()

const unsubscribeMcpDiagnosticsResponseSchema = z
  .object({
    status: z.enum(['alreadyClosed', 'unsubscribed']),
    subscriptionId: z.string().min(1),
  })
  .strict()

const mcpDiagnosticBatchPayloadSchema = z
  .object({
    events: z.array(mcpDiagnosticRecordSchema),
    phase: z.literal('live'),
    serverId: mcpServerIdSchema.optional(),
    subscriptionId: z.string().min(1),
  })
  .strict()

const skillIdSchema = z.string().trim().min(1)
const skillSourceKindSchema = z.enum(['workspace', 'user', 'bundled', 'plugin', 'mcp'])
const skillStatusSchema = z.enum(['ready', 'prerequisite_missing', 'disabled', 'rejected'])
const skillParamTypeSchema = z.enum(['string', 'number', 'boolean', 'path', 'url'])
const skillFileKindSchema = z.enum(['directory', 'file'])
const skillCatalogSourceIdSchema = z.string().trim().min(1)
const skillCatalogTrustLevelSchema = z.enum(['official', 'standard', 'curated', 'community'])

const skillInstallOriginSchema = z
  .object({
    commitSha: z.string().min(1).optional(),
    entryId: z.string().trim().min(1),
    homepageUrl: z.url().optional(),
    installedFromCatalog: z.literal(true),
    sourceId: skillCatalogSourceIdSchema,
    sourceLabel: z.string().min(1),
    version: z.string().min(1).optional(),
  })
  .strict()

const skillSummarySchema = z
  .object({
    category: z.string().min(1).optional(),
    description: z.string(),
    enabled: z.boolean(),
    id: skillIdSchema,
    importedAt: z.string().datetime({ offset: true }).optional(),
    manageable: z.boolean(),
    name: z.string().min(1),
    origin: skillInstallOriginSchema.optional(),
    sourcePluginId: z.string().min(1).optional(),
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

const skillScriptEnvSchema = z
  .object({
    configKey: z.string().trim().min(1),
    name: z.string().trim().min(1),
    secret: z.boolean(),
  })
  .strict()

const skillScriptSchema = z
  .object({
    env: z.array(skillScriptEnvSchema),
    id: z.string().trim().min(1),
    maxArtifactBytes: z.number().int().nonnegative(),
    maxArtifactCount: z.number().int().nonnegative(),
    maxOutputBytes: z.number().int().nonnegative(),
    maxStderrBytes: z.number().int().nonnegative(),
    maxStdoutBytes: z.number().int().nonnegative(),
    network: z.literal('deny'),
    path: z.string().trim().min(1),
    timeoutSeconds: z.number().int().positive(),
  })
  .strict()

const skillPrerequisiteSchema = z
  .object({
    missingConfigKeys: z.array(z.string().trim().min(1)),
    missingEnvVars: z.array(z.string().trim().min(1)),
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
    prerequisites: skillPrerequisiteSchema,
    scripts: z.array(skillScriptSchema),
    summary: skillSummarySchema,
    validationError: z.string().optional(),
  })
  .strict()

const skillConfigJsonSchema: z.ZodType<unknown> = z.lazy(() =>
  z.union([
    z.null(),
    z.boolean(),
    z.number(),
    z.string(),
    z.array(skillConfigJsonSchema),
    z.record(z.string(), skillConfigJsonSchema),
  ]),
)

const skillConfigDeclarationSchema = z
  .object({
    default: skillConfigJsonSchema.optional(),
    description: z.string().optional(),
    key: z.string().trim().min(1),
    required: z.boolean(),
    secret: z.boolean(),
    valueType: skillParamTypeSchema,
  })
  .strict()

const skillConfigEntrySchema = z
  .object({
    secrets: z.record(z.string().trim().min(1), z.object({ configured: z.boolean() }).strict()),
    values: z.record(z.string().trim().min(1), skillConfigJsonSchema),
  })
  .strict()

const getSkillConfigRequestSchema = z
  .object({
    skillId: skillIdSchema,
  })
  .strict()

const getSkillConfigResponseSchema = z
  .object({
    config: skillConfigEntrySchema,
    declarations: z.array(skillConfigDeclarationSchema),
    skillId: skillIdSchema,
  })
  .strict()

const setSkillConfigValueRequestSchema = z
  .object({
    key: z.string().trim().min(1),
    skillId: skillIdSchema,
    value: skillConfigJsonSchema,
  })
  .strict()

const setSkillSecretRequestSchema = z
  .object({
    key: z.string().trim().min(1),
    skillId: skillIdSchema,
    value: z.string().min(1),
  })
  .strict()

const clearSkillSecretRequestSchema = z
  .object({
    key: z.string().trim().min(1),
    skillId: skillIdSchema,
  })
  .strict()

const skillConfigMutationResponseSchema = z
  .object({
    configured: z.boolean(),
    key: z.string().trim().min(1),
    skillId: skillIdSchema,
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

const pluginIdSchema = z.string().trim().min(1)
const pluginSourceKindSchema = z.enum(['user', 'workspace', 'project', 'cargo_extension', 'inline'])
const pluginTrustLevelSchema = z.enum(['admin_trusted', 'user_controlled'])
const pluginLifecycleStateSchema = z.enum([
  'validated',
  'activating',
  'activated',
  'deactivating',
  'deactivated',
  'rejected',
  'failed',
])
const pluginProductStateSchema = z.union([
  z.enum([
    'discovered',
    'validated',
    'activating',
    'activated',
    'rejected',
    'failed',
    'deactivated',
  ]),
  z
    .object({
      disabled: z
        .object({
          last_state: pluginLifecycleStateSchema.nullable().optional(),
        })
        .strict(),
    })
    .strict(),
])
const pluginRuntimeCapabilityKindSchema = z.enum([
  'tool',
  'hook',
  'mcp_server',
  'skill',
  'steering',
  'memory_provider',
  'coordinator',
  'custom_toolset',
])
const pluginRuntimeCapabilitySchema = z
  .object({
    kind: pluginRuntimeCapabilityKindSchema,
    name: z.string().min(1).optional(),
    destructive: z.boolean().optional(),
    registered: z.boolean(),
  })
  .strict()
const pluginSummarySchema = z
  .object({
    id: pluginIdSchema,
    name: z.string().min(1),
    version: z.string().min(1),
    description: z.string().optional(),
    source: pluginSourceKindSchema,
    trustLevel: pluginTrustLevelSchema,
    enabled: z.boolean(),
    state: pluginProductStateSchema,
    capabilities: z.array(pluginRuntimeCapabilitySchema),
    warnings: z.array(z.string()),
  })
  .strict()
const pluginManifestOriginSchema = z.union([
  z
    .object({
      file: z
        .object({
          path: z.string().min(1),
        })
        .strict(),
    })
    .strict(),
  z
    .object({
      cargo_extension: z
        .object({
          binary: z.string().min(1),
        })
        .strict(),
    })
    .strict(),
  z
    .object({
      remote_registry: z
        .object({
          endpoint: z.string().min(1),
        })
        .strict(),
    })
    .strict(),
])
const pluginJsonSchema: z.ZodType<unknown> = z.lazy(() =>
  z.union([
    z.null(),
    z.boolean(),
    z.number(),
    z.string(),
    z.array(pluginJsonSchema),
    z.record(z.string(), pluginJsonSchema),
  ]),
)

function hasUnredactedSecretInJson(value: unknown): boolean {
  if (typeof value === 'string') {
    return hasObviousUnredactedSecret(value)
  }

  if (Array.isArray(value)) {
    return value.some((item) => hasUnredactedSecretInJson(item))
  }

  if (value && typeof value === 'object') {
    return Object.entries(value).some(([key, item]) => {
      if (/(?:api_?key|auth|authorization|bearer|password|secret|token)/i.test(key)) {
        return typeof item === 'string' && item.trim().length > 0
      }

      return hasUnredactedSecretInJson(item)
    })
  }

  return false
}

const pluginConfigValuesSchema = z
  .record(z.string().trim().min(1), pluginJsonSchema)
  .refine((value) => !hasUnredactedSecretInJson(value), {
    message: 'Plugin config values must not contain unredacted secrets',
  })
const pluginRecentEventSchema = z.enum(['loaded', 'failed', 'rejected', 'deactivated'])
const pluginDetailSchema = z
  .object({
    summary: pluginSummarySchema,
    manifestOrigin: pluginManifestOriginSchema,
    manifestHash: z.array(z.number().int().min(0).max(255)).length(32),
    manifest: pluginJsonSchema,
    configurationSchema: pluginJsonSchema.optional(),
    config: pluginConfigValuesSchema.or(z.null()),
    registeredCapabilities: z.array(pluginRuntimeCapabilitySchema),
    recentEvents: z.array(pluginRecentEventSchema),
    rejectionReason: z.unknown().optional(),
    failure: z.string().optional(),
  })
  .strict()
const pluginInstallReportSchema = z
  .object({
    sourcePath: z.string().min(1),
    valid: z.boolean(),
    summary: pluginSummarySchema.optional(),
    warnings: z.array(z.string()),
    reason: z.string().optional(),
  })
  .strict()
const pluginOperationStatusSchema = z.enum([
  'rejected',
  'installed',
  'enabled',
  'disabled',
  'configured',
  'uninstalled',
  'reloaded',
])
const pluginOperationResultSchema = z
  .object({
    pluginId: pluginIdSchema.optional(),
    status: pluginOperationStatusSchema,
    summary: pluginSummarySchema.optional(),
    report: pluginInstallReportSchema.optional(),
  })
  .strict()
const listPluginsResponseSchema = z
  .object({
    allowProjectPlugins: z.boolean(),
    plugins: z.array(pluginSummarySchema),
  })
  .strict()
const getPluginDetailRequestSchema = z
  .object({
    pluginId: pluginIdSchema,
  })
  .strict()
const getPluginDetailResponseSchema = z
  .object({
    plugin: pluginDetailSchema,
  })
  .strict()
const pluginPathRequestSchema = z
  .object({
    sourcePath: z.string().trim().min(1),
  })
  .strict()
const setPluginEnabledRequestSchema = z
  .object({
    pluginId: pluginIdSchema,
    enabled: z.boolean(),
  })
  .strict()
const setProjectPluginsEnabledRequestSchema = z
  .object({
    enabled: z.boolean(),
  })
  .strict()
const setProjectPluginsEnabledResponseSchema = z
  .object({
    allowProjectPlugins: z.boolean(),
  })
  .strict()
const updatePluginConfigRequestSchema = z
  .object({
    pluginId: pluginIdSchema,
    values: pluginConfigValuesSchema,
  })
  .strict()
const pluginIdRequestSchema = getPluginDetailRequestSchema

const skillCatalogSourceSchema = z
  .object({
    description: z.string(),
    id: skillCatalogSourceIdSchema,
    installable: z.boolean(),
    label: z.string().min(1),
    trustLevel: skillCatalogTrustLevelSchema,
  })
  .strict()

const skillCatalogEntrySchema = z
  .object({
    description: z.string(),
    entryId: z.string().trim().min(1),
    homepageUrl: z.url().optional(),
    installable: z.boolean(),
    installed: z.boolean(),
    name: z.string().min(1),
    sourceId: skillCatalogSourceIdSchema,
    sourceLabel: z.string().min(1),
    tags: z.array(z.string()),
    trustLevel: skillCatalogTrustLevelSchema,
    version: z.string().min(1).optional(),
  })
  .strict()

const skillCatalogValidationSchema = z
  .object({
    issueCodes: z.array(z.string().min(1)).optional(),
    issues: z.array(z.string()),
    status: z.enum(['ready', 'warning', 'blocked']),
  })
  .strict()

const listSkillCatalogSourcesResponseSchema = z
  .object({
    sources: z.array(skillCatalogSourceSchema),
  })
  .strict()

const listSkillCatalogEntriesRequestSchema = z
  .object({
    cursor: z.string().trim().min(1).optional(),
    limit: z.number().int().min(1).max(100).optional(),
    query: z.string().trim().optional(),
    sort: z.enum(['recommended', 'updated', 'downloads', 'trending']).optional(),
    sourceId: skillCatalogSourceIdSchema,
  })
  .strict()

const listSkillCatalogEntriesResponseSchema = z
  .object({
    entries: z.array(skillCatalogEntrySchema),
    nextCursor: z.string().min(1).optional(),
  })
  .strict()

const getSkillCatalogEntryRequestSchema = z
  .object({
    entryId: z.string().trim().min(1),
    sourceId: skillCatalogSourceIdSchema,
    version: z.string().trim().min(1).optional(),
  })
  .strict()

const catalogFilePathSchema = z
  .string()
  .trim()
  .min(1)
  .refine(
    (path) =>
      !path.startsWith('/') &&
      !/^[A-Za-z]:[\\/]/.test(path) &&
      !path.split('/').some((segment) => segment === '..'),
    'Catalog file path must be relative and stay inside the skill package.',
  )

const getSkillCatalogFileRequestSchema = getSkillCatalogEntryRequestSchema
  .extend({
    path: catalogFilePathSchema,
  })
  .strict()

const skillCatalogFileSchema = z
  .object({
    kind: skillFileKindSchema,
    path: z.string().min(1),
    sizeBytes: z.number().int().nonnegative().optional(),
  })
  .strict()

const getSkillCatalogEntryResponseSchema = z
  .object({
    entry: skillCatalogEntrySchema,
    files: z.array(skillCatalogFileSchema).optional(),
    readmePreview: z.string().optional(),
    validation: skillCatalogValidationSchema,
  })
  .strict()

const skillCatalogFileContentSchema = z
  .object({
    content: z.string(),
    path: z.string().min(1),
    truncated: z.boolean(),
  })
  .strict()

const getSkillCatalogFileResponseSchema = z
  .object({
    file: skillCatalogFileContentSchema,
  })
  .strict()

const installSkillFromCatalogRequestSchema = getSkillCatalogEntryRequestSchema
  .extend({
    operationId: z.string().trim().min(1).optional(),
  })
  .strict()

const skillCatalogInstallTaskSchema = z
  .object({
    entryId: z.string().min(1),
    message: z.string().min(1).optional(),
    operationId: z.string().min(1),
    percent: z.number().int().min(0).max(100),
    sourceId: skillCatalogSourceIdSchema,
    stage: z.enum([
      'preparing',
      'resolving',
      'checking',
      'downloading',
      'validating',
      'copying',
      'reloading',
      'completed',
      'failed',
      'interrupted',
    ]),
    startedAt: z.string().datetime({ offset: true }),
    status: z.enum(['running', 'completed', 'failed', 'interrupted']),
    updatedAt: z.string().datetime({ offset: true }),
    version: z.string().min(1).optional(),
  })
  .strict()

const listSkillCatalogInstallTasksResponseSchema = z
  .object({
    tasks: z.array(skillCatalogInstallTaskSchema),
  })
  .strict()

const installSkillFromCatalogResponseSchema = z
  .object({
    task: skillCatalogInstallTaskSchema,
  })
  .strict()

const skillCatalogInstallProgressPayloadSchema = z
  .object({
    entryId: z.string().min(1),
    message: z.string().min(1).optional(),
    operationId: z.string().min(1),
    percent: z.number().int().min(0).max(100),
    sourceId: skillCatalogSourceIdSchema,
    stage: z.enum([
      'preparing',
      'resolving',
      'checking',
      'downloading',
      'validating',
      'copying',
      'reloading',
      'completed',
      'failed',
      'interrupted',
    ]),
    version: z.string().min(1).optional(),
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

const defaultWorkspaceResponseSchema = z
  .object({
    path: z.string().min(1),
  })
  .strict()

const switchProjectRequestSchema = z
  .object({
    path: z.string().min(1),
  })
  .strict()

const moveProjectRequestSchema = z
  .object({
    direction: z.enum(['up', 'down']),
    path: z.string().min(1),
  })
  .strict()

const renameProjectRequestSchema = z
  .object({
    name: z.string().trim().min(1).max(120),
    path: z.string().min(1),
  })
  .strict()

const switchProjectResponseSchema = z
  .object({
    project: projectRecordSchema,
  })
  .strict()

const deleteProjectRequestSchema = z
  .object({
    path: z.string().min(1),
  })
  .strict()

const deleteProjectResponseSchema = z
  .object({
    activePath: z.string().min(1).nullable(),
    path: z.string().min(1),
    status: z.literal('deleted'),
  })
  .strict()

export type AppInfo = z.infer<typeof appInfoSchema>
export type RuntimeExecutionStatus = z.infer<typeof runtimeExecutionStatusSchema>
export type RuntimeToolSummary = z.infer<typeof runtimeToolSummarySchema>
export type ListRuntimeToolsResponse = z.infer<typeof listRuntimeToolsResponseSchema>
export type SetRuntimeToolEnabledRequest = z.infer<typeof setRuntimeToolEnabledRequestSchema>
export type ListProjectsResponse = z.infer<typeof listProjectsResponseSchema>
export type DefaultWorkspaceResponse = z.infer<typeof defaultWorkspaceResponseSchema>
export type MoveProjectDirection = z.infer<typeof moveProjectRequestSchema>['direction']
export type SwitchProjectResponse = z.infer<typeof switchProjectResponseSchema>
export type DeleteProjectResponse = z.infer<typeof deleteProjectResponseSchema>
export type ContextReference = z.infer<typeof contextReferenceSchema>
export type AttachmentReference = z.infer<typeof attachmentReferenceSchema>
export type AttachmentInputModality = Extract<
  z.infer<typeof modelModalitySchema>,
  'image' | 'video' | 'file'
>
export type ConversationModelCapability = z.infer<typeof conversationModelCapabilitySchema>
export type StartRunRequest = z.infer<typeof startRunRequestSchema>
export type ListReferenceCandidatesResponse = z.infer<typeof listReferenceCandidatesResponseSchema>
export type ProviderSettingsRequest = z.infer<typeof providerSettingsRequestSchema>
export type ValidateProviderSettingsRequest = z.infer<typeof validateProviderSettingsRequestSchema>
export type ValidateProviderSettingsResponse = z.infer<
  typeof validateProviderSettingsResponseSchema
>
export type ProbeProviderConfigRequest = z.infer<typeof probeProviderConfigRequestSchema>
export type ProbeProviderConfigResponse = z.infer<typeof probeProviderConfigResponseSchema>
export type ListProviderProbeSnapshotsResponse = z.infer<
  typeof listProviderProbeSnapshotsResponseSchema
>
export type GetModelUsageSummaryResponse = z.infer<typeof getModelUsageSummaryResponseSchema>
export type RefreshOfficialQuotaRequest = z.infer<typeof refreshOfficialQuotaRequestSchema>
export type RefreshOfficialQuotaResponse = z.infer<typeof refreshOfficialQuotaResponseSchema>
export type ListOfficialQuotaSnapshotsResponse = z.infer<
  typeof listOfficialQuotaSnapshotsResponseSchema
>
export type ModelProviderCatalogResponse = z.infer<typeof modelProviderCatalogResponseSchema>
export type ProviderConfig = z.infer<typeof providerConfigSchema>
export type ListProviderSettingsResponse = z.infer<typeof listProviderSettingsResponseSchema>
export type SaveProviderSettingsResponse = z.infer<typeof saveProviderSettingsResponseSchema>
export type ListProviderCapabilityRoutesResponse = z.infer<
  typeof listProviderCapabilityRoutesResponseSchema
>
export type ListProviderCapabilityRouteOptionsResponse = z.infer<
  typeof listProviderCapabilityRouteOptionsResponseSchema
>
export type ModelSettingsPageResponse = z.infer<typeof modelSettingsPageResponseSchema>
export type RefreshModelProviderCatalogResponse = z.infer<
  typeof refreshModelProviderCatalogResponseSchema
>
export type SaveProviderCapabilityRouteRequest = z.infer<
  typeof saveProviderCapabilityRouteRequestSchema
>
export type SaveProviderCapabilityRouteResponse = z.infer<
  typeof saveProviderCapabilityRouteResponseSchema
>
export type DeleteProviderCapabilityRouteRequest = z.infer<
  typeof deleteProviderCapabilityRouteRequestSchema
>
export type DeleteProviderCapabilityRouteResponse = z.infer<
  typeof deleteProviderCapabilityRouteResponseSchema
>
export type PermissionMode = z.infer<typeof permissionModeSchema>
export type ToolProfile = z.infer<typeof toolProfileSchema>
export type GetExecutionSettingsResponse = z.infer<typeof getExecutionSettingsResponseSchema>
export type GetExecutionSettingsRequest = z.infer<typeof getExecutionSettingsRequestSchema>
export type SetExecutionSettingsRequest = z.infer<typeof setExecutionSettingsRequestSchema>
export type SetExecutionSettingsResponse = z.infer<typeof setExecutionSettingsResponseSchema>
export type AgentCapabilities = z.infer<typeof agentCapabilitiesSchema>
export type AgentCapabilityUnavailableReason = z.infer<
  typeof agentCapabilityUnavailableReasonSchema
>
export type AgentProfile = z.infer<typeof agentProfileSchema>
export type AgentToolPolicy = z.infer<typeof agentToolPolicySchema>
export type ListAgentProfilesResponse = z.infer<typeof listAgentProfilesResponseSchema>
export type SaveAgentProfileResponse = z.infer<typeof saveAgentProfileResponseSchema>
export type DeleteAgentProfileRequest = z.infer<typeof deleteAgentProfileRequestSchema>
export type DeleteAgentProfileResponse = z.infer<typeof deleteAgentProfileResponseSchema>

export function parseAgentCapabilities(value: unknown): AgentCapabilities {
  return agentCapabilitiesSchema.parse(value)
}

export function parseAgentToolPolicy(value: unknown): AgentToolPolicy {
  return agentToolPolicySchema.parse(value)
}

export function parseAgentProfile(value: unknown): AgentProfile {
  return agentProfileSchema.parse(value)
}
export type RequestProviderConfigApiKeyRevealResponse = z.infer<
  typeof requestProviderConfigApiKeyRevealResponseSchema
>
export type GetProviderConfigApiKeyResponse = z.infer<typeof getProviderConfigApiKeyResponseSchema>
export type McpConfigLayer = z.infer<typeof mcpConfigLayerSchema>
export type McpServerSummary = z.infer<typeof mcpServerSummarySchema>
export type McpServerConfig = z.infer<typeof mcpServerConfigSchema>
export type ListMcpServersResponse = z.infer<typeof listMcpServersResponseSchema>
export type ListBrowserMcpPresetsResponse = z.infer<typeof listBrowserMcpPresetsResponseSchema>
export type SaveBrowserMcpPresetRequest = z.input<typeof saveBrowserMcpPresetRequestSchema>
export type SaveBrowserMcpPresetResponse = z.infer<typeof saveBrowserMcpPresetResponseSchema>
export type GetMcpServerConfigResponse = z.infer<typeof getMcpServerConfigResponseSchema>
export type SaveMcpServerRequest = z.input<typeof saveMcpServerRequestSchema>
export type SaveMcpServerResponse = z.infer<typeof saveMcpServerResponseSchema>
export type DeleteMcpServerResponse = z.infer<typeof deleteMcpServerResponseSchema>
export type SetMcpServerEnabledResponse = z.infer<typeof setMcpServerEnabledResponseSchema>
export type RestartMcpServerResponse = z.infer<typeof restartMcpServerResponseSchema>
export type McpDiagnosticRecord = z.infer<typeof mcpDiagnosticRecordSchema>
export type ListMcpDiagnosticsResponse = z.infer<typeof listMcpDiagnosticsResponseSchema>
export type ClearMcpDiagnosticsResponse = z.infer<typeof clearMcpDiagnosticsResponseSchema>
export type SubscribeMcpDiagnosticsRequest = z.infer<typeof subscribeMcpDiagnosticsRequestSchema>
export type SubscribeMcpDiagnosticsResponse = z.infer<typeof subscribeMcpDiagnosticsResponseSchema>
export type UnsubscribeMcpDiagnosticsResponse = z.infer<
  typeof unsubscribeMcpDiagnosticsResponseSchema
>
export type McpDiagnosticBatchPayload = z.infer<typeof mcpDiagnosticBatchPayloadSchema>
export type SkillSummary = z.infer<typeof skillSummarySchema>
export type SkillConfigDeclaration = z.infer<typeof skillConfigDeclarationSchema>
export type GetSkillConfigResponse = z.infer<typeof getSkillConfigResponseSchema>
export type SkillConfigMutationResponse = z.infer<typeof skillConfigMutationResponseSchema>
export type SkillFile = z.infer<typeof skillFileSchema>
export type SkillCatalogSource = z.infer<typeof skillCatalogSourceSchema>
export type SkillCatalogEntry = z.infer<typeof skillCatalogEntrySchema>
export type ListSkillCatalogEntriesRequest = z.infer<typeof listSkillCatalogEntriesRequestSchema>
export type GetSkillCatalogEntryRequest = z.infer<typeof getSkillCatalogEntryRequestSchema>
export type GetSkillCatalogFileRequest = z.infer<typeof getSkillCatalogFileRequestSchema>
export type InstallSkillFromCatalogRequest = z.infer<typeof installSkillFromCatalogRequestSchema>
export type ListSkillsResponse = z.infer<typeof listSkillsResponseSchema>
export type GetSkillDetailResponse = z.infer<typeof getSkillDetailResponseSchema>
export type GetSkillFileResponse = z.infer<typeof getSkillFileResponseSchema>
export type ImportSkillResponse = z.infer<typeof importSkillResponseSchema>
export type SetSkillEnabledResponse = z.infer<typeof setSkillEnabledResponseSchema>
export type DeleteSkillResponse = z.infer<typeof deleteSkillResponseSchema>
export type PluginRuntimeCapability = z.infer<typeof pluginRuntimeCapabilitySchema>
export type PluginSummary = z.infer<typeof pluginSummarySchema>
export type PluginDetail = z.infer<typeof pluginDetailSchema>
export type ListPluginsResponse = z.infer<typeof listPluginsResponseSchema>
export type GetPluginDetailResponse = z.infer<typeof getPluginDetailResponseSchema>
export type PluginInstallReport = z.infer<typeof pluginInstallReportSchema>
export type PluginOperationResult = z.infer<typeof pluginOperationResultSchema>
export type PluginConfigUpdate = z.infer<typeof updatePluginConfigRequestSchema>
export type SetProjectPluginsEnabledResponse = z.infer<
  typeof setProjectPluginsEnabledResponseSchema
>
export type ListSkillCatalogSourcesResponse = z.infer<typeof listSkillCatalogSourcesResponseSchema>
export type ListSkillCatalogEntriesResponse = z.infer<typeof listSkillCatalogEntriesResponseSchema>
export type GetSkillCatalogEntryResponse = z.infer<typeof getSkillCatalogEntryResponseSchema>
export type GetSkillCatalogFileResponse = z.infer<typeof getSkillCatalogFileResponseSchema>
export type SkillCatalogInstallTask = z.infer<typeof skillCatalogInstallTaskSchema>
export type ListSkillCatalogInstallTasksResponse = z.infer<
  typeof listSkillCatalogInstallTasksResponseSchema
>
export type InstallSkillFromCatalogResponse = z.infer<typeof installSkillFromCatalogResponseSchema>
export type SkillCatalogInstallProgressPayload = z.infer<
  typeof skillCatalogInstallProgressPayloadSchema
>
export interface CommandClient {
  clearSkillSecret: (skillId: string, key: string) => Promise<SkillConfigMutationResponse>
  deleteAgentProfile: (id: string) => Promise<DeleteAgentProfileResponse>
  deleteMcpServer: (
    configLayer: McpConfigLayer,
    id: string,
    projectPath?: string | null,
  ) => Promise<DeleteMcpServerResponse>
  uninstallPlugin: (pluginId: string) => Promise<PluginOperationResult>
  deleteSkill: (id: string) => Promise<DeleteSkillResponse>
  getAppInfo: () => Promise<AppInfo>
  getRuntimeExecutionStatus: () => Promise<RuntimeExecutionStatus>
  getModelSettingsPage: () => Promise<ModelSettingsPageResponse>
  listRuntimeTools: () => Promise<ListRuntimeToolsResponse>
  setRuntimeToolEnabled: (
    request: SetRuntimeToolEnabledRequest,
  ) => Promise<ListRuntimeToolsResponse>
  resetRuntimeTools: () => Promise<ListRuntimeToolsResponse>
  getModelUsageSummary: () => Promise<GetModelUsageSummaryResponse>
  refreshModelProviderCatalog: () => Promise<RefreshModelProviderCatalogResponse>
  listOfficialQuotaSnapshots: () => Promise<ListOfficialQuotaSnapshotsResponse>
  getMcpServerConfig: (
    configLayer: McpConfigLayer,
    id: string,
  ) => Promise<GetMcpServerConfigResponse>
  getPluginDetail: (pluginId: string) => Promise<GetPluginDetailResponse>
  getProviderConfigApiKey: (
    configId: string,
    revealToken: string,
  ) => Promise<GetProviderConfigApiKeyResponse>
  getSkillCatalogEntry: (
    request: GetSkillCatalogEntryRequest,
  ) => Promise<GetSkillCatalogEntryResponse>
  getSkillCatalogFile: (request: GetSkillCatalogFileRequest) => Promise<GetSkillCatalogFileResponse>
  getSkillConfig: (skillId: string) => Promise<GetSkillConfigResponse>
  getSkillDetail: (id: string) => Promise<GetSkillDetailResponse>
  getSkillFile: (id: string, path: string) => Promise<GetSkillFileResponse>
  importSkill: (sourcePath: string) => Promise<ImportSkillResponse>
  installPluginFromPath: (sourcePath: string) => Promise<PluginOperationResult>
  installSkillFromCatalog: (
    request: InstallSkillFromCatalogRequest,
  ) => Promise<InstallSkillFromCatalogResponse>
  listSkillCatalogInstallTasks: () => Promise<ListSkillCatalogInstallTasksResponse>
  listenSkillCatalogInstallProgress: (
    onProgress: (progress: SkillCatalogInstallProgressPayload) => void,
    onError?: (error: unknown) => void,
  ) => Promise<() => void>
  getExecutionSettings: (
    request?: GetExecutionSettingsRequest,
  ) => Promise<GetExecutionSettingsResponse>
  listAgentProfiles: () => Promise<ListAgentProfilesResponse>
  listBrowserMcpPresets: () => Promise<ListBrowserMcpPresetsResponse>
  listModelProviderCatalog: () => Promise<ModelProviderCatalogResponse>
  listMcpDiagnostics: (serverId?: string) => Promise<ListMcpDiagnosticsResponse>
  listMcpServers: (configLayer: McpConfigLayer) => Promise<ListMcpServersResponse>
  listPlugins: () => Promise<ListPluginsResponse>
  listProviderSettings: (workspaceRoot?: string) => Promise<ListProviderSettingsResponse>
  listProviderCapabilityRoutes: () => Promise<ListProviderCapabilityRoutesResponse>
  listProviderCapabilityRouteOptions: () => Promise<ListProviderCapabilityRouteOptionsResponse>
  listProviderProbeSnapshots: () => Promise<ListProviderProbeSnapshotsResponse>
  listProjects: () => Promise<ListProjectsResponse>
  getDefaultWorkspace: () => Promise<DefaultWorkspaceResponse>
  addProject: (path: string) => Promise<SwitchProjectResponse>
  switchProject: (path: string) => Promise<SwitchProjectResponse>
  deleteProject: (path: string) => Promise<DeleteProjectResponse>
  moveProject: (path: string, direction: MoveProjectDirection) => Promise<ListProjectsResponse>
  renameProject: (path: string, name: string) => Promise<SwitchProjectResponse>
  probeProviderConfig: (request: ProbeProviderConfigRequest) => Promise<ProbeProviderConfigResponse>
  refreshOfficialQuota: (
    request: RefreshOfficialQuotaRequest,
  ) => Promise<RefreshOfficialQuotaResponse>
  listSkillCatalogEntries: (
    request: ListSkillCatalogEntriesRequest,
  ) => Promise<ListSkillCatalogEntriesResponse>
  listSkillCatalogSources: () => Promise<ListSkillCatalogSourcesResponse>
  listSkills: () => Promise<ListSkillsResponse>
  reloadPlugin: (pluginId: string) => Promise<PluginOperationResult>
  requestProviderConfigApiKeyReveal: (
    configId: string,
  ) => Promise<RequestProviderConfigApiKeyRevealResponse>
  saveAgentProfile: (profile: AgentProfile) => Promise<SaveAgentProfileResponse>
  saveBrowserMcpPreset: (
    request: SaveBrowserMcpPresetRequest,
  ) => Promise<SaveBrowserMcpPresetResponse>
  saveMcpServer: (request: SaveMcpServerRequest) => Promise<SaveMcpServerResponse>
  setMcpServerEnabled: (
    configLayer: McpConfigLayer,
    id: string,
    enabled: boolean,
    projectPath?: string | null,
  ) => Promise<SetMcpServerEnabledResponse>
  setPluginEnabled: (pluginId: string, enabled: boolean) => Promise<PluginOperationResult>
  setProjectPluginsEnabled: (enabled: boolean) => Promise<SetProjectPluginsEnabledResponse>
  restartMcpServer: (
    configLayer: McpConfigLayer,
    id: string,
    projectPath?: string | null,
  ) => Promise<RestartMcpServerResponse>
  clearMcpDiagnostics: (serverId?: string) => Promise<ClearMcpDiagnosticsResponse>
  saveProviderSettings: (request: ProviderSettingsRequest) => Promise<SaveProviderSettingsResponse>
  saveProviderCapabilityRoute: (
    request: SaveProviderCapabilityRouteRequest,
  ) => Promise<SaveProviderCapabilityRouteResponse>
  deleteProviderCapabilityRoute: (
    request: DeleteProviderCapabilityRouteRequest,
  ) => Promise<DeleteProviderCapabilityRouteResponse>
  setExecutionSettings: (
    request: SetExecutionSettingsRequest,
  ) => Promise<SetExecutionSettingsResponse>
  setSkillEnabled: (id: string, enabled: boolean) => Promise<SetSkillEnabledResponse>
  setSkillConfigValue: (
    skillId: string,
    key: string,
    value: unknown,
  ) => Promise<SkillConfigMutationResponse>
  setSkillSecret: (
    skillId: string,
    key: string,
    value: string,
  ) => Promise<SkillConfigMutationResponse>
  subscribeMcpDiagnostics: (
    request: SubscribeMcpDiagnosticsRequest,
  ) => Promise<SubscribeMcpDiagnosticsResponse>
  listenMcpDiagnosticBatches: (
    onBatch: (batch: McpDiagnosticBatchPayload) => void,
  ) => Promise<() => void>
  unsubscribeMcpDiagnostics: (subscriptionId: string) => Promise<UnsubscribeMcpDiagnosticsResponse>
  updatePluginConfig: (
    pluginId: string,
    values: PluginConfigUpdate['values'],
  ) => Promise<PluginOperationResult>
  validatePluginFromPath: (sourcePath: string) => Promise<PluginInstallReport>
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
    async clearSkillSecret(skillId, key) {
      const command = 'clear_skill_secret'
      const args = parseArgs(command, clearSkillSecretRequestSchema, { key, skillId })
      return parsePayload(command, skillConfigMutationResponseSchema, await invoke(command, args))
    },
    async deleteAgentProfile(id) {
      const command = 'delete_agent_profile'
      const args = parseArgs(command, deleteAgentProfileRequestSchema, { id })
      return parsePayload(command, deleteAgentProfileResponseSchema, await invoke(command, args))
    },
    async deleteMcpServer(configLayer, id, projectPath = null) {
      const command = 'delete_mcp_server'
      const args = parseArgs(command, deleteMcpServerRequestSchema, {
        configLayer,
        id,
        ...(projectPath === null ? {} : { projectPath }),
      })
      return parsePayload(command, deleteMcpServerResponseSchema, await invoke(command, args))
    },
    async uninstallPlugin(pluginId) {
      const command = 'uninstall_plugin'
      const args = parseArgs(command, pluginIdRequestSchema, { pluginId })
      return parsePayload(command, pluginOperationResultSchema, await invoke(command, args))
    },
    async deleteSkill(id) {
      const command = 'delete_skill'
      const args = parseArgs(command, deleteSkillRequestSchema, { id })
      return parsePayload(command, deleteSkillResponseSchema, await invoke(command, args))
    },
    async getExecutionSettings(request) {
      const command = 'get_execution_settings'
      const args =
        request === undefined
          ? undefined
          : parseArgs(command, getExecutionSettingsRequestSchema, request)
      return parsePayload(
        command,
        getExecutionSettingsResponseSchema,
        args === undefined ? await invoke(command) : await invoke(command, args),
      )
    },
    async getAppInfo() {
      const command = 'get_app_info'
      return parsePayload(command, appInfoSchema, await invoke(command))
    },
    async getRuntimeExecutionStatus() {
      const command = 'get_runtime_execution_status'
      return parsePayload(command, runtimeExecutionStatusSchema, await invoke(command))
    },
    async getModelSettingsPage() {
      const command = 'get_model_settings_page'
      return parsePayload(command, modelSettingsPageResponseSchema, await invoke(command))
    },
    async listRuntimeTools() {
      const command = 'list_runtime_tools'
      return parsePayload(command, listRuntimeToolsResponseSchema, await invoke(command))
    },
    async setRuntimeToolEnabled(request) {
      const command = 'set_runtime_tool_enabled'
      const args = parseArgs(command, setRuntimeToolEnabledRequestSchema, request)
      return parsePayload(command, listRuntimeToolsResponseSchema, await invoke(command, args))
    },
    async resetRuntimeTools() {
      const command = 'reset_runtime_tools'
      return parsePayload(command, listRuntimeToolsResponseSchema, await invoke(command))
    },
    async getModelUsageSummary() {
      const command = 'get_model_usage_summary'
      return parsePayload(command, getModelUsageSummaryResponseSchema, await invoke(command))
    },
    async refreshModelProviderCatalog() {
      const command = 'refresh_model_provider_catalog'
      return parsePayload(command, refreshModelProviderCatalogResponseSchema, await invoke(command))
    },
    async listOfficialQuotaSnapshots() {
      const command = 'list_official_quota_snapshots'
      return parsePayload(command, listOfficialQuotaSnapshotsResponseSchema, await invoke(command))
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
    async getSkillCatalogEntry(request) {
      const command = 'get_skill_catalog_entry'
      const args = parseArgs(command, getSkillCatalogEntryRequestSchema, request)
      return parsePayload(command, getSkillCatalogEntryResponseSchema, await invoke(command, args))
    },
    async getSkillCatalogFile(request) {
      const command = 'get_skill_catalog_file'
      const args = parseArgs(command, getSkillCatalogFileRequestSchema, request)
      return parsePayload(command, getSkillCatalogFileResponseSchema, await invoke(command, args))
    },
    async getSkillConfig(skillId) {
      const command = 'get_skill_config'
      const args = parseArgs(command, getSkillConfigRequestSchema, { skillId })
      return parsePayload(command, getSkillConfigResponseSchema, await invoke(command, args))
    },
    async probeProviderConfig(request) {
      const command = 'probe_provider_config'
      const args = parseArgs(command, probeProviderConfigRequestSchema, request)
      return parsePayload(command, probeProviderConfigResponseSchema, await invoke(command, args))
    },
    async refreshOfficialQuota(request) {
      const command = 'refresh_official_quota'
      const args = parseArgs(command, refreshOfficialQuotaRequestSchema, request)
      return parsePayload(command, refreshOfficialQuotaResponseSchema, await invoke(command, args))
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
    async installSkillFromCatalog(request) {
      const command = 'install_skill_from_catalog'
      const args = parseArgs(command, installSkillFromCatalogRequestSchema, request)
      return parsePayload(
        command,
        installSkillFromCatalogResponseSchema,
        await invoke(command, args),
      )
    },
    async listSkillCatalogInstallTasks() {
      const command = 'list_skill_catalog_install_tasks'
      return parsePayload(
        command,
        listSkillCatalogInstallTasksResponseSchema,
        await invoke(command),
      )
    },
    async listenSkillCatalogInstallProgress(onProgress, onError) {
      const unlisten = await tauriListen<unknown>('skill_catalog_install_progress', (event) => {
        try {
          onProgress(
            parsePayload(
              'skill_catalog_install_progress',
              skillCatalogInstallProgressPayloadSchema,
              event.payload,
            ),
          )
        } catch (error) {
          if (onError) {
            onError(error)
            return
          }
          throw error
        }
      })

      return unlisten
    },
    async listBrowserMcpPresets() {
      const command = 'list_browser_mcp_presets'
      return parsePayload(command, listBrowserMcpPresetsResponseSchema, await invoke(command))
    },
    async listModelProviderCatalog() {
      const command = 'list_model_provider_catalog'
      return parsePayload(command, modelProviderCatalogResponseSchema, await invoke(command))
    },
    async listMcpDiagnostics(serverId) {
      const command = 'list_mcp_diagnostics'
      const args = parseArgs(command, listMcpDiagnosticsRequestSchema, {
        serverId,
      })
      return parsePayload(command, listMcpDiagnosticsResponseSchema, await invoke(command, args))
    },
    async listMcpServers(configLayer) {
      const command = 'list_mcp_servers'
      const args = parseArgs(command, z.object({ configLayer: mcpConfigLayerSchema }).strict(), {
        configLayer,
      })
      return parsePayload(command, listMcpServersResponseSchema, await invoke(command, args))
    },
    async getMcpServerConfig(configLayer, id) {
      const command = 'get_mcp_server_config'
      const args = parseArgs(command, getMcpServerConfigRequestSchema, { configLayer, id })
      return parsePayload(command, getMcpServerConfigResponseSchema, await invoke(command, args))
    },
    async getPluginDetail(pluginId) {
      const command = 'get_plugin_detail'
      const args = parseArgs(command, getPluginDetailRequestSchema, {
        pluginId,
      })
      return parsePayload(command, getPluginDetailResponseSchema, await invoke(command, args))
    },
    async listSkillCatalogEntries(request) {
      const command = 'list_skill_catalog_entries'
      const args = parseArgs(command, listSkillCatalogEntriesRequestSchema, request)
      return parsePayload(
        command,
        listSkillCatalogEntriesResponseSchema,
        await invoke(command, args),
      )
    },
    async listSkillCatalogSources() {
      const command = 'list_skill_catalog_sources'
      return parsePayload(command, listSkillCatalogSourcesResponseSchema, await invoke(command))
    },
    async listProviderSettings(workspaceRoot) {
      const command = 'list_provider_settings'
      return parsePayload(
        command,
        listProviderSettingsResponseSchema,
        workspaceRoot === undefined
          ? await invoke(command)
          : await invoke(command, { workspaceRoot }),
      )
    },
    async listProviderCapabilityRoutes() {
      const command = 'list_provider_capability_routes'
      return parsePayload(
        command,
        listProviderCapabilityRoutesResponseSchema,
        await invoke(command),
      )
    },
    async listProviderCapabilityRouteOptions() {
      const command = 'list_provider_capability_route_options'
      return parsePayload(
        command,
        listProviderCapabilityRouteOptionsResponseSchema,
        await invoke(command),
      )
    },
    async listProviderProbeSnapshots() {
      const command = 'list_provider_probe_snapshots'
      return parsePayload(command, listProviderProbeSnapshotsResponseSchema, await invoke(command))
    },
    async listProjects() {
      const command = 'list_projects'
      return parsePayload(command, listProjectsResponseSchema, await invoke(command))
    },
    async getDefaultWorkspace() {
      const command = 'get_default_workspace'
      return parsePayload(command, defaultWorkspaceResponseSchema, await invoke(command))
    },
    async listPlugins() {
      const command = 'list_plugins'
      return parsePayload(command, listPluginsResponseSchema, await invoke(command))
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
    async moveProject(path, direction) {
      const command = 'move_project'
      const args = parseArgs(command, moveProjectRequestSchema, { direction, path })
      return parsePayload(command, listProjectsResponseSchema, await invoke(command, args))
    },
    async renameProject(path, name) {
      const command = 'rename_project'
      const args = parseArgs(command, renameProjectRequestSchema, { name, path })
      return parsePayload(command, switchProjectResponseSchema, await invoke(command, args))
    },
    async deleteProject(path) {
      const command = 'delete_project'
      const args = parseArgs(command, deleteProjectRequestSchema, { path })
      return parsePayload(command, deleteProjectResponseSchema, await invoke(command, args))
    },
    async listSkills() {
      const command = 'list_skills'
      return parsePayload(command, listSkillsResponseSchema, await invoke(command))
    },
    async listAgentProfiles() {
      const command = 'list_agent_profiles'
      return parsePayload(command, listAgentProfilesResponseSchema, await invoke(command))
    },
    async reloadPlugin(pluginId) {
      const command = 'reload_plugin'
      const args = parseArgs(command, pluginIdRequestSchema, { pluginId })
      return parsePayload(command, pluginOperationResultSchema, await invoke(command, args))
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
    async saveAgentProfile(profile) {
      const command = 'save_agent_profile'
      const args = parseArgs(command, agentProfileSchema, profile)
      return parsePayload(command, saveAgentProfileResponseSchema, await invoke(command, args))
    },
    async saveProviderSettings(request) {
      const command = 'save_provider_settings'
      const args = parseArgs(command, providerSettingsRequestSchema, request)
      return parsePayload(command, saveProviderSettingsResponseSchema, await invoke(command, args))
    },
    async saveProviderCapabilityRoute(request) {
      const command = 'save_provider_capability_route'
      const args = parseArgs(command, saveProviderCapabilityRouteRequestSchema, request)
      return parsePayload(
        command,
        saveProviderCapabilityRouteResponseSchema,
        await invoke(command, args),
      )
    },
    async deleteProviderCapabilityRoute(request) {
      const command = 'delete_provider_capability_route'
      const args = parseArgs(command, deleteProviderCapabilityRouteRequestSchema, request)
      return parsePayload(
        command,
        deleteProviderCapabilityRouteResponseSchema,
        await invoke(command, args),
      )
    },
    async setExecutionSettings(request) {
      const command = 'set_execution_settings'
      const args = parseArgs(command, setExecutionSettingsRequestSchema, request)
      return parsePayload(command, setExecutionSettingsResponseSchema, await invoke(command, args))
    },
    async saveBrowserMcpPreset(request) {
      const command = 'save_browser_mcp_preset'
      const args = parseArgs(command, saveBrowserMcpPresetRequestSchema, request)
      return parsePayload(command, saveBrowserMcpPresetResponseSchema, await invoke(command, args))
    },
    async saveMcpServer(request) {
      const command = 'save_mcp_server'
      const args = parseArgs(command, saveMcpServerRequestSchema, request)
      return parsePayload(command, saveMcpServerResponseSchema, await invoke(command, args))
    },
    async setMcpServerEnabled(configLayer, id, enabled, projectPath = null) {
      const command = 'set_mcp_server_enabled'
      const args = parseArgs(command, setMcpServerEnabledRequestSchema, {
        configLayer,
        enabled,
        id,
        ...(projectPath === null ? {} : { projectPath }),
      })
      return parsePayload(command, setMcpServerEnabledResponseSchema, await invoke(command, args))
    },
    async setPluginEnabled(pluginId, enabled) {
      const command = 'set_plugin_enabled'
      const args = parseArgs(command, setPluginEnabledRequestSchema, {
        enabled,
        pluginId,
      })
      return parsePayload(command, pluginOperationResultSchema, await invoke(command, args))
    },
    async setProjectPluginsEnabled(enabled) {
      const command = 'set_project_plugins_enabled'
      const args = parseArgs(command, setProjectPluginsEnabledRequestSchema, {
        enabled,
      })
      return parsePayload(
        command,
        setProjectPluginsEnabledResponseSchema,
        await invoke(command, args),
      )
    },
    async restartMcpServer(configLayer, id, projectPath = null) {
      const command = 'restart_mcp_server'
      const args = parseArgs(command, restartMcpServerRequestSchema, {
        configLayer,
        id,
        ...(projectPath === null ? {} : { projectPath }),
      })
      return parsePayload(command, restartMcpServerResponseSchema, await invoke(command, args))
    },
    async clearMcpDiagnostics(serverId) {
      const command = 'clear_mcp_diagnostics'
      const args = parseArgs(command, clearMcpDiagnosticsRequestSchema, {
        serverId,
      })
      return parsePayload(command, clearMcpDiagnosticsResponseSchema, await invoke(command, args))
    },
    async setSkillEnabled(id, enabled) {
      const command = 'set_skill_enabled'
      const args = parseArgs(command, setSkillEnabledRequestSchema, {
        enabled,
        id,
      })
      return parsePayload(command, setSkillEnabledResponseSchema, await invoke(command, args))
    },
    async setSkillConfigValue(skillId, key, value) {
      const command = 'set_skill_config_value'
      const args = parseArgs(command, setSkillConfigValueRequestSchema, { key, skillId, value })
      return parsePayload(command, skillConfigMutationResponseSchema, await invoke(command, args))
    },
    async setSkillSecret(skillId, key, value) {
      const command = 'set_skill_secret'
      const args = parseArgs(command, setSkillSecretRequestSchema, { key, skillId, value })
      return parsePayload(command, skillConfigMutationResponseSchema, await invoke(command, args))
    },
    async installPluginFromPath(sourcePath) {
      const command = 'install_plugin_from_path'
      const args = parseArgs(command, pluginPathRequestSchema, { sourcePath })
      return parsePayload(command, pluginOperationResultSchema, await invoke(command, args))
    },
    async subscribeMcpDiagnostics(request) {
      const command = 'subscribe_mcp_diagnostics'
      const args = parseArgs(command, subscribeMcpDiagnosticsRequestSchema, request)
      return parsePayload(
        command,
        subscribeMcpDiagnosticsResponseSchema,
        await invoke(command, args),
      )
    },
    async listenMcpDiagnosticBatches(onBatch) {
      const unlisten = await tauriListen<unknown>('mcp_diagnostic_batch', (event) => {
        onBatch(
          parsePayload('mcp_diagnostic_batch', mcpDiagnosticBatchPayloadSchema, event.payload),
        )
      })

      return unlisten
    },
    async unsubscribeMcpDiagnostics(subscriptionId) {
      const command = 'unsubscribe_mcp_diagnostics'
      const args = parseArgs(command, unsubscribeMcpDiagnosticsRequestSchema, {
        subscriptionId,
      })
      return parsePayload(
        command,
        unsubscribeMcpDiagnosticsResponseSchema,
        await invoke(command, args),
      )
    },
    async updatePluginConfig(pluginId, values) {
      const command = 'update_plugin_config'
      const args = parseArgs(command, updatePluginConfigRequestSchema, {
        pluginId,
        values,
      })
      return parsePayload(command, pluginOperationResultSchema, await invoke(command, args))
    },
    async validatePluginFromPath(sourcePath) {
      const command = 'validate_plugin_from_path'
      const args = parseArgs(command, pluginPathRequestSchema, { sourcePath })
      return parsePayload(command, pluginInstallReportSchema, await invoke(command, args))
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

export function getRuntimeExecutionStatus(
  client: CommandClient = tauriCommandClient,
): Promise<RuntimeExecutionStatus> {
  return client.getRuntimeExecutionStatus()
}

export function listRuntimeTools(
  client: CommandClient = tauriCommandClient,
): Promise<ListRuntimeToolsResponse> {
  return client.listRuntimeTools()
}

export function listModelProviderCatalog(
  client: CommandClient = tauriCommandClient,
): Promise<ModelProviderCatalogResponse> {
  return client.listModelProviderCatalog()
}

export function getModelSettingsPage(
  client: CommandClient = tauriCommandClient,
): Promise<ModelSettingsPageResponse> {
  return client.getModelSettingsPage()
}

export function refreshModelProviderCatalog(
  client: CommandClient = tauriCommandClient,
): Promise<RefreshModelProviderCatalogResponse> {
  return client.refreshModelProviderCatalog()
}

export function listMcpServers(
  configLayer: McpConfigLayer,
  client: CommandClient = tauriCommandClient,
): Promise<ListMcpServersResponse> {
  return client.listMcpServers(configLayer)
}

export function listBrowserMcpPresets(
  client: CommandClient = tauriCommandClient,
): Promise<ListBrowserMcpPresetsResponse> {
  return client.listBrowserMcpPresets()
}

export function getMcpServerConfig(
  configLayer: McpConfigLayer,
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<GetMcpServerConfigResponse> {
  return client.getMcpServerConfig(configLayer, id)
}

export function listMcpDiagnostics(
  serverId?: string,
  client: CommandClient = tauriCommandClient,
): Promise<ListMcpDiagnosticsResponse> {
  return client.listMcpDiagnostics(serverId)
}

export function saveMcpServer(
  request: SaveMcpServerRequest,
  client: CommandClient = tauriCommandClient,
): Promise<SaveMcpServerResponse> {
  return client.saveMcpServer(request)
}

export function saveBrowserMcpPreset(
  request: SaveBrowserMcpPresetRequest,
  client: CommandClient = tauriCommandClient,
): Promise<SaveBrowserMcpPresetResponse> {
  return client.saveBrowserMcpPreset(request)
}

export function setMcpServerEnabled(
  configLayer: McpConfigLayer,
  id: string,
  enabled: boolean,
  client: CommandClient = tauriCommandClient,
  projectPath: string | null = null,
): Promise<SetMcpServerEnabledResponse> {
  return client.setMcpServerEnabled(configLayer, id, enabled, projectPath)
}

export function restartMcpServer(
  configLayer: McpConfigLayer,
  id: string,
  client: CommandClient = tauriCommandClient,
  projectPath: string | null = null,
): Promise<RestartMcpServerResponse> {
  return client.restartMcpServer(configLayer, id, projectPath)
}

export function clearMcpDiagnostics(
  serverId?: string,
  client: CommandClient = tauriCommandClient,
): Promise<ClearMcpDiagnosticsResponse> {
  return client.clearMcpDiagnostics(serverId)
}

export function subscribeMcpDiagnostics(
  request: SubscribeMcpDiagnosticsRequest,
  client: CommandClient = tauriCommandClient,
): Promise<SubscribeMcpDiagnosticsResponse> {
  return client.subscribeMcpDiagnostics(request)
}

export function listenMcpDiagnosticBatches(
  onBatch: (batch: McpDiagnosticBatchPayload) => void,
  client: CommandClient = tauriCommandClient,
): Promise<() => void> {
  return client.listenMcpDiagnosticBatches(onBatch)
}

export function unsubscribeMcpDiagnostics(
  subscriptionId: string,
  client: CommandClient = tauriCommandClient,
): Promise<UnsubscribeMcpDiagnosticsResponse> {
  return client.unsubscribeMcpDiagnostics(subscriptionId)
}

export function deleteMcpServer(
  configLayer: McpConfigLayer,
  id: string,
  client: CommandClient = tauriCommandClient,
  projectPath: string | null = null,
): Promise<DeleteMcpServerResponse> {
  return client.deleteMcpServer(configLayer, id, projectPath)
}

export function listPlugins(
  client: CommandClient = tauriCommandClient,
): Promise<ListPluginsResponse> {
  return client.listPlugins()
}

export function getPluginDetail(
  pluginId: string,
  client: CommandClient = tauriCommandClient,
): Promise<GetPluginDetailResponse> {
  return client.getPluginDetail(pluginId)
}

export function validatePluginFromPath(
  sourcePath: string,
  client: CommandClient = tauriCommandClient,
): Promise<PluginInstallReport> {
  return client.validatePluginFromPath(sourcePath)
}

export function installPluginFromPath(
  sourcePath: string,
  client: CommandClient = tauriCommandClient,
): Promise<PluginOperationResult> {
  return client.installPluginFromPath(sourcePath)
}

export function setPluginEnabled(
  pluginId: string,
  enabled: boolean,
  client: CommandClient = tauriCommandClient,
): Promise<PluginOperationResult> {
  return client.setPluginEnabled(pluginId, enabled)
}

export function setProjectPluginsEnabled(
  enabled: boolean,
  client: CommandClient = tauriCommandClient,
): Promise<SetProjectPluginsEnabledResponse> {
  return client.setProjectPluginsEnabled(enabled)
}

export function updatePluginConfig(
  pluginId: string,
  values: PluginConfigUpdate['values'],
  client: CommandClient = tauriCommandClient,
): Promise<PluginOperationResult> {
  return client.updatePluginConfig(pluginId, values)
}

export function uninstallPlugin(
  pluginId: string,
  client: CommandClient = tauriCommandClient,
): Promise<PluginOperationResult> {
  return client.uninstallPlugin(pluginId)
}

export function reloadPlugin(
  pluginId: string,
  client: CommandClient = tauriCommandClient,
): Promise<PluginOperationResult> {
  return client.reloadPlugin(pluginId)
}

export function listSkills(
  client: CommandClient = tauriCommandClient,
): Promise<ListSkillsResponse> {
  return client.listSkills()
}

export function getSkillConfig(
  skillId: string,
  client: CommandClient = tauriCommandClient,
): Promise<GetSkillConfigResponse> {
  return client.getSkillConfig(skillId)
}

export function setSkillConfigValue(
  skillId: string,
  key: string,
  value: unknown,
  client: CommandClient = tauriCommandClient,
): Promise<SkillConfigMutationResponse> {
  return client.setSkillConfigValue(skillId, key, value)
}

export function setSkillSecret(
  skillId: string,
  key: string,
  value: string,
  client: CommandClient = tauriCommandClient,
): Promise<SkillConfigMutationResponse> {
  return client.setSkillSecret(skillId, key, value)
}

export function clearSkillSecret(
  skillId: string,
  key: string,
  client: CommandClient = tauriCommandClient,
): Promise<SkillConfigMutationResponse> {
  return client.clearSkillSecret(skillId, key)
}

export function listAgentProfiles(
  client: CommandClient = tauriCommandClient,
): Promise<ListAgentProfilesResponse> {
  return client.listAgentProfiles()
}

export function saveAgentProfile(
  profile: AgentProfile,
  client: CommandClient = tauriCommandClient,
): Promise<SaveAgentProfileResponse> {
  return client.saveAgentProfile(profile)
}

export function deleteAgentProfile(
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<DeleteAgentProfileResponse> {
  return client.deleteAgentProfile(id)
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

export function listSkillCatalogSources(
  client: CommandClient = tauriCommandClient,
): Promise<ListSkillCatalogSourcesResponse> {
  return client.listSkillCatalogSources()
}

export function listSkillCatalogEntries(
  request: ListSkillCatalogEntriesRequest,
  client: CommandClient = tauriCommandClient,
): Promise<ListSkillCatalogEntriesResponse> {
  return client.listSkillCatalogEntries(request)
}

export function getSkillCatalogEntry(
  request: GetSkillCatalogEntryRequest,
  client: CommandClient = tauriCommandClient,
): Promise<GetSkillCatalogEntryResponse> {
  return client.getSkillCatalogEntry(request)
}

export function getSkillCatalogFile(
  request: GetSkillCatalogFileRequest,
  client: CommandClient = tauriCommandClient,
): Promise<GetSkillCatalogFileResponse> {
  return client.getSkillCatalogFile(request)
}

export function installSkillFromCatalog(
  request: InstallSkillFromCatalogRequest,
  client: CommandClient = tauriCommandClient,
): Promise<InstallSkillFromCatalogResponse> {
  return client.installSkillFromCatalog(request)
}

export function listSkillCatalogInstallTasks(
  client: CommandClient = tauriCommandClient,
): Promise<ListSkillCatalogInstallTasksResponse> {
  return client.listSkillCatalogInstallTasks()
}

export function listenSkillCatalogInstallProgress(
  onProgress: (progress: SkillCatalogInstallProgressPayload) => void,
  client: CommandClient = tauriCommandClient,
  onError?: (error: unknown) => void,
): Promise<() => void> {
  return client.listenSkillCatalogInstallProgress(onProgress, onError)
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

export function saveProviderSettings(
  request: ProviderSettingsRequest,
  client: CommandClient = tauriCommandClient,
): Promise<SaveProviderSettingsResponse> {
  return client.saveProviderSettings(request)
}

export function listProviderSettings(
  client: CommandClient = tauriCommandClient,
  workspaceRoot?: string,
): Promise<ListProviderSettingsResponse> {
  return client.listProviderSettings(workspaceRoot)
}

export function listProviderCapabilityRoutes(
  client: CommandClient = tauriCommandClient,
): Promise<ListProviderCapabilityRoutesResponse> {
  return client.listProviderCapabilityRoutes()
}

export function listProviderCapabilityRouteOptions(
  client: CommandClient = tauriCommandClient,
): Promise<ListProviderCapabilityRouteOptionsResponse> {
  return client.listProviderCapabilityRouteOptions()
}

export function getModelUsageSummary(
  client: CommandClient = tauriCommandClient,
): Promise<GetModelUsageSummaryResponse> {
  return client.getModelUsageSummary()
}

export function listOfficialQuotaSnapshots(
  client: CommandClient = tauriCommandClient,
): Promise<ListOfficialQuotaSnapshotsResponse> {
  return client.listOfficialQuotaSnapshots()
}

export function refreshOfficialQuota(
  request: RefreshOfficialQuotaRequest,
  client: CommandClient = tauriCommandClient,
): Promise<RefreshOfficialQuotaResponse> {
  return client.refreshOfficialQuota(request)
}

export function listProviderProbeSnapshots(
  client: CommandClient = tauriCommandClient,
): Promise<ListProviderProbeSnapshotsResponse> {
  return client.listProviderProbeSnapshots()
}

export function saveProviderCapabilityRoute(
  request: SaveProviderCapabilityRouteRequest,
  client: CommandClient = tauriCommandClient,
): Promise<SaveProviderCapabilityRouteResponse> {
  return client.saveProviderCapabilityRoute(request)
}

export function deleteProviderCapabilityRoute(
  request: DeleteProviderCapabilityRouteRequest,
  client: CommandClient = tauriCommandClient,
): Promise<DeleteProviderCapabilityRouteResponse> {
  return client.deleteProviderCapabilityRoute(request)
}

export function listProjects(
  client: CommandClient = tauriCommandClient,
): Promise<ListProjectsResponse> {
  return client.listProjects()
}

export function getDefaultWorkspace(
  client: CommandClient = tauriCommandClient,
): Promise<DefaultWorkspaceResponse> {
  return client.getDefaultWorkspace()
}

export function moveProject(
  path: string,
  direction: MoveProjectDirection,
  client: CommandClient = tauriCommandClient,
): Promise<ListProjectsResponse> {
  return client.moveProject(path, direction)
}

export function renameProject(
  path: string,
  name: string,
  client: CommandClient = tauriCommandClient,
): Promise<SwitchProjectResponse> {
  return client.renameProject(path, name)
}

export function deleteProject(
  path: string,
  client: CommandClient = tauriCommandClient,
): Promise<DeleteProjectResponse> {
  return client.deleteProject(path)
}

export function getExecutionSettings(
  client: CommandClient = tauriCommandClient,
  request?: GetExecutionSettingsRequest,
): Promise<GetExecutionSettingsResponse> {
  return client.getExecutionSettings(request)
}

export function setRuntimeToolEnabled(
  request: SetRuntimeToolEnabledRequest,
  client: CommandClient = tauriCommandClient,
): Promise<ListRuntimeToolsResponse> {
  return client.setRuntimeToolEnabled(request)
}

export function resetRuntimeTools(
  client: CommandClient = tauriCommandClient,
): Promise<ListRuntimeToolsResponse> {
  return client.resetRuntimeTools()
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

export function probeProviderConfig(
  request: ProbeProviderConfigRequest,
  client: CommandClient = tauriCommandClient,
): Promise<ProbeProviderConfigResponse> {
  return client.probeProviderConfig(request)
}
