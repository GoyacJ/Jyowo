import { useQuery } from '@tanstack/react-query'
import { useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type {
  TaskEventEnvelope,
  TimelineItemProjection,
  TypedUlid,
} from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { useUiStore } from '@/shared/state/ui-store'
import type { TaskWorkbenchTarget } from '@/shared/state/workbench-selection'
import { providerSettingsQueryKey } from '@/shared/state/workspace-scope'
import type {
  ConversationModelCapability,
  ListReferenceCandidatesResponse,
  PermissionMode,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { pickAttachmentPath } from '@/shared/tauri/file-dialog'
import { useCommandClient, useDaemonClient } from '@/shared/tauri/react'
import { PendingPermissionDecision } from './PendingPermissionDecision'
import { QueuedMessages } from './queue/QueuedMessages'
import { normalizeModelConfigId, TaskComposer } from './TaskComposer'
import { deriveLiveTaskSnapshot, liveTimelineItems } from './task-live-projection'
import type { TaskConnectionState, TaskSnapshot } from './task-store'
import { TaskTimeline } from './timeline/TaskTimeline'
import { timelineSummary } from './timeline/timeline-summary'
import { useTask } from './use-task'
import { useTaskCommandExecutor } from './use-task-command-executor'
import { TaskWorkbench } from './workbench/TaskWorkbench'
import { TaskWorkbenchSummary } from './workbench/TaskWorkbenchSummary'
import {
  isTaskWorkbenchSidebarTarget,
  taskWorkbenchTargetFromTimelineItem,
} from './workbench/task-workbench-target'

export const timelineItems = liveTimelineItems

export function TaskWorkspace({ taskId }: { taskId: TypedUlid }) {
  const { t } = useTranslation('tasks')
  const task = useTask(taskId)
  const daemonClient = useDaemonClient()
  const commandClient = useCommandClient()
  const workspaceRoot = task.snapshot?.projection.workspace?.root
  const providerSettingsQuery = useQuery({
    queryFn: () => commandClient.listProviderSettings(workspaceRoot),
    queryKey: [...providerSettingsQueryKey, 'list', workspaceRoot ?? null],
  })
  const providerSettings = providerSettingsQuery.data
  const requiresProviderCatalog =
    providerSettings?.configs.some((config) => !config.hasApiKey) ?? false
  const providerCatalogQuery = useQuery({
    enabled: requiresProviderCatalog,
    queryFn: () => commandClient.listModelProviderCatalog(),
    queryKey: ['model-provider-catalog'],
  })
  const providerCatalog = providerCatalogQuery.data
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
  const modelConfigId = normalizeModelConfigId(
    modelOverride?.taskId === taskId ? modelOverride.value : undefined,
  )
  const capabilityModelConfigId =
    modelConfigId ?? normalizeModelConfigId(providerSettings?.defaultConfigId ?? undefined)
  const selectedModel = configuredModels.find((config) => config.id === capabilityModelConfigId)
  const modelSettingsError =
    providerSettingsQuery.error ?? (requiresProviderCatalog ? providerCatalogQuery.error : null)
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
      modelSettingsError={
        modelSettingsError === null ? null : getCommandErrorMessage(modelSettingsError)
      }
      modelConfigs={configuredModels.map((config) => ({
        id: config.id,
        label: `${config.displayName} / ${config.modelId}${
          config.id === providerSettings?.defaultConfigId ? ` (${t('model.default')})` : ''
        }`,
      }))}
      onListReferenceCandidates={() => daemonClient.listReferenceCandidates(taskId)}
      onModelConfigChange={(value) => setModelOverride({ taskId, value })}
      onRetryModelSettings={() => {
        if (providerSettingsQuery.isError) {
          void providerSettingsQuery.refetch()
          return
        }
        if (requiresProviderCatalog) {
          void providerCatalogQuery.refetch()
        }
      }}
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
  modelSettingsError,
  onListReferenceCandidates,
  onModelConfigChange,
  onPickAttachmentPath,
  onPermissionModeChange,
  permissionMode,
  onRetryModelSettings,
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
  modelSettingsError?: string | null
  onListReferenceCandidates?: () => Promise<ListReferenceCandidatesResponse>
  onModelConfigChange?: (modelConfigId: string) => void
  onPickAttachmentPath?: Parameters<typeof TaskComposer>[0]['onPickAttachmentPath']
  onPermissionModeChange?: (mode: PermissionMode) => void
  onRetryModelSettings?: () => void
  permissionMode?: PermissionMode
  snapshot: TaskSnapshot | null
}) {
  const { t: tTasks } = useTranslation('tasks')
  const { t: tCommon } = useTranslation('common')
  const snapshotTaskId = snapshot?.projection.taskId ?? null
  const openWorkbench = useUiStore((state) => state.openTaskWorkbench)
  const workbenchSession = useUiStore((state) =>
    snapshotTaskId ? state.taskWorkbenchByTaskId[snapshotTaskId] : undefined,
  )
  const closeWorkbench = useUiStore((state) => state.closeTaskWorkbench)
  const workspaceContainerRef = useRef<HTMLElement>(null)
  const readingColumnRef = useRef<HTMLDivElement>(null)
  const workbenchOpenerRef = useRef<{
    element: HTMLElement
    sourceEventId?: string
    taskId: string
  } | null>(null)
  const activeTaskIdRef = useRef(snapshotTaskId)
  activeTaskIdRef.current = snapshotTaskId
  const [workspaceLayoutMode, setWorkspaceLayoutMode] = useState<TaskWorkspaceLayoutMode | null>(
    null,
  )
  const [timelineFocusRequest, setTimelineFocusRequest] = useState<{
    eventId: string
    nonce: number
  } | null>(null)
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

  useLayoutEffect(() => {
    const container = workspaceContainerRef.current
    if (!container) return
    const update = () =>
      setWorkspaceLayoutMode(
        taskWorkspaceLayoutModeForWidth(container.getBoundingClientRect().width),
      )
    update()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(update)
    observer.observe(container)
    return () => observer.disconnect()
  }, [snapshotTaskId])

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
  const activeWorkbenchTarget = workbenchSession?.tabs.find(
    (tab) => tab.id === workbenchSession.activeTabId,
  )?.target
  const showWorkbench = Boolean(
    workbenchSession?.open === true &&
      workbenchSession.activeTabId !== null &&
      isTaskWorkbenchSidebarTarget(activeWorkbenchTarget) &&
      client?.loadTaskEvents &&
      client.readBlob,
  )
  const fullscreenWorkbench = showWorkbench && workspaceLayoutMode === 'fullscreen'

  function openTarget(target: TaskWorkbenchTarget, trigger?: HTMLElement | null) {
    if (!isTaskWorkbenchSidebarTarget(target)) return
    const activeElement =
      trigger ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null)
    workbenchOpenerRef.current =
      activeElement && readingColumnRef.current?.contains(activeElement)
        ? {
            element: activeElement,
            sourceEventId: target.sourceEventId,
            taskId: target.taskId,
          }
        : null
    openWorkbench(target)
  }

  function restoreWorkbenchFocus() {
    const opener = workbenchOpenerRef.current
    workbenchOpenerRef.current = null
    queueMicrotask(() => {
      if (activeTaskIdRef.current !== taskId) return
      if (opener?.taskId === taskId && opener.element.isConnected) {
        opener.element.focus()
        return
      }
      if (opener?.taskId === taskId && opener.sourceEventId) {
        const event = Array.from(
          readingColumnRef.current?.querySelectorAll<HTMLElement>('[data-event-id]') ?? [],
        ).find((element) => element.dataset.eventId === opener.sourceEventId)
        const eventTrigger = event?.querySelector<HTMLElement>('button')
        if (eventTrigger) {
          eventTrigger.focus()
          return
        }
      }
      readingColumnRef.current?.focus()
    })
  }

  function selectTimelineItem(item: TimelineItemProjection, trigger?: HTMLElement) {
    const target = taskWorkbenchTargetFromTimelineItem(item, taskId, timelineSummary(item, tTasks))
    if (!target) return
    openTarget(target, trigger)
  }

  return (
    <section
      className="task-workspace-container flex h-full min-h-0 w-full flex-col"
      ref={workspaceContainerRef}
    >
      <div
        className="task-workspace-layout relative flex min-h-0 flex-1 flex-col overflow-hidden"
        data-workbench-open={showWorkbench ? 'true' : undefined}
      >
        <div
          aria-hidden={fullscreenWorkbench ? true : undefined}
          className="task-reading-column relative mx-auto flex h-full min-w-0 w-full max-w-[820px] shrink-0 flex-col"
          data-testid="task-reading-column"
          inert={fullscreenWorkbench || undefined}
          onKeyDown={(event) => {
            if (
              event.key !== 'Escape' ||
              event.defaultPrevented ||
              !showWorkbench ||
              workspaceLayoutMode !== 'overlay'
            ) {
              return
            }
            event.preventDefault()
            closeWorkbench(taskId)
            restoreWorkbenchFocus()
          }}
          ref={readingColumnRef}
          tabIndex={-1}
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
            <span className="task-workspace-header-status mt-1 shrink-0 text-muted-foreground text-xs">
              {tTasks(connectionStateKey(connectionState))}
            </span>
          </header>
          <div className="flex min-h-0 min-w-0 flex-1 pt-6">
            <TaskTimeline
              blobLoader={client?.readBlob}
              focusRequest={timelineFocusRequest}
              items={items}
              onSelectItem={selectTimelineItem}
              taskId={liveSnapshot.projection.taskId}
            />
          </div>
          {client ? (
            <div className="shrink-0 border-border/70 border-t bg-background/95 px-1 pt-3 pb-1 backdrop-blur-sm">
              {modelSettingsError ? (
                <div
                  className="mb-3 flex items-center justify-between gap-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm"
                  role="alert"
                >
                  <span>{modelSettingsError}</span>
                  <button
                    className="shrink-0 rounded-md border border-destructive/30 px-2 py-1 font-medium"
                    onClick={onRetryModelSettings}
                    type="button"
                  >
                    {tCommon('retry')}
                  </button>
                </div>
              ) : null}
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
        <TaskWorkbenchSummary
          events={events}
          onOpen={(target, trigger) => openTarget(target, trigger)}
          projection={liveSnapshot.projection}
          timeline={items}
          mobile={workspaceLayoutMode === 'fullscreen'}
        />
        {showWorkbench && client?.loadTaskEvents && client.readBlob ? (
          <TaskWorkbench
            client={{
              loadTaskEvents: client.loadTaskEvents,
              readBlob: client.readBlob,
              request: client.request,
            }}
            events={events}
            projection={liveSnapshot.projection}
            onClosed={() => {
              restoreWorkbenchFocus()
            }}
            onLocateInTimeline={(eventId) =>
              setTimelineFocusRequest((current) => ({
                eventId,
                nonce: (current?.nonce ?? 0) + 1,
              }))
            }
            snapshotOffset={snapshot.snapshotOffset}
            timeline={items}
          />
        ) : null}
      </div>
    </section>
  )
}

type TaskWorkspaceLayoutMode = 'docked' | 'fullscreen' | 'overlay'

export function taskWorkspaceLayoutModeForWidth(width: number): TaskWorkspaceLayoutMode {
  if (width < 720) return 'fullscreen'
  if (width < 1040) return 'overlay'
  return 'docked'
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
