import { describe, expect, it } from 'vitest'

import { type RunEvent, runEventSchema, runEventsSchema } from '@/shared/events/run-event-schema'
import { runEventFixtures } from '@/testing/run-event-fixtures'

import { toRunEventViewModel, toRunEventViewModels } from './run-event-view-model'

describe('run-event-view-model', () => {
  it('maps parsed events to activity view models and preserves ordering fields', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[4],
      payload: {
        argumentsSummary: 'Input withheld from conversation timeline.',
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
        outputSummary: 'Output withheld from conversation timeline.',
        toolUseId: 'tool-001',
      },
      visibility: 'redacted',
    })

    expect(toRunEventViewModel(event).rawJson).toEqual({
      payload: {
        durationMs: 128,
        outputSummary: 'Output withheld from conversation timeline.',
        toolUseId: 'tool-001',
      },
    })
  })

  it('maps permission request events to pending review details', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[9],
      payload: {
        actorSource: { type: 'parentRun' },
        decisionScope: 'current run',
        exposure: 'Can modify package metadata and lockfile.',
        operation: 'Install dependencies',
        reason: 'The run requested package installation.',
        requestId: '01HZ0000000000000000000001',
        severity: 'critical',
        target: 'workspace package manager',
        toolUseId: 'tool-001',
        workspaceBoundary: 'workspace://local',
      },
    })

    expect(toRunEventViewModel(event).details?.permissions).toEqual([
      {
        decisionScope: 'current run',
        exposure: 'Can modify package metadata and lockfile.',
        id: '01HZ0000000000000000000001',
        label: 'Install dependencies',
        operation: 'Install dependencies',
        reason: 'The run requested package installation.',
        risk: 'critical',
        state: 'pending',
        target: 'workspace package manager',
        toolUseId: 'tool-001',
        workspaceBoundary: 'workspace://local',
      },
    ])
  })

  it('maps auto-resolved permission request events to approved activity details', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[9],
      payload: {
        actorSource: { type: 'parentRun' },
        autoResolved: true,
        decisionScope: 'current run',
        exposure: 'Can inspect workspace metadata.',
        operation: 'Read workspace metadata',
        reason: 'Automatically allowed by the run permission mode.',
        requestId: '01HZ0000000000000000000001',
        severity: 'medium',
        target: 'workspace metadata',
        toolUseId: 'tool-001',
        workspaceBoundary: 'workspace://local',
      },
    })
    const viewModel = toRunEventViewModel(event)

    expect(viewModel.activityItem.status).toBe('success')
    expect(viewModel.details?.permissions?.[0]).toMatchObject({
      id: '01HZ0000000000000000000001',
      state: 'approved',
    })
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
        label: 'permission',
        risk: 'medium',
        state: 'denied',
      },
    ])
  })

  it('maps plugin lifecycle events to activity labels and statuses', () => {
    const events = runEventsSchema.parse([
      {
        id: 'evt-plugin-loaded',
        conversationSequence: 18,
        runId: 'plugin-runtime',
        sequence: 1,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'plugin.loaded',
        source: 'plugin',
        visibility: 'redacted',
        payload: {
          capabilityCount: 2,
          pluginId: 'formatter@1.0.0',
          pluginName: 'formatter',
          trustLevel: 'user_controlled',
        },
      },
      {
        id: 'evt-plugin-rejected',
        conversationSequence: 19,
        runId: 'plugin-runtime',
        sequence: 2,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'plugin.rejected',
        source: 'plugin',
        visibility: 'redacted',
        payload: {
          pluginId: 'bad-plugin@1.0.0',
          pluginName: 'bad-plugin',
          reason: 'ManifestInvalid',
          trustLevel: 'user_controlled',
        },
      },
      {
        id: 'evt-plugin-failed',
        conversationSequence: 20,
        runId: 'plugin-runtime',
        sequence: 3,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'plugin.failed',
        source: 'plugin',
        visibility: 'redacted',
        payload: {
          message: 'Plugin failure withheld from conversation timeline.',
          pluginId: 'crash-plugin@1.0.0',
          pluginName: 'crash-plugin',
          trustLevel: 'admin_trusted',
        },
      },
    ])

    expect(toRunEventViewModels(events).map((viewModel) => viewModel.activityItem)).toMatchObject([
      { label: 'formatter', status: 'success' },
      { label: 'bad-plugin', status: 'failed' },
      { label: 'crash-plugin', status: 'failed' },
    ])
  })

  it('maps background lifecycle and permission events', () => {
    const events = runEventsSchema.parse([
      {
        id: 'evt-background-started',
        conversationSequence: 21,
        runId: 'run-background',
        sequence: 1,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'background.started',
        source: 'background',
        visibility: 'public',
        payload: {
          backgroundAgentId: 'bg-agent-001',
          title: 'Background run',
        },
      },
      {
        id: 'evt-background-permission-requested',
        conversationSequence: 22,
        runId: 'run-background',
        sequence: 2,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'background.permission.requested',
        source: 'policy',
        visibility: 'public',
        payload: {
          backgroundAgentId: 'bg-agent-001',
          reason: 'Permission required',
          requestId: '01HZ0000000000000000000003',
        },
      },
      {
        id: 'evt-background-permission-resolved',
        conversationSequence: 23,
        runId: 'run-background',
        sequence: 3,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'background.permission.resolved',
        source: 'policy',
        visibility: 'public',
        payload: {
          backgroundAgentId: 'bg-agent-001',
          decision: 'approve',
          requestId: '01HZ0000000000000000000003',
        },
      },
    ])
    const viewModels = toRunEventViewModels(events)

    expect(viewModels.map((viewModel) => viewModel.activityItem)).toMatchObject([
      { label: 'Background run', status: 'running' },
      { label: '01HZ0000000000000000000003', status: 'blocked' },
      { label: '01HZ0000000000000000000003', status: 'success' },
    ])
    expect(viewModels[1]?.details?.permissions?.[0]).toMatchObject({
      id: '01HZ0000000000000000000003',
      label: 'background permission',
      state: 'approved',
    })
    expect(viewModels[2]?.details).toBeUndefined()
  })

  it('merges permission resolved events into the matching requested details', () => {
    const events = runEventsSchema.parse([
      {
        ...runEventFixtures[9],
        sequence: 1,
        payload: {
          actorSource: { type: 'parentRun' },
          decisionScope: 'current run',
          exposure: 'Can modify package metadata and lockfile.',
          operation: 'Install dependencies',
          reason: 'The run requested package installation.',
          requestId: '01HZ0000000000000000000001',
          severity: 'high',
          target: 'workspace package manager',
          toolUseId: 'tool-001',
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
      label: 'permission',
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
