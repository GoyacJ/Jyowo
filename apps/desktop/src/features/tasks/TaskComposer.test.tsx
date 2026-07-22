import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { RunProjection } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'

import { TaskComposer } from './TaskComposer'

describe('TaskComposer', () => {
  it('stays editable during a run and submits a generated queue-capable command', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    renderComposer({ client: clientWith(request), taskState: 'running' })

    const editor = screen.getByPlaceholderText('Ask Jyowo anything about this project…')
    expect(editor).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Pause' })).toBeInTheDocument()
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

  it('requests a safe-point pause for the active foreground run', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    renderComposer({ client: clientWith(request), taskState: 'running' })

    fireEvent.click(screen.getByRole('button', { name: 'Pause' }))

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith({
        metadata: {
          commandId: expect.stringMatching(/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/),
          expectedStreamVersion: 9,
          idempotencyKey: expect.any(String),
        },
        mode: 'safe_point',
        taskId,
        type: 'stop_run',
      }),
    )
  })

  it('continues a run that completed a safe-point pause', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    renderComposer({
      client: clientWith(request),
      currentRun: {
        ...activeRun('interrupted'),
        endedAt: '2026-07-18T00:00:01Z',
        terminalReason: 'cancelled',
      },
      taskState: 'interrupted',
    })

    fireEvent.click(screen.getByRole('button', { name: 'Continue' }))

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith({
        indeterminateTools: [],
        metadata: {
          commandId: expect.stringMatching(/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/),
          expectedStreamVersion: 9,
          idempotencyKey: expect.any(String),
        },
        taskId,
        type: 'continue_task',
      }),
    )
  })

  it('submits the selected model and permission mode to the daemon', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    render(
      <TaskComposer
        client={clientWith(request)}
        connectionState="connected"
        modelConfigId="provider-config-001"
        modelConfigs={[{ id: 'provider-config-001', label: 'OpenAI / GPT-5' }]}
        permissionMode="auto"
        streamVersion={9}
        taskId={taskId}
        taskState="idle"
      />,
    )

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project…'), {
      target: { value: 'Use the selected runtime' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith(
        expect.objectContaining({
          modelConfigId: 'provider-config-001',
          permissionMode: 'auto',
          type: 'submit_message',
        }),
      ),
    )
  })

  it('treats a whitespace-only model override as inherited', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    render(
      <TaskComposer
        client={clientWith(request)}
        connectionState="connected"
        modelConfigId="   "
        streamVersion={9}
        taskId={taskId}
        taskState="idle"
      />,
    )

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project…'), {
      target: { value: 'Use the inherited model' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))
    expect(request.mock.calls[0]?.[0]).not.toHaveProperty('modelConfigId')
  })

  it('keeps submission errors scoped to their task', async () => {
    const request = vi.fn().mockRejectedValue(new Error('Task A failed'))
    const client = clientWith(request)
    const { rerender } = render(
      <TaskComposer
        client={client}
        connectionState="connected"
        streamVersion={9}
        taskId={taskId}
        taskState="idle"
      />,
    )

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project…'), {
      target: { value: 'Submit task A' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Task A failed')

    rerender(
      <TaskComposer
        client={client}
        connectionState="connected"
        streamVersion={3}
        taskId={taskBId}
        taskState="idle"
      />,
    )
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()

    rerender(
      <TaskComposer
        client={client}
        connectionState="connected"
        streamVersion={9}
        taskId={taskId}
        taskState="idle"
      />,
    )
    expect(screen.getByRole('alert')).toHaveTextContent('Task A failed')
  })

  it('lets the next task submit while an earlier task fails late', async () => {
    let rejectTaskA!: (error: Error) => void
    const taskARequest = new Promise<ReturnType<typeof acceptedFrame>>((_, reject) => {
      rejectTaskA = reject
    })
    const request = vi.fn((requestBody: { taskId: string }) =>
      requestBody.taskId === taskId ? taskARequest : Promise.resolve(acceptedFrame(taskBId)),
    )
    const client = clientWith(request)
    const { rerender } = render(
      <TaskComposer
        client={client}
        connectionState="connected"
        streamVersion={9}
        taskId={taskId}
        taskState="idle"
      />,
    )

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project…'), {
      target: { value: 'Pending task A' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))

    rerender(
      <TaskComposer
        client={client}
        connectionState="connected"
        streamVersion={3}
        taskId={taskBId}
        taskState="idle"
      />,
    )
    const taskBEditor = screen.getByPlaceholderText('Ask Jyowo anything about this project…')
    fireEvent.change(taskBEditor, { target: { value: 'Independent task B' } })
    expect(screen.getByRole('button', { name: 'Send message' })).toBeEnabled()
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))
    await waitFor(() =>
      expect(request).toHaveBeenCalledWith(expect.objectContaining({ taskId: taskBId })),
    )

    rejectTaskA(new Error('Task A failed late'))
    await waitFor(() => expect(request).toHaveBeenCalledTimes(2))
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    expect(taskBEditor).toHaveValue('')

    rerender(
      <TaskComposer
        client={client}
        connectionState="connected"
        streamVersion={9}
        taskId={taskId}
        taskState="idle"
      />,
    )
    expect(await screen.findByRole('alert')).toHaveTextContent('Task A failed late')
  })

  it('preserves a tagged context reference in the daemon command', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    render(
      <TaskComposer
        client={clientWith(request)}
        connectionState="connected"
        onListReferenceCandidates={vi.fn().mockResolvedValue({
          artifacts: [],
          conversations: [],
          files: [{ label: 'lib.rs', path: 'src/lib.rs' }],
          memories: [],
          mcpServers: [],
          skills: [],
          tools: [],
        })}
        streamVersion={9}
        taskId={taskId}
        taskState="idle"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Reference project object' }))
    fireEvent.click(await screen.findByRole('option', { name: 'lib.rs' }))
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project…'), {
      target: { value: 'Inspect this file' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith(
        expect.objectContaining({
          contextReferences: [
            {
              kind: 'workspace_file',
              label: 'lib.rs',
              path: 'src/lib.rs',
            },
          ],
        }),
      ),
    )
  })

  it('stages a selected attachment in the daemon task blob store', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    const stageBlobFromPath = vi.fn().mockResolvedValue({
      attachment: {
        blobRef: {
          contentHash: Array.from({ length: 32 }, () => 1),
          contentType: 'text/plain',
          id: '01J00000000000000000000005',
          size: 5,
        },
        id: `attachment-${'01'.repeat(32)}`,
        mimeType: 'text/plain',
        name: 'notes.txt',
        sizeBytes: 5,
      },
    })
    render(
      <TaskComposer
        client={{ ...clientWith(request), stageBlobFromPath } as never}
        connectionState="connected"
        onPickAttachmentPath={vi.fn().mockResolvedValue('/tmp/notes.txt')}
        streamVersion={9}
        taskId={taskId}
        taskState="idle"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Attach file' }))

    await waitFor(() => expect(stageBlobFromPath).toHaveBeenCalledWith(taskId, '/tmp/notes.txt'))
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

  it('reuses command metadata when retrying an uncertain submit result', async () => {
    const request = vi
      .fn()
      .mockRejectedValueOnce(new Error('connection closed before response'))
      .mockResolvedValueOnce(acceptedFrame())
    renderComposer({ client: clientWith(request), taskState: 'running' })

    const editor = screen.getByPlaceholderText('Ask Jyowo anything about this project…')
    fireEvent.change(editor, { target: { value: 'Queue exactly once' } })
    fireEvent.click(screen.getByRole('button', { name: 'Queue message' }))
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Queue message' })).toBeEnabled())

    fireEvent.click(screen.getByRole('button', { name: 'Queue message' }))
    await waitFor(() => expect(request).toHaveBeenCalledTimes(2))

    expect(request.mock.calls[1]?.[0].metadata).toEqual(request.mock.calls[0]?.[0].metadata)
  })
})

const taskId = '01J00000000000000000000000'
const taskBId = '01J00000000000000000000006'

function renderComposer({
  client,
  connectionState = 'connected',
  currentRun,
  taskState = 'idle',
}: {
  client: Pick<DaemonClient, 'connect' | 'request'>
  connectionState?: 'connected' | 'disconnected'
  currentRun?: RunProjection
  taskState?: 'idle' | 'running' | 'waiting_permission' | 'yielding' | 'interrupted'
}) {
  return render(
    <TaskComposer
      client={client}
      connectionState={connectionState}
      currentRun={
        currentRun ??
        (taskState === 'running' || taskState === 'waiting_permission' || taskState === 'yielding'
          ? activeRun(taskState)
          : undefined)
      }
      streamVersion={9}
      taskId={taskId}
      taskState={taskState}
    />,
  )
}

function activeRun(state: RunProjection['state']): RunProjection {
  return {
    incompleteOutput: false,
    segmentId: '01J00000000000000000000009',
    startedAt: '2026-07-18T00:00:00Z',
    state,
  }
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

function acceptedFrame(acceptedTaskId = taskId) {
  return {
    message: {
      commandId: '01J00000000000000000000001',
      committedOffset: 14,
      streamVersion: 10,
      taskId: acceptedTaskId,
      type: 'command_accepted' as const,
    },
    protocolVersion: 7,
    requestId: 'request-1',
  }
}
