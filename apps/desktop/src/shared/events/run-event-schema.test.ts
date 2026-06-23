import { describe, expect, it } from 'vitest'

import {
  getRunEventLabel,
  mapRunEventContractType,
  type RunEvent,
  type RunEventContractType,
  type RunEventSource,
  type RunEventType,
  type RunEventVisibility,
  runEventContractTypeSchema,
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

  it('maps canonical Rust event tags to frontend render event types', () => {
    const mappings: Array<[RunEventContractType, RunEventType]> = [
      ['run_started', 'run.started'],
      ['run_ended', 'run.ended'],
      ['user_message_appended', 'user.message.appended'],
      ['assistant_delta_produced', 'assistant.delta'],
      ['assistant_message_completed', 'assistant.completed'],
      ['tool_use_requested', 'tool.requested'],
      ['tool_use_approved', 'tool.approved'],
      ['tool_use_denied', 'tool.denied'],
      ['tool_use_completed', 'tool.completed'],
      ['tool_use_failed', 'tool.failed'],
      ['permission_requested', 'permission.requested'],
      ['permission_resolved', 'permission.resolved'],
      ['artifact_created', 'artifact.created'],
      ['artifact_updated', 'artifact.updated'],
      ['engine_failed', 'engine.failed'],
    ]

    expect(runEventContractTypeSchema.options).toEqual(
      mappings.map(([contractType]) => contractType),
    )
    expect(mappings.map(([contractType]) => mapRunEventContractType(contractType))).toEqual(
      mappings.map(([, eventType]) => eventType),
    )
    expect(() => runEventContractTypeSchema.parse('tool_use_heartbeat')).toThrow()
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
      'artifact.created',
      'artifact.updated',
      'engine.failed',
    ])
  })

  it('accepts artifact lifecycle events without artifact content', () => {
    const event = runEventSchema.parse({
      id: 'evt-artifact-created',
      conversationSequence: 12,
      runId: 'run-001',
      sequence: 12,
      timestamp: '2026-06-17T00:00:00.000Z',
      type: 'artifact.created',
      source: 'engine',
      visibility: 'public',
      payload: {
        artifactId: 'artifact-001',
        status: 'ready',
      },
    })

    expect(event.payload).toEqual({
      artifactId: 'artifact-001',
      status: 'ready',
    })
  })

  it('rejects non-v4 client message ids on user message events', () => {
    expect(() =>
      runEventSchema.parse({
        id: 'evt-user-message',
        conversationSequence: 12,
        runId: 'run-001',
        sequence: 12,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'user.message.appended',
        source: 'user',
        visibility: 'public',
        payload: {
          body: 'Continue',
          clientMessageId: '00000000-0000-1000-8000-000000000001',
          messageId: 'message-001',
        },
      }),
    ).toThrow()
  })

  it('rejects raw permission command payloads at the renderer boundary', () => {
    expect(() =>
      runEventSchema.parse({
        id: 'evt-permission',
        conversationSequence: 12,
        runId: 'run-001',
        sequence: 12,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'permission.requested',
        source: 'policy',
        visibility: 'public',
        payload: {
          command: {
            executable: 'bash',
            argv: ['-lc', 'echo hello'],
            cwd: 'workspace',
          },
          decisionScope: 'current run',
          exposure: 'Can run a command.',
          operation: 'Run command',
          reason: 'The run requested a command.',
          requestId: '01HZ0000000000000000000001',
          severity: 'high',
          target: 'workspace command',
          workspaceBoundary: 'workspace://local',
        },
      }),
    ).toThrow()
  })

  it.each([
    'AKIAIOSFODNN7EXAMPLE',
    'AIzaSyD-123456789012345678901234567890123',
    'Basic dXNlcjpwYXNz',
    'eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.signature',
  ])('rejects backend-redactor secret pattern %s', (secret) => {
    expect(() =>
      runEventSchema.parse({
        id: `evt-secret-${secret.slice(0, 4)}`,
        conversationSequence: 12,
        runId: 'run-001',
        sequence: 12,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'engine.failed',
        source: 'engine',
        visibility: 'redacted',
        payload: {
          message: secret,
        },
      }),
    ).toThrow()
  })

  it('rejects backend-redactor slack token pattern', () => {
    const secret = ['xoxb', '123456789012', '123456789012', 'abcdefghijklmnopqrstuvwx'].join('-')

    expect(() =>
      runEventSchema.parse({
        id: 'evt-secret-slack',
        conversationSequence: 12,
        runId: 'run-001',
        sequence: 12,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'engine.failed',
        source: 'engine',
        visibility: 'redacted',
        payload: {
          message: secret,
        },
      }),
    ).toThrow()
  })

  it('rejects private paths adjacent to punctuation in event payloads', () => {
    expect(() =>
      runEventSchema.parse({
        id: 'evt-private-path',
        conversationSequence: 12,
        runId: 'run-001',
        sequence: 12,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'assistant.delta',
        source: 'assistant',
        visibility: 'public',
        payload: {
          text: 'error(path=/Users/alice/.ssh/config)',
        },
      }),
    ).toThrow()
  })

  it('accepts redacted-safe usage summaries on run ended events', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[1],
      payload: {
        reason: 'completed',
        usage: {
          cacheReadTokens: 3,
          cacheWriteTokens: 5,
          costMicros: 260,
          inputTokens: 11,
          outputTokens: 7,
          toolCalls: 2,
        },
      },
    })

    expect(event.payload).toMatchObject({
      usage: {
        inputTokens: 11,
        outputTokens: 7,
        toolCalls: 2,
      },
    })
  })

  it.each([
    'conversationSequence',
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

  it('rejects duplicate or decreasing conversation sequence values', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[0],
        conversationSequence: 0,
      }),
    ).toThrow()

    expect(() =>
      runEventsSchema.parse([
        { ...runEventFixtures[0], conversationSequence: 2 },
        { ...runEventFixtures[1], conversationSequence: 2 },
      ]),
    ).toThrow()

    expect(() =>
      runEventsSchema.parse([
        { ...runEventFixtures[0], conversationSequence: 2 },
        { ...runEventFixtures[1], conversationSequence: 1 },
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

  it('validates event-specific payload shapes', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[2],
        payload: { text: 42 },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[4],
        payload: { toolUseId: 'tool-001' },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: { requestId: '01HZ0000000000000000000001', severity: 'severe' },
      }),
    ).toThrow()
  })

  it('rejects free-form tool summaries and errors at the renderer boundary', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[4],
        payload: {
          argumentsSummary: 'read /Users/goya/.ssh/config',
          toolName: 'read_file',
          toolUseId: 'tool-001',
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[7],
        payload: {
          outputSummary: 'read 4 files',
          toolUseId: 'tool-001',
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[8],
        payload: {
          code: 'execution',
          message: 'permission denied',
          toolUseId: 'tool-001',
        },
      }),
    ).toThrow()
  })

  it('parses permission request details needed for human review', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[9],
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
    })

    expect(event.payload).toMatchObject({
      operation: 'Install dependencies',
      target: 'workspace package manager',
    })
  })

  it('rejects permission requests without minimum review context', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          requestId: '01HZ0000000000000000000001',
          severity: 'high',
        },
      }),
    ).toThrow()
  })

  it('rejects whitespace-only permission review context', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          ...(runEventFixtures[9].payload as Record<string, unknown>),
          reason: '   ',
        },
      }),
    ).toThrow()
  })

  it('rejects invalid request IDs and lowercase secret markers', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          ...(runEventFixtures[9].payload as Record<string, unknown>),
          requestId: 'perm-001',
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[10],
        payload: {
          ...(runEventFixtures[10].payload as Record<string, unknown>),
          requestId: 'ghp_abcdefghijklmnopqrstuvwxyz0123456789',
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          ...(runEventFixtures[9].payload as Record<string, unknown>),
          requestId: ' 01HZ0000000000000000000001 ',
        },
      }),
    ).toThrow()
  })

  it('rejects permission events not emitted by policy', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        source: 'tool',
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[10],
        source: 'assistant',
      }),
    ).toThrow()
  })

  it('rejects obvious unredacted secrets in permission review payloads', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          ...(runEventFixtures[9].payload as Record<string, unknown>),
          command: {
            argv: ['curl', '-H', 'Authorization: Bearer secret-token'],
            executable: 'curl',
          },
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          ...(runEventFixtures[9].payload as Record<string, unknown>),
          reason: 'Uses api_key=secret-token to call the service.',
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          ...(runEventFixtures[9].payload as Record<string, unknown>),
          command: {
            argv: ['env', 'AWS_SECRET_ACCESS_KEY', 'secret-token'],
            executable: 'env',
          },
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          ...(runEventFixtures[9].payload as Record<string, unknown>),
          command: {
            argv: ['curl', '-H', 'Authorization', 'Bearer', 'secret-token'],
            executable: 'curl',
          },
        },
      }),
    ).toThrow()
  })

  it('rejects obvious unredacted secrets split across permission command arguments', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          ...(runEventFixtures[9].payload as Record<string, unknown>),
          command: {
            argv: ['curl', '-H', 'Authorization:', 'Bearer', 'secret-token'],
            executable: 'curl',
          },
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          ...(runEventFixtures[9].payload as Record<string, unknown>),
          command: {
            argv: ['deploy', '--api-key', 'secret-token'],
            executable: 'deploy',
          },
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[9],
        payload: {
          ...(runEventFixtures[9].payload as Record<string, unknown>),
          command: {
            argv: [
              'git',
              'push',
              'https://ghp_abcdefghijklmnopqrstuvwxyz0123456789@github.com/org/repo',
            ],
            executable: 'git',
          },
        },
      }),
    ).toThrow()
  })

  it('rejects obvious unredacted secrets in any visible event payload', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[2],
        payload: { text: 'Do not render sk-secretkey1234567890' },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[7],
        payload: {
          outputSummary: 'pushed with https://ghp_abcdefghijklmnopqrstuvwxyz0123456789@github.com',
          toolUseId: 'tool-001',
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[11],
        payload: { message: 'request failed with api_key=secret-token' },
        visibility: 'redacted',
      }),
    ).toThrow()
  })

  it('rejects unknown top-level event fields', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[0],
        command: 'raw command should not cross the render boundary',
      }),
    ).toThrow()
  })

  it('rejects free-text summaries at the render event boundary', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[0],
        summary: 'raw command or secret text should not cross the render boundary',
      }),
    ).toThrow()
  })

  it('rejects unknown event-specific payload fields', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[4],
        payload: {
          env: { API_KEY: 'should-not-cross-boundary' },
          toolName: 'read_file',
          toolUseId: 'tool-001',
        },
      }),
    ).toThrow()
  })

  it('rejects public and redacted events without event-specific payloads', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[2],
        payload: undefined,
        visibility: 'public',
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[4],
        payload: undefined,
        visibility: 'redacted',
      }),
    ).toThrow()
  })

  it('allows withheld events without carrying a renderable payload', () => {
    const event = runEventSchema.parse({
      ...runEventFixtures[4],
      visibility: 'withheld',
      payload: undefined,
    })

    expect(event.visibility).toBe('withheld')
    expect(event.payload).toBeUndefined()
  })

  it('rejects withheld events that still carry payload fields', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[4],
        visibility: 'withheld',
        payload: {
          toolName: 'secret-tool',
          toolUseId: 'secret-tool-use',
        },
      }),
    ).toThrow()
  })
})
