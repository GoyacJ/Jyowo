import { useState } from 'react'

import type {
  TaskEventEnvelope,
  TimelineItemProjection,
  TypedUlid,
} from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { pickAttachmentPath } from '@/shared/tauri/file-dialog'
import { useCommandClient, useDaemonClient } from '@/shared/tauri/react'
import { QueuedMessages } from './queue/QueuedMessages'
import { TaskComposer } from './TaskComposer'
import type { TaskConnectionState, TaskSnapshot } from './task-store'
import { TaskTimeline } from './timeline/TaskTimeline'
import { useTask } from './use-task'

export function TaskWorkspace({ taskId }: { taskId: TypedUlid }) {
  const task = useTask(taskId)
  const daemonClient = useDaemonClient()
  const commandClient = useCommandClient()
  return (
    <TaskWorkspaceView
      client={daemonClient}
      connectionError={task.connectionError}
      connectionState={task.connectionState}
      events={task.events}
      onCreateAttachmentFromPath={(path) => commandClient.createAttachmentFromPath(path)}
      onPickAttachmentPath={pickAttachmentPath}
      snapshot={task.snapshot}
    />
  )
}

export function TaskWorkspaceView({
  connectionError,
  connectionState,
  client,
  events = [],
  onCreateAttachmentFromPath,
  onPickAttachmentPath,
  snapshot,
}: {
  client?: Pick<DaemonClient, 'connect' | 'request'>
  connectionError?: string | null
  connectionState: TaskConnectionState
  events?: TaskEventEnvelope[]
  onCreateAttachmentFromPath?: Parameters<typeof TaskComposer>[0]['onCreateAttachmentFromPath']
  onPickAttachmentPath?: Parameters<typeof TaskComposer>[0]['onPickAttachmentPath']
  snapshot: TaskSnapshot | null
}) {
  const snapshotTaskId = snapshot?.projection.taskId ?? null
  const projectedStreamVersion = snapshot
    ? events.reduce(
        (version, event) => Math.max(version, event.streamSequence),
        snapshot.projection.streamVersion,
      )
    : 0
  const [acceptedCommandCursor, setAcceptedCommandCursor] = useState<{
    taskId: TypedUlid | null
    version: number
  }>({ taskId: snapshotTaskId, version: projectedStreamVersion })
  const commandStreamVersion =
    acceptedCommandCursor.taskId === snapshotTaskId
      ? Math.max(acceptedCommandCursor.version, projectedStreamVersion)
      : projectedStreamVersion
  const commandAccepted = (version: number) => {
    if (!snapshotTaskId) return
    setAcceptedCommandCursor((current) => ({
      taskId: snapshotTaskId,
      version: current.taskId === snapshotTaskId ? Math.max(current.version, version) : version,
    }))
  }

  if (connectionState === 'protocol_error') {
    return (
      <div className="grid h-full place-items-center">
        <div
          className="max-w-md rounded-xl border border-destructive/30 bg-destructive/5 px-5 py-4 text-destructive text-sm"
          role="alert"
        >
          {connectionError ?? 'The task stream could not be read.'}
        </div>
      </div>
    )
  }

  if (!snapshot) {
    return (
      <div className="grid h-full place-items-center text-muted-foreground text-sm" role="status">
        {connectionState === 'disconnected' ? 'Task unavailable' : 'Loading task…'}
      </div>
    )
  }

  const items = timelineItems(snapshot, events)
  const queue = queueItems(snapshot, events)
  return (
    <section
      className="mx-auto flex h-full w-full max-w-[820px] min-h-0 flex-col"
      data-testid="task-reading-column"
    >
      <header className="flex items-start justify-between gap-6 border-border/70 border-b px-1 pb-4">
        <div className="min-w-0">
          <h1 className="truncate font-semibold text-lg tracking-[-0.015em]">
            {snapshot.projection.title}
          </h1>
          <p className="mt-1 text-muted-foreground text-xs capitalize">
            {snapshot.projection.state.replace('_', ' ')}
          </p>
        </div>
        <span className="mt-1 shrink-0 text-muted-foreground text-xs">
          {connectionLabel(connectionState)}
        </span>
      </header>
      <div className="flex min-h-0 flex-1 pt-6">
        <TaskTimeline currentRun={snapshot.projection.currentRun} items={items} />
      </div>
      {client ? (
        <div className="shrink-0 border-border/70 border-t bg-background/95 px-1 pt-3 pb-1 backdrop-blur-sm">
          <QueuedMessages
            client={client}
            expectedStreamVersion={commandStreamVersion}
            items={queue}
            onCommandAccepted={commandAccepted}
            taskId={snapshot.projection.taskId}
          />
          <TaskComposer
            client={client}
            connectionState={connectionState}
            onCommandAccepted={commandAccepted}
            onCreateAttachmentFromPath={onCreateAttachmentFromPath}
            onPickAttachmentPath={onPickAttachmentPath}
            streamVersion={commandStreamVersion}
            taskId={snapshot.projection.taskId}
            taskState={snapshot.projection.state}
          />
        </div>
      ) : null}
    </section>
  )
}

export function queueItems(snapshot: TaskSnapshot, events: TaskEventEnvelope[]) {
  const byId = new Map(snapshot.projection.queue.map((item) => [item.queueItemId, item]))
  for (const event of events) {
    if (event.globalOffset <= snapshot.snapshotOffset) continue
    const payload = record(event.payload)
    const queueItemId = stringValue(payload?.queueItemId)
    if (!queueItemId) continue
    const current = byId.get(queueItemId)

    if (event.eventType === 'message.queued') {
      const content = stringValue(payload?.content)
      const createdAt = stringValue(payload?.createdAt)
      if (!content || !createdAt) continue
      byId.set(queueItemId, {
        attachments: stringArray(payload?.attachments),
        content,
        contextReferences: stringArray(payload?.contextReferences),
        createdAt,
        createdGlobalOffset: event.globalOffset,
        queueItemId,
        revision: 1,
        state: 'queued',
      })
      continue
    }
    if (!current) continue
    const revision = numberValue(payload?.revision)
    if (revision === undefined || revision < current.revision) continue

    if (event.eventType === 'message.edited') {
      const content = stringValue(payload?.content)
      if (!content) continue
      byId.set(queueItemId, {
        ...current,
        attachments: stringArray(payload?.attachments),
        content,
        contextReferences: stringArray(payload?.contextReferences),
        revision,
      })
    } else if (event.eventType === 'message.promoted') {
      byId.set(queueItemId, { ...current, state: 'promoting' })
    } else if (event.eventType === 'message.recovered') {
      byId.set(queueItemId, { ...current, state: 'queued' })
    } else if (event.eventType === 'message.consumed' || event.eventType === 'message.deleted') {
      byId.delete(queueItemId)
    }
  }
  return [...byId.values()].sort(
    (left, right) =>
      left.createdGlobalOffset - right.createdGlobalOffset ||
      left.queueItemId.localeCompare(right.queueItemId),
  )
}

export function timelineItems(snapshot: TaskSnapshot, events: TaskEventEnvelope[]) {
  const byOffset = new Map(snapshot.timeline.map((item) => [item.globalOffset, item]))
  const queuedContent = queueContent(snapshot, events)
  for (const event of events) {
    if (event.globalOffset <= snapshot.snapshotOffset || byOffset.has(event.globalOffset)) continue
    const item = projectEvent(event, queuedContent)
    if (item) byOffset.set(item.globalOffset, item)
  }
  return [...byOffset.values()].sort((left, right) => left.globalOffset - right.globalOffset)
}

function projectEvent(
  event: TaskEventEnvelope,
  queuedContent: Map<string, string>,
): TimelineItemProjection | null {
  const payload = record(event.payload)
  if (
    [
      'message.queued',
      'message.edited',
      'message.promoted',
      'message.deleted',
      'message.recovered',
    ].includes(event.eventType)
  )
    return null

  const runSegmentId =
    stringValue(payload?.runSegmentId) ?? stringValue(payload?.segmentId) ?? childSegment(payload)
  const projection: Record<
    string,
    Pick<TimelineItemProjection, 'kind' | 'summary' | 'incomplete'>
  > = {
    'message.consumed': {
      kind: 'user_message',
      summary: queuedContent.get(stringValue(payload?.queueItemId) ?? '') ?? 'Message submitted',
      incomplete: false,
    },
    'permission.invalidated': {
      kind: 'permission',
      summary: 'Permission expired after restart',
      incomplete: false,
    },
    'permission.requested': {
      kind: 'permission',
      summary: 'Permission requested',
      incomplete: false,
    },
    'permission.resolved': {
      kind: 'permission',
      summary: 'Permission resolved',
      incomplete: false,
    },
    'run.completed': {
      kind: 'notice',
      summary: runTerminalSummary(payload),
      incomplete: Boolean(payload?.incompleteOutput),
    },
    'run.force_stop_timed_out': {
      kind: 'notice',
      summary: 'Run force-stop timed out',
      incomplete: true,
    },
    'run.safe_point_reached': {
      kind: 'notice',
      summary: payload?.forced ? 'Run force-stopped' : 'Run safe point reached',
      incomplete: Boolean(payload?.incompleteOutput),
    },
    'run.started': { kind: 'notice', summary: 'Run started', incomplete: false },
    'run.yield_requested': {
      kind: 'notice',
      summary: payload?.force ? 'Run force-stop requested' : 'Run yield requested',
      incomplete: false,
    },
    'subagent.backgrounded': {
      kind: 'subagent',
      summary: 'Subagent continuing in background',
      incomplete: false,
    },
    'subagent.linked': { kind: 'subagent', summary: 'Subagent linked', incomplete: false },
    'subagent.spawned': { kind: 'subagent', summary: 'Subagent started', incomplete: false },
    'subagent.state_changed': {
      kind: 'subagent',
      summary: childSummary(payload) ?? 'Subagent state changed',
      incomplete: false,
    },
    'subagent.summary_updated': {
      kind: 'subagent',
      summary: childSummary(payload) ?? 'Subagent summary updated',
      incomplete: false,
    },
    'subagent.terminal': {
      kind: 'subagent',
      summary: childSummary(payload) ?? 'Subagent finished',
      incomplete: childState(payload) === 'failed',
    },
    'task.actor_failed': { kind: 'error', summary: 'Task actor failed', incomplete: true },
    'task.archived': {
      kind: 'notice',
      summary: payload?.archived ? 'Task archived' : 'Task restored',
      incomplete: false,
    },
    'task.created': { kind: 'notice', summary: 'Task created', incomplete: false },
    'task.title_changed': { kind: 'notice', summary: 'Task title changed', incomplete: false },
    'tool.indeterminate': {
      kind: 'tool_activity',
      summary: 'Tool outcome is indeterminate after restart',
      incomplete: true,
    },
    'workspace.acquired': { kind: 'notice', summary: 'Workspace acquired', incomplete: false },
    'workspace.cleanup_blocked': {
      kind: 'notice',
      summary: 'Workspace cleanup blocked',
      incomplete: false,
    },
    'workspace.cleanup_pending': {
      kind: 'notice',
      summary: 'Workspace cleanup pending',
      incomplete: false,
    },
    'workspace.override_applied': {
      kind: 'notice',
      summary: 'Workspace write override applied',
      incomplete: false,
    },
    'workspace.preparing': { kind: 'notice', summary: 'Workspace preparing', incomplete: false },
    'workspace.released': { kind: 'notice', summary: 'Workspace released', incomplete: false },
    'workspace.waiting': { kind: 'notice', summary: 'Workspace lease waiting', incomplete: false },
  }
  const known = projection[event.eventType]
  const fallback = event.eventType.startsWith('engine.')
    ? {
        kind: 'notice' as const,
        summary: event.eventType.slice(7).replaceAll('_', ' '),
        incomplete: false,
      }
    : null
  const selected = known ?? fallback
  if (!selected) return null
  return {
    ...selected,
    globalOffset: event.globalOffset,
    id: event.eventId,
    runSegmentId,
  }
}

function queueContent(snapshot: TaskSnapshot, events: TaskEventEnvelope[]) {
  const content = new Map(snapshot.projection.queue.map((item) => [item.queueItemId, item.content]))
  for (const event of events) {
    if (event.eventType !== 'message.queued' && event.eventType !== 'message.edited') continue
    const payload = record(event.payload)
    const id = stringValue(payload?.queueItemId)
    const value = stringValue(payload?.content)
    if (id && value) content.set(id, value)
  }
  return content
}

function connectionLabel(state: TaskConnectionState) {
  const labels: Record<TaskConnectionState, string> = {
    connected: 'Connected',
    connecting: 'Connecting',
    disconnected: 'Disconnected',
    protocol_error: 'Protocol error',
    resyncing: 'Resyncing',
  }
  return labels[state]
}

function runTerminalSummary(payload: Record<string, unknown> | null) {
  const reason = stringValue(payload?.terminalReason)
  const summaries: Record<string, string> = {
    cancelled: 'Run cancelled',
    completed: 'Run completed',
    failed: 'Run failed',
    forced_interruption: 'Run force-stopped',
    interrupted_by_restart: 'Run interrupted by restart',
    superseded: 'Run superseded',
  }
  return (reason && summaries[reason]) || 'Run completed'
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function stringValue(value: unknown) {
  return typeof value === 'string' ? value : undefined
}

function numberValue(value: unknown) {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : undefined
}

function stringArray(value: unknown) {
  return Array.isArray(value) && value.every((item) => typeof item === 'string')
    ? (value as string[])
    : []
}

function childRecord(payload: Record<string, unknown> | null) {
  return record(payload?.child)
}

function childSegment(payload: Record<string, unknown> | null) {
  return stringValue(childRecord(payload)?.segmentId)
}

function childState(payload: Record<string, unknown> | null) {
  return stringValue(childRecord(payload)?.state)
}

function childSummary(payload: Record<string, unknown> | null) {
  return stringValue(childRecord(payload)?.summary) ?? childState(payload)
}
