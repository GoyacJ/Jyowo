import { describe, expect, it } from 'vitest'

import type { ClientFrame } from '@/generated/daemon-protocol'

import { parseClientFrame, parseServerFrame } from './protocol'

const completeSubmitMessageFrame: ClientFrame = {
  requestId: 'req-typed-command',
  protocolVersion: 2,
  request: {
    type: 'submit_message',
    metadata: {
      commandId: '00000000000000000000000001',
      idempotencyKey: 'submit-1',
      expectedStreamVersion: 4,
    },
    taskId: '00000000000000000000000002',
    content: 'continue with the queued change',
    attachments: ['00000000000000000000000003'],
    contextReferences: [
      {
        kind: 'workspace_file',
        label: 'src/main.ts:10',
        path: 'src/main.ts:10',
      },
    ],
  },
}

describe('daemon protocol validation', () => {
  it('rejects protocol v1 client and server frames', () => {
    expect(() => parseClientFrame({ ...completeSubmitMessageFrame, protocolVersion: 1 })).toThrow(
      'Invalid daemon client frame',
    )
    expect(() =>
      parseServerFrame({
        requestId: null,
        protocolVersion: 1,
        message: { type: 'event_batch', afterOffset: 0, latestOffset: 0, gap: false, events: [] },
      }),
    ).toThrow('Invalid daemon server frame')
  })

  it('accepts a generated event batch frame', () => {
    const frame = parseServerFrame({
      requestId: null,
      protocolVersion: 2,
      message: {
        type: 'event_batch',
        afterOffset: 40,
        latestOffset: 40,
        gap: false,
        events: [],
      },
    })

    expect(frame.message.type).toBe('event_batch')
    if (frame.message.type === 'event_batch') {
      expect(frame.message.events).toEqual([])
    }
  })

  it('accepts a complete generated command payload', () => {
    expect(parseClientFrame(completeSubmitMessageFrame)).toEqual(completeSubmitMessageFrame)
  })

  it.each([
    {
      kind: 'skill',
      skillId: 'user:review',
      label: 'Review',
      version: 2,
    },
    {
      kind: 'skill',
      skillId: 'user:review',
      label: 'Review',
      version: 1,
      unexpected: true,
    },
  ])('rejects a skill reference outside the current typed contract', (reference) => {
    expect(() =>
      parseClientFrame({
        ...completeSubmitMessageFrame,
        request: {
          ...completeSubmitMessageFrame.request,
          contextReferences: [reference],
        },
      }),
    ).toThrow('Invalid daemon client frame')
  })

  it('accepts bounded printable ASCII request IDs', () => {
    const frame = { ...completeSubmitMessageFrame, requestId: '~'.repeat(128) }

    expect(parseClientFrame(frame)).toEqual(frame)
  })

  it.each([
    '',
    'r'.repeat(129),
    'request-\n1',
    '请求-1',
  ])('rejects a non-printable or non-ASCII request ID: %j', (requestId) => {
    expect(() => parseClientFrame({ ...completeSubmitMessageFrame, requestId })).toThrow(
      'Invalid daemon client frame',
    )
  })

  it('rejects an unknown server message type', () => {
    expect(() =>
      parseServerFrame({
        requestId: null,
        protocolVersion: 2,
        message: { type: 'future_event' },
      }),
    ).toThrow('Invalid daemon server frame')
  })

  it('rejects a frame without a protocol version', () => {
    expect(() =>
      parseServerFrame({
        requestId: null,
        message: { type: 'task_list', tasks: [] },
      }),
    ).toThrow('Invalid daemon server frame')
  })

  it('rejects raw paths in blob read requests', () => {
    expect(() =>
      parseClientFrame({
        requestId: 'req-1',
        protocolVersion: 2,
        request: {
          type: 'read_blob',
          blobId: '00000000000000000000000001',
          blobPath: '/tmp/secret',
        },
      }),
    ).toThrow('Invalid daemon client frame')
  })

  it('rejects invalid ULIDs', () => {
    expect(() =>
      parseClientFrame({
        requestId: 'req-1',
        protocolVersion: 2,
        request: {
          type: 'read_blob',
          blobId: '/tmp/secret',
        },
      }),
    ).toThrow('Invalid daemon client frame')
  })

  it('rejects non-canonical ULID overflow', () => {
    expect(() =>
      parseClientFrame({
        requestId: 'req-1',
        protocolVersion: 2,
        request: {
          type: 'read_blob',
          blobId: '80000000000000000000000000',
        },
      }),
    ).toThrow('Invalid daemon client frame')
  })

  it('rejects invalid RFC 3339 timestamps', () => {
    expect(() =>
      parseServerFrame({
        requestId: null,
        protocolVersion: 2,
        message: {
          type: 'event_batch',
          afterOffset: 40,
          latestOffset: 41,
          gap: false,
          events: [
            {
              globalOffset: 41,
              taskId: '00000000000000000000000001',
              streamSequence: 1,
              eventId: '00000000000000000000000002',
              eventType: 'assistant.text',
              schemaVersion: 1,
              recordedAt: 'not-a-timestamp',
              source: { kind: 'assistant', actorId: null, clientId: null },
              payload: {},
            },
          ],
        },
      }),
    ).toThrow('Invalid daemon server frame')
  })

  it.each([
    '2015-02-18 12:00:00Z',
    '2015-02-18T12:00:00+0500',
    '2015-02-18T12:00:00+05',
  ])('rejects non-standard RFC 3339 separators: %s', (recordedAt) => {
    expect(() => parseServerFrame(eventBatchFrame(recordedAt))).toThrow(
      'Invalid daemon server frame',
    )
  })

  it('accepts an RFC 3339 timestamp with a colon in the offset', () => {
    expect(parseServerFrame(eventBatchFrame('2015-02-18T12:00:00+05:00')).message.type).toBe(
      'event_batch',
    )
  })
})

function eventBatchFrame(recordedAt: string) {
  return {
    requestId: null,
    protocolVersion: 2,
    message: {
      type: 'event_batch',
      afterOffset: 40,
      latestOffset: 41,
      gap: false,
      events: [
        {
          globalOffset: 41,
          taskId: '00000000000000000000000001',
          streamSequence: 1,
          eventId: '00000000000000000000000002',
          eventType: 'assistant.text',
          schemaVersion: 1,
          recordedAt,
          source: { kind: 'assistant', actorId: null, clientId: null },
          payload: {},
        },
      ],
    },
  }
}
