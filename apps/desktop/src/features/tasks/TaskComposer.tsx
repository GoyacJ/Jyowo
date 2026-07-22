import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Composer, type ComposerSubmitPayload } from '@/features/conversation/Composer'
import type { RunProjection, ServerFrame, TaskState, TypedUlid } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import type {
  AttachmentInputModality,
  AttachmentReference,
  ConversationModelCapability,
  ListReferenceCandidatesResponse,
  PermissionMode,
} from '@/shared/tauri/commands'
import { createTaskCommandMetadata, requireAcceptedCommand } from './task-command'
import type { TaskConnectionState } from './task-store'
import type { TaskCommandExecutor } from './use-task-command-executor'

const queueStates = new Set<TaskState>([
  'running',
  'waiting_permission',
  'waiting_input',
  'yielding',
])
const idleSubmissionState = {
  controlError: null,
  controlPending: null,
  submitError: null,
  submitting: false,
} satisfies SubmissionState

type SubmissionState = {
  controlError: string | null
  controlPending: 'continue' | 'pause' | null
  submitting: boolean
  submitError: string | null
}

export function TaskComposer({
  client,
  connectionState,
  executeCommand,
  effectiveModelConfigId,
  effectivePermissionMode,
  modelCapability,
  modelConfigId,
  modelConfigs = [],
  onCommandAccepted,
  onListReferenceCandidates,
  onModelConfigChange,
  onPickAttachmentPath,
  onPermissionModeChange,
  permissionMode,
  currentRun,
  streamVersion,
  taskId,
  taskState,
}: {
  client: Pick<DaemonClient, 'connect' | 'request'> & {
    stageBlobFromPath?: (
      taskId: TypedUlid,
      path: string,
    ) => Promise<{ attachment: AttachmentReference }>
  }
  connectionState: TaskConnectionState
  executeCommand?: TaskCommandExecutor
  effectiveModelConfigId?: string
  effectivePermissionMode?: PermissionMode
  modelCapability?: ConversationModelCapability | null
  modelConfigId?: string
  modelConfigs?: Array<{ id: string; label: string }>
  onCommandAccepted?: (streamVersion: number) => void
  onListReferenceCandidates?: () => Promise<ListReferenceCandidatesResponse>
  onModelConfigChange?: (modelConfigId: string) => void
  onPickAttachmentPath?: (modalities: AttachmentInputModality[]) => Promise<string | null>
  onPermissionModeChange?: (mode: PermissionMode) => void
  permissionMode?: PermissionMode
  currentRun?: RunProjection | null
  streamVersion: number
  taskId: TypedUlid
  taskState: TaskState
}) {
  const { t } = useTranslation('tasks')
  const [submissionStates, setSubmissionStates] = useState(
    () => new Map<TypedUlid, SubmissionState>(),
  )
  const submissionStatesRef = useRef(submissionStates)
  const pendingMetadata = useRef(new Map<string, ReturnType<typeof createTaskCommandMetadata>>())
  const queues = queueStates.has(taskState)
  const disconnected = connectionState === 'disconnected' || connectionState === 'protocol_error'
  const normalizedModelConfigId = normalizeModelConfigId(modelConfigId)
  const displayedModelConfigId =
    normalizeModelConfigId(effectiveModelConfigId) ?? normalizedModelConfigId
  const submissionState = submissionStates.get(taskId) ?? idleSubmissionState
  const runState = currentRun?.state ?? taskState
  const canPause =
    runState === 'running' || runState === 'waiting_permission' || runState === 'waiting_input'
  const pausePending = submissionState.controlPending === 'pause' || runState === 'yielding'
  const canContinue =
    currentRun?.state === 'interrupted' && currentRun.terminalReason === 'cancelled'

  function updateSubmissionState(
    submittedTaskId: TypedUlid,
    update: (current: SubmissionState) => SubmissionState,
  ) {
    const nextStates = new Map(submissionStatesRef.current)
    const nextState = update(nextStates.get(submittedTaskId) ?? idleSubmissionState)
    nextStates.set(submittedTaskId, nextState)
    submissionStatesRef.current = nextStates
    setSubmissionStates(nextStates)
  }

  async function submit(payload: ComposerSubmitPayload) {
    const submittedTaskId = taskId
    if (submissionStatesRef.current.get(submittedTaskId)?.submitting) return
    updateSubmissionState(submittedTaskId, (current) => ({
      ...current,
      submitError: null,
      submitting: true,
    }))
    const requestBody = {
      attachments: (payload.attachments ?? []).map((attachment) => attachment.blobRef.id),
      content: payload.prompt,
      contextReferences: payload.contextReferences ?? [],
      ...(payload.modelConfigId ? { modelConfigId: payload.modelConfigId } : {}),
      ...(permissionMode ? { permissionMode } : {}),
      taskId: submittedTaskId,
      type: 'submit_message' as const,
    }
    const operation = `submit:${JSON.stringify(requestBody)}`
    try {
      const buildRequest = (metadata: ReturnType<typeof createTaskCommandMetadata>) => ({
        ...requestBody,
        metadata,
      })
      await runCommand(submittedTaskId, operation, buildRequest)
    } catch (error) {
      updateSubmissionState(submittedTaskId, (current) => ({
        ...current,
        submitError: disconnected ? t('composer.disconnected') : commandError(error),
      }))
      throw error
    } finally {
      updateSubmissionState(submittedTaskId, (current) => ({ ...current, submitting: false }))
    }
  }

  async function pauseRun() {
    if (!canPause || submissionState.controlPending) return
    const operation = `pause:${currentRun?.segmentId ?? taskId}`
    updateSubmissionState(taskId, (current) => ({
      ...current,
      controlError: null,
      controlPending: 'pause',
    }))
    try {
      await runCommand(taskId, operation, (metadata) => ({
        metadata,
        mode: 'safe_point',
        taskId,
        type: 'stop_run',
      }))
    } catch (error) {
      updateSubmissionState(taskId, (current) => ({
        ...current,
        controlError: commandError(error),
      }))
      throw error
    } finally {
      updateSubmissionState(taskId, (current) => ({ ...current, controlPending: null }))
    }
  }

  async function continueRun() {
    if (!canContinue || submissionState.controlPending) return
    const operation = `continue:${currentRun.segmentId}`
    updateSubmissionState(taskId, (current) => ({
      ...current,
      controlError: null,
      controlPending: 'continue',
    }))
    try {
      await runCommand(taskId, operation, (metadata) => ({
        indeterminateTools: [],
        metadata,
        taskId,
        type: 'continue_task',
      }))
    } catch (error) {
      updateSubmissionState(taskId, (current) => ({
        ...current,
        controlError: commandError(error),
      }))
      throw error
    } finally {
      updateSubmissionState(taskId, (current) => ({ ...current, controlPending: null }))
    }
  }

  async function runCommand(
    commandTaskId: TypedUlid,
    operation: string,
    buildRequest: Parameters<TaskCommandExecutor>[1],
  ) {
    let frame: ServerFrame
    if (executeCommand) {
      frame = await executeCommand(operation, buildRequest)
    } else {
      let metadata = pendingMetadata.current.get(operation)
      if (!metadata) {
        metadata = createTaskCommandMetadata(commandTaskId, streamVersion, operation)
        pendingMetadata.current.set(operation, metadata)
      }
      frame = await client.request(buildRequest(metadata))
      pendingMetadata.current.delete(operation)
    }
    const accepted = requireAcceptedCommand(frame, commandTaskId)
    onCommandAccepted?.(accepted.streamVersion)
  }

  return (
    <Composer
      autoModeAvailable
      draftKey={`task:${taskId}`}
      errorMessage={
        submissionState.controlError ??
        submissionState.submitError ??
        (disconnected ? t('composer.disconnected') : undefined)
      }
      mode={
        submissionState.submitting
          ? { kind: 'submitting' }
          : queues
            ? { kind: 'queue' }
            : { kind: 'ready' }
      }
      modelCapability={modelCapability}
      modelConfigId={displayedModelConfigId}
      modelConfigs={modelConfigs}
      submitModelConfigId={normalizedModelConfigId ?? ''}
      onCreateAttachmentFromPath={
        client.stageBlobFromPath
          ? (path) =>
              client.stageBlobFromPath?.(taskId, path) as Promise<{
                attachment: AttachmentReference
              }>
          : undefined
      }
      onListReferenceCandidates={onListReferenceCandidates}
      onModelConfigChange={onModelConfigChange}
      onPickAttachmentPath={onPickAttachmentPath}
      onPauseRun={canPause || pausePending ? pauseRun : undefined}
      pausePending={pausePending}
      onContinueRun={canContinue ? continueRun : undefined}
      continuePending={submissionState.controlPending === 'continue'}
      permissionMode={effectivePermissionMode ?? permissionMode}
      onPermissionModeChange={onPermissionModeChange}
      onRetry={
        disconnected
          ? () => {
              const retryTaskId = taskId
              updateSubmissionState(retryTaskId, (current) => ({
                ...current,
                submitError: null,
              }))
              void client.connect().catch((error) =>
                updateSubmissionState(retryTaskId, (current) => ({
                  ...current,
                  submitError: commandError(error),
                })),
              )
            }
          : undefined
      }
      onSubmit={submit}
      submitAriaLabel={queues ? t('composer.queueMessage') : t('composer.sendMessage')}
      submitLabel={queues ? t('composer.queue') : t('composer.send')}
    />
  )
}

export function normalizeModelConfigId(value: string | undefined): string | undefined {
  const normalized = value?.trim()
  return normalized ? normalized : undefined
}

function commandError(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
