import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type {
  TaskEventEnvelope,
  TimelineItemProjection,
  TypedUlid,
} from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { useUiStore } from '@/shared/state/ui-store'
import type { TaskWorkbenchPanel } from '@/shared/state/workbench-selection'
import type {
  ConversationModelCapability,
  ListReferenceCandidatesResponse,
  PermissionMode,
} from '@/shared/tauri/commands'
import { pickAttachmentPath } from '@/shared/tauri/file-dialog'
import { useCommandClient, useDaemonClient } from '@/shared/tauri/react'
import { PendingPermissionDecision } from './PendingPermissionDecision'
import { QueuedMessages } from './queue/QueuedMessages'
import { RunStatusBar } from './RunStatusBar'
import { TaskComposer } from './TaskComposer'
import type { TaskConnectionState, TaskSnapshot } from './task-store'
import { TaskTimeline } from './timeline/TaskTimeline'
import { useTask } from './use-task'
import { useTaskCommandExecutor } from './use-task-command-executor'
import { TaskWorkbench } from './workbench/TaskWorkbench'

export function TaskWorkspace({ taskId }: { taskId: TypedUlid }) {
  const task = useTask(taskId)
  const daemonClient = useDaemonClient()
  const commandClient = useCommandClient()
  const providerSettings = useQuery({
    queryFn: () => commandClient.listProviderSettings(),
    queryKey: ['task-model-configs'],
  }).data
  const executionSettings = useQuery({
    queryFn: () => commandClient.getExecutionSettings(),
    queryKey: ['task-execution-settings'],
  }).data
  const [modelOverride, setModelOverride] = useState<{ taskId: TypedUlid; value: string } | null>(
    null,
  )
  const [permissionOverride, setPermissionOverride] = useState<{
    taskId: TypedUlid
    value: PermissionMode
  } | null>(null)
  const configuredModels = providerSettings?.configs.filter((config) => config.hasApiKey) ?? []
  const modelConfigId =
    modelOverride?.taskId === taskId
      ? modelOverride.value
      : (providerSettings?.defaultConfigId ?? undefined)
  const selectedModel = configuredModels.find((config) => config.id === modelConfigId)
  const permissionMode =
    permissionOverride?.taskId === taskId
      ? permissionOverride.value
      : executionSettings?.permissionMode
  return (
    <TaskWorkspaceView
      client={daemonClient}
      connectionError={task.connectionError}
      connectionState={task.connectionState}
      events={task.events}
      modelCapability={selectedModel?.modelDescriptor.conversationCapability ?? null}
      modelConfigId={modelConfigId}
      modelConfigs={configuredModels.map((config) => ({
        id: config.id,
        label: `${config.displayName} / ${config.modelId}${
          config.id === providerSettings?.defaultConfigId ? ' (default)' : ''
        }`,
      }))}
      onListReferenceCandidates={() => daemonClient.listReferenceCandidates(taskId)}
      onModelConfigChange={(value) => setModelOverride({ taskId, value })}
      onPickAttachmentPath={pickAttachmentPath}
      onPermissionModeChange={(value) => setPermissionOverride({ taskId, value })}
      permissionMode={permissionMode}
      snapshot={task.snapshot}
    />
  )
}

export function TaskWorkspaceView({
  connectionError,
  connectionState,
  client,
  events = [],
  modelCapability,
  modelConfigId,
  modelConfigs,
  onListReferenceCandidates,
  onModelConfigChange,
  onPickAttachmentPath,
  onPermissionModeChange,
  permissionMode,
  snapshot,
}: {
  client?: Pick<DaemonClient, 'connect' | 'request'> &
    Partial<Pick<DaemonClient, 'readBlob' | 'stageBlobFromPath'>>
  connectionError?: string | null
  connectionState: TaskConnectionState
  events?: TaskEventEnvelope[]
  modelCapability?: ConversationModelCapability | null
  modelConfigId?: string
  modelConfigs?: Array<{ id: string; label: string }>
  onListReferenceCandidates?: () => Promise<ListReferenceCandidatesResponse>
  onModelConfigChange?: (modelConfigId: string) => void
  onPickAttachmentPath?: Parameters<typeof TaskComposer>[0]['onPickAttachmentPath']
  onPermissionModeChange?: (mode: PermissionMode) => void
  permissionMode?: PermissionMode
  snapshot: TaskSnapshot | null
}) {
  const { t } = useTranslation('shell')
  const snapshotTaskId = snapshot?.projection.taskId ?? null
  const workbenchMode = useUiStore((state) => state.taskWorkbenchMode)
  const workbenchSelection = useUiStore((state) => state.taskWorkbenchSelection)
  const setWorkbenchMode = useUiStore((state) => state.setTaskWorkbenchMode)
  const setWorkbenchSelection = useUiStore((state) => state.setTaskWorkbenchSelection)
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
  const executeCommand = useTaskCommandExecutor({
    client: client ?? null,
    expectedStreamVersion: commandStreamVersion,
    onCommandAccepted: commandAccepted,
    taskId: snapshotTaskId,
  })
  useEffect(() => {
    if (workbenchSelection && workbenchSelection.taskId !== snapshotTaskId) {
      setWorkbenchSelection(null)
    }
  }, [setWorkbenchSelection, snapshotTaskId, workbenchSelection])

  if (connectionState === 'protocol_error') {
    return (
      <div className="grid h-full place-items-center">
        <div
          className="max-w-md rounded-xl border border-destructive/30 bg-destructive/5 px-5 py-4 text-destructive text-sm"
          role="alert"
        >
          {connectionError ?? t('sidebar.unavailable')}
        </div>
      </div>
    )
  }

  if (!snapshot) {
    return (
      <div className="grid h-full place-items-center text-muted-foreground text-sm" role="status">
        {connectionState === 'disconnected'
          ? t('sidebar.unavailable')
          : t('sidebar.loadingConversation')}
      </div>
    )
  }

  const items = timelineItems(snapshot, events)
  const queue = queueItems(snapshot, events)
  const taskId = snapshot.projection.taskId
  const showWorkbench =
    workbenchMode !== 'closed' && workbenchSelection?.taskId === taskId && client?.readBlob

  function selectTimelineItem(item: TimelineItemProjection) {
    const panel = workbenchPanel(item)
    if (!panel) return
    setWorkbenchSelection({
      blobId: item.blobId ?? undefined,
      eventId: item.id,
      panel,
      segmentId: item.runSegmentId ?? undefined,
      taskId,
    })
    if (workbenchMode === 'closed') setWorkbenchMode('inspector')
  }

  return (
    <section className="task-workspace-container flex h-full min-h-0 w-full flex-col">
      <div className="task-workspace-layout relative flex min-h-0 flex-1 flex-col overflow-y-auto">
        <div
          className="task-reading-column mx-auto flex h-full min-w-0 w-full max-w-[820px] shrink-0 flex-col"
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
            <TaskTimeline
              currentRun={snapshot.projection.currentRun}
              items={items}
              onSelectItem={selectTimelineItem}
            />
          </div>
          {client ? (
            <div className="shrink-0 border-border/70 border-t bg-background/95 px-1 pt-3 pb-1 backdrop-blur-sm">
              {snapshot.projection.pendingPermission && executeCommand ? (
                <PendingPermissionDecision
                  executeCommand={executeCommand}
                  key={`${snapshot.projection.pendingPermission.requestId}:${snapshot.projection.pendingPermission.revision}`}
                  permission={snapshot.projection.pendingPermission}
                  taskId={snapshot.projection.taskId}
                />
              ) : null}
              <QueuedMessages
                client={client}
                expectedStreamVersion={commandStreamVersion}
                executeCommand={executeCommand}
                items={queue}
                onCommandAccepted={commandAccepted}
                taskId={snapshot.projection.taskId}
              />
              <TaskComposer
                client={client}
                connectionState={connectionState}
                executeCommand={executeCommand}
                modelCapability={modelCapability}
                modelConfigId={modelConfigId}
                modelConfigs={modelConfigs}
                onCommandAccepted={commandAccepted}
                onListReferenceCandidates={onListReferenceCandidates}
                onModelConfigChange={onModelConfigChange}
                onPickAttachmentPath={onPickAttachmentPath}
                onPermissionModeChange={onPermissionModeChange}
                permissionMode={permissionMode}
                streamVersion={commandStreamVersion}
                taskId={snapshot.projection.taskId}
                taskState={snapshot.projection.state}
              />
            </div>
          ) : null}
        </div>
        {showWorkbench && client.readBlob ? (
          <TaskWorkbench
            client={{ readBlob: client.readBlob }}
            events={events}
            projection={snapshot.projection}
            timeline={items}
          />
        ) : null}
      </div>
      <RunStatusBar items={items} projection={snapshot.projection} />
    </section>
  )
}

function workbenchPanel(item: TimelineItemProjection): TaskWorkbenchPanel | null {
  if (item.kind === 'diff') return 'changes'
  if (item.kind === 'command') return 'commands'
  if (item.kind === 'subagent') return 'agents'
  if (item.kind === 'image') return 'sources'
  if (item.kind === 'notice' && item.summary.toLowerCase().startsWith('workspace')) {
    return 'environment'
  }
  if (['compaction', 'error', 'notice', 'permission', 'tool_activity'].includes(item.kind)) {
    return 'audit'
  }
  return null
}

function queueItems(snapshot: TaskSnapshot, events: TaskEventEnvelope[]) {
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
