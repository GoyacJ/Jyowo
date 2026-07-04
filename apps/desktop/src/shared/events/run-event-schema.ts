import { z } from 'zod'

import { assertNever } from './assert-never'

export const runEventVisibilitySchema = z.enum(['public', 'redacted', 'withheld'])
export const runEventSourceSchema = z.enum([
  'user',
  'assistant',
  'tool',
  'engine',
  'policy',
  'agent',
  'background',
  'plugin',
])
export const runEventContractTypeSchema = z.enum([
  'run_started',
  'run_ended',
  'user_message_appended',
  'assistant_delta_produced',
  'assistant_message_completed',
  'tool_use_requested',
  'tool_use_approved',
  'tool_use_denied',
  'tool_use_completed',
  'tool_use_failed',
  'permission_requested',
  'permission_resolved',
  'artifact_created',
  'artifact_updated',
  'assistant_review_requested',
  'assistant_clarification_requested',
  'assistant_notice',
  'engine_failed',
  'plugin_loaded',
  'plugin_rejected',
  'plugin_failed',
])

const payloadSchema = z.record(z.string(), z.unknown())
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

function hasObviousUnredactedSecret(value: string): boolean {
  return unredactedSecretPatterns.some((pattern) => pattern.test(value))
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
    /(?:\/Applications|\/Library|\/System|\/Users|\/Volumes|\/dev|\/etc|\/home|\/media|\/mnt|\/opt|\/private|\/root|\/run|\/tmp|\/usr|\/var)(?:[\\/]|$)/.test(
      value,
    )
  )
}

function containsObviousUnredactedSecret(value: unknown): boolean {
  if (typeof value === 'string') {
    return hasObviousUnredactedSecret(value)
  }

  if (Array.isArray(value)) {
    return value.some((item) => containsObviousUnredactedSecret(item))
  }

  if (value !== null && typeof value === 'object') {
    return Object.values(value).some((item) => containsObviousUnredactedSecret(item))
  }

  return false
}

function containsUnsafeDisplayReference(value: unknown): boolean {
  if (typeof value === 'string') {
    return hasUnsafeDisplayReference(value)
  }

  if (Array.isArray(value)) {
    return value.some((item) => containsUnsafeDisplayReference(item))
  }

  if (value !== null && typeof value === 'object') {
    return Object.values(value).some((item) => containsUnsafeDisplayReference(item))
  }

  return false
}

const permissionDisplayTextSchema = z
  .string()
  .trim()
  .min(1)
  .refine((value) => !hasObviousUnredactedSecret(value), {
    message: 'permission review payload must not contain obvious unredacted secrets',
  })
  .refine((value) => !hasUnsafeDisplayReference(value), {
    message: 'permission review payload must not contain unsafe display references',
  })
const requestIdSchema = z.string().regex(/^[0-9A-HJKMNP-TV-Z]{26}$/, {
  message: 'requestId must be a canonical ULID',
})
const uuidV4Schema = z
  .uuid()
  .regex(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i)
const toolInputWithheldMessage = 'Input withheld from conversation timeline.'
const toolErrorWithheldMessage = 'Tool error withheld from conversation timeline.'
const toolDisplayTextSchema = permissionDisplayTextSchema
const permissionModeSchema = z.enum([
  'default',
  'plan',
  'accept_edits',
  'bypass_permissions',
  'dont_ask',
  'auto',
])
const actionPlanHashSchema = z.string().regex(/^[0-9a-f]{64}$/)
const modelProtocolSchema = z.enum([
  'chat_completions',
  'responses',
  'messages',
  'generate_content',
])
const runModelSnapshotSchema = z
  .object({
    modelConfigId: z.string().min(1).nullable().optional(),
    providerId: z.string().min(1),
    modelId: z.string().min(1),
    displayName: z.string().min(1),
    protocol: modelProtocolSchema,
  })
  .strict()
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
const toolDiffFileSchema = z
  .object({
    path: toolDisplayTextSchema,
    addedLines: z.number().int().nonnegative(),
    removedLines: z.number().int().nonnegative(),
    preview: toolDisplayTextSchema.optional(),
  })
  .strict()
const toolDiffSchema = z
  .object({
    files: z.array(toolDiffFileSchema),
  })
  .strict()
const runStartedPayloadSchema = z
  .object({
    model: runModelSnapshotSchema,
    permissionMode: permissionModeSchema.optional(),
    sessionId: z.string().min(1),
  })
  .strict()
const usageSummarySchema = z
  .object({
    cacheReadTokens: z.number().int().nonnegative(),
    cacheWriteTokens: z.number().int().nonnegative(),
    costMicros: z.number().int().nonnegative(),
    inputTokens: z.number().int().nonnegative(),
    outputTokens: z.number().int().nonnegative(),
    toolCalls: z.number().int().nonnegative(),
  })
  .strict()
const runEndedPayloadSchema = z
  .object({
    reason: z.string().min(1),
    usage: usageSummarySchema.optional(),
  })
  .strict()
const attachmentReferenceSchema = z
  .object({
    blobRef: z
      .object({
        contentHash: z.array(z.number().int().min(0).max(255)).length(32),
        contentType: mimeTypeMetadataSchema.nullable().optional(),
        id: z.string().trim().min(1),
        size: z.number().int().nonnegative(),
      })
      .strict(),
    id: z
      .string()
      .trim()
      .regex(/^attachment-[0-9a-fA-F]{64}$/),
    mimeType: mimeTypeMetadataSchema,
    name: toolDisplayTextSchema.pipe(z.string().trim().min(1)),
    sizeBytes: z.number().int().nonnegative(),
  })
  .strict()
const assistantDeltaPayloadSchema = z
  .object({
    messageId: z.string().min(1),
    text: z.string(),
  })
  .strict()
const userMessageAppendedPayloadSchema = z
  .object({
    body: z.string(),
    attachments: z.array(attachmentReferenceSchema).optional(),
    clientMessageId: uuidV4Schema.optional(),
    messageId: z.string().min(1),
  })
  .strict()
const assistantCompletedToolUseSchema = z
  .object({
    toolName: toolDisplayTextSchema,
    toolUseId: z.string().min(1),
  })
  .strict()
const assistantCompletedPayloadSchema = z
  .object({
    body: z.string().optional(),
    messageId: z.string().min(1),
    toolUses: z.array(assistantCompletedToolUseSchema).optional(),
  })
  .strict()
const toolRequestedPayloadSchema = z
  .object({
    argumentsSummary: z.literal(toolInputWithheldMessage).optional(),
    command: permissionDisplayTextSchema.optional(),
    query: toolDisplayTextSchema.optional(),
    targetPath: toolDisplayTextSchema.optional(),
    toolName: z.string().min(1),
    toolUseId: z.string().min(1),
  })
  .strict()
const toolResolvedPayloadSchema = z
  .object({
    toolUseId: z.string().min(1),
  })
  .strict()
const toolCompletedPayloadSchema = z
  .object({
    diff: toolDiffSchema.optional(),
    durationMs: z.number().int().nonnegative().optional(),
    exitCode: z.number().int().optional(),
    itemCount: z.number().int().nonnegative().optional(),
    outputSummary: toolDisplayTextSchema.optional(),
    toolName: toolDisplayTextSchema.optional(),
    toolUseId: z.string().min(1),
  })
  .strict()
const toolFailedPayloadSchema = z
  .object({
    code: z.literal('tool_error'),
    message: z.literal(toolErrorWithheldMessage).optional(),
    toolUseId: z.string().min(1),
  })
  .strict()
const permissionActorSourceSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('parentRun'),
    })
    .strict(),
  z
    .object({
      parentRunId: z.string().min(1),
      parentSessionId: z.string().min(1),
      subagentId: z.string().min(1),
      teamId: z.string().min(1).optional(),
      teamMemberProfileId: z.string().min(1).optional(),
      type: z.literal('subagent'),
    })
    .strict(),
  z
    .object({
      agentId: z.string().min(1),
      parentRunId: z.string().min(1).optional(),
      role: permissionDisplayTextSchema,
      teamId: z.string().min(1),
      type: z.literal('teamMember'),
    })
    .strict(),
  z
    .object({
      attemptId: z.string().min(1).optional(),
      backgroundAgentId: z.string().min(1),
      conversationId: z.string().min(1),
      type: z.literal('backgroundAgent'),
    })
    .strict(),
  z
    .object({
      automationId: permissionDisplayTextSchema,
      conversationId: z.string().min(1),
      runId: z.string().min(1).optional(),
      type: z.literal('automation'),
    })
    .strict(),
  z
    .object({
      origin: z.discriminatedUnion('type', [
        z
          .object({
            path: permissionDisplayTextSchema,
            type: z.literal('file'),
          })
          .strict(),
        z
          .object({
            binary: permissionDisplayTextSchema,
            type: z.literal('cargoExtension'),
          })
          .strict(),
        z
          .object({
            endpoint: permissionDisplayTextSchema,
            type: z.literal('remoteRegistry'),
          })
          .strict(),
      ]),
      scope: z.discriminatedUnion('type', [
        z
          .object({
            type: z.literal('global'),
          })
          .strict(),
        z
          .object({
            conversationId: z.string().min(1),
            type: z.literal('session'),
          })
          .strict(),
        z
          .object({
            agentId: z.string().min(1),
            type: z.literal('agent'),
          })
          .strict(),
      ]),
      serverId: permissionDisplayTextSchema,
      type: z.literal('mcpServer'),
    })
    .strict(),
])
const permissionConfirmationSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('none'),
    })
    .strict(),
  z
    .object({
      label: permissionDisplayTextSchema,
      type: z.literal('explicitButton'),
    })
    .strict(),
  z
    .object({
      expected: permissionDisplayTextSchema,
      type: z.literal('typeToConfirm'),
    })
    .strict(),
])
const permissionReviewDetailSchema = z
  .object({
    label: permissionDisplayTextSchema,
    redacted: z.boolean().optional().default(false),
    value: permissionDisplayTextSchema,
  })
  .strict()
const permissionReviewSchema = z
  .object({
    confirmation: permissionConfirmationSchema,
    details: z.array(permissionReviewDetailSchema),
    redacted: z.boolean().optional().default(false),
    summary: permissionDisplayTextSchema,
  })
  .strict()
const sandboxPolicySummarySchema = z
  .object({
    mode: z.union([
      z.enum(['none', 'container', 'remote']),
      z
        .object({
          osLevel: z.enum(['none', 'bubblewrap', 'seatbelt', 'job_object']),
        })
        .strict(),
    ]),
    network: z.union([
      z.enum(['none', 'loopback_only', 'unrestricted']),
      z
        .object({
          allow_list: z.array(
            z
              .object({
                pattern: permissionDisplayTextSchema,
                ports: z.array(z.number().int().min(0).max(65535)).optional().nullable(),
              })
              .strict(),
          ),
        })
        .strict(),
    ]),
    resourceLimits: z
      .object({
        maxCpuCores: z.number().positive().optional().nullable(),
        maxMemoryBytes: z.number().int().positive().optional().nullable(),
        maxOpenFiles: z.number().int().positive().optional().nullable(),
        maxPids: z.number().int().positive().optional().nullable(),
        maxWallClockMs: z.number().int().positive().optional().nullable(),
      })
      .strict(),
    scope: z.union([
      z.enum(['workspace_only', 'unrestricted']),
      z
        .object({
          workspacePlus: z.array(permissionDisplayTextSchema),
        })
        .strict(),
    ]),
  })
  .strict()
const permissionRequestedPayloadSchema = z
  .object({
    actorSource: permissionActorSourceSchema,
    actionPlanHash: actionPlanHashSchema.optional(),
    autoResolved: z.boolean().optional().default(false),
    decisionScope: permissionDisplayTextSchema,
    diffSummary: permissionDisplayTextSchema.optional(),
    effectiveMode: permissionModeSchema.optional(),
    exposure: permissionDisplayTextSchema,
    operation: permissionDisplayTextSchema,
    reason: permissionDisplayTextSchema,
    review: permissionReviewSchema.optional(),
    requestId: requestIdSchema,
    sandboxPolicy: sandboxPolicySummarySchema.optional(),
    severity: z.enum(['low', 'medium', 'high', 'critical']),
    target: permissionDisplayTextSchema,
    toolUseId: z.string().min(1),
    workspaceBoundary: permissionDisplayTextSchema,
  })
  .strict()
const permissionResolvedPayloadSchema = z
  .object({
    actionPlanHash: actionPlanHashSchema.optional(),
    autoResolved: z.boolean().optional().default(false),
    decision: z.enum(['approve', 'deny']),
    decisionId: z.string().min(1).optional(),
    requestId: requestIdSchema,
  })
  .strict()
const backgroundStartedPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
    title: z.string().min(1),
  })
  .strict()
const backgroundPermissionRequestedPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
    reason: permissionDisplayTextSchema,
    requestId: requestIdSchema,
  })
  .strict()
const backgroundPermissionResolvedPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
    decision: z.enum(['approve', 'deny']),
    requestId: requestIdSchema,
  })
  .strict()
const safeArtifactMimeTypeSchema = z.enum([
  'image/png',
  'image/jpeg',
  'image/gif',
  'image/webp',
  'image/avif',
  'video/mp4',
  'video/webm',
  'video/quicktime',
  'audio/mpeg',
  'audio/mp4',
  'audio/ogg',
  'audio/wav',
  'audio/webm',
  'text/plain',
  'text/markdown',
  'text/csv',
  'application/json',
  'application/pdf',
  'application/zip',
  'application/octet-stream',
])
const safeArtifactImageMimeTypeSchema = z.enum([
  'image/png',
  'image/jpeg',
  'image/gif',
  'image/webp',
  'image/avif',
])
const safeArtifactVideoMimeTypeSchema = z.enum(['video/mp4', 'video/webm', 'video/quicktime'])
const safeArtifactAudioMimeTypeSchema = z.enum([
  'audio/mpeg',
  'audio/mp4',
  'audio/ogg',
  'audio/wav',
  'audio/webm',
])
const safeArtifactFileMimeTypeSchema = z.enum([
  'text/plain',
  'text/markdown',
  'text/csv',
  'application/json',
  'application/pdf',
  'application/zip',
  'application/octet-stream',
])
const artifactMediaPreviewSchema = z
  .object({
    kind: z.enum(['image', 'video', 'audio', 'file']),
    mimeType: safeArtifactMimeTypeSchema,
    sizeBytes: z.number().int().nonnegative(),
  })
  .strict()
  .superRefine((media, context) => {
    const schemaByKind = {
      audio: safeArtifactAudioMimeTypeSchema,
      file: safeArtifactFileMimeTypeSchema,
      image: safeArtifactImageMimeTypeSchema,
      video: safeArtifactVideoMimeTypeSchema,
    }
    if (!schemaByKind[media.kind].safeParse(media.mimeType).success) {
      context.addIssue({
        code: 'custom',
        message: 'artifact media metadata MIME type must match media kind',
        path: ['mimeType'],
      })
    }
  })
const artifactLifecyclePayloadSchema = z
  .object({
    artifactId: z.string().min(1),
    kind: z.string().min(1).optional(),
    source: z.enum(['assistant', 'tool', 'file', 'model_service']).optional(),
    status: z.enum(['failed', 'pending', 'ready', 'running']).optional(),
    title: z.string().min(1).optional(),
    summary: z.string().min(1).optional(),
    media: artifactMediaPreviewSchema.optional(),
  })
  .strict()
const assistantReviewRequestedPayloadSchema = z
  .object({
    requestId: requestIdSchema,
    title: z.string().min(1),
    body: z.string().min(1).optional(),
  })
  .strict()
const assistantClarificationRequestedPayloadSchema = z
  .object({
    requestId: requestIdSchema,
    prompt: z.string().min(1),
  })
  .strict()
const assistantNoticePayloadSchema = z
  .object({
    noticeId: requestIdSchema,
    body: z.string().min(1),
    code: z.string().min(1).optional(),
  })
  .strict()
const engineFailedPayloadSchema = z
  .object({
    message: z.string().min(1),
  })
  .strict()
const pluginTrustLevelSchema = z.enum(['admin_trusted', 'user_controlled'])
const pluginLoadedPayloadSchema = z
  .object({
    capabilityCount: z.number().int().nonnegative().optional(),
    pluginId: permissionDisplayTextSchema,
    pluginName: permissionDisplayTextSchema,
    trustLevel: pluginTrustLevelSchema.optional(),
  })
  .strict()
const pluginRejectedPayloadSchema = z
  .object({
    pluginId: permissionDisplayTextSchema,
    pluginName: permissionDisplayTextSchema,
    reason: permissionDisplayTextSchema,
    trustLevel: pluginTrustLevelSchema.optional(),
  })
  .strict()
const pluginFailedPayloadSchema = z
  .object({
    message: z.literal('Plugin failure withheld from conversation timeline.'),
    pluginId: permissionDisplayTextSchema,
    pluginName: permissionDisplayTextSchema,
    trustLevel: pluginTrustLevelSchema.optional(),
  })
  .strict()

const baseRunEventSchema = z
  .object({
    id: z.string().min(1),
    conversationSequence: z.number().int().positive(),
    runId: z.string().min(1),
    sequence: z.number().int().nonnegative(),
    timestamp: z.string().datetime({ offset: true }),
    source: runEventSourceSchema,
    visibility: runEventVisibilitySchema,
    payload: payloadSchema.optional(),
  })
  .strict()

function eventSchema<TType extends string, TPayloadSchema extends z.ZodType>(
  type: TType,
  eventPayloadSchema: TPayloadSchema,
) {
  return baseRunEventSchema.extend({
    payload: eventPayloadSchema.optional(),
    type: z.literal(type),
  })
}

const assistantThinkingDeltaPayloadSchema = z
  .object({
    status: z.enum(['running', 'complete', 'completed', 'withheld']).optional(),
    safeSummary: z.string().min(1).optional(),
    safeSummaryDelta: z.string().min(1).optional(),
  })
  .strict()

export const runEventSchema = z
  .discriminatedUnion('type', [
    eventSchema('run.started', runStartedPayloadSchema),
    eventSchema('run.ended', runEndedPayloadSchema),
    eventSchema('user.message.appended', userMessageAppendedPayloadSchema),
    eventSchema('assistant.delta', assistantDeltaPayloadSchema),
    eventSchema('assistant.thinking.delta', assistantThinkingDeltaPayloadSchema),
    eventSchema('assistant.completed', assistantCompletedPayloadSchema),
    eventSchema('tool.requested', toolRequestedPayloadSchema),
    eventSchema('tool.approved', toolResolvedPayloadSchema),
    eventSchema('tool.denied', toolResolvedPayloadSchema),
    eventSchema('tool.completed', toolCompletedPayloadSchema),
    eventSchema('tool.failed', toolFailedPayloadSchema),
    eventSchema('permission.requested', permissionRequestedPayloadSchema),
    eventSchema('permission.resolved', permissionResolvedPayloadSchema),
    eventSchema('background.started', backgroundStartedPayloadSchema),
    eventSchema('background.permission.requested', backgroundPermissionRequestedPayloadSchema),
    eventSchema('background.permission.resolved', backgroundPermissionResolvedPayloadSchema),
    eventSchema('artifact.created', artifactLifecyclePayloadSchema),
    eventSchema('artifact.updated', artifactLifecyclePayloadSchema),
    eventSchema('assistant.review.requested', assistantReviewRequestedPayloadSchema),
    eventSchema('assistant.clarification.requested', assistantClarificationRequestedPayloadSchema),
    eventSchema('assistant.notice', assistantNoticePayloadSchema),
    eventSchema('engine.failed', engineFailedPayloadSchema),
    eventSchema('plugin.loaded', pluginLoadedPayloadSchema),
    eventSchema('plugin.rejected', pluginRejectedPayloadSchema),
    eventSchema('plugin.failed', pluginFailedPayloadSchema),
  ])
  .superRefine((event, context) => {
    if (
      (event.type === 'permission.requested' ||
        event.type === 'permission.resolved' ||
        event.type === 'background.permission.requested' ||
        event.type === 'background.permission.resolved') &&
      event.source !== 'policy'
    ) {
      context.addIssue({
        code: 'custom',
        message: 'permission events must be emitted by policy',
        path: ['source'],
      })
    }

    if (event.type === 'background.started' && event.source !== 'background') {
      context.addIssue({
        code: 'custom',
        message: 'background events must be emitted by background',
        path: ['source'],
      })
    }

    if (
      (event.type === 'plugin.loaded' ||
        event.type === 'plugin.rejected' ||
        event.type === 'plugin.failed') &&
      event.source !== 'plugin'
    ) {
      context.addIssue({
        code: 'custom',
        message: 'plugin events must be emitted by plugin',
        path: ['source'],
      })
    }

    if (event.visibility === 'withheld') {
      if (event.payload !== undefined) {
        context.addIssue({
          code: 'custom',
          message: '`payload` must be omitted when event visibility is `withheld`',
          path: ['payload'],
        })
      }

      return
    }

    if (event.payload !== undefined) {
      if (containsObviousUnredactedSecret(event.payload)) {
        context.addIssue({
          code: 'custom',
          message: 'visible event payload must not contain obvious unredacted secrets',
          path: ['payload'],
        })
      }

      if (containsUnsafeDisplayReference(event.payload)) {
        context.addIssue({
          code: 'custom',
          message: 'visible event payload must not contain unsafe display references',
          path: ['payload'],
        })
      }

      return
    }

    context.addIssue({
      code: 'custom',
      message: '`payload` is required unless event visibility is `withheld`',
      path: ['payload'],
    })
  })

export const runEventsSchema = z.array(runEventSchema).superRefine((events, context) => {
  const lastSequenceByRun = new Map<string, number>()
  let lastConversationSequence: number | undefined

  events.forEach((event, index) => {
    if (
      lastConversationSequence !== undefined &&
      event.conversationSequence <= lastConversationSequence
    ) {
      context.addIssue({
        code: 'custom',
        message: '`conversationSequence` must be strictly monotonic',
        path: [index, 'conversationSequence'],
      })
    }
    lastConversationSequence = event.conversationSequence

    const lastSequence = lastSequenceByRun.get(event.runId)

    if (lastSequence !== undefined && event.sequence <= lastSequence) {
      context.addIssue({
        code: 'custom',
        message: '`sequence` must be strictly monotonic inside a run',
        path: [index, 'sequence'],
      })
    }

    lastSequenceByRun.set(event.runId, event.sequence)
  })
})

export type RunEvent = z.infer<typeof runEventSchema>
export type RunEventType = RunEvent['type']
export type RunEventVisibility = z.infer<typeof runEventVisibilitySchema>
export type RunEventSource = z.infer<typeof runEventSourceSchema>
export type RunEventContractType = z.infer<typeof runEventContractTypeSchema>

export function mapRunEventContractType(contractType: RunEventContractType): RunEventType {
  switch (contractType) {
    case 'run_started':
      return 'run.started'
    case 'run_ended':
      return 'run.ended'
    case 'user_message_appended':
      return 'user.message.appended'
    case 'assistant_delta_produced':
      return 'assistant.delta'
    case 'assistant_message_completed':
      return 'assistant.completed'
    case 'tool_use_requested':
      return 'tool.requested'
    case 'tool_use_approved':
      return 'tool.approved'
    case 'tool_use_denied':
      return 'tool.denied'
    case 'tool_use_completed':
      return 'tool.completed'
    case 'tool_use_failed':
      return 'tool.failed'
    case 'permission_requested':
      return 'permission.requested'
    case 'permission_resolved':
      return 'permission.resolved'
    case 'artifact_created':
      return 'artifact.created'
    case 'artifact_updated':
      return 'artifact.updated'
    case 'assistant_review_requested':
      return 'assistant.review.requested'
    case 'assistant_clarification_requested':
      return 'assistant.clarification.requested'
    case 'assistant_notice':
      return 'assistant.notice'
    case 'engine_failed':
      return 'engine.failed'
    case 'plugin_loaded':
      return 'plugin.loaded'
    case 'plugin_rejected':
      return 'plugin.rejected'
    case 'plugin_failed':
      return 'plugin.failed'
    default:
      return assertNever(contractType)
  }
}

export function getRunEventLabel(event: RunEvent): string {
  switch (event.type) {
    case 'run.started':
      return 'Run started'
    case 'run.ended':
      return 'Run ended'
    case 'user.message.appended':
      return 'User message appended'
    case 'assistant.delta':
      return 'Assistant delta'
    case 'assistant.thinking.delta':
      return 'Assistant thinking delta'
    case 'assistant.completed':
      return 'Assistant completed'
    case 'tool.requested':
      return 'Tool requested'
    case 'tool.approved':
      return 'Tool approved'
    case 'tool.denied':
      return 'Tool denied'
    case 'tool.completed':
      return 'Tool completed'
    case 'tool.failed':
      return 'Tool failed'
    case 'permission.requested':
      return 'Permission requested'
    case 'permission.resolved':
      return 'Permission resolved'
    case 'background.started':
      return 'Background started'
    case 'background.permission.requested':
      return 'Background permission requested'
    case 'background.permission.resolved':
      return 'Background permission resolved'
    case 'artifact.created':
      return 'Artifact created'
    case 'artifact.updated':
      return 'Artifact updated'
    case 'assistant.review.requested':
      return 'Assistant review requested'
    case 'assistant.clarification.requested':
      return 'Assistant clarification requested'
    case 'assistant.notice':
      return 'Assistant notice'
    case 'engine.failed':
      return 'Engine failed'
    case 'plugin.loaded':
      return 'Plugin loaded'
    case 'plugin.rejected':
      return 'Plugin rejected'
    case 'plugin.failed':
      return 'Plugin failed'
    default:
      return assertNever(event)
  }
}
