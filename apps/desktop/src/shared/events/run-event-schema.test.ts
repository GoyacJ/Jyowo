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
    expect(runEventSourceSchema.options).toEqual([
      'user',
      'assistant',
      'tool',
      'engine',
      'policy',
      'plugin',
    ])
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
      ['assistant_review_requested', 'assistant.review.requested'],
      ['assistant_clarification_requested', 'assistant.clarification.requested'],
      ['assistant_notice', 'assistant.notice'],
      ['engine_failed', 'engine.failed'],
      ['plugin_loaded', 'plugin.loaded'],
      ['plugin_rejected', 'plugin.rejected'],
      ['plugin_failed', 'plugin.failed'],
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
      'assistant.review.requested',
      'assistant.clarification.requested',
      'assistant.notice',
      'engine.failed',
    ])
  })

  it('accepts redacted plugin lifecycle events without unsafe details', () => {
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
          pluginId: 'formatter@1.0.0',
          pluginName: 'formatter',
          reason: 'CapabilityDenied',
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
          pluginId: 'formatter@1.0.0',
          pluginName: 'formatter',
          trustLevel: 'user_controlled',
        },
      },
    ])

    expect(events.map((event) => event.type)).toEqual([
      'plugin.loaded',
      'plugin.rejected',
      'plugin.failed',
    ])
  })

  it('accepts safe assistant completed tool use metadata', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[3],
        payload: {
          body: 'Inspecting workspace files.',
          messageId: 'msg-001',
          toolUses: [
            {
              toolName: 'read_file',
              toolUseId: 'tool-001',
            },
          ],
        },
      }),
    ).not.toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[3],
        payload: {
          messageId: 'msg-001',
          toolUses: [],
        },
      }),
    ).not.toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[3],
        payload: {
          messageId: 'msg-001',
          toolUses: [
            {
              toolName: 'sk-secretkey1234567890',
              toolUseId: 'tool-001',
            },
          ],
        },
      }),
    ).toThrow()
  })

  it('accepts assistant review, clarification, and notice timeline events', () => {
    expect(
      runEventsSchema
        .parse([
          {
            id: 'evt-review',
            conversationSequence: 12,
            runId: 'run-001',
            sequence: 12,
            timestamp: '2026-06-17T00:00:00.000Z',
            type: 'assistant.review.requested',
            source: 'assistant',
            visibility: 'public',
            payload: {
              requestId: '01HZ0000000000000000000001',
              title: 'Review changes',
              body: 'Confirm before applying.',
            },
          },
          {
            id: 'evt-clarification',
            conversationSequence: 13,
            runId: 'run-001',
            sequence: 13,
            timestamp: '2026-06-17T00:00:00.000Z',
            type: 'assistant.clarification.requested',
            source: 'assistant',
            visibility: 'public',
            payload: {
              requestId: '01HZ0000000000000000000002',
              prompt: 'Which style should I use?',
            },
          },
          {
            id: 'evt-notice',
            conversationSequence: 14,
            runId: 'run-001',
            sequence: 14,
            timestamp: '2026-06-17T00:00:00.000Z',
            type: 'assistant.notice',
            source: 'assistant',
            visibility: 'public',
            payload: {
              noticeId: '01HZ0000000000000000000003',
              body: 'Tool output was summarized.',
              code: 'contextCompacted',
            },
          },
        ])
        .map((event) => event.type),
    ).toEqual([
      'assistant.review.requested',
      'assistant.clarification.requested',
      'assistant.notice',
    ])
  })

  it('accepts unknown assistant notice codes for forward-compatible normal rendering', () => {
    const event = runEventSchema.parse({
      id: 'evt-notice',
      conversationSequence: 14,
      runId: 'run-001',
      sequence: 14,
      timestamp: '2026-06-17T00:00:00.000Z',
      type: 'assistant.notice',
      source: 'assistant',
      visibility: 'public',
      payload: {
        noticeId: '01HZ0000000000000000000003',
        body: 'Tool output was summarized.',
        code: 'futureNoticeCode',
      },
    })

    expect(event.payload).toEqual({
      noticeId: '01HZ0000000000000000000003',
      body: 'Tool output was summarized.',
      code: 'futureNoticeCode',
    })
  })

  it('accepts assistant review requests without optional body text', () => {
    const event = runEventSchema.parse({
      id: 'evt-review',
      conversationSequence: 12,
      runId: 'run-001',
      sequence: 12,
      timestamp: '2026-06-17T00:00:00.000Z',
      type: 'assistant.review.requested',
      source: 'assistant',
      visibility: 'public',
      payload: {
        requestId: '01HZ0000000000000000000001',
        title: 'Review changes',
      },
    })

    expect(event.payload).toEqual({
      requestId: '01HZ0000000000000000000001',
      title: 'Review changes',
    })
  })

  it('rejects durable snake_case assistant segment event names at the raw event boundary', () => {
    expect(() =>
      runEventSchema.parse({
        id: 'evt-review',
        conversationSequence: 12,
        runId: 'run-001',
        sequence: 12,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'assistant_review_requested',
        source: 'assistant',
        visibility: 'public',
        payload: {
          requestId: '01HZ0000000000000000000001',
          title: 'Review changes',
        },
      }),
    ).toThrow()
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

  it('accepts artifact lifecycle display metadata but not artifact content references', () => {
    const event = runEventSchema.parse({
      id: 'evt-artifact-updated',
      conversationSequence: 13,
      runId: 'run-001',
      sequence: 13,
      timestamp: '2026-06-17T00:00:00.000Z',
      type: 'artifact.updated',
      source: 'engine',
      visibility: 'public',
      payload: {
        artifactId: 'artifact-001',
        status: 'ready',
        title: 'Generated image',
        summary: 'Image artifact ready',
        kind: 'image',
        source: 'tool',
        media: {
          kind: 'image',
          mimeType: 'image/png',
          sizeBytes: 128,
        },
      },
    })

    expect(event.payload).toEqual({
      artifactId: 'artifact-001',
      status: 'ready',
      title: 'Generated image',
      summary: 'Image artifact ready',
      kind: 'image',
      source: 'tool',
      media: {
        kind: 'image',
        mimeType: 'image/png',
        sizeBytes: 128,
      },
    })

    for (const unsafePayload of [
      {
        artifactId: 'artifact-001',
        blobRef: 'blob-001',
      },
      {
        artifactId: 'artifact-001',
        media: {
          kind: 'image',
          mimeType: 'image/png',
          sizeBytes: 128,
          url: 'https://asset.example/image.png',
        },
      },
      {
        artifactId: 'artifact-001',
        media: {
          kind: 'image',
          mimeType: 'image/png',
          sizeBytes: 128,
          path: '/Users/goya/.jyowo/runtime/blobs/private.png',
        },
      },
    ]) {
      expect(() =>
        runEventSchema.parse({
          id: 'evt-artifact-content',
          conversationSequence: 14,
          runId: 'run-001',
          sequence: 14,
          timestamp: '2026-06-17T00:00:00.000Z',
          type: 'artifact.updated',
          source: 'engine',
          visibility: 'public',
          payload: unsafePayload,
        }),
      ).toThrow()
    }
  })

  it('rejects artifact lifecycle metadata containing private paths', () => {
    expect(() =>
      runEventSchema.parse({
        id: 'evt-artifact-content',
        conversationSequence: 14,
        runId: 'run-001',
        sequence: 14,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'artifact.updated',
        source: 'engine',
        visibility: 'public',
        payload: {
          artifactId: 'artifact-001',
          title: '/Users/goya/private/image.png',
        },
      }),
    ).toThrow()
  })

  it.each([
    'image/svg+xml',
    'IMAGE/PNG;name=/tmp/private.png',
    'image/png/foo',
    'video/sk-abcdefghijklmnopqrstuvwxyz0123456789',
  ])('rejects unsafe artifact media MIME type %s', (mimeType) => {
    expect(() =>
      runEventSchema.parse({
        id: 'evt-artifact-unsafe-mime',
        conversationSequence: 14,
        runId: 'run-001',
        sequence: 14,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'artifact.updated',
        source: 'engine',
        visibility: 'public',
        payload: {
          artifactId: 'artifact-001',
          media: {
            kind: mimeType.startsWith('video/') ? 'video' : 'image',
            mimeType,
            sizeBytes: 128,
          },
        },
      }),
    ).toThrow()
  })

  it.each([
    ['video', 'audio/mpeg'],
    ['audio', 'video/mp4'],
    ['file', 'image/png'],
  ] as const)('rejects %s artifact media with %s MIME type', (kind, mimeType) => {
    expect(() =>
      runEventSchema.parse({
        id: 'evt-artifact-mime-mismatch',
        conversationSequence: 14,
        runId: 'run-001',
        sequence: 14,
        timestamp: '2026-06-17T00:00:00.000Z',
        type: 'artifact.updated',
        source: 'engine',
        visibility: 'public',
        payload: {
          artifactId: 'artifact-001',
          media: {
            kind,
            mimeType,
            sizeBytes: 128,
          },
        },
      }),
    ).toThrow()
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

  it('accepts historical attachments on user message events', () => {
    const event = runEventSchema.parse({
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
        messageId: 'message-001',
        attachments: [
          {
            blobRef: {
              contentHash: Array.from({ length: 32 }, () => 7),
              contentType: 'text/plain',
              id: 'blob-001',
              size: 128,
            },
            id: 'attachment-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
            mimeType: 'text/plain',
            name: 'notes.txt',
            sizeBytes: 128,
          },
        ],
      },
    })

    expect(event.type).toBe('user.message.appended')
    if (event.type !== 'user.message.appended') {
      throw new Error('expected user message event')
    }
    if (!event.payload) {
      throw new Error('expected user message payload')
    }

    expect(event.payload.attachments).toEqual([
      expect.objectContaining({
        mimeType: 'text/plain',
        name: 'notes.txt',
        sizeBytes: 128,
      }),
    ])
  })

  it('rejects unsafe historical attachment metadata on user message events', () => {
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
          messageId: 'message-001',
          attachments: [
            {
              blobRef: {
                contentHash: Array.from({ length: 32 }, () => 7),
                contentType: 'file:///Users/alice/private.txt',
                id: 'blob-001',
                size: 128,
              },
              id: 'attachment-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
              mimeType: 'text/plain authorization bearer secret-token',
              name: '/Users/alice/.ssh/id_rsa',
              sizeBytes: 128,
            },
          ],
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
          toolUseId: 'tool-001',
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

  it('accepts ordinary token-counting text in visible event payloads', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[2],
        payload: {
          messageId: 'msg-001',
          text: 'Anthropic compatible API, model list, Token 计数, Token: 统计, and token authentication.',
        },
      }),
    ).not.toThrow()
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
          messageId: 'msg-001',
          text: 'error(path=/Users/alice/.ssh/config)',
        },
      }),
    ).toThrow()
  })

  it('rejects unsafe display references in visible event payload text', () => {
    for (const body of [
      '下载：https://provider.example/signed',
      'Inline data:image/svg+xml,<svg onload=alert(1)>',
      'Action javascript:alert(1)',
      'Contact mailto:admin@example.test',
      '路径：.jyowo/runtime/blobs/blob-001',
      '路径：.JYOWO/runtime/blobs/blob-001',
      'log/tmp/provider-output',
      'home~/secret',
      'path=C:/Users/goya/private.txt',
      'cache /var/tmp/provider-output',
    ]) {
      expect(() =>
        runEventSchema.parse({
          id: 'evt-unsafe-display-reference',
          conversationSequence: 12,
          runId: 'run-001',
          sequence: 12,
          timestamp: '2026-06-17T00:00:00.000Z',
          type: 'assistant.notice',
          source: 'assistant',
          visibility: 'public',
          payload: {
            noticeId: '01HZ0000000000000000000001',
            body,
          },
        }),
      ).toThrow()
    }
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
        payload: { messageId: 'msg-001', text: 42 },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[2],
        payload: { text: 'missing message id' },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[2],
        type: 'assistant.thinking.delta',
        payload: {
          status: 'running',
          text: 'raw private chain',
        },
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

  it('accepts safe tool projection fields and rejects unsafe tool details', () => {
    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[4],
        payload: {
          argumentsSummary: 'Input withheld from conversation timeline.',
          command: 'pnpm check:desktop',
          toolName: 'Bash',
          toolUseId: 'tool-001',
        },
      }),
    ).not.toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[4],
        payload: {
          argumentsSummary: 'Input withheld from conversation timeline.',
          query: 'ProcessPanel',
          targetPath: 'apps/desktop/src/features/conversation/timeline/process-panel.tsx',
          toolName: 'read_file',
          toolUseId: 'tool-001',
        },
      }),
    ).not.toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[7],
        payload: {
          diff: {
            files: [
              {
                path: 'apps/desktop/src/features/conversation/timeline/process-panel.tsx',
                addedLines: 2,
                removedLines: 1,
                preview: '@@\n- old\n+ new',
              },
            ],
          },
          durationMs: 12,
          exitCode: 0,
          itemCount: 1,
          outputSummary: 'desktop checks passed',
          toolName: 'apply_patch',
          toolUseId: 'tool-001',
        },
      }),
    ).not.toThrow()

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
        ...runEventFixtures[4],
        payload: {
          argumentsSummary: 'Input withheld from conversation timeline.',
          command: 'cat /Users/goya/.ssh/config',
          toolName: 'Bash',
          toolUseId: 'tool-001',
        },
      }),
    ).toThrow()

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[7],
        payload: {
          outputSummary: 'read /Users/goya/.ssh/config',
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
        toolUseId: 'tool-001',
        workspaceBoundary: 'workspace://local',
      },
    })

    expect(event.payload).toMatchObject({
      operation: 'Install dependencies',
      target: 'workspace package manager',
      toolUseId: 'tool-001',
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
        payload: { messageId: 'msg-001', text: 'Do not render sk-secretkey1234567890' },
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

    expect(() =>
      runEventSchema.parse({
        ...runEventFixtures[11],
        payload: { message: 'request failed with token=secret' },
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
