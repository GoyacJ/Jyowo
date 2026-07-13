import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Composer, type ComposerSubmitPayload } from '@/features/conversation/Composer'
import { referenceKey } from '@/features/conversation/composer/ReferenceCombobox'
import type { ServerFrame, TaskState, TypedUlid } from '@/generated/daemon-protocol'
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

const queueStates = new Set<TaskState>(['running', 'waiting_permission', 'yielding'])
const idleSubmissionState = { submitting: false, submitError: null }

type SubmissionState = {
  submitting: boolean
  submitError: string | null
}

export function TaskComposer({
  client,
  connectionState,
  executeCommand,
  modelCapability,
  modelConfigId,
  modelConfigs = [],
  onCommandAccepted,
  onListReferenceCandidates,
  onModelConfigChange,
  onPickAttachmentPath,
  onPermissionModeChange,
  permissionMode,
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
  modelCapability?: ConversationModelCapability | null
  modelConfigId?: string
  modelConfigs?: Array<{ id: string; label: string }>
  onCommandAccepted?: (streamVersion: number) => void
  onListReferenceCandidates?: () => Promise<ListReferenceCandidatesResponse>
  onModelConfigChange?: (modelConfigId: string) => void
  onPickAttachmentPath?: (modalities: AttachmentInputModality[]) => Promise<string | null>
  onPermissionModeChange?: (mode: PermissionMode) => void
  permissionMode?: PermissionMode
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
  const submissionState = submissionStates.get(taskId) ?? idleSubmissionState

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
    updateSubmissionState(submittedTaskId, () => ({ submitting: true, submitError: null }))
    const requestBody = {
      attachments: (payload.attachments ?? []).map((attachment) => attachment.blobRef.id),
      content: payload.prompt,
      contextReferences: (payload.contextReferences ?? []).map(referenceKey),
      ...(payload.modelConfigId ? { modelConfigId: payload.modelConfigId } : {}),
      ...(permissionMode ? { permissionMode: payload.permissionMode } : {}),
      taskId: submittedTaskId,
      type: 'submit_message' as const,
    }
    const operation = `submit:${JSON.stringify(requestBody)}`
    try {
      const buildRequest = (metadata: ReturnType<typeof createTaskCommandMetadata>) => ({
        ...requestBody,
        metadata,
      })
      let frame: ServerFrame
      if (executeCommand) {
        frame = await executeCommand(operation, buildRequest)
      } else {
        let metadata = pendingMetadata.current.get(operation)
        if (!metadata) {
          metadata = createTaskCommandMetadata(submittedTaskId, streamVersion, operation)
          pendingMetadata.current.set(operation, metadata)
        }
        frame = await client.request(buildRequest(metadata))
        pendingMetadata.current.delete(operation)
      }
      const accepted = requireAcceptedCommand(frame, submittedTaskId)
      onCommandAccepted?.(accepted.streamVersion)
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

  return (
    <Composer
      autoModeAvailable
      draftKey={`task:${taskId}`}
      errorMessage={
        submissionState.submitError ?? (disconnected ? t('composer.disconnected') : undefined)
      }
      mode={
        submissionState.submitting
          ? { kind: 'submitting' }
          : queues
            ? { kind: 'queue' }
            : { kind: 'ready' }
      }
      modelCapability={modelCapability}
      modelConfigId={normalizedModelConfigId}
      modelConfigs={modelConfigs}
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
      permissionMode={permissionMode}
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
