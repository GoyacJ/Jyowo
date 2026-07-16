import type {
  ConversationContextReference,
  PermissionProjection,
  QueueItemProjection,
  RunProjection,
  RunTerminalReason,
  SubagentProjection,
  TaskEventEnvelope,
  TaskProjection,
  TaskState,
  TimelineArtifactProjection,
  TimelineContentBlock,
  TimelineEventKind,
  TimelineItemProjection,
  TimelineToolOperation,
  TimelineToolProjection,
  TimelineToolStatus,
  TypedUlid,
} from '@/generated/daemon-protocol'

import type { TaskSnapshot } from './task-store'

type SkillContextReference = Extract<ConversationContextReference, { kind: 'skill' }>

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
  const queueAttachments = new Map(
    projection.queue.map((item) => [item.queueItemId, [...item.attachments]]),
  )
  const ordered = [...events]
    .filter((event) => event.globalOffset > snapshot.snapshotOffset)
    .sort((left, right) => left.globalOffset - right.globalOffset)
  const seenOffsets = new Set<number>()

  for (const event of ordered) {
    if (seenOffsets.has(event.globalOffset)) continue
    seenOffsets.add(event.globalOffset)
    projection.lastGlobalOffset = Math.max(projection.lastGlobalOffset, event.globalOffset)
    projection.streamVersion = Math.max(projection.streamVersion, event.streamSequence)
    applyProjectionEvent(projection, queue, queueContent, queueAttachments, event)
    projectTimelineEvent(timeline, queueContent, queueAttachments, event)
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
  queueAttachments: Map<string, TypedUlid[]>,
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
    case 'task.actor_failed': {
      const segmentId = typedId(payload.segmentId)
      const activeRun =
        segmentId &&
        projection.currentRun?.segmentId === segmentId &&
        ['running', 'waiting_permission', 'yielding'].includes(projection.currentRun.state)
          ? projection.currentRun
          : null
      const wasYielding = activeRun?.state === 'yielding'
      projection.state = 'failed'
      if (activeRun) {
        projection.currentRun = {
          ...activeRun,
          endedAt: stringValue(payload.failedAt) ?? event.recordedAt,
          incompleteOutput: true,
          state: 'failed',
          terminalReason: 'failed',
        }
      }
      if (wasYielding) {
        for (const [queueItemId, item] of queue) {
          if (item.state === 'promoting') queue.set(queueItemId, { ...item, state: 'queued' })
        }
      }
      projection.pendingPermission = null
      return
    }
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
      projection.state = 'running'
      return
    case 'run.safe_point_reached':
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
      } else if (!projection.currentRun) {
        projection.state = 'idle'
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
      applyQueueEvent(queue, queueContent, queueAttachments, event, payload)
  }
}

function applyQueueEvent(
  queue: Map<TypedUlid, QueueItemProjection>,
  queueContent: Map<string, string>,
  queueAttachments: Map<string, TypedUlid[]>,
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
      contextReferences: contextReferenceArray(payload.contextReferences),
      createdAt,
      createdGlobalOffset: event.globalOffset,
      queueItemId,
      revision: 1,
      state: 'queued',
    }
    queue.set(queueItemId, item)
    queueContent.set(queueItemId, content)
    queueAttachments.set(queueItemId, [...item.attachments])
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
      contextReferences: contextReferenceArray(payload.contextReferences),
      revision,
    })
    queueContent.set(queueItemId, content)
    queueAttachments.set(queueItemId, typedIdArray(payload.attachments))
  } else if (event.eventType === 'message.promoted') {
    queue.set(queueItemId, { ...current, state: 'promoting' })
  } else if (event.eventType === 'message.recovered') {
    queue.set(queueItemId, { ...current, state: 'queued' })
  } else if (event.eventType === 'message.consumed' || event.eventType === 'message.deleted') {
    queue.delete(queueItemId)
  }
}

function projectTimelineEvent(
  timeline: Map<number, TimelineItemProjection>,
  queueContent: Map<string, string>,
  queueAttachments: Map<string, TypedUlid[]>,
  event: TaskEventEnvelope,
) {
  if (timeline.has(event.globalOffset)) return
  if (event.eventType.startsWith('engine.')) {
    projectEngineEvent(timeline, event)
    return
  }
  const payload = record(event.payload)
  if (!payload || ignoredTaskTimelineEvent(event.eventType)) return
  const runSegmentId =
    typedId(payload.runSegmentId) ?? typedId(payload.segmentId) ?? childSegment(payload)
  const selected = taskTimelineDescription(event.eventType, payload, queueContent, queueAttachments)
  if (!selected) return
  timeline.set(event.globalOffset, {
    ...selected,
    globalOffset: event.globalOffset,
    id: event.eventId,
    runSegmentId,
  })
}

function projectEngineEvent(
  timeline: Map<number, TimelineItemProjection>,
  envelope: TaskEventEnvelope,
) {
  const payload = record(envelope.payload)
  const event = record(payload?.event)
  const type = stringValue(event?.type)
  if (!event || !type) return
  const runSegmentId = typedId(payload?.runSegmentId)
  if (!runSegmentId) return
  if (type === 'assistant_delta_produced') {
    const messageId = stringValue(event.message_id)
    const text = stringValue(record(event.delta)?.text)
    if (!messageId || !text) return
    timeline.set(envelope.globalOffset, {
      contentBlocks: [{ format: 'markdown', text, type: 'text' }],
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
    const contentBlocks = messageContentBlocks(event.content)
    if (contentBlocks.length === 0) return
    const previous = [...timeline.values()]
      .reverse()
      .find(
        (item) =>
          item.kind === 'assistant_text' &&
          item.runSegmentId === runSegmentId &&
          item.semanticGroupId === messageId,
      )
    if (previous) {
      previous.incomplete = false
      previous.contentBlocks = contentBlocks
      previous.summary = contentBlocksSummary(contentBlocks)
      previous.blobId = firstArtifactBlobId(contentBlocks)
      return
    }
    timeline.set(envelope.globalOffset, {
      blobId: firstArtifactBlobId(contentBlocks),
      contentBlocks,
      globalOffset: envelope.globalOffset,
      id: envelope.eventId,
      incomplete: false,
      kind: 'assistant_text',
      runSegmentId,
      semanticGroupId: messageId,
      summary: contentBlocksSummary(contentBlocks),
    })
    return
  }
  if (type.startsWith('tool_use_')) {
    projectToolTimelineEvent(timeline, envelope, runSegmentId, type, event)
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
): Pick<
  TimelineItemProjection,
  'blobId' | 'contentBlocks' | 'incomplete' | 'kind' | 'summary'
> | null {
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
      const title = stringValue(event.title) ?? 'Artifact updated'
      const kind = stringValue(event.kind)
      const artifact = timelineArtifactProjection(event, blob, title, kind)
      return {
        ...description(artifactKind(kind), title),
        blobId: typedId(blob?.id),
        contentBlocks: [{ artifact, type: 'artifact' }],
      }
    }
    case 'compaction_applied':
      return description('compaction', 'Context compacted')
    case 'unexpected_error':
      return description('error', stringValue(event.error) ?? 'Unexpected error', true)
    default:
      return null
  }
}

function projectToolTimelineEvent(
  timeline: Map<number, TimelineItemProjection>,
  envelope: TaskEventEnvelope,
  runSegmentId: TypedUlid,
  type: string,
  event: Record<string, unknown>,
) {
  const toolUseId = typedId(event.tool_use_id)
  if (!toolUseId) return
  if (type === 'tool_use_requested') {
    const toolName = stringValue(event.tool_name) ?? 'tool'
    const operation = timelineToolOperation(toolName)
    const tool: TimelineToolProjection = {
      command: timelineToolCommand(operation, event.input),
      operation,
      status: 'requested',
      subject: timelineToolSubject(operation, event.input),
      toolName,
      toolUseId,
    }
    timeline.set(envelope.globalOffset, {
      contentBlocks: [{ activity: tool, type: 'tool_activity' }],
      globalOffset: envelope.globalOffset,
      id: envelope.eventId,
      incomplete: true,
      kind: 'tool_activity',
      runSegmentId,
      semanticGroupId: toolUseId,
      summary: timelineToolSummary(tool),
      tool,
    })
    return
  }

  const status = timelineToolStatus(type)
  if (!status) return
  const previous = [...timeline.values()]
    .reverse()
    .find((item) => item.tool?.toolUseId === toolUseId)
  const resultSummary =
    type === 'tool_use_failed'
      ? boundedToolText(stringValue(record(event.error)?.message) ?? 'Tool failed')
      : type === 'tool_use_completed'
        ? toolResultSummary(event.result)
        : undefined
  const resultOutput =
    type === 'tool_use_failed'
      ? boundedToolPreview(stringValue(record(event.error)?.message) ?? 'Tool failed')
      : type === 'tool_use_completed'
        ? toolResultPreview(event.result)
        : undefined
  const durationMs = type === 'tool_use_completed' ? numberValue(event.duration_ms) : undefined

  if (!previous?.tool) {
    const tool: TimelineToolProjection = {
      durationMs,
      operation: 'other',
      resultSummary,
      status,
      toolName: 'tool',
      toolUseId,
    }
    timeline.set(envelope.globalOffset, {
      contentBlocks: [{ activity: tool, type: 'tool_activity' }],
      globalOffset: envelope.globalOffset,
      id: envelope.eventId,
      incomplete: status === 'requested' || status === 'running',
      kind: 'tool_activity',
      runSegmentId,
      semanticGroupId: toolUseId,
      summary: timelineToolSummary(tool),
      tool,
    })
    return
  }

  previous.tool = {
    ...previous.tool,
    durationMs: durationMs ?? previous.tool.durationMs,
    output:
      previous.tool.operation === 'command'
        ? (resultOutput ?? previous.tool.output)
        : previous.tool.output,
    resultSummary: resultSummary ?? previous.tool.resultSummary,
    status,
  }
  previous.incomplete = status === 'requested' || status === 'running'
  previous.summary = timelineToolSummary(previous.tool)
  previous.contentBlocks = [{ activity: previous.tool, type: 'tool_activity' }]
}

function timelineToolStatus(type: string): TimelineToolStatus | undefined {
  const statuses: Record<string, TimelineToolStatus> = {
    tool_use_completed: 'completed',
    tool_use_denied: 'denied',
    tool_use_failed: 'failed',
    tool_use_started: 'running',
  }
  return statuses[type]
}

function timelineToolOperation(toolName: string): TimelineToolOperation {
  const name = toolName.toLowerCase()
  if (name.includes('edit') || name.includes('write') || name.includes('patch')) return 'edit'
  if (name.includes('read') || name.includes('load')) return 'read'
  if (
    name.includes('search') ||
    name.includes('find') ||
    name.includes('grep') ||
    name === 'rg' ||
    name.includes('glob')
  )
    return 'search'
  if (
    name.includes('exec') ||
    name.includes('command') ||
    name.includes('shell') ||
    name.includes('terminal') ||
    name === 'bash'
  )
    return 'command'
  if (name.includes('browser') || name.includes('fetch') || name.includes('web')) return 'browse'
  if (name.includes('image') || name.includes('generate')) return 'generate'
  if (name.includes('agent') || name.includes('delegate') || name.includes('spawn')) {
    return 'delegate'
  }
  return 'other'
}

function timelineToolSubject(operation: TimelineToolOperation, input: unknown) {
  const value = record(input)
  if (!value) return undefined
  const keys =
    operation === 'read' || operation === 'edit'
      ? ['path', 'file_path', 'filePath', 'filename']
      : operation === 'generate' || operation === 'delegate'
        ? ['name', 'target', 'output_path', 'outputPath']
        : []
  const subject = keys.map((key) => stringValue(value[key])).find(Boolean)
  if (!subject) return undefined
  const parts = subject.replaceAll('\\', '/').split('/').filter(Boolean)
  return boundedToolText(parts.slice(-2).join('/'))
}

function timelineToolCommand(operation: TimelineToolOperation, input: unknown) {
  if (operation !== 'command') return undefined
  const value = record(input)
  if (!value) return undefined
  const command = ['cmd', 'command', 'script']
    .map((key) => stringValue(value[key]))
    .find((entry) => entry !== undefined)
  return command === undefined ? undefined : boundedToolPreview(command)
}

function timelineToolSummary(tool: TimelineToolProjection) {
  const actions: Record<TimelineToolStatus, Partial<Record<TimelineToolOperation, string>>> = {
    completed: {
      browse: 'Browsed',
      command: 'Ran command',
      delegate: 'Delegated',
      edit: 'Edited',
      generate: 'Generated',
      other: 'Used tool',
      read: 'Read',
      search: 'Searched',
    },
    denied: {},
    failed: {},
    requested: {},
    running: {},
  }
  const activeActions: Record<TimelineToolOperation, string> = {
    browse: 'Browsing',
    command: 'Running command',
    delegate: 'Delegating',
    edit: 'Editing',
    generate: 'Generating',
    other: 'Using tool',
    read: 'Reading',
    search: 'Searching',
  }
  const action =
    tool.status === 'denied'
      ? 'Tool denied'
      : tool.status === 'failed'
        ? 'Tool failed'
        : (actions[tool.status][tool.operation] ?? activeActions[tool.operation])
  const subject = tool.subject ?? (tool.operation === 'other' ? tool.toolName : undefined)
  return subject ? `${action} ${subject}` : action
}

function toolResultSummary(value: unknown) {
  const result = record(value)
  if (!result) return 'Result received'
  const text = stringValue(result.text)
  if (text !== undefined) return `${Math.max(1, text.split(/\r?\n/).length)} lines returned`
  if ('structured' in result) return 'Structured result'
  const blob = record(result.blob)
  if (blob) return stringValue(blob.content_type) ?? 'Blob result'
  const mixed = Array.isArray(result.mixed) ? result.mixed : null
  if (mixed) return `${mixed.length} result parts`
  return 'Result received'
}

function toolResultPreview(value: unknown) {
  const result = record(value)
  if (!result) return undefined
  const text = stringValue(result.text)
  if (text !== undefined) return boundedToolPreview(text)
  if ('structured' in result) {
    return boundedToolPreview(JSON.stringify(result.structured, null, 2))
  }
  const mixed = Array.isArray(result.mixed) ? result.mixed : null
  if (!mixed) return undefined
  const parts = mixed
    .map((part) => toolResultPartPreview(part))
    .filter((part): part is string => Boolean(part))
  return parts.length > 0 ? boundedToolPreview(parts.join('\n')) : undefined
}

function toolResultPartPreview(value: unknown) {
  const part = record(value)
  if (!part) return undefined
  const text = stringValue(part.text)
  if (text !== undefined) return text
  if ('value' in part) return JSON.stringify(part.value, null, 2)
  return (
    stringValue(part.summary) ??
    stringValue(part.detail) ??
    stringValue(part.message) ??
    stringValue(part.preview) ??
    stringValue(part.title) ??
    stringValue(part.caption)
  )
}

function boundedToolText(value: string) {
  return [...value].slice(0, 160).join('')
}

function boundedToolPreview(value: string) {
  const chars = [...value]
  return chars.length > 8_000 ? `${chars.slice(0, 8_000).join('')}…` : value
}

function taskTimelineDescription(
  eventType: string,
  payload: Record<string, unknown>,
  queueContent: Map<string, string>,
  queueAttachments: Map<string, TypedUlid[]>,
): Pick<
  TimelineItemProjection,
  'blobId' | 'contentBlocks' | 'incomplete' | 'kind' | 'summary'
> | null {
  switch (eventType) {
    case 'message.consumed': {
      const queueItemId = stringValue(payload.queueItemId) ?? ''
      const summary = queueContent.get(queueItemId) ?? 'Message submitted'
      const attachments = queueAttachments.get(queueItemId) ?? []
      return {
        ...description('user_message', summary),
        blobId: attachments[0],
        contentBlocks: userMessageContentBlocks(summary, attachments),
      }
    }
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
    case 'task.pinned':
      return description('notice', payload.pinned ? 'Task pinned' : 'Task unpinned')
    case 'task.removed':
      return description('notice', payload.removed ? 'Task removed' : 'Task restored')
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
): Pick<TimelineItemProjection, 'contentBlocks' | 'incomplete' | 'kind' | 'summary'> {
  return { contentBlocks: defaultContentBlocks(kind, summary), incomplete, kind, summary }
}

function defaultContentBlocks(kind: TimelineEventKind, summary: string): TimelineContentBlock[] {
  if (kind === 'assistant_text' || kind === 'user_message') {
    return [
      {
        format: kind === 'assistant_text' ? 'markdown' : 'plain',
        text: summary,
        type: 'text',
      },
    ]
  }
  return [
    {
      level: kind === 'error' ? 'error' : kind === 'permission' ? 'warning' : 'info',
      text: summary,
      type: 'notice',
    },
  ]
}

function userMessageContentBlocks(
  summary: string,
  attachments: TypedUlid[],
): TimelineContentBlock[] {
  return [
    { format: 'plain', text: summary, type: 'text' },
    ...attachments.map(
      (blobId, index): TimelineContentBlock => ({
        artifact: {
          artifactKind: 'file',
          blobId,
          mediaType: 'application/octet-stream',
          presentation: { preferredSurface: 'card' },
          title: attachments.length === 1 ? summary : `Attachment ${index + 1}`,
        },
        type: 'artifact',
      }),
    ),
  ]
}

function timelineArtifactProjection(
  event: Record<string, unknown>,
  blob: Record<string, unknown> | null,
  title: string,
  artifactKindValue?: string,
): TimelineArtifactProjection {
  const mediaType = stringValue(blob?.content_type) ?? 'application/octet-stream'
  const artifactKind = artifactKindValue?.toLowerCase()
  return {
    artifactId: stringValue(event.artifact_id),
    artifactKind,
    blobId: typedId(blob?.id),
    mediaType,
    presentation: {
      preferredSurface: prefersInlineArtifact(artifactKind, mediaType) ? 'inline' : 'card',
    },
    preview: stringValue(event.preview),
    size: numberValue(blob?.size),
    title,
  }
}

function prefersInlineArtifact(artifactKind: string | undefined, mediaType: string) {
  return (
    ['audio', 'image', 'map', 'screenshot', 'video'].includes(artifactKind ?? '') ||
    /^(audio|image|video)\//i.test(mediaType) ||
    /geo(\+)?json/i.test(mediaType)
  )
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

function messageContentBlocks(value: unknown): TimelineContentBlock[] {
  const content = record(value)
  const text = stringValue(content?.text)
  if (text !== undefined) {
    return text ? [{ format: 'markdown', text, type: 'text' }] : []
  }
  if (content && 'structured' in content) {
    const structured = JSON.stringify(content.structured)
    return structured === undefined ? [] : [{ format: 'plain', text: structured, type: 'text' }]
  }
  const parts = Array.isArray(content?.multimodal) ? content.multimodal : []
  return parts.flatMap((part): TimelineContentBlock[] => {
    const value = record(part)
    const partText = stringValue(value?.text)
    if (partText !== undefined) {
      return partText ? [{ format: 'markdown', text: partText, type: 'text' }] : []
    }
    for (const [key, title, kind] of [
      ['image', 'Image', 'image'],
      ['video', 'Video', 'video'],
      ['file', 'File', 'file'],
    ] as const) {
      const media = record(value?.[key])
      const blob = record(media?.blob_ref)
      const blobId = typedId(blob?.id)
      if (!media || !blob || !blobId) continue
      const mediaType =
        stringValue(media.mime_type)?.trim() ||
        stringValue(blob.content_type) ||
        'application/octet-stream'
      return [
        {
          artifact: {
            artifactKind: kind,
            blobId,
            mediaType,
            presentation: {
              preferredSurface: prefersInlineArtifact(kind, mediaType) ? 'inline' : 'card',
            },
            size: numberValue(blob.size),
            title,
          },
          type: 'artifact',
        },
      ]
    }
    return []
  })
}

function contentBlocksSummary(blocks: TimelineContentBlock[]) {
  const text = blocks
    .filter(
      (block): block is Extract<TimelineContentBlock, { type: 'text' }> => block.type === 'text',
    )
    .map((block) => block.text)
    .join('')
  if (text) return text
  return (
    blocks.find((block) => block.type === 'artifact')?.artifact.title ??
    blocks.find((block) => block.type === 'notice')?.text ??
    ''
  )
}

function firstArtifactBlobId(blocks: TimelineContentBlock[]) {
  return blocks.find((block) => block.type === 'artifact')?.artifact.blobId ?? undefined
}

function artifactKind(kind?: string): TimelineEventKind {
  if (kind === 'image' || kind === 'screenshot') return 'image'
  if (kind === 'command' || kind === 'terminal') return 'command'
  if (kind === 'diff' || kind === 'patch') return 'diff'
  if (kind === 'file') return 'file'
  return 'artifact'
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

function contextReferenceArray(value: unknown): ConversationContextReference[] {
  if (!Array.isArray(value)) return []
  const references: ConversationContextReference[] = []
  for (const candidate of value) {
    if (typeof candidate === 'string') {
      references.push({ kind: 'workspace_file', label: candidate, path: candidate })
      continue
    }
    const reference = contextReference(candidate)
    if (!reference) return []
    references.push(reference)
  }
  return references
}

function contextReference(value: unknown): ConversationContextReference | null {
  const candidate = record(value)
  if (!candidate) return null
  const kind = stringValue(candidate.kind)
  const label = stringValue(candidate.label)
  if (!kind || !label) return null
  if (kind === 'workspace_file') {
    const path = stringValue(candidate.path)
    return path ? { kind, label, path } : null
  }
  if (kind === 'skill') {
    const skillId = stringValue(candidate.skillId)
    const version = candidate.version === undefined ? 1 : numberValue(candidate.version)
    const parameters = candidate.parameters === undefined ? {} : record(candidate.parameters)
    const source = skillSource(candidate.source)
    if (
      !skillId ||
      version !== 1 ||
      !parameters ||
      (candidate.source !== undefined && source === undefined)
    ) {
      return null
    }
    return { kind, label, parameters, skillId, source, version }
  }
  const id = stringValue(candidate.id)
  if (!id) return null
  if (kind === 'artifact' || kind === 'conversation' || kind === 'tool' || kind === 'mcp_server') {
    return { id, kind, label }
  }
  if (kind === 'memory') {
    const resolved_content = stringValue(candidate.resolved_content)
    return { id, kind, label, resolved_content }
  }
  return null
}

function skillSource(value: unknown): SkillContextReference['source'] {
  if (value === undefined || value === null) return value
  if (value === 'bundled' || value === 'workspace' || value === 'user') return value
  const source = record(value)
  if (!source || Object.keys(source).length !== 1) return undefined
  const plugin = stringValue(source.plugin)
  if (plugin) return { plugin }
  const mcp = stringValue(source.mcp)
  return mcp ? { mcp } : undefined
}

function typedIdArray(value: unknown) {
  return stringArray(value) as TypedUlid[]
}
