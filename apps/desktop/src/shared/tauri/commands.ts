import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { listen as tauriListen } from '@tauri-apps/api/event'
import { z } from 'zod'

import { runEventsSchema } from '@/shared/events/run-event-schema'

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
    /(?:\/Applications|\/Library|\/System|\/Users|\/Volumes|\/dev|\/etc|\/home|\/media|\/mnt|\/opt|\/private|\/root|\/run|\/tmp|\/usr|\/var)(?:[\\/]|$)/.test(
      value,
    )
  )
}

const conversationDisplayTextSchema = z
  .string()
  .refine((value) => !hasObviousUnredactedSecret(value), {
    message: 'conversation message body must not contain obvious unredacted secrets',
  })
  .refine((value) => !hasPrivateAbsolutePath(value), {
    message: 'conversation message body must not contain private absolute paths',
  })
  .refine((value) => !hasUnsafeDisplayReference(value), {
    message: 'conversation display text must not contain unsafe display references',
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

const startRunRequestSchema = z
  .object({
    attachments: z.array(attachmentReferenceSchema).optional(),
    clientMessageId: z.uuid().regex(uuidV4Pattern).optional(),
    conversationId: z.string().min(1),
    contextReferences: z.array(contextReferenceSchema).optional(),
    permissionMode: permissionModeSchema.optional(),
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

const conversationEventRefSchema = z
  .object({
    eventId: z.string().min(1),
    cursor: conversationCursorSchema,
  })
  .strict()

const conversationTurnCursorSchema = z
  .object({
    turnId: z.string().min(1),
    position: z.number().int().nonnegative(),
  })
  .strict()

const thinkingStepSchema = z
  .object({
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    kind: z.enum([
      'status',
      'reasoningSummary',
      'toolPlanning',
      'toolResult',
      'synthesis',
      'withheld',
    ]),
    status: z.enum(['running', 'complete', 'failed', 'withheld']),
    title: conversationDisplayTextSchema,
    body: conversationDisplayTextSchema.optional(),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const thinkingSegmentSchema = z
  .object({
    kind: z.literal('thinking'),
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    status: z.enum(['running', 'complete', 'withheld']),
    summary: z
      .object({
        text: conversationDisplayTextSchema,
      })
      .strict(),
    steps: z.array(thinkingStepSchema).optional(),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const textSegmentSchema = z
  .object({
    kind: z.literal('text'),
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    messageId: z.string().min(1),
    body: conversationDisplayTextSchema,
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const toolPermissionStateSchema = z
  .object({
    id: z.string().min(1),
    requestId: z.string().min(1),
    toolUseId: z.string().min(1),
    status: z.enum(['pending', 'submitting', 'approved', 'denied', 'failed']),
    summary: conversationDisplayTextSchema.optional(),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const toolAttemptSchema = z
  .object({
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    toolUseId: z.string().min(1),
    toolName: conversationDisplayTextSchema,
    status: z.enum(['queued', 'waitingPermission', 'running', 'completed', 'failed', 'denied']),
    permission: toolPermissionStateSchema.optional(),
    failureSummary: conversationDisplayTextSchema.optional(),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const toolGroupSegmentSchema = z
  .object({
    kind: z.literal('toolGroup'),
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    attempts: z.array(toolAttemptSchema),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const artifactSegmentStatusSchema = z.enum(['failed', 'pending', 'ready', 'running'])
const artifactSourceSchema = z.enum(['assistant', 'tool', 'file', 'model_service'])
const artifactMediaKindSchema = z.enum(['image', 'video', 'audio', 'file'])
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
    kind: artifactMediaKindSchema,
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
        code: z.ZodIssueCode.custom,
        message: 'artifact media metadata MIME type must match media kind',
        path: ['mimeType'],
      })
    }
  })

const processDiffFileSchema = z
  .object({
    path: conversationDisplayTextSchema,
    addedLines: z.number().int().nonnegative(),
    removedLines: z.number().int().nonnegative(),
    preview: conversationDisplayTextSchema.optional(),
  })
  .strict()

const processStepDetailSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('activity'),
      summary: conversationDisplayTextSchema,
      itemCount: z.number().int().nonnegative().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('command'),
      command: conversationDisplayTextSchema,
      output: conversationDisplayTextSchema.optional(),
      exitCode: z.number().int().optional(),
      durationMs: z.number().int().nonnegative().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('diff'),
      files: z.array(processDiffFileSchema),
    })
    .strict(),
  z
    .object({
      type: z.literal('tool'),
      toolName: conversationDisplayTextSchema,
      outputSummary: conversationDisplayTextSchema.optional(),
      durationMs: z.number().int().nonnegative().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('artifact'),
      artifactId: z.string().min(1),
      media: artifactMediaPreviewSchema,
    })
    .strict(),
])

const processStepSchema = z
  .object({
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    kind: z.enum([
      'reasoning',
      'activity',
      'command',
      'fileRead',
      'fileSearch',
      'fileEdit',
      'diff',
      'tool',
      'artifact',
      'synthesis',
      'withheld',
    ]),
    status: z.enum(['running', 'complete', 'failed', 'withheld']),
    title: conversationDisplayTextSchema,
    body: conversationDisplayTextSchema.optional(),
    detail: processStepDetailSchema.optional(),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const processSegmentSchema = z
  .object({
    kind: z.literal('process'),
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    status: z.enum(['running', 'complete', 'failed', 'cancelled', 'withheld']),
    summary: conversationDisplayTextSchema,
    steps: z.array(processStepSchema).optional(),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const artifactSegmentSchema = z
  .object({
    kind: z.literal('artifact'),
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    artifactId: z.string().min(1),
    artifactKind: z.string().min(1).optional(),
    status: artifactSegmentStatusSchema.optional(),
    source: artifactSourceSchema.optional(),
    title: conversationDisplayTextSchema,
    summary: conversationDisplayTextSchema.optional(),
    media: artifactMediaPreviewSchema.optional(),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const reviewRequestSegmentSchema = z
  .object({
    kind: z.literal('reviewRequest'),
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    requestId: z.string().min(1),
    title: conversationDisplayTextSchema,
    body: conversationDisplayTextSchema.optional(),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const clarificationRequestSegmentSchema = z
  .object({
    kind: z.literal('clarificationRequest'),
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    requestId: z.string().min(1),
    prompt: conversationDisplayTextSchema,
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const noticeSegmentSchema = z
  .object({
    kind: z.literal('notice'),
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    body: conversationDisplayTextSchema,
    code: z.string().min(1).optional(),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const errorSegmentSchema = z
  .object({
    kind: z.literal('error'),
    id: z.string().min(1),
    order: z.number().int().nonnegative(),
    body: conversationDisplayTextSchema,
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const assistantSegmentSchema = z.discriminatedUnion('kind', [
  processSegmentSchema,
  thinkingSegmentSchema,
  textSegmentSchema,
  toolGroupSegmentSchema,
  artifactSegmentSchema,
  reviewRequestSegmentSchema,
  clarificationRequestSegmentSchema,
  noticeSegmentSchema,
  errorSegmentSchema,
])

const assistantWorkSchema = z
  .object({
    id: z.string().min(1),
    runId: z.string().min(1),
    status: z.enum(['running', 'complete', 'failed', 'cancelled']),
    segments: z.array(assistantSegmentSchema),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const conversationTurnUserMessageSchema = z
  .object({
    id: z.string().min(1),
    messageId: z.string().min(1),
    body: conversationDisplayTextSchema,
    clientMessageId: z.string().min(1).optional(),
    attachments: z.array(attachmentReferenceSchema).optional(),
    timestamp: z.string().datetime({ offset: true }),
    eventRefs: z.array(conversationEventRefSchema).optional(),
  })
  .strict()

const conversationTurnSchema = z
  .object({
    id: z.string().min(1),
    conversationId: z.string().min(1),
    position: z.number().int().nonnegative(),
    user: conversationTurnUserMessageSchema,
    assistant: assistantWorkSchema.optional(),
  })
  .strict()

const pageConversationWorktreeRequestSchema = z
  .object({
    conversationId: z.string().min(1),
    pageCursor: conversationTurnCursorSchema.optional(),
    direction: z.enum(['before', 'after']).optional(),
    limit: z.number().int().positive().max(200).optional(),
  })
  .strict()

const pageConversationWorktreeResponseSchema = z
  .object({
    turns: z.array(conversationTurnSchema),
    pageCursor: conversationTurnCursorSchema.optional(),
    eventCursor: conversationCursorSchema.optional(),
    hasMoreBefore: z.boolean(),
    hasMoreAfter: z.boolean(),
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

const artifactStatusSchema = artifactSegmentStatusSchema
const maxArtifactPreviewBytes = 16 * 1024
const artifactPreviewSchema = z
  .string()
  .refine((value) => !hasObviousUnredactedSecret(value), {
    message: 'Artifact preview must not contain obvious unredacted secrets',
  })
  .refine((value) => !hasUnsafeDisplayReference(value), {
    message: 'Artifact preview must not contain unsafe display references',
  })
  .refine((value) => new TextEncoder().encode(value).byteLength <= maxArtifactPreviewBytes, {
    message: `Artifact preview must be at most ${maxArtifactPreviewBytes} UTF-8 bytes`,
  })
const artifactDisplayTextSchema = conversationDisplayTextSchema

const artifactSummarySchema = z
  .object({
    actionLabel: artifactDisplayTextSchema.min(1),
    description: artifactDisplayTextSchema,
    id: z.string().min(1),
    kind: artifactDisplayTextSchema.min(1),
    preview: artifactPreviewSchema.optional(),
    status: artifactStatusSchema,
    title: artifactDisplayTextSchema.min(1),
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

const getArtifactMediaPreviewRequestSchema = z
  .object({
    conversationId: z.string().min(1),
    artifactId: z.string().min(1),
  })
  .strict()

const maxArtifactMediaPreviewBytes = 10 * 1024 * 1024
const maxArtifactMediaPreviewDataUrlBytes = Math.ceil(maxArtifactMediaPreviewBytes * 1.4) + 128
const artifactMediaPreviewDataUrlSchema = z
  .string()
  .max(maxArtifactMediaPreviewDataUrlBytes)
  .regex(/^data:image\/(?:png|jpeg|gif|webp|avif);base64,[A-Za-z0-9+/]+={0,2}$/, {
    message: 'artifact image preview must be an image data URL',
  })
  .refine((value) => !hasObviousUnredactedSecret(value), {
    message: 'artifact image preview must not contain obvious unredacted secrets',
  })
  .refine((value) => !hasPrivateAbsolutePath(value), {
    message: 'artifact image preview must not contain private absolute paths',
  })

const getArtifactMediaPreviewResponseSchema = z
  .object({
    dataUrl: artifactMediaPreviewDataUrlSchema,
    mimeType: safeArtifactImageMimeTypeSchema,
    sizeBytes: z.number().int().nonnegative().max(maxArtifactMediaPreviewBytes),
  })
  .strict()

const getAttachmentMediaPreviewRequestSchema = z
  .object({
    conversationId: z.string().min(1),
    attachmentId: z.string().min(1),
  })
  .strict()

const maxAttachmentMediaPreviewBytes = 5 * 1024 * 1024
const maxAttachmentMediaPreviewDataUrlBytes = Math.ceil(maxAttachmentMediaPreviewBytes * 1.4) + 128
const attachmentMediaPreviewDataUrlSchema = z
  .string()
  .max(maxAttachmentMediaPreviewDataUrlBytes)
  .regex(/^data:image\/(?:png|jpeg|gif|webp|avif);base64,[A-Za-z0-9+/]+={0,2}$/, {
    message: 'attachment image preview must be an image data URL',
  })
  .refine((value) => !hasObviousUnredactedSecret(value), {
    message: 'attachment image preview must not contain obvious unredacted secrets',
  })
  .refine((value) => !hasPrivateAbsolutePath(value), {
    message: 'attachment image preview must not contain private absolute paths',
  })

const imageDataUrlMimeType = (value: string): string | null => {
  const match = /^data:(image\/(?:png|jpeg|gif|webp|avif));base64,/.exec(value)
  return match?.[1] ?? null
}

const getAttachmentMediaPreviewResponseSchema = z
  .object({
    dataUrl: attachmentMediaPreviewDataUrlSchema,
    mimeType: safeArtifactImageMimeTypeSchema,
    sizeBytes: z.number().int().nonnegative().max(maxAttachmentMediaPreviewBytes),
  })
  .strict()
  .superRefine((value, context) => {
    if (imageDataUrlMimeType(value.dataUrl) !== value.mimeType) {
      context.addIssue({
        code: 'custom',
        message: 'attachment image preview data URL MIME must match mimeType',
        path: ['dataUrl'],
      })
    }
  })

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

const contextCompressionTriggerRatioSchema = z.number().min(0.5).max(0.95)
const agentCapabilityKindSchema = z.enum(['subagents', 'agentTeams', 'backgroundAgents'])
const agentCapabilityUnavailableReasonSchema = z
  .object({
    capability: agentCapabilityKindSchema,
    type: z.literal('notCompiled'),
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
    toolProfile: toolProfileSchema,
  })
  .strict()

const isoDateTimeSchema = z.string().datetime({ offset: true })

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

const automationIdSchema = z
  .string()
  .trim()
  .min(1)
  .max(96)
  .regex(/^[A-Za-z0-9][A-Za-z0-9._-]*$/)
const localIsolationTagSchema = z.enum(['none', 'bubblewrap', 'seatbelt', 'job_object'])
const sandboxModeSchema = z.union([
  z.enum(['none', 'container', 'remote']),
  z.object({ os_level: localIsolationTagSchema }).strict(),
])
const workspaceAccessSchema = z.union([
  z.enum(['none', 'read_only']),
  z
    .object({
      read_write: z
        .object({
          allowed_writable_subpaths: z.array(z.string().min(1)).default([]),
        })
        .strict(),
    })
    .strict(),
])
const missedRunPolicySchema = z.enum(['skip', 'run_once'])
const automationScheduleSchema = z
  .object({
    intervalMinutes: z.number().int().positive(),
  })
  .strict()
const automationSpecSchema = z
  .object({
    createdAt: isoDateTimeSchema,
    enabled: z.boolean().default(false),
    id: automationIdSchema,
    missedRunPolicy: missedRunPolicySchema.default('skip'),
    permissionMode: permissionModeSchema,
    prompt: z
      .string()
      .trim()
      .min(1)
      .max(64 * 1024)
      .refine((value) => !hasObviousUnredactedSecret(value), {
        message: 'automation prompt must not contain obvious unredacted secrets',
      }),
    sandboxMode: sandboxModeSchema,
    schedule: automationScheduleSchema,
    toolProfile: toolProfileSchema,
    updatedAt: isoDateTimeSchema,
    workspaceAccess: workspaceAccessSchema,
    workspaceScope: z.literal('current_workspace'),
  })
  .strict()
const automationRunStatusSchema = z.enum(['started', 'rejected', 'failed'])
const automationRunRecordSchema = z
  .object({
    automationId: automationIdSchema,
    completedAt: isoDateTimeSchema.optional(),
    id: z.string().min(1).max(128),
    message: z.string().max(4096).optional(),
    runId: z.string().min(1).optional(),
    startedAt: isoDateTimeSchema,
    status: automationRunStatusSchema,
  })
  .strict()
const listAutomationsResponseSchema = z
  .object({
    automations: z.array(automationSpecSchema),
  })
  .strict()
const saveAutomationRequestSchema = z
  .object({
    automation: automationSpecSchema,
  })
  .strict()
const saveAutomationResponseSchema = z
  .object({
    automation: automationSpecSchema,
    status: z.literal('saved'),
  })
  .strict()
const deleteAutomationRequestSchema = z
  .object({
    id: automationIdSchema,
  })
  .strict()
const deleteAutomationResponseSchema = z
  .object({
    id: automationIdSchema,
    status: z.literal('deleted'),
  })
  .strict()
const setAutomationEnabledRequestSchema = z
  .object({
    enabled: z.boolean(),
    id: automationIdSchema,
  })
  .strict()
const setAutomationEnabledResponseSchema = z
  .object({
    automation: automationSpecSchema,
    status: z.literal('saved'),
  })
  .strict()
const runAutomationNowRequestSchema = deleteAutomationRequestSchema
const runAutomationNowResponseSchema = z
  .object({
    record: automationRunRecordSchema,
  })
  .strict()
const listAutomationRunsRequestSchema = z
  .object({
    automationId: automationIdSchema.optional(),
  })
  .strict()
const listAutomationRunsResponseSchema = z
  .object({
    runs: z.array(automationRunRecordSchema),
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

const capabilityRouteKindSchema = z.enum([
  'image_generation',
  'video_generation',
  'text_to_speech',
  'speech_to_text',
  'music_generation',
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

const mcpServerOriginSchema = z.enum(['managed', 'plugin', 'policy', 'user', 'workspace'])

const mcpDiagnosticSeveritySchema = z.enum(['info', 'warning', 'error'])

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
    serverId: mcpServerIdSchema,
    severity: mcpDiagnosticSeveritySchema,
    summary: mcpDiagnosticSummarySchema,
    timestamp: z.string().min(1),
  })
  .strict()

const mcpServerSummarySchema = z
  .object({
    displayName: z.string().min(1),
    enabled: z.boolean(),
    exposedToolCount: z.number().int().min(0),
    id: mcpServerIdSchema,
    lastDiagnostic: mcpDiagnosticSummarySchema.optional(),
    lastDiagnosticAt: z.string().min(1).optional(),
    lastDiagnosticSeverity: mcpDiagnosticSeveritySchema.optional(),
    lastError: z.string().min(1).optional(),
    manageable: z.boolean(),
    origin: mcpServerOriginSchema,
    scope: mcpServerScopeSchema,
    sourcePluginId: z.string().min(1).optional(),
    status: mcpServerStatusSchema,
    transport: mcpServerTransportKindSchema,
  })
  .strict()

const listMcpServersResponseSchema = z
  .object({
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
  return /(?:auth|api_?key|authorization|bearer|password|secret|token)/i.test(value)
}

function isSensitiveHeaderName(value: string): boolean {
  return /^(?:authorization|cookie|set-cookie|proxy-authorization)$/i.test(value.trim())
}

const mcpNameValueRecordSchema = z
  .object({
    key: z.string().trim().min(1),
    value: z.string(),
  })
  .strict()

const mcpStdioEnvRecordSchema = mcpNameValueRecordSchema
  .refine((record) => mcpEnvVarNameSchema.safeParse(record.key).success, {
    message: 'MCP stdio env key must be an environment variable name',
  })
  .refine((record) => !isSensitiveEnvName(record.key), {
    message: 'MCP stdio inline env must not contain secret-bearing keys',
  })
  .refine((record) => !hasObviousUnredactedSecret(record.value), {
    message: 'MCP stdio inline env must not contain obvious unredacted secrets',
  })

const mcpHttpHeaderRecordSchema = mcpNameValueRecordSchema
  .refine((record) => !isSensitiveHeaderName(record.key), {
    message: 'MCP static headers must not contain sensitive header names',
  })
  .refine((record) => !hasObviousUnredactedSecret(record.value), {
    message: 'MCP static headers must not contain obvious unredacted secrets',
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

const mcpStdioTransportRequestSchema = z
  .object({
    args: z.array(z.string()).max(64).default([]),
    command: z.string().trim().min(1),
    env: z.array(mcpStdioEnvRecordSchema).max(64).default([]),
    inheritEnv: z.array(mcpEnvVarNameSchema).max(128).default([]),
    kind: z.literal('stdio'),
    workingDir: z.string().trim().min(1).optional(),
  })
  .strict()

const mcpHttpTransportRequestSchema = z
  .object({
    bearerTokenEnvVar: mcpEnvVarNameSchema.optional(),
    headers: z.array(mcpHttpHeaderRecordSchema).max(64).default([]),
    headersFromEnv: z.array(mcpHeaderEnvRecordSchema).max(64).default([]),
    kind: z.literal('http'),
    url: z
      .string()
      .trim()
      .url()
      .refine((value) => /^https?:\/\//i.test(value), {
        message: 'MCP HTTP URL must use http or https',
      }),
  })
  .strict()

const mcpServerTransportRequestSchema = z.discriminatedUnion('kind', [
  mcpStdioTransportRequestSchema,
  mcpHttpTransportRequestSchema,
])

const saveMcpServerRequestSchema = z
  .object({
    displayName: z.string().trim().min(1),
    enabled: z.boolean().default(true),
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

const mcpServerConfigSchema = z
  .object({
    displayName: z.string().trim().min(1),
    enabled: z.boolean(),
    id: mcpServerIdSchema,
    scope: mcpServerScopeSchema,
    transport: mcpServerTransportRequestSchema,
  })
  .strict()

const getMcpServerConfigRequestSchema = z
  .object({
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
    id: mcpServerIdSchema,
  })
  .strict()

const deleteMcpServerResponseSchema = z
  .object({
    id: mcpServerIdSchema,
    status: z.literal('deleted'),
  })
  .strict()

const setMcpServerEnabledRequestSchema = z
  .object({
    enabled: z.boolean(),
    id: mcpServerIdSchema,
  })
  .strict()

const setMcpServerEnabledResponseSchema = z
  .object({
    server: mcpServerSummarySchema,
  })
  .strict()

const restartMcpServerRequestSchema = z
  .object({
    id: mcpServerIdSchema,
  })
  .strict()

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
const skillCatalogSourceIdSchema = z.enum([
  'anthropic',
  'agent-skills-spec',
  'awesome-agent-skills',
  'clawhub',
])
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
    sourceId: z.string().min(1),
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
    ]),
    startedAt: z.string().min(1),
    status: z.enum(['running', 'completed', 'failed']),
    updatedAt: z.string().min(1),
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
    sourceId: z.string().min(1),
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
    ]),
    version: z.string().min(1).optional(),
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
export type HarnessHealthcheck = z.infer<typeof harnessHealthcheckSchema>
export type ListConversationsResponse = z.infer<typeof listConversationsResponseSchema>
export type ListProjectsResponse = z.infer<typeof listProjectsResponseSchema>
export type SwitchProjectResponse = z.infer<typeof switchProjectResponseSchema>
export type DeleteProjectResponse = z.infer<typeof deleteProjectResponseSchema>
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
export type ConversationEventRef = z.infer<typeof conversationEventRefSchema>
export type ConversationTurnCursor = z.infer<typeof conversationTurnCursorSchema>
export type ThinkingSegment = z.infer<typeof thinkingSegmentSchema>
export type TextSegment = z.infer<typeof textSegmentSchema>
export type ToolPermissionState = z.infer<typeof toolPermissionStateSchema>
export type ToolAttempt = z.infer<typeof toolAttemptSchema>
export type ToolGroupSegment = z.infer<typeof toolGroupSegmentSchema>
export type ProcessSegment = z.infer<typeof processSegmentSchema>
export type ProcessStep = z.infer<typeof processStepSchema>
export type ArtifactSegment = z.infer<typeof artifactSegmentSchema>
export type ReviewRequestSegment = z.infer<typeof reviewRequestSegmentSchema>
export type ClarificationRequestSegment = z.infer<typeof clarificationRequestSegmentSchema>
export type NoticeSegment = z.infer<typeof noticeSegmentSchema>
export type ErrorSegment = z.infer<typeof errorSegmentSchema>
export type AssistantSegment = z.infer<typeof assistantSegmentSchema>
export type AssistantWork = z.infer<typeof assistantWorkSchema>
export type ConversationTurn = z.infer<typeof conversationTurnSchema>
type PageConversationWorktreeRequest = z.infer<typeof pageConversationWorktreeRequestSchema>
export type PageConversationWorktreeResponse = z.infer<
  typeof pageConversationWorktreeResponseSchema
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
export type GetArtifactMediaPreviewRequest = z.infer<typeof getArtifactMediaPreviewRequestSchema>
export type GetArtifactMediaPreviewResponse = z.infer<typeof getArtifactMediaPreviewResponseSchema>
export type GetAttachmentMediaPreviewRequest = z.infer<
  typeof getAttachmentMediaPreviewRequestSchema
>
export type GetAttachmentMediaPreviewResponse = z.infer<
  typeof getAttachmentMediaPreviewResponseSchema
>
export type GetContextSnapshotRequest = z.infer<typeof getContextSnapshotRequestSchema>
export type GetContextSnapshotResponse = z.infer<typeof getContextSnapshotResponseSchema>
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
export type AutomationSpec = z.infer<typeof automationSpecSchema>
export type AutomationRunRecord = z.infer<typeof automationRunRecordSchema>
export type ListAutomationsResponse = z.infer<typeof listAutomationsResponseSchema>
export type SaveAutomationRequest = z.input<typeof saveAutomationRequestSchema>
export type SaveAutomationResponse = z.infer<typeof saveAutomationResponseSchema>
export type DeleteAutomationResponse = z.infer<typeof deleteAutomationResponseSchema>
export type SetAutomationEnabledResponse = z.infer<typeof setAutomationEnabledResponseSchema>
export type RunAutomationNowResponse = z.infer<typeof runAutomationNowResponseSchema>
export type ListAutomationRunsResponse = z.infer<typeof listAutomationRunsResponseSchema>
export type RequestProviderConfigApiKeyRevealResponse = z.infer<
  typeof requestProviderConfigApiKeyRevealResponseSchema
>
export type GetProviderConfigApiKeyResponse = z.infer<typeof getProviderConfigApiKeyResponseSchema>
export type SetConversationModelConfigResponse = z.infer<
  typeof setConversationModelConfigResponseSchema
>
export type McpServerSummary = z.infer<typeof mcpServerSummarySchema>
export type McpServerConfig = z.infer<typeof mcpServerConfigSchema>
export type ListMcpServersResponse = z.infer<typeof listMcpServersResponseSchema>
export type BrowserMcpPreset = z.infer<typeof browserMcpPresetSchema>
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
  deleteAutomation: (id: string) => Promise<DeleteAutomationResponse>
  deleteConversation: (conversationId: string) => Promise<DeleteConversationResponse>
  deleteMcpServer: (id: string) => Promise<DeleteMcpServerResponse>
  deleteMemoryItem: (id: string) => Promise<DeleteMemoryItemResponse>
  uninstallPlugin: (pluginId: string) => Promise<PluginOperationResult>
  deleteSkill: (id: string) => Promise<DeleteSkillResponse>
  getContextSnapshot: (request: GetContextSnapshotRequest) => Promise<GetContextSnapshotResponse>
  getConversation: (conversationId: string) => Promise<GetConversationResponse>
  getAppInfo: () => Promise<AppInfo>
  getHarnessHealthcheck: () => Promise<HarnessHealthcheck>
  getMemoryItem: (id: string) => Promise<GetMemoryItemResponse>
  getMcpServerConfig: (id: string) => Promise<GetMcpServerConfigResponse>
  getPluginDetail: (pluginId: string) => Promise<GetPluginDetailResponse>
  getProviderConfigApiKey: (
    configId: string,
    revealToken: string,
  ) => Promise<GetProviderConfigApiKeyResponse>
  getReplayTimeline: (request: ReplayTimelineRequest) => Promise<ReplayTimelineResponse>
  getSkillCatalogEntry: (
    request: GetSkillCatalogEntryRequest,
  ) => Promise<GetSkillCatalogEntryResponse>
  getSkillCatalogFile: (request: GetSkillCatalogFileRequest) => Promise<GetSkillCatalogFileResponse>
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
  ) => Promise<() => void>
  exportMemoryItems: () => Promise<ExportMemoryItemsResponse>
  exportSupportBundle: (request: ExportSupportBundleRequest) => Promise<ExportSupportBundleResponse>
  getExecutionSettings: (
    request?: GetExecutionSettingsRequest,
  ) => Promise<GetExecutionSettingsResponse>
  listActivity: (request: ListActivityRequest) => Promise<ListActivityResponse>
  listArtifacts: (request: ListArtifactsRequest) => Promise<ListArtifactsResponse>
  getArtifactMediaPreview: (
    request: GetArtifactMediaPreviewRequest,
  ) => Promise<GetArtifactMediaPreviewResponse>
  getAttachmentMediaPreview: (
    request: GetAttachmentMediaPreviewRequest,
  ) => Promise<GetAttachmentMediaPreviewResponse>
  listAutomationRuns: (automationId?: string) => Promise<ListAutomationRunsResponse>
  listAutomations: () => Promise<ListAutomationsResponse>
  listConversations: () => Promise<ListConversationsResponse>
  listEvalCases: () => Promise<ListEvalCasesResponse>
  listBrowserMcpPresets: () => Promise<ListBrowserMcpPresetsResponse>
  listModelProviderCatalog: () => Promise<ModelProviderCatalogResponse>
  listMcpDiagnostics: (serverId?: string) => Promise<ListMcpDiagnosticsResponse>
  listMcpServers: () => Promise<ListMcpServersResponse>
  listMemoryItems: () => Promise<ListMemoryItemsResponse>
  listPlugins: () => Promise<ListPluginsResponse>
  listProviderSettings: () => Promise<ListProviderSettingsResponse>
  listProviderCapabilityRoutes: () => Promise<ListProviderCapabilityRoutesResponse>
  listProviderCapabilityRouteOptions: () => Promise<ListProviderCapabilityRouteOptionsResponse>
  listProviderProbeSnapshots: () => Promise<ListProviderProbeSnapshotsResponse>
  listProjects: () => Promise<ListProjectsResponse>
  addProject: (path: string) => Promise<SwitchProjectResponse>
  switchProject: (path: string) => Promise<SwitchProjectResponse>
  deleteProject: (path: string) => Promise<DeleteProjectResponse>
  pageConversationTimeline: (
    request: PageConversationTimelineRequest,
  ) => Promise<PageConversationTimelineResponse>
  pageConversationWorktree: (
    request: PageConversationWorktreeRequest,
  ) => Promise<PageConversationWorktreeResponse>
  probeProviderConfig: (request: ProbeProviderConfigRequest) => Promise<ProbeProviderConfigResponse>
  listReferenceCandidates: (
    request: ListReferenceCandidatesRequest,
  ) => Promise<ListReferenceCandidatesResponse>
  listSkillCatalogEntries: (
    request: ListSkillCatalogEntriesRequest,
  ) => Promise<ListSkillCatalogEntriesResponse>
  listSkillCatalogSources: () => Promise<ListSkillCatalogSourcesResponse>
  listSkills: () => Promise<ListSkillsResponse>
  resolvePermission: (request: ResolvePermissionRequest) => Promise<ResolvePermissionResponse>
  reloadPlugin: (pluginId: string) => Promise<PluginOperationResult>
  requestProviderConfigApiKeyReveal: (
    configId: string,
  ) => Promise<RequestProviderConfigApiKeyRevealResponse>
  runAutomationNow: (id: string) => Promise<RunAutomationNowResponse>
  runEvalCase: (caseId: string) => Promise<RunEvalCaseResponse>
  saveAutomation: (request: SaveAutomationRequest) => Promise<SaveAutomationResponse>
  saveBrowserMcpPreset: (
    request: SaveBrowserMcpPresetRequest,
  ) => Promise<SaveBrowserMcpPresetResponse>
  saveMcpServer: (request: SaveMcpServerRequest) => Promise<SaveMcpServerResponse>
  setMcpServerEnabled: (id: string, enabled: boolean) => Promise<SetMcpServerEnabledResponse>
  setPluginEnabled: (pluginId: string, enabled: boolean) => Promise<PluginOperationResult>
  setProjectPluginsEnabled: (enabled: boolean) => Promise<SetProjectPluginsEnabledResponse>
  restartMcpServer: (id: string) => Promise<RestartMcpServerResponse>
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
  setAutomationEnabled: (id: string, enabled: boolean) => Promise<SetAutomationEnabledResponse>
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
  subscribeMcpDiagnostics: (
    request: SubscribeMcpDiagnosticsRequest,
  ) => Promise<SubscribeMcpDiagnosticsResponse>
  listenMcpDiagnosticBatches: (
    onBatch: (batch: McpDiagnosticBatchPayload) => void,
  ) => Promise<() => void>
  unsubscribeMcpDiagnostics: (subscriptionId: string) => Promise<UnsubscribeMcpDiagnosticsResponse>
  unsubscribeConversationEvents: (
    subscriptionId: string,
  ) => Promise<UnsubscribeConversationEventsResponse>
  updateMemoryItem: (request: UpdateMemoryItemRequest) => Promise<UpdateMemoryItemResponse>
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
    async deleteAutomation(id) {
      const command = 'delete_automation'
      const args = parseArgs(command, deleteAutomationRequestSchema, { id })
      return parsePayload(command, deleteAutomationResponseSchema, await invoke(command, args))
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
    async pageConversationTimeline(request) {
      const command = 'page_conversation_timeline'
      const args = parseArgs(command, pageConversationTimelineRequestSchema, request)
      return parsePayload(
        command,
        pageConversationTimelineResponseSchema,
        await invoke(command, args),
      )
    },
    async pageConversationWorktree(request) {
      const command = 'page_conversation_worktree'
      const args = parseArgs(command, pageConversationWorktreeRequestSchema, request)
      return parsePayload(
        command,
        pageConversationWorktreeResponseSchema,
        await invoke(command, args),
      )
    },
    async probeProviderConfig(request) {
      const command = 'probe_provider_config'
      const args = parseArgs(command, probeProviderConfigRequestSchema, request)
      return parsePayload(command, probeProviderConfigResponseSchema, await invoke(command, args))
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
    async listenSkillCatalogInstallProgress(onProgress) {
      const unlisten = await tauriListen<unknown>('skill_catalog_install_progress', (event) => {
        onProgress(
          parsePayload(
            'skill_catalog_install_progress',
            skillCatalogInstallProgressPayloadSchema,
            event.payload,
          ),
        )
      })

      return unlisten
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
    async getArtifactMediaPreview(request) {
      const command = 'get_artifact_media_preview'
      const args = parseArgs(command, getArtifactMediaPreviewRequestSchema, request)
      return parsePayload(
        command,
        getArtifactMediaPreviewResponseSchema,
        await invoke(command, args),
      )
    },
    async getAttachmentMediaPreview(request) {
      const command = 'get_attachment_media_preview'
      const args = parseArgs(command, getAttachmentMediaPreviewRequestSchema, request)
      return parsePayload(
        command,
        getAttachmentMediaPreviewResponseSchema,
        await invoke(command, args),
      )
    },
    async listConversations() {
      const command = 'list_conversations'
      return parsePayload(command, listConversationsResponseSchema, await invoke(command))
    },
    async listAutomations() {
      const command = 'list_automations'
      return parsePayload(command, listAutomationsResponseSchema, await invoke(command))
    },
    async listAutomationRuns(automationId) {
      const command = 'list_automation_runs'
      const args = parseArgs(command, listAutomationRunsRequestSchema, { automationId })
      return parsePayload(command, listAutomationRunsResponseSchema, await invoke(command, args))
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
      const args = parseArgs(command, listMcpDiagnosticsRequestSchema, { serverId })
      return parsePayload(command, listMcpDiagnosticsResponseSchema, await invoke(command, args))
    },
    async listMcpServers() {
      const command = 'list_mcp_servers'
      return parsePayload(command, listMcpServersResponseSchema, await invoke(command))
    },
    async getMcpServerConfig(id) {
      const command = 'get_mcp_server_config'
      const args = parseArgs(command, getMcpServerConfigRequestSchema, { id })
      return parsePayload(command, getMcpServerConfigResponseSchema, await invoke(command, args))
    },
    async getPluginDetail(pluginId) {
      const command = 'get_plugin_detail'
      const args = parseArgs(command, getPluginDetailRequestSchema, { pluginId })
      return parsePayload(command, getPluginDetailResponseSchema, await invoke(command, args))
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
    async listProviderSettings() {
      const command = 'list_provider_settings'
      return parsePayload(command, listProviderSettingsResponseSchema, await invoke(command))
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
    async deleteProject(path) {
      const command = 'delete_project'
      const args = parseArgs(command, deleteProjectRequestSchema, { path })
      return parsePayload(command, deleteProjectResponseSchema, await invoke(command, args))
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
    async runEvalCase(caseId) {
      const command = 'run_eval_case'
      const args = parseArgs(command, runEvalCaseRequestSchema, { caseId })
      return parsePayload(command, runEvalCaseResponseSchema, await invoke(command, args))
    },
    async runAutomationNow(id) {
      const command = 'run_automation_now'
      const args = parseArgs(command, runAutomationNowRequestSchema, { id })
      return parsePayload(command, runAutomationNowResponseSchema, await invoke(command, args))
    },
    async saveAutomation(request) {
      const command = 'save_automation'
      const args = parseArgs(command, saveAutomationRequestSchema, request)
      return parsePayload(command, saveAutomationResponseSchema, await invoke(command, args))
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
    async setAutomationEnabled(id, enabled) {
      const command = 'set_automation_enabled'
      const args = parseArgs(command, setAutomationEnabledRequestSchema, { enabled, id })
      return parsePayload(command, setAutomationEnabledResponseSchema, await invoke(command, args))
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
    async setMcpServerEnabled(id, enabled) {
      const command = 'set_mcp_server_enabled'
      const args = parseArgs(command, setMcpServerEnabledRequestSchema, { enabled, id })
      return parsePayload(command, setMcpServerEnabledResponseSchema, await invoke(command, args))
    },
    async setPluginEnabled(pluginId, enabled) {
      const command = 'set_plugin_enabled'
      const args = parseArgs(command, setPluginEnabledRequestSchema, { enabled, pluginId })
      return parsePayload(command, pluginOperationResultSchema, await invoke(command, args))
    },
    async setProjectPluginsEnabled(enabled) {
      const command = 'set_project_plugins_enabled'
      const args = parseArgs(command, setProjectPluginsEnabledRequestSchema, { enabled })
      return parsePayload(
        command,
        setProjectPluginsEnabledResponseSchema,
        await invoke(command, args),
      )
    },
    async restartMcpServer(id) {
      const command = 'restart_mcp_server'
      const args = parseArgs(command, restartMcpServerRequestSchema, { id })
      return parsePayload(command, restartMcpServerResponseSchema, await invoke(command, args))
    },
    async clearMcpDiagnostics(serverId) {
      const command = 'clear_mcp_diagnostics'
      const args = parseArgs(command, clearMcpDiagnosticsRequestSchema, { serverId })
      return parsePayload(command, clearMcpDiagnosticsResponseSchema, await invoke(command, args))
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
    async installPluginFromPath(sourcePath) {
      const command = 'install_plugin_from_path'
      const args = parseArgs(command, pluginPathRequestSchema, { sourcePath })
      return parsePayload(command, pluginOperationResultSchema, await invoke(command, args))
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
    async updatePluginConfig(pluginId, values) {
      const command = 'update_plugin_config'
      const args = parseArgs(command, updatePluginConfigRequestSchema, { pluginId, values })
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

export function getArtifactMediaPreview(
  request: GetArtifactMediaPreviewRequest,
  client: CommandClient = tauriCommandClient,
): Promise<GetArtifactMediaPreviewResponse> {
  return client.getArtifactMediaPreview(request)
}

export function getAttachmentMediaPreview(
  request: GetAttachmentMediaPreviewRequest,
  client: CommandClient = tauriCommandClient,
): Promise<GetAttachmentMediaPreviewResponse> {
  return client.getAttachmentMediaPreview(request)
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

export function listAutomations(
  client: CommandClient = tauriCommandClient,
): Promise<ListAutomationsResponse> {
  return client.listAutomations()
}

export function saveAutomation(
  request: SaveAutomationRequest,
  client: CommandClient = tauriCommandClient,
): Promise<SaveAutomationResponse> {
  return client.saveAutomation(request)
}

export function deleteAutomation(
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<DeleteAutomationResponse> {
  return client.deleteAutomation(id)
}

export function setAutomationEnabled(
  id: string,
  enabled: boolean,
  client: CommandClient = tauriCommandClient,
): Promise<SetAutomationEnabledResponse> {
  return client.setAutomationEnabled(id, enabled)
}

export function runAutomationNow(
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<RunAutomationNowResponse> {
  return client.runAutomationNow(id)
}

export function listAutomationRuns(
  automationId?: string,
  client: CommandClient = tauriCommandClient,
): Promise<ListAutomationRunsResponse> {
  return client.listAutomationRuns(automationId)
}

export function listMcpServers(
  client: CommandClient = tauriCommandClient,
): Promise<ListMcpServersResponse> {
  return client.listMcpServers()
}

export function listBrowserMcpPresets(
  client: CommandClient = tauriCommandClient,
): Promise<ListBrowserMcpPresetsResponse> {
  return client.listBrowserMcpPresets()
}

export function getMcpServerConfig(
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<GetMcpServerConfigResponse> {
  return client.getMcpServerConfig(id)
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
  id: string,
  enabled: boolean,
  client: CommandClient = tauriCommandClient,
): Promise<SetMcpServerEnabledResponse> {
  return client.setMcpServerEnabled(id, enabled)
}

export function restartMcpServer(
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<RestartMcpServerResponse> {
  return client.restartMcpServer(id)
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
  id: string,
  client: CommandClient = tauriCommandClient,
): Promise<DeleteMcpServerResponse> {
  return client.deleteMcpServer(id)
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
): Promise<() => void> {
  return client.listenSkillCatalogInstallProgress(onProgress)
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

export function pageConversationWorktree(
  request: PageConversationWorktreeRequest,
  client: CommandClient = tauriCommandClient,
): Promise<PageConversationWorktreeResponse> {
  return client.pageConversationWorktree(request)
}

export function getContextSnapshot(
  request: GetContextSnapshotRequest,
  client: CommandClient = tauriCommandClient,
): Promise<GetContextSnapshotResponse> {
  return client.getContextSnapshot(request)
}
