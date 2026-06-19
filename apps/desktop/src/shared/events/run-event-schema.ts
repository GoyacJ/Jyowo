import { z } from 'zod'

import { assertNever } from './assert-never'

export const runEventVisibilitySchema = z.enum(['public', 'redacted', 'withheld'])
export const runEventSourceSchema = z.enum(['user', 'assistant', 'tool', 'engine', 'policy'])
export const runEventContractTypeSchema = z.enum([
  'run_started',
  'run_ended',
  'assistant_delta_produced',
  'assistant_message_completed',
  'tool_use_requested',
  'tool_use_approved',
  'tool_use_denied',
  'tool_use_completed',
  'tool_use_failed',
  'permission_requested',
  'permission_resolved',
  'engine_failed',
])

const payloadSchema = z.record(z.string(), z.unknown())
const unredactedSecretPatterns = [
  /\bAuthorization:?\s*Bearer\s+\S+/i,
  /\b(?:api[_-]?key|token|secret|password)\b\s*(?:=|\s+)\s*\S+/i,
  /\b--(?:api-key|token|secret|password)\b\s+\S+/i,
  /\b[A-Za-z0-9_]*(?:API_KEY|TOKEN|SECRET|PASSWORD|ACCESS_KEY)[A-Za-z0-9_]*\s*=\s*\S+/i,
  /\b[A-Za-z0-9_]*(?:API_KEY|TOKEN|SECRET|PASSWORD|ACCESS_KEY)[A-Za-z0-9_]*\s+\S+/i,
  /\bsk-[A-Za-z0-9]{12,}/i,
  /\bgh[pousr]_[A-Za-z0-9_]{20,}/i,
]

function hasObviousUnredactedSecret(value: string): boolean {
  return unredactedSecretPatterns.some((pattern) => pattern.test(value))
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

const permissionDisplayTextSchema = z
  .string()
  .trim()
  .min(1)
  .refine((value) => !hasObviousUnredactedSecret(value), {
    message: 'permission review payload must not contain obvious unredacted secrets',
  })
const requestIdSchema = z.string().regex(/^[0-9A-HJKMNP-TV-Z]{26}$/, {
  message: 'requestId must be a canonical ULID',
})
const permissionArgsSchema = z.array(permissionDisplayTextSchema).superRefine((argv, context) => {
  if (!hasObviousUnredactedSecret(argv.join(' '))) {
    return
  }

  context.addIssue({
    code: 'custom',
    message: 'permission command args must not contain obvious unredacted secrets',
  })
})
const runStartedPayloadSchema = z
  .object({
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
const assistantDeltaPayloadSchema = z
  .object({
    text: z.string(),
  })
  .strict()
const assistantCompletedPayloadSchema = z
  .object({
    messageId: z.string().min(1),
  })
  .strict()
const toolRequestedPayloadSchema = z
  .object({
    argumentsSummary: z.string().optional(),
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
    durationMs: z.number().int().nonnegative().optional(),
    outputSummary: z.string().optional(),
    toolUseId: z.string().min(1),
  })
  .strict()
const toolFailedPayloadSchema = z
  .object({
    code: z.string().min(1),
    message: z.string().optional(),
    toolUseId: z.string().min(1),
  })
  .strict()
const permissionCommandSchema = z
  .object({
    argv: permissionArgsSchema.optional(),
    cwd: permissionDisplayTextSchema.optional(),
    executable: permissionDisplayTextSchema,
  })
  .strict()
const permissionRequestedPayloadSchema = z
  .object({
    command: permissionCommandSchema.optional(),
    decisionScope: permissionDisplayTextSchema,
    diffSummary: permissionDisplayTextSchema.optional(),
    exposure: permissionDisplayTextSchema,
    operation: permissionDisplayTextSchema,
    reason: permissionDisplayTextSchema,
    requestId: requestIdSchema,
    severity: z.enum(['low', 'medium', 'high', 'critical']),
    target: permissionDisplayTextSchema,
    workspaceBoundary: permissionDisplayTextSchema,
  })
  .strict()
const permissionResolvedPayloadSchema = z
  .object({
    decision: z.enum(['approve', 'deny']),
    requestId: requestIdSchema,
  })
  .strict()
const engineFailedPayloadSchema = z
  .object({
    message: z.string().min(1),
  })
  .strict()

const baseRunEventSchema = z
  .object({
    id: z.string().min(1),
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

export const runEventSchema = z
  .discriminatedUnion('type', [
    eventSchema('run.started', runStartedPayloadSchema),
    eventSchema('run.ended', runEndedPayloadSchema),
    eventSchema('assistant.delta', assistantDeltaPayloadSchema),
    eventSchema('assistant.completed', assistantCompletedPayloadSchema),
    eventSchema('tool.requested', toolRequestedPayloadSchema),
    eventSchema('tool.approved', toolResolvedPayloadSchema),
    eventSchema('tool.denied', toolResolvedPayloadSchema),
    eventSchema('tool.completed', toolCompletedPayloadSchema),
    eventSchema('tool.failed', toolFailedPayloadSchema),
    eventSchema('permission.requested', permissionRequestedPayloadSchema),
    eventSchema('permission.resolved', permissionResolvedPayloadSchema),
    eventSchema('engine.failed', engineFailedPayloadSchema),
  ])
  .superRefine((event, context) => {
    if (
      (event.type === 'permission.requested' || event.type === 'permission.resolved') &&
      event.source !== 'policy'
    ) {
      context.addIssue({
        code: 'custom',
        message: 'permission events must be emitted by policy',
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

  events.forEach((event, index) => {
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
    case 'engine_failed':
      return 'engine.failed'
    default:
      return assertNever(contractType)
  }
}

const timestamp = '2026-06-17T00:00:00.000Z'

export const runEventFixtures: Array<Record<string, unknown>> = [
  {
    id: 'evt-001',
    runId: 'run-001',
    sequence: 1,
    timestamp,
    type: 'run.started',
    source: 'engine',
    visibility: 'public',
    payload: { sessionId: 'session-001' },
  },
  {
    id: 'evt-002',
    runId: 'run-001',
    sequence: 2,
    timestamp,
    type: 'run.ended',
    source: 'engine',
    visibility: 'public',
    payload: { reason: 'completed' },
  },
  {
    id: 'evt-003',
    runId: 'run-001',
    sequence: 3,
    timestamp,
    type: 'assistant.delta',
    source: 'assistant',
    visibility: 'public',
    payload: { text: 'Hello' },
  },
  {
    id: 'evt-004',
    runId: 'run-001',
    sequence: 4,
    timestamp,
    type: 'assistant.completed',
    source: 'assistant',
    visibility: 'public',
    payload: { messageId: 'msg-001' },
  },
  {
    id: 'evt-005',
    runId: 'run-001',
    sequence: 5,
    timestamp,
    type: 'tool.requested',
    source: 'tool',
    visibility: 'redacted',
    payload: { toolUseId: 'tool-001', toolName: 'read' },
  },
  {
    id: 'evt-006',
    runId: 'run-001',
    sequence: 6,
    timestamp,
    type: 'tool.approved',
    source: 'tool',
    visibility: 'public',
    payload: { toolUseId: 'tool-001' },
  },
  {
    id: 'evt-007',
    runId: 'run-001',
    sequence: 7,
    timestamp,
    type: 'tool.denied',
    source: 'tool',
    visibility: 'public',
    payload: { toolUseId: 'tool-002' },
  },
  {
    id: 'evt-008',
    runId: 'run-001',
    sequence: 8,
    timestamp,
    type: 'tool.completed',
    source: 'tool',
    visibility: 'redacted',
    payload: { toolUseId: 'tool-001', durationMs: 42 },
  },
  {
    id: 'evt-009',
    runId: 'run-001',
    sequence: 9,
    timestamp,
    type: 'tool.failed',
    source: 'tool',
    visibility: 'public',
    payload: { toolUseId: 'tool-003', code: 'failed' },
  },
  {
    id: 'evt-010',
    runId: 'run-001',
    sequence: 10,
    timestamp,
    type: 'permission.requested',
    source: 'policy',
    visibility: 'public',
    payload: {
      decisionScope: 'current run',
      exposure: 'Can run inside the local workspace boundary.',
      operation: 'Review permission',
      reason: 'The runtime requires a human permission decision.',
      requestId: '01HZ0000000000000000000001',
      severity: 'medium',
      target: 'local workspace',
      workspaceBoundary: 'workspace://local',
    },
  },
  {
    id: 'evt-011',
    runId: 'run-001',
    sequence: 11,
    timestamp,
    type: 'permission.resolved',
    source: 'policy',
    visibility: 'public',
    payload: { requestId: '01HZ0000000000000000000001', decision: 'approve' },
  },
  {
    id: 'evt-012',
    runId: 'run-001',
    sequence: 12,
    timestamp,
    type: 'engine.failed',
    source: 'engine',
    visibility: 'public',
    payload: { message: 'model stream failed' },
  },
]

export function getRunEventLabel(event: RunEvent): string {
  switch (event.type) {
    case 'run.started':
      return 'Run started'
    case 'run.ended':
      return 'Run ended'
    case 'assistant.delta':
      return 'Assistant delta'
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
    case 'engine.failed':
      return 'Engine failed'
    default:
      return assertNever(event)
  }
}
