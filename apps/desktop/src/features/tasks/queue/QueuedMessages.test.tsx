import '@testing-library/jest-dom/vitest'

import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { QueueItemProjection, ServerFrame } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'

import { QueuedMessages } from './QueuedMessages'

describe('QueuedMessages', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('edits a queued message with its server revision', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    renderQueue({ client: clientWith(request), items: [queuedItem()] })

    fireEvent.click(screen.getByRole('button', { name: 'Edit queued message 1' }))
    const editor = screen.getByRole('textbox', { name: 'Edit queued message 1' })
    fireEvent.change(editor, { target: { value: 'Use the repaired scheduler' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save queued message' }))

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith(
        expect.objectContaining({
          content: 'Use the repaired scheduler',
          expectedRevision: 3,
          queueItemId: queueItemId,
          taskId,
          type: 'edit_queued_message',
        }),
      ),
    )
  })

  it('deletes a queued message with its server revision', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    renderQueue({ client: clientWith(request), items: [queuedItem()] })

    fireEvent.click(screen.getByRole('button', { name: 'Delete queued message 1' }))

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith(
        expect.objectContaining({
          expectedRevision: 3,
          queueItemId,
          taskId,
          type: 'delete_queued_message',
        }),
      ),
    )
  })

  it('promotes safely by default', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    renderQueue({ client: clientWith(request), items: [queuedItem()] })

    fireEvent.click(screen.getByRole('button', { name: 'Run queued message 1 next' }))

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith(
        expect.objectContaining({
          expectedRevision: 3,
          mode: 'safe_point',
          queueItemId,
          type: 'promote_queued_message',
        }),
      ),
    )
  })

  it('explains and confirms force promotion before requesting it', async () => {
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true)
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    renderQueue({ client: clientWith(request), items: [queuedItem()] })

    fireEvent.click(screen.getByRole('button', { name: 'Stop now and run queued message 1' }))

    expect(confirm).toHaveBeenCalledWith(
      expect.stringMatching(
        /running processes.*terminated.*committed side effects.*not.*rolled back/i,
      ),
    )
    await waitFor(() =>
      expect(request).toHaveBeenCalledWith(
        expect.objectContaining({
          mode: 'force_stop',
          queueItemId,
          type: 'promote_queued_message',
        }),
      ),
    )
  })

  it('replaces a stale local row with the latest server item and announces the conflict', async () => {
    const latest = queuedItem({ content: 'Latest server text', revision: 4 })
    const request = vi.fn().mockResolvedValue(rejectedFrame(latest))
    renderQueue({ client: clientWith(request), items: [queuedItem()] })

    fireEvent.click(screen.getByRole('button', { name: 'Edit queued message 1' }))
    fireEvent.change(screen.getByRole('textbox', { name: 'Edit queued message 1' }), {
      target: { value: 'Outdated local text' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save queued message' }))

    expect(await screen.findByText('Latest server text')).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent(
      'This queued message changed elsewhere. The latest version is shown.',
    )
  })

  it('advances the command cursor from a stale-revision response', async () => {
    const latest = queuedItem({ revision: 4 })
    const rejected = rejectedFrame(latest)
    const request = vi.fn().mockResolvedValue({
      ...rejected,
      message: { ...rejected.message, currentStreamVersion: 12 },
    })
    const onCommandAccepted = vi.fn()
    renderQueue({ client: clientWith(request), items: [queuedItem()], onCommandAccepted })

    fireEvent.click(screen.getByRole('button', { name: 'Delete queued message 1' }))

    await waitFor(() => expect(onCommandAccepted).toHaveBeenCalledWith(12))
  })

  it('serializes commands across different queue rows', async () => {
    let finishFirst!: (frame: ServerFrame) => void
    const request = vi
      .fn()
      .mockReturnValueOnce(new Promise<ServerFrame>((resolve) => (finishFirst = resolve)))
      .mockResolvedValue(acceptedFrame())
    renderQueue({
      client: clientWith(request),
      items: [queuedItem(), queuedItem({ queueItemId: consumedId, content: 'Second queued item' })],
    })

    fireEvent.click(screen.getByRole('button', { name: 'Delete queued message 1' }))
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))
    expect(screen.getByRole('button', { name: 'Delete queued message 2' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Delete queued message 2' }))
    expect(request).toHaveBeenCalledTimes(1)

    finishFirst(acceptedFrame())
  })

  it('does not cache stale metadata for a command rejected by the busy guard', async () => {
    let finishFirst!: (frame: ServerFrame) => void
    const request = vi
      .fn()
      .mockReturnValueOnce(new Promise<ServerFrame>((resolve) => (finishFirst = resolve)))
      .mockResolvedValueOnce(acceptedFrame())
    const second = queuedItem({ queueItemId: consumedId, content: 'Second queued item' })
    const { rerender } = renderQueue({
      client: clientWith(request),
      items: [queuedItem(), second],
    })

    act(() => {
      screen.getByRole('button', { name: 'Delete queued message 1' }).click()
      screen.getByRole('button', { name: 'Delete queued message 2' }).click()
    })
    expect(request).toHaveBeenCalledTimes(1)

    await act(async () => finishFirst(acceptedFrame()))
    rerender(
      <QueuedMessages
        client={clientWith(request)}
        expectedStreamVersion={10}
        items={[queuedItem(), second]}
        taskId={taskId}
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Delete queued message 2' }))

    await waitFor(() => expect(request).toHaveBeenCalledTimes(2))
    expect(request.mock.calls[1]?.[0].metadata.expectedStreamVersion).toBe(10)
  })

  it('reuses command metadata after an uncertain transport failure', async () => {
    const request = vi
      .fn()
      .mockRejectedValueOnce(new Error('connection closed before response'))
      .mockResolvedValueOnce(acceptedFrame())
    renderQueue({ client: clientWith(request), items: [queuedItem()] })

    fireEvent.click(screen.getByRole('button', { name: 'Delete queued message 1' }))
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))
    fireEvent.click(screen.getByRole('button', { name: 'Delete queued message 1' }))
    await waitFor(() => expect(request).toHaveBeenCalledTimes(2))

    expect(request.mock.calls[1]?.[0].metadata).toEqual(request.mock.calls[0]?.[0].metadata)
  })

  it('keeps edited text open after a failed save', async () => {
    const request = vi.fn().mockRejectedValue(new Error('daemon unavailable'))
    renderQueue({ client: clientWith(request), items: [queuedItem()] })

    fireEvent.click(screen.getByRole('button', { name: 'Edit queued message 1' }))
    const editor = screen.getByRole('textbox', { name: 'Edit queued message 1' })
    fireEvent.change(editor, { target: { value: 'Keep this repaired text' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save queued message' }))

    expect(await screen.findByRole('status')).toHaveTextContent('daemon unavailable')
    expect(screen.getByRole('textbox', { name: 'Edit queued message 1' })).toHaveValue(
      'Keep this repaired text',
    )
  })

  it('disables edit and delete while a message is promoting', () => {
    renderQueue({
      client: clientWith(vi.fn()),
      items: [queuedItem({ state: 'promoting' })],
    })

    expect(screen.getByRole('button', { name: 'Edit queued message 1' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Delete queued message 1' })).toBeDisabled()
    expect(screen.getByText('Preparing to run')).toBeInTheDocument()
  })

  it('closes an open inline editor when the item starts promoting', () => {
    const { rerender } = renderQueue({ client: clientWith(vi.fn()), items: [queuedItem()] })
    fireEvent.click(screen.getByRole('button', { name: 'Edit queued message 1' }))
    expect(screen.getByRole('textbox', { name: 'Edit queued message 1' })).toBeInTheDocument()

    rerender(
      <QueuedMessages
        client={clientWith(vi.fn())}
        expectedStreamVersion={9}
        items={[queuedItem({ state: 'promoting' })]}
        taskId={taskId}
      />,
    )

    expect(screen.queryByRole('textbox', { name: 'Edit queued message 1' })).not.toBeInTheDocument()
  })

  it('omits consumed and deleted items from the active queue', () => {
    renderQueue({
      client: clientWith(vi.fn()),
      items: [
        queuedItem(),
        queuedItem({ content: 'Already consumed', queueItemId: consumedId, state: 'consumed' }),
        queuedItem({ content: 'Already deleted', queueItemId: deletedId, state: 'deleted' }),
      ],
    })

    const queue = screen.getByRole('list', { name: 'Queued messages' })
    expect(within(queue).getByText('Repair the scheduler')).toBeInTheDocument()
    expect(within(queue).queryByText('Already consumed')).not.toBeInTheDocument()
    expect(within(queue).queryByText('Already deleted')).not.toBeInTheDocument()
  })
})

const taskId = '01J00000000000000000000000'
const queueItemId = '01J00000000000000000000001'
const consumedId = '01J00000000000000000000002'
const deletedId = '01J00000000000000000000003'

function queuedItem(overrides: Partial<QueueItemProjection> = {}): QueueItemProjection {
  return {
    attachments: [],
    content: 'Repair the scheduler',
    contextReferences: [],
    createdAt: '2026-07-11T00:00:00Z',
    createdGlobalOffset: 12,
    queueItemId,
    revision: 3,
    state: 'queued',
    ...overrides,
  }
}

function renderQueue({
  client,
  items,
  onCommandAccepted,
}: {
  client: Pick<DaemonClient, 'request'>
  items: QueueItemProjection[]
  onCommandAccepted?: (streamVersion: number) => void
}) {
  return render(
    <QueuedMessages
      client={client}
      expectedStreamVersion={9}
      items={items}
      onCommandAccepted={onCommandAccepted}
      taskId={taskId}
    />,
  )
}

function clientWith(request: ReturnType<typeof vi.fn>): Pick<DaemonClient, 'request'> {
  return { request: request as unknown as DaemonClient['request'] }
}

function acceptedFrame(): ServerFrame {
  return {
    message: {
      commandId: '01J00000000000000000000004',
      committedOffset: 13,
      streamVersion: 10,
      taskId,
      type: 'command_accepted',
    },
    protocolVersion: 1,
    requestId: 'request-1',
  }
}

function rejectedFrame(latestQueueItem: QueueItemProjection): ServerFrame {
  return {
    message: {
      commandId: '01J00000000000000000000004',
      latestQueueItem,
      reason: 'stale_queue_revision',
      taskId,
      type: 'command_rejected',
    },
    protocolVersion: 1,
    requestId: 'request-1',
  }
}
