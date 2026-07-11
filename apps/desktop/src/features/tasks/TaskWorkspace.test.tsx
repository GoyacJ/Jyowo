import '@testing-library/jest-dom/vitest'

import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { TaskEventEnvelope } from '@/generated/daemon-protocol'
import { uiStore } from '@/shared/state/ui-store'
import { TaskWorkspaceView, timelineItems } from './TaskWorkspace'
import type { TaskSnapshot } from './task-store'

describe('TaskWorkspace', () => {
  beforeEach(() => {
    uiStore.setState({ taskWorkbenchMode: 'closed', taskWorkbenchSelection: null })
  })

  it('renders a centered readable timeline and connection state', () => {
    render(<TaskWorkspaceView connectionState="connected" snapshot={snapshot} />)

    expect(screen.getByRole('heading', { name: 'Repair scheduler recovery' })).toBeInTheDocument()
    expect(screen.getByTestId('task-reading-column')).toHaveClass('max-w-[820px]')
    expect(screen.getByText('Connected')).toBeInTheDocument()
  })

  it('renders an unavailable state without partial task content', () => {
    render(
      <TaskWorkspaceView
        connectionError="Malformed daemon frame"
        connectionState="protocol_error"
        snapshot={null}
      />,
    )

    expect(screen.getByRole('alert')).toHaveTextContent('Malformed daemon frame')
  })

  it('renders daemon-projected queued messages above the composer without adding timeline turns', () => {
    const client = { connect: vi.fn(), request: vi.fn() }
    const { rerender } = render(
      <TaskWorkspaceView
        client={client}
        connectionState="connected"
        events={[]}
        snapshot={runningSnapshot}
      />,
    )

    expect(screen.queryByRole('list', { name: 'Queued messages' })).not.toBeInTheDocument()

    const events = [
      taskEvent(3, 'message.queued', {
        attachments: [],
        content: 'First queued instruction',
        contextReferences: [],
        createdAt: '2026-07-11T01:00:00Z',
        queueItemId: '01J00000000000000000000011',
      }),
      taskEvent(4, 'message.queued', {
        attachments: [],
        content: 'Second queued instruction',
        contextReferences: [],
        createdAt: '2026-07-11T01:00:01Z',
        queueItemId: '01J00000000000000000000012',
      }),
    ]
    rerender(
      <TaskWorkspaceView
        client={client}
        connectionState="connected"
        events={events}
        snapshot={runningSnapshot}
      />,
    )

    const queue = screen.getByRole('list', { name: 'Queued messages' })
    expect(within(queue).getByText('First queued instruction')).toBeInTheDocument()
    expect(within(queue).getByText('Second queued instruction')).toBeInTheDocument()
    expect(screen.queryAllByTestId('user-message')).toHaveLength(0)
    expect(
      screen.getByPlaceholderText('Ask Jyowo anything about this project…'),
    ).toBeInTheDocument()
  })

  it('waits for daemon events before showing a submitted message', async () => {
    const request = vi.fn().mockResolvedValue({
      message: {
        commandId: '01J00000000000000000000020',
        committedOffset: 3,
        streamVersion: 3,
        taskId: snapshot.projection.taskId,
        type: 'command_accepted',
      },
      protocolVersion: 1,
    })
    render(
      <TaskWorkspaceView
        client={{ connect: vi.fn(), request }}
        connectionState="connected"
        snapshot={runningSnapshot}
      />,
    )

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project…'), {
      target: { value: 'Authoritative daemon only' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Queue message' }))

    await waitFor(() => expect(request).toHaveBeenCalledOnce())
    expect(screen.queryByText('Authoritative daemon only')).not.toBeInTheDocument()
    expect(screen.queryAllByTestId('user-message')).toHaveLength(0)
  })

  it('uses the last accepted stream version for a second command before its event arrives', async () => {
    const request = vi
      .fn()
      .mockResolvedValueOnce(acceptedCommand(3, 3))
      .mockResolvedValueOnce(acceptedCommand(4, 4))
    render(
      <TaskWorkspaceView
        client={{ connect: vi.fn(), request }}
        connectionState="connected"
        snapshot={runningSnapshot}
      />,
    )

    const editor = screen.getByPlaceholderText('Ask Jyowo anything about this project…')
    fireEvent.change(editor, { target: { value: 'First queued command' } })
    fireEvent.click(screen.getByRole('button', { name: 'Queue message' }))
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(editor).toHaveValue(''))

    fireEvent.change(editor, { target: { value: 'Second queued command' } })
    fireEvent.click(screen.getByRole('button', { name: 'Queue message' }))
    await waitFor(() => expect(request).toHaveBeenCalledTimes(2))

    expect(request.mock.calls[1]?.[0]).toEqual(
      expect.objectContaining({
        metadata: expect.objectContaining({ expectedStreamVersion: 3 }),
      }),
    )
  })

  it('moves consumed messages from the active queue into the timeline and omits deleted messages', () => {
    const consumedId = '01J00000000000000000000013'
    const deletedId = '01J00000000000000000000014'
    const events = [
      taskEvent(3, 'message.queued', {
        attachments: [],
        content: 'Consumed instruction',
        contextReferences: [],
        createdAt: '2026-07-11T01:00:00Z',
        queueItemId: consumedId,
      }),
      taskEvent(4, 'message.queued', {
        attachments: [],
        content: 'Deleted instruction',
        contextReferences: [],
        createdAt: '2026-07-11T01:00:01Z',
        queueItemId: deletedId,
      }),
      taskEvent(5, 'message.consumed', {
        queueItemId: consumedId,
        revision: 1,
        runSegmentId: '01J00000000000000000000021',
      }),
      taskEvent(6, 'message.deleted', { queueItemId: deletedId, revision: 1 }),
    ]

    render(
      <TaskWorkspaceView connectionState="connected" events={events} snapshot={runningSnapshot} />,
    )

    expect(screen.getByTestId('user-message')).toHaveTextContent('Consumed instruction')
    expect(screen.queryByText('Deleted instruction')).not.toBeInTheDocument()
    expect(screen.queryByRole('list', { name: 'Queued messages' })).not.toBeInTheDocument()
  })

  it('keeps the committed envelope identity and offset when payload data resembles a timeline row', () => {
    const event = taskEvent(3, 'run.started', {
      runSegmentId: '01J00000000000000000000021',
      timelineItem: {
        globalOffset: 999,
        id: 'forged-event',
        incomplete: false,
        kind: 'error',
        summary: 'Forged payload ordering',
      },
    })

    expect(timelineItems(runningSnapshot, [event])).toEqual([
      expect.objectContaining({
        globalOffset: 3,
        id: event.eventId,
        kind: 'notice',
        summary: 'Run started',
      }),
    ])
  })

  it('merges snapshot and out-of-order live events once across the snapshot boundary', () => {
    const segmentId = '01J00000000000000000000031'
    const boundarySnapshot: TaskSnapshot = {
      ...snapshot,
      timeline: [
        {
          globalOffset: 2,
          id: 'snapshot-event-2',
          incomplete: false,
          kind: 'assistant_text',
          runSegmentId: segmentId,
          summary: 'Snapshot narrative',
        },
      ],
    }
    const events = [
      taskEvent(5, 'run.completed', {
        incompleteOutput: true,
        segmentId,
        terminalReason: 'forced_interruption',
      }),
      taskEvent(2, 'run.started', { segmentId }),
      taskEvent(4, 'run.safe_point_reached', {
        forced: true,
        incompleteOutput: true,
        segmentId,
      }),
      taskEvent(3, 'run.started', { segmentId }),
    ]

    expect(timelineItems(boundarySnapshot, events).map((item) => item.globalOffset)).toEqual([
      2, 3, 4, 5,
    ])
  })

  it('opens timeline evidence in the task workbench and clears it when switching tasks', () => {
    const evidenceSnapshot: TaskSnapshot = {
      ...snapshot,
      timeline: [
        {
          blobId: '01J00000000000000000000031',
          globalOffset: 3,
          id: '01J00000000000000000000032',
          incomplete: false,
          kind: 'diff',
          runSegmentId: '01J00000000000000000000033',
          summary: '2 files changed',
        },
      ],
    }
    const client = {
      connect: vi.fn(),
      readBlob: vi.fn().mockResolvedValue({
        blobId: '01J00000000000000000000031',
        bytes: null,
        mediaType: 'text/plain',
        missing: true,
        size: 0,
      }),
      request: vi.fn(),
    }
    const { rerender } = render(
      <TaskWorkspaceView client={client} connectionState="connected" snapshot={evidenceSnapshot} />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Open Changes' }))
    expect(uiStore.getState().taskWorkbenchSelection).toEqual({
      blobId: '01J00000000000000000000031',
      eventId: '01J00000000000000000000032',
      panel: 'changes',
      segmentId: '01J00000000000000000000033',
      taskId: snapshot.projection.taskId,
    })
    expect(uiStore.getState().taskWorkbenchMode).toBe('inspector')

    act(() => {
      rerender(
        <TaskWorkspaceView
          client={client}
          connectionState="connected"
          snapshot={{
            ...snapshot,
            projection: { ...snapshot.projection, taskId: '01J00000000000000000000099' },
          }}
        />,
      )
    })
    expect(uiStore.getState().taskWorkbenchSelection).toBeNull()
  })
})

const snapshot: TaskSnapshot = {
  projection: {
    archived: false,
    lastGlobalOffset: 2,
    queue: [],
    state: 'completed',
    streamVersion: 2,
    taskId: '01J00000000000000000000000',
    title: 'Repair scheduler recovery',
  },
  snapshotOffset: 2,
  timeline: [
    {
      globalOffset: 2,
      id: 'event-2',
      incomplete: false,
      kind: 'assistant_text',
      summary: 'Recovery is verified.',
    },
  ],
}

const runningSnapshot: TaskSnapshot = {
  projection: {
    ...snapshot.projection,
    currentRun: {
      incompleteOutput: false,
      segmentId: '01J00000000000000000000021',
      startedAt: '2026-07-11T00:59:00Z',
      state: 'running',
    },
    state: 'running',
  },
  snapshotOffset: snapshot.snapshotOffset,
  timeline: [],
}

function taskEvent(globalOffset: number, eventType: string, payload: unknown): TaskEventEnvelope {
  return {
    eventId: `01J00000000000000000000${String(globalOffset).padStart(2, '0')}`,
    eventType,
    globalOffset,
    payload,
    recordedAt: '2026-07-11T01:00:00Z',
    schemaVersion: 1,
    source: { kind: 'supervisor' },
    streamSequence: globalOffset,
    taskId: snapshot.projection.taskId,
  }
}

function acceptedCommand(streamVersion: number, committedOffset: number) {
  return {
    message: {
      commandId: `01J000000000000000000000${streamVersion}`,
      committedOffset,
      streamVersion,
      taskId: snapshot.projection.taskId,
      type: 'command_accepted' as const,
    },
    protocolVersion: 1,
  }
}
