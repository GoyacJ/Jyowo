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
import { deriveLiveTaskSnapshot, liveTimelineItems } from './task-live-projection'
import type { TaskConnectionState, TaskSnapshot } from './task-store'
import { TaskTimeline } from './timeline/TaskTimeline'
import { useTask } from './use-task'
import { useTaskCommandExecutor } from './use-task-command-executor'
import { TaskWorkbench } from './workbench/TaskWorkbench'

export const timelineItems = liveTimelineItems

export function TaskWorkspace({ taskId }: { taskId: TypedUlid }) {
  const { t } = useTranslation('tasks')
  const task = useTask(taskId)
  const daemonClient = useDaemonClient()
  const commandClient = useCommandClient()
  const workspaceRoot = task.snapshot?.projection.workspace?.root
  const providerSettings = useQuery({
    queryFn: () => commandClient.listProviderSettings(workspaceRoot),
    queryKey: ['task-model-configs', workspaceRoot],
  }).data
  const providerCatalog = useQuery({
    enabled: providerSettings?.configs.some((config) => !config.hasApiKey) ?? false,
    queryFn: () => commandClient.listModelProviderCatalog(),
    queryKey: ['model-provider-catalog'],
  }).data
  const [modelOverride, setModelOverride] = useState<{ taskId: TypedUlid; value: string } | null>(
    null,
  )
  const [permissionOverride, setPermissionOverride] = useState<{
    taskId: TypedUlid
    value: PermissionMode
  } | null>(null)
  const authenticationFreeProviders = new Set(
    providerCatalog?.providers
      .filter((provider) => provider.runtimeCapability.authScheme === 'none')
      .map((provider) => provider.providerId) ?? [],
  )
  const configuredModels =
    providerSettings?.configs.filter(
      (config) =>
        config.modelDescriptor.runtimeStatus.kind === 'runnable' &&
        (config.hasApiKey || authenticationFreeProviders.has(config.providerId)),
    ) ?? []
  const modelConfigId = modelOverride?.taskId === taskId ? modelOverride.value : undefined
  const capabilityModelConfigId = modelConfigId ?? providerSettings?.defaultConfigId
  const selectedModel = configuredModels.find((config) => config.id === capabilityModelConfigId)
  const permissionMode =
    permissionOverride?.taskId === taskId ? permissionOverride.value : undefined
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
          config.id === providerSettings?.defaultConfigId ? ` (${t('model.default')})` : ''
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
    Partial<Pick<DaemonClient, 'loadTaskEvents' | 'readBlob' | 'stageBlobFromPath'>>
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
  const { t: tTasks } = useTranslation('tasks')
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
          {connectionError ?? tTasks('workspace.unavailable')}
        </div>
      </div>
    )
  }

  if (!snapshot) {
    return (
      <div className="grid h-full place-items-center text-muted-foreground text-sm" role="status">
        {connectionState === 'disconnected'
          ? tTasks('workspace.unavailable')
          : tTasks('workspace.loading')}
      </div>
    )
  }

  const liveSnapshot = deriveLiveTaskSnapshot(snapshot, events)
  const items = liveSnapshot.timeline
  const queue = liveSnapshot.projection.queue
  const taskId = liveSnapshot.projection.taskId
  const showWorkbench =
    workbenchMode !== 'closed' &&
    workbenchSelection?.taskId === taskId &&
    client?.loadTaskEvents &&
    client.readBlob

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
                {liveSnapshot.projection.title}
              </h1>
              <p className="mt-1 text-muted-foreground text-xs capitalize">
                {tTasks(taskStateKey(liveSnapshot.projection.state))}
              </p>
            </div>
            <span className="mt-1 shrink-0 text-muted-foreground text-xs">
              {tTasks(connectionStateKey(connectionState))}
            </span>
          </header>
          <div className="flex min-h-0 flex-1 pt-6">
            <TaskTimeline
              currentRun={liveSnapshot.projection.currentRun}
              items={items}
              onSelectItem={selectTimelineItem}
            />
          </div>
          {client ? (
            <div className="shrink-0 border-border/70 border-t bg-background/95 px-1 pt-3 pb-1 backdrop-blur-sm">
              {liveSnapshot.projection.pendingPermission && executeCommand ? (
                <PendingPermissionDecision
                  executeCommand={executeCommand}
                  key={`${liveSnapshot.projection.pendingPermission.requestId}:${liveSnapshot.projection.pendingPermission.revision}`}
                  permission={liveSnapshot.projection.pendingPermission}
                  taskId={liveSnapshot.projection.taskId}
                />
              ) : null}
              <QueuedMessages
                client={client}
                expectedStreamVersion={commandStreamVersion}
                executeCommand={executeCommand}
                items={queue}
                onCommandAccepted={commandAccepted}
                taskId={liveSnapshot.projection.taskId}
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
                taskId={liveSnapshot.projection.taskId}
                taskState={liveSnapshot.projection.state}
              />
            </div>
          ) : null}
        </div>
        {showWorkbench && client.loadTaskEvents && client.readBlob ? (
          <TaskWorkbench
            client={{ loadTaskEvents: client.loadTaskEvents, readBlob: client.readBlob }}
            events={events}
            projection={liveSnapshot.projection}
            snapshotOffset={snapshot.snapshotOffset}
            timeline={items}
          />
        ) : null}
      </div>
      <RunStatusBar items={items} projection={liveSnapshot.projection} />
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

function connectionStateKey(state: TaskConnectionState) {
  const keys = {
    connected: 'workspace.connection.connected',
    connecting: 'workspace.connection.connecting',
    disconnected: 'workspace.connection.disconnected',
    protocol_error: 'workspace.connection.protocolError',
    resyncing: 'workspace.connection.resyncing',
  } as const
  return keys[state]
}

function taskStateKey(state: TaskSnapshot['projection']['state']) {
  const keys = {
    completed: 'workspace.state.completed',
    failed: 'workspace.state.failed',
    idle: 'workspace.state.idle',
    interrupted: 'workspace.state.interrupted',
    running: 'workspace.state.running',
    waiting_permission: 'workspace.state.waitingPermission',
    yielding: 'workspace.state.yielding',
  } as const
  return keys[state]
}
