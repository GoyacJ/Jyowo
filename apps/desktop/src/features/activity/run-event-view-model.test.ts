import { describe, expect, it } from 'vitest'

import {
  type RunEvent,
  runEventFixtures,
  runEventSchema,
  runEventsSchema,
} from '@/shared/events/run-event-schema'

import { toRunEventViewModel, toRunEventViewModels } from './run-event-view-model'

describe('run-event-view-model', () => {
  it('maps parsed events to activity view models and preserves ordering fields', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[4],
      payload: {
        argumentsSummary: 'Read package metadata',
        toolName: 'read_file',
        toolUseId: 'tool-001',
      },
    })

    expect(toRunEventViewModel(event)).toMatchObject({
      activityItem: {
        id: 'evt-005',
        label: 'read_file',
        status: 'queued',
        time: '2026-06-17T00:00:00.000Z',
      },
      order: {
        runId: 'run-001',
        sequence: 5,
        timestamp: '2026-06-17T00:00:00.000Z',
      },
    })
  })

  it('does not expose unparsed or withheld payloads to Raw JSON', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[4],
      visibility: 'withheld',
      payload: undefined,
    })

    expect(toRunEventViewModel(event).rawJson).toEqual({
      payload: {},
      withheld: true,
    })
  })

  it('does not use withheld payload fields for activity labels', () => {
    const event = {
      ...runEventSchema.parse({
        ...runEventFixtures[4],
        visibility: 'withheld',
        payload: undefined,
      }),
      payload: {
        toolName: 'secret-tool',
        toolUseId: 'secret-tool-use',
      },
    } as unknown as RunEvent

    expect(toRunEventViewModel(event).activityItem.label).toBe('tool')
  })

  it('uses only parsed redacted payloads for Raw JSON details', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[7],
      payload: {
        durationMs: 128,
        outputSummary: 'Read 4 files',
        toolUseId: 'tool-001',
      },
      visibility: 'redacted',
    })

    expect(toRunEventViewModel(event).rawJson).toEqual({
      payload: {
        durationMs: 128,
        outputSummary: 'Read 4 files',
        toolUseId: 'tool-001',
      },
    })
  })

  it('maps permission request events to pending review details', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[9],
      payload: {
        command: {
          argv: ['pnpm', 'install'],
          cwd: 'workspace://local',
          executable: 'pnpm',
        },
        decisionScope: 'current run',
        exposure: 'Can modify package metadata and lockfile.',
        operation: 'Install dependencies',
        reason: 'The run requested package installation.',
        requestId: '01HZ0000000000000000000001',
        severity: 'critical',
        target: 'workspace package manager',
        workspaceBoundary: 'workspace://local',
      },
    })

    expect(toRunEventViewModel(event).details?.permissions).toEqual([
      {
        command: {
          args: ['install'],
          cwd: 'workspace://local',
          executable: 'pnpm',
          risk: 'critical',
        },
        decisionScope: 'current run',
        exposure: 'Can modify package metadata and lockfile.',
        id: '01HZ0000000000000000000001',
        label: 'Install dependencies',
        operation: 'Install dependencies',
        reason: 'The run requested package installation.',
        risk: 'critical',
        state: 'pending',
        target: 'workspace package manager',
        workspaceBoundary: 'workspace://local',
      },
    ])
  })

  it('maps permission resolved events to immutable decision details', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[10],
      payload: {
        decision: 'deny',
        requestId: '01HZ0000000000000000000001',
      },
    })

    expect(toRunEventViewModel(event).details?.permissions).toEqual([
      {
        id: '01HZ0000000000000000000001',
        label: 'Permission denied',
        risk: 'medium',
        state: 'denied',
      },
    ])
  })

  it('does not drop permission command args when argv omits the executable', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[9],
      payload: {
        command: {
          argv: ['install'],
          executable: 'pnpm',
        },
        decisionScope: 'current run',
        exposure: 'Can modify package metadata and lockfile.',
        operation: 'Install dependencies',
        reason: 'The run requested package installation.',
        requestId: '01HZ0000000000000000000001',
        severity: 'high',
        target: 'workspace package manager',
        workspaceBoundary: 'workspace://local',
      },
    })

    expect(toRunEventViewModel(event).details?.permissions?.[0]?.command?.args).toEqual(['install'])
  })

  it('merges permission resolved events into the matching requested details', () => {
    const events = runEventsSchema.parse([
      {
        ...runEventFixtures[9],
        sequence: 1,
        payload: {
          decisionScope: 'current run',
          exposure: 'Can modify package metadata and lockfile.',
          operation: 'Install dependencies',
          reason: 'The run requested package installation.',
          requestId: '01HZ0000000000000000000001',
          severity: 'high',
          target: 'workspace package manager',
          workspaceBoundary: 'workspace://local',
        },
      },
      {
        ...runEventFixtures[10],
        sequence: 2,
        payload: {
          decision: 'approve',
          requestId: '01HZ0000000000000000000001',
        },
      },
    ])
    const viewModels = toRunEventViewModels(events)

    expect(viewModels[0]?.details?.permissions?.[0]).toMatchObject({
      id: '01HZ0000000000000000000001',
      label: 'Permission approved',
      risk: 'high',
      state: 'approved',
    })
    expect(viewModels[1]?.details).toBeUndefined()
  })

  it('maps ordered parsed event lists without reordering them', () => {
    const events = runEventsSchema.parse(runEventFixtures.slice(0, 3))

    expect(toRunEventViewModels(events).map((event) => event.order.sequence)).toEqual([1, 2, 3])
  })

  it('fails closed when an unknown event type reaches the adapter', () => {
    const event = {
      ...runEventSchema.parse(runEventFixtures[0]),
      type: 'future.event',
    } as unknown as RunEvent

    expect(() => toRunEventViewModel(event)).toThrow('Unhandled value')
  })
})
