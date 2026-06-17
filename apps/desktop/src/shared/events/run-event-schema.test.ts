import { describe, expect, it } from 'vitest'

import {
  getRunEventLabel,
  type RunEvent,
  type RunEventSource,
  type RunEventType,
  type RunEventVisibility,
  runEventFixtures,
  runEventSchema,
  runEventSourceSchema,
  runEventsSchema,
  runEventVisibilitySchema,
} from './run-event-schema'

describe('RunEvent schema', () => {
  it('exports the core event type enums', () => {
    const eventType: RunEventType = 'run.started'
    const source: RunEventSource = runEventSourceSchema.parse('engine')
    const visibility: RunEventVisibility = runEventVisibilitySchema.parse('public')

    expect(eventType).toBe('run.started')
    expect(source).toBe('engine')
    expect(visibility).toBe('public')
  })

  it('keeps event source values aligned with the frontend specification', () => {
    expect(runEventSourceSchema.options).toEqual(['user', 'assistant', 'tool', 'engine', 'policy'])
    expect(() => runEventSourceSchema.parse('permission')).toThrow()
    expect(() => runEventSourceSchema.parse('system')).toThrow()
  })

  it('parses all MVP event fixtures', () => {
    const events = runEventsSchema.parse(runEventFixtures)

    expect(events.map((event) => event.type)).toEqual([
      'run.started',
      'run.ended',
      'assistant.delta',
      'assistant.completed',
      'tool.requested',
      'tool.approved',
      'tool.denied',
      'tool.completed',
      'tool.failed',
      'permission.requested',
      'permission.resolved',
      'engine.failed',
    ])
  })

  it.each([
    'runId',
    'sequence',
    'timestamp',
    'type',
  ] as const)('rejects events missing %s', (field) => {
    const event = { ...runEventFixtures[0] }
    delete event[field]

    expect(() => runEventSchema.parse(event)).toThrow()
  })

  it('rejects duplicate or decreasing sequence values inside the same run', () => {
    expect(() =>
      runEventsSchema.parse([
        { ...runEventFixtures[0], sequence: 2 },
        { ...runEventFixtures[1], sequence: 2 },
      ]),
    ).toThrow()

    expect(() =>
      runEventsSchema.parse([
        { ...runEventFixtures[0], sequence: 2 },
        { ...runEventFixtures[1], sequence: 1 },
      ]),
    ).toThrow()
  })

  it('allows independent monotonic sequences for separate runs', () => {
    const events = runEventsSchema.parse([
      { ...runEventFixtures[0], runId: 'run-a', sequence: 1 },
      { ...runEventFixtures[1], runId: 'run-b', sequence: 1 },
      { ...runEventFixtures[2], runId: 'run-a', sequence: 2 },
    ])

    expect(events).toHaveLength(3)
  })

  it('keeps visibility explicit for redaction decisions', () => {
    const event: RunEvent = runEventSchema.parse({
      ...runEventFixtures[0],
      visibility: 'redacted',
    })

    expect(event.visibility).toBe('redacted')
    expect(getRunEventLabel(event)).toBe('Run started')
  })
})
