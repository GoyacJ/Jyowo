import type {
  PermissionProjection,
  QueueItemProjection,
  RunProjection,
  RunTerminalReason,
  SubagentProjection,
  TaskEventEnvelope,
  TaskProjection,
  TaskState,
  TimelineEventKind,
  TimelineItemProjection,
  TypedUlid,
} from '@/generated/daemon-protocol'

import type { TaskSnapshot } from './task-store'

export function deriveLiveTaskSnapshot(
  snapshot: TaskSnapshot,
  events: TaskEventEnvelope[],
): TaskSnapshot {
  const projection: TaskProjection = {
    ...snapshot.projection,
    currentRun: snapshot.projection.currentRun
      ? { ...snapshot.projection.currentRun }
      : snapshot.projection.currentRun,
    pendingPermission: snapshot.projection.pendingPermission
      ? { ...snapshot.projection.pendingPermission }
      : snapshot.projection.pendingPermission,
    queue: snapshot.projection.queue.map((item) => ({ ...item })),
    subagents: snapshot.projection.subagents?.map((item) => ({ ...item })),
  }
  const timeline = new Map(snapshot.timeline.map((item) => [item.globalOffset, { ...item }]))
  const queue = new Map(projection.queue.map((item) => [item.queueItemId, item]))
  const queueContent = new Map(projection.queue.map((item) => [item.queueItemId, item.content]))
  const ordered = [...events]
    .filter((event) => event.globalOffset > snapshot.snapshotOffset)
    .sort((left, right) => left.globalOffset - right.globalOffset)
  const seenOffsets = new Set<number>()

  for (const event of ordered) {
    if (seenOffsets.has(event.globalOffset)) continue
    seenOffsets.add(event.globalOffset)
    projection.lastGlobalOffset = Math.max(projection.lastGlobalOffset, event.globalOffset)
    projection.streamVersion = Math.max(projection.streamVersion, event.streamSequence)
    applyProjectionEvent(projection, queue, queueContent, event)
    projectTimelineEvent(projection, timeline, queueContent, event)
  }

  projection.queue = [...queue.values()].sort(
    (left, right) =>
      left.createdGlobalOffset - right.createdGlobalOffset ||
      left.queueItemId.localeCompare(right.queueItemId),
  )
  return {
    projection,
    snapshotOffset: snapshot.snapshotOffset,
    timeline: [...timeline.values()].sort((left, right) => left.globalOffset - right.globalOffset),
  }
}

export function liveTimelineItems(snapshot: TaskSnapshot, events: TaskEventEnvelope[]) {
  return deriveLiveTaskSnapshot(snapshot, events).timeline
}

function applyProjectionEvent(
  projection: TaskProjection,
  queue: Map<TypedUlid, QueueItemProjection>,
  queueContent: Map<string, string>,
  event: TaskEventEnvelope,
) {
  const payload = record(event.payload)
  if (!payload) return
  switch (event.eventType) {
    case 'task.title_changed': {
      const title = stringValue(payload.title)
      if (title) projection.title = title
      return
    }
    case 'task.pinned':
      if (typeof payload.pinned === 'boolean') projection.pinned = payload.pinned
      return
    case 'task.archived':
      if (typeof payload.archived === 'boolean') projection.archived = payload.archived
      return
    case 'task.removed':
      if (typeof payload.removed === 'boolean') projection.removed = payload.removed
      return
    case 'task.actor_failed':
      projection.state = 'failed'
      if (projection.currentRun) {
        projection.currentRun = {
          ...projection.currentRun,
          endedAt: stringValue(payload.failedAt) ?? event.recordedAt,
          incompleteOutput: true,
          state: 'failed',
          terminalReason: 'failed',
        }
      }
      projection.pendingPermission = null
      return
    case 'run.started': {
      const segmentId = typedId(payload.segmentId)
      const startedAt = stringValue(payload.startedAt)
      if (!segmentId || !startedAt) return
      projection.currentRun = {
        incompleteOutput: false,
        segmentId,
        startedAt,
        state: 'running',
      }
      projection.pendingPermission = null
      projection.state = 'running'
      return
    }
    case 'run.completed': {
      const segmentId = typedId(payload.segmentId)
      const terminalReason = runTerminalReason(payload.terminalReason)
      if (!segmentId || !terminalReason || projection.currentRun?.segmentId !== segmentId) return
      const state = terminalState(terminalReason)
      projection.currentRun = {
        ...projection.currentRun,
        endedAt: stringValue(payload.endedAt) ?? event.recordedAt,
        incompleteOutput: Boolean(payload.incompleteOutput),
        state,
        terminalReason,
      }
      projection.pendingPermission = null
      projection.state = state
      return
    }
    case 'run.yield_requested':
      if (projection.currentRun) projection.currentRun.state = 'yielding'
      projection.state = 'yielding'
      return
    case 'run.safe_point_reached':
      if (payload.forced && projection.currentRun) {
        projection.currentRun.state = 'interrupted'
        projection.currentRun.incompleteOutput = Boolean(payload.incompleteOutput)
        projection.state = 'interrupted'
      }
      return
    case 'permission.requested':
      {
        const permission = permissionProjection(payload)
        if (!permission) return
        projection.pendingPermission = permission
        projection.state = 'waiting_permission'
        if (projection.currentRun) projection.currentRun.state = 'waiting_permission'
      }
      return
    case 'permission.resolved':
    case 'permission.invalidated':
      projection.pendingPermission = null
      if (projection.currentRun?.state === 'waiting_permission') {
        projection.currentRun.state = 'running'
        projection.state = 'running'
      }
      return
    case 'subagent.spawned': {
      const child = subagentProjection(record(payload.child))
      if (child) upsertSubagent(projection, child)
      return
    }
    case 'subagent.linked': {
      const actorId = typedId(payload.actorId)
      const contextCursor = numberValue(payload.contextCursor)
      const parent = record(payload.parent)
      const parentTaskId = typedId(parent?.parentTaskId)
      const parentSegmentId = typedId(parent?.parentSegmentId)
      const delegationId = typedId(parent?.delegationId)
      if (
        actorId &&
        contextCursor !== undefined &&
        parentTaskId &&
        parentSegmentId &&
        delegationId
      ) {
        projection.actorId = actorId
        projection.contextCursor = contextCursor
        projection.parent = { delegationId, parentSegmentId, parentTaskId }
      }
      return
    }
    case 'subagent.state_changed':
    case 'subagent.summary_updated':
    case 'subagent.backgrounded':
    case 'subagent.terminal': {
      const child = subagentProjection(payload)
      if (child) upsertSubagent(projection, child)
      return
    }
    default:
      applyQueueEvent(queue, queueContent, event, payload)
  }
}

function applyQueueEvent(
  queue: Map<TypedUlid, QueueItemProjection>,
  queueContent: Map<string, string>,
  event: TaskEventEnvelope,
  payload: Record<string, unknown>,
) {
  const queueItemId = typedId(payload.queueItemId)
  if (!queueItemId) return
  const current = queue.get(queueItemId)
  if (event.eventType === 'message.queued') {
    const content = stringValue(payload.content)
    const createdAt = stringValue(payload.createdAt)
    if (content === undefined || !createdAt) return
    const item: QueueItemProjection = {
      attachments: typedIdArray(payload.attachments),
      content,
      contextReferences: stringArray(payload.contextReferences),
      createdAt,
      createdGlobalOffset: event.globalOffset,
      queueItemId,
      revision: 1,
      state: 'queued',
    }
    queue.set(queueItemId, item)
    queueContent.set(queueItemId, content)
    return
  }
  if (!current) return
  const revision = numberValue(payload.revision)
  if (revision !== undefined && revision < current.revision) return
  if (event.eventType === 'message.edited') {
    const content = stringValue(payload.content)
    if (content === undefined || revision === undefined) return
    queue.set(queueItemId, {
      ...current,
      attachments: typedIdArray(payload.attachments),
      content,
      contextReferences: stringArray(payload.contextReferences),
      revision,
    })
    queueContent.set(queueItemId, content)
  } else if (event.eventType === 'message.promoted') {
    queue.set(queueItemId, { ...current, state: 'promoting' })
  } else if (event.eventType === 'message.recovered') {
    queue.set(queueItemId, { ...current, state: 'queued' })
  } else if (event.eventType === 'message.consumed' || event.eventType === 'message.deleted') {
    queue.delete(queueItemId)
  }
}

function projectTimelineEvent(
  projection: TaskProjection,
  timeline: Map<number, TimelineItemProjection>,
  queueContent: Map<string, string>,
  event: TaskEventEnvelope,
) {
  if (timeline.has(event.globalOffset)) return
  if (event.eventType.startsWith('engine.')) {
    projectEngineEvent(projection, timeline, event)
    return
  }
  const payload = record(event.payload)
  if (!payload || ignoredTaskTimelineEvent(event.eventType)) return
  const runSegmentId =
    typedId(payload.runSegmentId) ?? typedId(payload.segmentId) ?? childSegment(payload)
  const selected = taskTimelineDescription(event.eventType, payload, queueContent)
  if (!selected) return
  timeline.set(event.globalOffset, {
    ...selected,
    globalOffset: event.globalOffset,
    id: event.eventId,
    runSegmentId,
  })
}

function projectEngineEvent(
  projection: TaskProjection,
  timeline: Map<number, TimelineItemProjection>,
  envelope: TaskEventEnvelope,
) {
  const payload = record(envelope.payload)
  const event = record(payload?.event)
  const type = stringValue(event?.type)
  if (!event || !type) return
  const runSegmentId = projection.currentRun?.segmentId
  if (type === 'assistant_delta_produced') {
    const messageId = stringValue(event.message_id)
    const text = stringValue(record(event.delta)?.text)
    if (!messageId || !text) return
    timeline.set(envelope.globalOffset, {
      globalOffset: envelope.globalOffset,
      id: envelope.eventId,
      incomplete: true,
      kind: 'assistant_text',
      runSegmentId,
      semanticGroupId: messageId,
      summary: text,
    })
    return
  }
  if (type === 'assistant_message_completed') {
    const messageId = stringValue(event.message_id)
    if (!messageId) return
    const previous = [...timeline.values()]
      .reverse()
      .find((item) => item.kind === 'assistant_text' && item.semanticGroupId === messageId)
    if (previous) {
      previous.incomplete = false
      return
    }
    const text = messageContentText(event.content)
    if (!text) return
    timeline.set(envelope.globalOffset, {
      globalOffset: envelope.globalOffset,
      id: envelope.eventId,
      incomplete: false,
      kind: 'assistant_text',
      runSegmentId,
      semanticGroupId: messageId,
      summary: text,
    })
    return
  }
  const selected = engineTimelineDescription(type, event)
  if (!selected) return
  timeline.set(envelope.globalOffset, {
    ...selected,
    globalOffset: envelope.globalOffset,
    id: envelope.eventId,
    runSegmentId,
  })
}

function engineTimelineDescription(
  type: string,
  event: Record<string, unknown>,
): Pick<TimelineItemProjection, 'blobId' | 'incomplete' | 'kind' | 'summary'> | null {
  switch (type) {
    case 'assistant_notice':
      return description('notice', stringValue(event.body) ?? 'Assistant notice')
    case 'assistant_review_requested':
      return description('notice', stringValue(event.title) ?? 'Review requested')
    case 'assistant_clarification_requested':
      return description('notice', stringValue(event.prompt) ?? 'Clarification requested')
    case 'artifact_created':
    case 'artifact_updated': {
      const blob = record(event.blob_ref)
      return {
        ...description(
          artifactKind(stringValue(event.kind)),
          stringValue(event.title) ?? 'Artifact updated',
        ),
        blobId: typedId(blob?.id),
      }
    }
    case 'tool_use_requested':
      return description('tool_activity', `Using ${stringValue(event.tool_name) ?? 'tool'}`, true)
    case 'tool_use_started':
      return description('tool_activity', 'Tool started', true)
    case 'tool_use_denied':
      return description('tool_activity', 'Tool denied')
    case 'tool_use_completed':
      return description('tool_activity', 'Tool completed')
    case 'tool_use_failed':
      return description('error', stringValue(record(event.error)?.message) ?? 'Tool failed', true)
    case 'compaction_applied':
      return description('compaction', 'Context compacted')
    case 'unexpected_error':
      return description('error', stringValue(event.error) ?? 'Unexpected error', true)
    default:
      return null
  }
}

function taskTimelineDescription(
  eventType: string,
  payload: Record<string, unknown>,
  queueContent: Map<string, string>,
): Pick<TimelineItemProjection, 'incomplete' | 'kind' | 'summary'> | null {
  switch (eventType) {
    case 'message.consumed':
      return description(
        'user_message',
        queueContent.get(stringValue(payload.queueItemId) ?? '') ?? 'Message submitted',
      )
    case 'permission.invalidated':
      return description('permission', 'Permission expired after restart')
    case 'permission.requested':
      return description('permission', 'Permission requested')
    case 'permission.resolved':
      return description('permission', 'Permission resolved')
    case 'run.started':
      return description('notice', 'Run started')
    case 'run.completed':
      return description('notice', runTerminalSummary(payload), Boolean(payload.incompleteOutput))
    case 'run.force_stop_timed_out':
      return description('notice', 'Run force-stop timed out', true)
    case 'run.safe_point_reached':
      return description(
        'notice',
        payload.forced ? 'Run force-stopped' : 'Run safe point reached',
        Boolean(payload.incompleteOutput),
      )
    case 'run.yield_requested':
      return description(
        'notice',
        payload.force ? 'Run force-stop requested' : 'Run yield requested',
      )
    case 'task.actor_failed':
      return description('error', 'Task actor failed', true)
    case 'task.created':
      return description('notice', 'Task created')
    case 'task.title_changed':
      return description('notice', 'Task title changed')
    case 'task.archived':
      return description('notice', payload.archived ? 'Task archived' : 'Task restored')
    case 'tool.indeterminate':
      return description('tool_activity', 'Tool outcome is indeterminate after restart', true)
    case 'subagent.spawned':
      return description('subagent', 'Subagent started')
    case 'subagent.linked':
      return description('subagent', 'Subagent linked')
    case 'subagent.backgrounded':
      return description('subagent', 'Subagent continuing in background')
    case 'subagent.state_changed':
      return description('subagent', childSummary(payload) ?? 'Subagent state changed')
    case 'subagent.summary_updated':
      return description('subagent', childSummary(payload) ?? 'Subagent summary updated')
    case 'subagent.terminal':
      return description(
        'subagent',
        childSummary(payload) ?? 'Subagent finished',
        childState(payload) === 'failed',
      )
    case 'workspace.acquired':
      return description('notice', 'Workspace acquired')
    case 'workspace.preparing':
      return description('notice', 'Workspace preparing')
    case 'workspace.waiting':
      return description('notice', 'Workspace lease waiting')
    case 'workspace.released':
      return description('notice', 'Workspace released')
    case 'workspace.cleanup_blocked':
      return description('notice', 'Workspace cleanup blocked')
    case 'workspace.cleanup_pending':
      return description('notice', 'Workspace cleanup pending')
    case 'workspace.override_applied':
      return description('notice', 'Workspace write override applied')
    default:
      return null
  }
}

function description(
  kind: TimelineEventKind,
  summary: string,
  incomplete = false,
): Pick<TimelineItemProjection, 'incomplete' | 'kind' | 'summary'> {
  return { incomplete, kind, summary }
}

function ignoredTaskTimelineEvent(eventType: string) {
  return [
    'message.queued',
    'message.edited',
    'message.promoted',
    'message.deleted',
    'message.recovered',
  ].includes(eventType)
}

function messageContentText(value: unknown) {
  const content = record(value)
  const text = stringValue(content?.text)
  if (text !== undefined) return text
  const parts = Array.isArray(content?.multimodal) ? content.multimodal : []
  return parts.map((part) => stringValue(record(part)?.text) ?? '').join('')
}

function artifactKind(kind?: string): TimelineEventKind {
  if (kind === 'image' || kind === 'screenshot') return 'image'
  if (kind === 'command' || kind === 'terminal') return 'command'
  return 'diff'
}

function runTerminalSummary(payload: Record<string, unknown>) {
  const summaries: Record<string, string> = {
    cancelled: 'Run cancelled',
    completed: 'Run completed',
    failed: 'Run failed',
    forced_interruption: 'Run force-stopped',
    interrupted_by_restart: 'Run interrupted by restart',
    superseded: 'Run superseded',
  }
  return summaries[stringValue(payload.terminalReason) ?? ''] ?? 'Run completed'
}

function terminalState(reason: RunTerminalReason): Extract<TaskState, RunProjection['state']> {
  if (reason === 'completed') return 'completed'
  if (reason === 'failed') return 'failed'
  return 'interrupted'
}

function runTerminalReason(value: unknown): RunTerminalReason | undefined {
  return [
    'cancelled',
    'completed',
    'failed',
    'forced_interruption',
    'interrupted_by_restart',
    'superseded',
  ].includes(String(value))
    ? (value as RunTerminalReason)
    : undefined
}

function permissionProjection(value: Record<string, unknown>): PermissionProjection | null {
  const requestId = typedId(value.requestId)
  const revision = numberValue(value.revision)
  const route = value.route
  if (
    !requestId ||
    revision === undefined ||
    (route !== 'foreground_task' && route !== 'saved_policy')
  )
    return null
  return {
    details: record(value.details) as PermissionProjection['details'],
    requestId,
    revision,
    route,
  }
}

function subagentProjection(value: Record<string, unknown> | null): SubagentProjection | null {
  if (!value) return null
  const actorId = typedId(value.actorId)
  const childTaskId = typedId(value.childTaskId)
  const contextCursor = numberValue(value.contextCursor)
  const delegationId = typedId(value.delegationId)
  const parentSegmentId = typedId(value.parentSegmentId)
  const parentTaskId = typedId(value.parentTaskId)
  const segmentId = typedId(value.segmentId)
  const startedAt = stringValue(value.startedAt)
  const state = stringValue(value.state)
  if (
    !actorId ||
    !childTaskId ||
    contextCursor === undefined ||
    !delegationId ||
    typeof value.detached !== 'boolean' ||
    !parentSegmentId ||
    !parentTaskId ||
    !segmentId ||
    !startedAt ||
    !state ||
    !['background', 'cancelled', 'completed', 'failed', 'running', 'starting', 'yielding'].includes(
      state,
    )
  )
    return null
  return {
    actorId,
    childTaskId,
    contextCursor,
    delegationId,
    detached: value.detached,
    endedAt: stringValue(value.endedAt),
    parentSegmentId,
    parentTaskId,
    segmentId,
    startedAt,
    state: state as SubagentProjection['state'],
    summary: stringValue(value.summary),
    workspaceLeaseId: typedId(value.workspaceLeaseId),
  }
}

function upsertSubagent(projection: TaskProjection, child: SubagentProjection) {
  const subagents = projection.subagents ?? []
  const index = subagents.findIndex((item) => item.childTaskId === child.childTaskId)
  if (index < 0) subagents.push(child)
  else subagents[index] = child
  projection.subagents = subagents
}

function childRecord(payload: Record<string, unknown>) {
  return record(payload.child) ?? payload
}

function childSegment(payload: Record<string, unknown>) {
  return typedId(childRecord(payload).segmentId)
}

function childState(payload: Record<string, unknown>) {
  return stringValue(childRecord(payload).state)
}

function childSummary(payload: Record<string, unknown>) {
  return stringValue(childRecord(payload).summary) ?? childState(payload)
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function stringValue(value: unknown) {
  return typeof value === 'string' ? value : undefined
}

function typedId(value: unknown) {
  return stringValue(value) as TypedUlid | undefined
}

function numberValue(value: unknown) {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : undefined
}

function stringArray(value: unknown) {
  return Array.isArray(value) && value.every((item) => typeof item === 'string')
    ? (value as string[])
    : []
}

function typedIdArray(value: unknown) {
  return stringArray(value) as TypedUlid[]
}
