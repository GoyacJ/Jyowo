import { z } from 'zod'

import { assertNever } from './assert-never'

export const runEventVisibilitySchema = z.enum(['public', 'redacted', 'withheld'])
export const runEventSourceSchema = z.enum(['user', 'assistant', 'tool', 'engine', 'policy'])

const payloadSchema = z.record(z.string(), z.unknown())

const baseRunEventSchema = z.object({
  id: z.string().min(1),
  runId: z.string().min(1),
  sequence: z.number().int().nonnegative(),
  timestamp: z.string().datetime({ offset: true }),
  source: runEventSourceSchema,
  visibility: runEventVisibilitySchema,
  summary: z.string().optional(),
  payload: payloadSchema.optional(),
})

function eventSchema<TType extends string>(type: TType) {
  return baseRunEventSchema.extend({
    type: z.literal(type),
  })
}

export const runEventSchema = z.discriminatedUnion('type', [
  eventSchema('run.started'),
  eventSchema('run.ended'),
  eventSchema('assistant.delta'),
  eventSchema('assistant.completed'),
  eventSchema('tool.requested'),
  eventSchema('tool.approved'),
  eventSchema('tool.denied'),
  eventSchema('tool.completed'),
  eventSchema('tool.failed'),
  eventSchema('permission.requested'),
  eventSchema('permission.resolved'),
  eventSchema('engine.failed'),
])

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
    payload: { requestId: 'perm-001', severity: 'medium' },
  },
  {
    id: 'evt-011',
    runId: 'run-001',
    sequence: 11,
    timestamp,
    type: 'permission.resolved',
    source: 'policy',
    visibility: 'public',
    payload: { requestId: 'perm-001', decision: 'approve' },
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
