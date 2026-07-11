import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { DaemonClient } from '@/shared/daemon/client'

import { TaskComposer } from './TaskComposer'

describe('TaskComposer', () => {
  it('stays editable during a run and submits a generated queue-capable command', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    renderComposer({ client: clientWith(request), taskState: 'running' })

    const editor = screen.getByPlaceholderText('Ask Jyowo anything about this project…')
    expect(editor).toBeEnabled()
    fireEvent.change(editor, { target: { value: 'Inspect the next failure' } })
    fireEvent.click(screen.getByRole('button', { name: 'Queue message' }))

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith({
        attachments: [],
        content: 'Inspect the next failure',
        contextReferences: [],
        metadata: {
          commandId: expect.stringMatching(/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/),
          expectedStreamVersion: 9,
          idempotencyKey: expect.any(String),
        },
        taskId,
        type: 'submit_message',
      }),
    )
  })

  it.each([
    'waiting_permission',
    'yielding',
  ] as const)('uses Queue semantics while the task is %s', (taskState) => {
    renderComposer({ client: clientWith(vi.fn()), taskState })

    expect(screen.getByRole('button', { name: 'Queue message' })).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Ask Jyowo anything about this project…')).toBeEnabled()
  })

  it('uses Send semantics when no segment is active', () => {
    renderComposer({ client: clientWith(vi.fn()), taskState: 'idle' })

    expect(screen.getByRole('button', { name: 'Send message' })).toBeInTheDocument()
  })

  it('disables only duplicate submission while a request is in flight', async () => {
    let acceptRequest!: (frame: ReturnType<typeof acceptedFrame>) => void
    const request = vi.fn().mockReturnValue(
      new Promise((resolve) => {
        acceptRequest = resolve
      }),
    )
    renderComposer({ client: clientWith(request), taskState: 'running' })

    const editor = screen.getByPlaceholderText('Ask Jyowo anything about this project…')
    fireEvent.change(editor, { target: { value: 'Queue once' } })
    fireEvent.click(screen.getByRole('button', { name: 'Queue message' }))

    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Queue message' })).toBeDisabled(),
    )
    expect(editor).toBeEnabled()

    acceptRequest(acceptedFrame())
  })

  it('preserves a disconnected draft and exposes a retryable connection state', async () => {
    const request = vi.fn().mockRejectedValue(new Error('daemon unavailable'))
    const connect = vi.fn().mockResolvedValue(undefined)
    renderComposer({ client: clientWith(request, connect), connectionState: 'disconnected' })

    const editor = screen.getByPlaceholderText('Ask Jyowo anything about this project…')
    fireEvent.change(editor, { target: { value: 'Do not lose this draft' } })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Connection lost. Your draft is preserved.',
    )
    expect(editor).toHaveValue('Do not lose this draft')
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(connect).toHaveBeenCalledOnce()
  })
})

const taskId = '01J00000000000000000000000'

function renderComposer({
  client,
  connectionState = 'connected',
  taskState = 'idle',
}: {
  client: Pick<DaemonClient, 'connect' | 'request'>
  connectionState?: 'connected' | 'disconnected'
  taskState?: 'idle' | 'running' | 'waiting_permission' | 'yielding'
}) {
  return render(
    <TaskComposer
      client={client}
      connectionState={connectionState}
      streamVersion={9}
      taskId={taskId}
      taskState={taskState}
    />,
  )
}

function clientWith(
  request: ReturnType<typeof vi.fn>,
  connect: ReturnType<typeof vi.fn> = vi.fn().mockResolvedValue(undefined),
): Pick<DaemonClient, 'connect' | 'request'> {
  return {
    connect: connect as unknown as DaemonClient['connect'],
    request: request as unknown as DaemonClient['request'],
  }
}

function acceptedFrame() {
  return {
    message: {
      commandId: '01J00000000000000000000001',
      committedOffset: 14,
      streamVersion: 10,
      taskId,
      type: 'command_accepted' as const,
    },
    protocolVersion: 1,
    requestId: 'request-1',
  }
}
