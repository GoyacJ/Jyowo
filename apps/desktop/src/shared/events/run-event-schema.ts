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
  'tool_deferred_pool_changed',
  'permission_requested',
  'permission_resolved',
  'subagent_spawned',
  'subagent_announced',
  'subagent_terminated',
  'subagent_stalled',
  'subagent_permission_forwarded',
  'subagent_permission_resolved',
  'team_created',
  'team_member_joined',
  'team_member_left',
  'team_member_stalled',
  'team_task_updated',
  'agent_message_sent',
  'agent_message_routed',
  'team_turn_completed',
  'team_terminated',
  'background_started',
  'background_state_changed',
  'background_input_requested',
  'background_input_submitted',
  'background_permission_requested',
  'background_permission_resolved',
  'background_cancelled',
  'background_completed',
  'background_failed',
  'background_interrupted',
  'background_archived',
  'background_deleted',
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
    /(?:^|[^A-Za-z0-9_.-])\/(?:Applications|Library|System|Users|Volumes|dev|etc|home|media|mnt|opt|private|root|run|tmp|usr|var)(?:[\\/]|$)/.test(
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
  'dashscope',
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
    resultKind: z.enum(['text', 'structured', 'blob', 'mixed', 'offloaded']).optional(),
    toolName: toolDisplayTextSchema.optional(),
    toolUseId: z.string().min(1),
    truncated: z.boolean().optional(),
  })
  .strict()
const toolFailedPayloadSchema = z
  .object({
    code: z.literal('tool_error'),
    failureKind: z.enum(['capabilityMissing', 'toolError']).optional(),
    message: z.literal(toolErrorWithheldMessage).optional(),
    toolUseId: z.string().min(1),
  })
  .strict()
const deferredToolHintSchema = z
  .object({
    name: toolDisplayTextSchema,
    hint: toolDisplayTextSchema.nullable().optional(),
  })
  .strict()
const toolPoolChangeSourceSchema = z.union([
  z.literal('initial_classification'),
  z
    .object({
      mcp_list_changed: z.object({ server_id: toolDisplayTextSchema }).strict(),
    })
    .strict(),
  z
    .object({
      plugin_registration: z.object({ plugin_id: toolDisplayTextSchema }).strict(),
    })
    .strict(),
  z
    .object({
      skill_hot_reload: z.object({ skill_id: toolDisplayTextSchema }).strict(),
    })
    .strict(),
])
const toolDeferredPoolChangedPayloadSchema = z
  .object({
    added: z.array(deferredToolHintSchema),
    at: z.string().datetime({ offset: true }).optional(),
    deferredTotal: z.number().int().nonnegative(),
    removed: z.array(toolDisplayTextSchema),
    sessionId: z.string().min(1),
    source: toolPoolChangeSourceSchema,
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
        z
          .object({
            type: z.literal('unknown'),
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
        z
          .object({
            type: z.literal('unknown'),
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
const permissionDecisionOptionSchema = z
  .object({
    decision: z.enum(['approve', 'deny']),
    id: z.string().min(1),
    label: permissionDisplayTextSchema,
    lifetime: z.enum(['once', 'run', 'session', 'persisted']),
    matcher: z
      .object({
        kind: z.enum([
          'exactCommand',
          'exactArgs',
          'toolName',
          'category',
          'pathPrefix',
          'globPattern',
          'executeCodeScript',
          'any',
        ]),
        label: permissionDisplayTextSchema,
      })
      .strict(),
    requiresConfirmation: z.boolean(),
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
    actionPlanHash: actionPlanHashSchema,
    autoResolved: z.boolean(),
    decisionOptions: z.array(permissionDecisionOptionSchema).optional(),
    decisionScope: permissionDisplayTextSchema,
    diffSummary: permissionDisplayTextSchema.optional(),
    effectiveMode: permissionModeSchema,
    exposure: permissionDisplayTextSchema,
    operation: permissionDisplayTextSchema,
    reason: permissionDisplayTextSchema,
    review: permissionReviewSchema,
    requestId: requestIdSchema,
    sandboxPolicy: sandboxPolicySummarySchema,
    severity: z.enum(['low', 'medium', 'high', 'critical']),
    target: permissionDisplayTextSchema,
    toolUseId: z.string().min(1),
    workspaceBoundary: permissionDisplayTextSchema,
  })
  .strict()
const permissionResolvedPayloadSchema = z
  .object({
    actionPlanHash: actionPlanHashSchema,
    autoResolved: z.boolean(),
    decision: z.enum(['approve', 'deny']),
    decisionId: z.string().min(1),
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
const backgroundAgentStateSchema = z.enum([
  'queued',
  'running',
  'waiting_for_permission',
  'waiting_for_input',
  'paused',
  'cancelling',
  'cancelled',
  'succeeded',
  'failed',
  'interrupted',
  'recoverable',
  'archived',
])
const backgroundStateChangedPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
    from: backgroundAgentStateSchema,
    reason: permissionDisplayTextSchema.nullable().optional(),
    to: backgroundAgentStateSchema,
  })
  .strict()
const backgroundInputRequestedPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
    prompt: permissionDisplayTextSchema,
    requestId: requestIdSchema,
  })
  .strict()
const backgroundInputSubmittedPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
    requestId: requestIdSchema,
  })
  .strict()
const backgroundCancelledPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
    reason: permissionDisplayTextSchema.nullable().optional(),
  })
  .strict()
const backgroundCompletedPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
    summary: permissionDisplayTextSchema.nullable().optional(),
  })
  .strict()
const backgroundFailedPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
    error: permissionDisplayTextSchema,
  })
  .strict()
const backgroundInterruptedPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
    reason: permissionDisplayTextSchema,
  })
  .strict()
const backgroundIdOnlyPayloadSchema = z
  .object({
    backgroundAgentId: z.string().min(1),
  })
  .strict()
const subagentStatusSchema = z.enum([
  'completed',
  'cancelled',
  'failed',
  'stalled',
  'max_iterations_reached',
  'maxIterationsReached',
  'max_budget',
])
const subagentTerminationReasonSchema = z.enum([
  'natural_completion',
  'naturalCompletion',
  'parent_cancelled',
  'parentCancelled',
  'admin_interrupted',
  'adminInterrupted',
  'stalled',
  'bridge_broken',
  'bridgeBroken',
  'failed',
])
const subagentSpawnedPayloadSchema = z
  .object({
    depth: z.number().int().nonnegative(),
    role: permissionDisplayTextSchema,
    subagentId: z.string().min(1),
    taskSummary: permissionDisplayTextSchema,
    triggerToolUseId: z.string().min(1).nullable().optional(),
  })
  .strict()
const subagentAnnouncedPayloadSchema = z
  .object({
    redacted: z.boolean().optional().default(false),
    resultSummary: permissionDisplayTextSchema,
    status: subagentStatusSchema,
    subagentId: z.string().min(1),
  })
  .strict()
const subagentTerminatedPayloadSchema = z
  .object({
    reason: subagentTerminationReasonSchema,
    subagentId: z.string().min(1),
  })
  .strict()
const subagentIdOnlyPayloadSchema = z
  .object({
    subagentId: z.string().min(1),
  })
  .strict()
const subagentPermissionForwardedPayloadSchema = z
  .object({
    reason: permissionDisplayTextSchema,
    requestId: requestIdSchema,
    subagentId: z.string().min(1),
  })
  .strict()
const subagentPermissionResolvedPayloadSchema = z
  .object({
    decision: z.enum(['approve', 'deny']),
    requestId: requestIdSchema,
    subagentId: z.string().min(1),
  })
  .strict()
const teamCreatedPayloadSchema = z
  .object({
    name: permissionDisplayTextSchema,
    teamId: z.string().min(1),
    topologyKind: z.enum(['coordinator_worker', 'peer_to_peer', 'role_routed', 'custom']),
  })
  .strict()
const teamMemberJoinedPayloadSchema = z
  .object({
    agentId: z.string().min(1),
    role: permissionDisplayTextSchema,
    teamId: z.string().min(1),
  })
  .strict()
const teamMemberLeftPayloadSchema = z
  .object({
    agentId: z.string().min(1),
    reason: z.enum([
      'goal_achieved',
      'quota_exceeded',
      'interrupted',
      'error',
      'removed',
      'stalled_removed',
    ]),
    teamId: z.string().min(1),
  })
  .strict()
const teamMemberStalledPayloadSchema = z
  .object({
    agentId: z.string().min(1),
    teamId: z.string().min(1),
  })
  .strict()
const teamTaskUpdatedPayloadSchema = z
  .object({
    assigneeProfileId: permissionDisplayTextSchema.nullable().optional(),
    status: permissionDisplayTextSchema,
    taskId: permissionDisplayTextSchema,
    teamId: z.string().min(1),
    title: permissionDisplayTextSchema,
  })
  .strict()
const teamTerminatedPayloadSchema = z
  .object({
    reason: z.enum(['completed', 'cancelled', 'error', 'member_failed', 'idle_timeout', 'timeout']),
    teamId: z.string().min(1),
  })
  .strict()
const agentMessageSentPayloadSchema = z
  .object({
    messageId: z.string().min(1),
    teamId: z.string().min(1),
  })
  .strict()
const agentMessageRoutedPayloadSchema = z
  .object({
    messageId: z.string().min(1),
    resolvedRecipients: z.array(z.string().min(1)),
    routingPolicy: z.enum(['direct', 'role', 'broadcast', 'coordinator', 'custom']),
    teamId: z.string().min(1),
  })
  .strict()
const teamTurnCompletedPayloadSchema = z
  .object({
    participatingAgents: z.array(z.string().min(1)),
    teamId: z.string().min(1),
    turnId: z.string().min(1),
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
    eventSchema('tool.deferred_pool_changed', toolDeferredPoolChangedPayloadSchema),
    eventSchema('permission.requested', permissionRequestedPayloadSchema),
    eventSchema('permission.resolved', permissionResolvedPayloadSchema),
    eventSchema('subagent.spawned', subagentSpawnedPayloadSchema),
    eventSchema('subagent.announced', subagentAnnouncedPayloadSchema),
    eventSchema('subagent.terminated', subagentTerminatedPayloadSchema),
    eventSchema('subagent.stalled', subagentIdOnlyPayloadSchema),
    eventSchema('subagent.permission.forwarded', subagentPermissionForwardedPayloadSchema),
    eventSchema('subagent.permission.resolved', subagentPermissionResolvedPayloadSchema),
    eventSchema('team.created', teamCreatedPayloadSchema),
    eventSchema('team.member.joined', teamMemberJoinedPayloadSchema),
    eventSchema('team.member.left', teamMemberLeftPayloadSchema),
    eventSchema('team.member.stalled', teamMemberStalledPayloadSchema),
    eventSchema('team.task.updated', teamTaskUpdatedPayloadSchema),
    eventSchema('agent.message.sent', agentMessageSentPayloadSchema),
    eventSchema('agent.message.routed', agentMessageRoutedPayloadSchema),
    eventSchema('team.turn.completed', teamTurnCompletedPayloadSchema),
    eventSchema('team.terminated', teamTerminatedPayloadSchema),
    eventSchema('background.started', backgroundStartedPayloadSchema),
    eventSchema('background.state.changed', backgroundStateChangedPayloadSchema),
    eventSchema('background.input.requested', backgroundInputRequestedPayloadSchema),
    eventSchema('background.input.submitted', backgroundInputSubmittedPayloadSchema),
    eventSchema('background.permission.requested', backgroundPermissionRequestedPayloadSchema),
    eventSchema('background.permission.resolved', backgroundPermissionResolvedPayloadSchema),
    eventSchema('background.cancelled', backgroundCancelledPayloadSchema),
    eventSchema('background.completed', backgroundCompletedPayloadSchema),
    eventSchema('background.failed', backgroundFailedPayloadSchema),
    eventSchema('background.interrupted', backgroundInterruptedPayloadSchema),
    eventSchema('background.archived', backgroundIdOnlyPayloadSchema),
    eventSchema('background.deleted', backgroundIdOnlyPayloadSchema),
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

    if (
      (event.type === 'background.started' ||
        event.type === 'background.state.changed' ||
        event.type === 'background.input.requested' ||
        event.type === 'background.input.submitted' ||
        event.type === 'background.cancelled' ||
        event.type === 'background.completed' ||
        event.type === 'background.failed' ||
        event.type === 'background.interrupted' ||
        event.type === 'background.archived' ||
        event.type === 'background.deleted') &&
      event.source !== 'background'
    ) {
      context.addIssue({
        code: 'custom',
        message: 'background events must be emitted by background',
        path: ['source'],
      })
    }

    if (
      (event.type === 'subagent.permission.forwarded' ||
        event.type === 'subagent.permission.resolved') &&
      event.source !== 'policy'
    ) {
      context.addIssue({
        code: 'custom',
        message: 'subagent permission events must be emitted by policy',
        path: ['source'],
      })
    }

    if (
      (event.type === 'subagent.spawned' ||
        event.type === 'subagent.announced' ||
        event.type === 'subagent.terminated' ||
        event.type === 'subagent.stalled' ||
        event.type === 'team.created' ||
        event.type === 'team.member.joined' ||
        event.type === 'team.member.left' ||
        event.type === 'team.member.stalled' ||
        event.type === 'team.task.updated' ||
        event.type === 'agent.message.sent' ||
        event.type === 'agent.message.routed' ||
        event.type === 'team.turn.completed' ||
        event.type === 'team.terminated') &&
      event.source !== 'agent'
    ) {
      context.addIssue({
        code: 'custom',
        message: 'agent events must be emitted by agent',
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
    case 'tool_deferred_pool_changed':
      return 'tool.deferred_pool_changed'
    case 'permission_requested':
      return 'permission.requested'
    case 'permission_resolved':
      return 'permission.resolved'
    case 'subagent_spawned':
      return 'subagent.spawned'
    case 'subagent_announced':
      return 'subagent.announced'
    case 'subagent_terminated':
      return 'subagent.terminated'
    case 'subagent_stalled':
      return 'subagent.stalled'
    case 'subagent_permission_forwarded':
      return 'subagent.permission.forwarded'
    case 'subagent_permission_resolved':
      return 'subagent.permission.resolved'
    case 'team_created':
      return 'team.created'
    case 'team_member_joined':
      return 'team.member.joined'
    case 'team_member_left':
      return 'team.member.left'
    case 'team_member_stalled':
      return 'team.member.stalled'
    case 'team_task_updated':
      return 'team.task.updated'
    case 'agent_message_sent':
      return 'agent.message.sent'
    case 'agent_message_routed':
      return 'agent.message.routed'
    case 'team_turn_completed':
      return 'team.turn.completed'
    case 'team_terminated':
      return 'team.terminated'
    case 'background_started':
      return 'background.started'
    case 'background_state_changed':
      return 'background.state.changed'
    case 'background_input_requested':
      return 'background.input.requested'
    case 'background_input_submitted':
      return 'background.input.submitted'
    case 'background_permission_requested':
      return 'background.permission.requested'
    case 'background_permission_resolved':
      return 'background.permission.resolved'
    case 'background_cancelled':
      return 'background.cancelled'
    case 'background_completed':
      return 'background.completed'
    case 'background_failed':
      return 'background.failed'
    case 'background_interrupted':
      return 'background.interrupted'
    case 'background_archived':
      return 'background.archived'
    case 'background_deleted':
      return 'background.deleted'
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
    case 'tool.deferred_pool_changed':
      return 'Deferred tools changed'
    case 'permission.requested':
      return 'Permission requested'
    case 'permission.resolved':
      return 'Permission resolved'
    case 'subagent.spawned':
      return 'Subagent spawned'
    case 'subagent.announced':
      return 'Subagent announced'
    case 'subagent.terminated':
      return 'Subagent terminated'
    case 'subagent.stalled':
      return 'Subagent stalled'
    case 'subagent.permission.forwarded':
      return 'Subagent permission forwarded'
    case 'subagent.permission.resolved':
      return 'Subagent permission resolved'
    case 'team.created':
      return 'Team created'
    case 'team.member.joined':
      return 'Team member joined'
    case 'team.member.left':
      return 'Team member left'
    case 'team.member.stalled':
      return 'Team member stalled'
    case 'team.task.updated':
      return 'Team task updated'
    case 'agent.message.sent':
      return 'Agent message sent'
    case 'agent.message.routed':
      return 'Agent message routed'
    case 'team.turn.completed':
      return 'Team turn completed'
    case 'team.terminated':
      return 'Team terminated'
    case 'background.started':
      return 'Background started'
    case 'background.state.changed':
      return 'Background state changed'
    case 'background.input.requested':
      return 'Background input requested'
    case 'background.input.submitted':
      return 'Background input submitted'
    case 'background.permission.requested':
      return 'Background permission requested'
    case 'background.permission.resolved':
      return 'Background permission resolved'
    case 'background.cancelled':
      return 'Background cancelled'
    case 'background.completed':
      return 'Background completed'
    case 'background.failed':
      return 'Background failed'
    case 'background.interrupted':
      return 'Background interrupted'
    case 'background.archived':
      return 'Background archived'
    case 'background.deleted':
      return 'Background deleted'
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
