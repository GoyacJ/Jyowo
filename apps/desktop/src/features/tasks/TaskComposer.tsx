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
  const [submitting, setSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)
  const pendingMetadata = useRef(new Map<string, ReturnType<typeof createTaskCommandMetadata>>())
  const queues = queueStates.has(taskState)
  const disconnected = connectionState === 'disconnected' || connectionState === 'protocol_error'

  async function submit(payload: ComposerSubmitPayload) {
    if (submitting) return
    setSubmitting(true)
    setSubmitError(null)
    const requestBody = {
      attachments: (payload.attachments ?? []).map((attachment) => attachment.blobRef.id),
      content: payload.prompt,
      contextReferences: (payload.contextReferences ?? []).map(referenceKey),
      ...(payload.modelConfigId ? { modelConfigId: payload.modelConfigId } : {}),
      ...(permissionMode ? { permissionMode: payload.permissionMode } : {}),
      taskId,
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
          metadata = createTaskCommandMetadata(taskId, streamVersion, operation)
          pendingMetadata.current.set(operation, metadata)
        }
        frame = await client.request(buildRequest(metadata))
        pendingMetadata.current.delete(operation)
      }
      const accepted = requireAcceptedCommand(frame, taskId)
      onCommandAccepted?.(accepted.streamVersion)
    } catch (error) {
      setSubmitError(disconnected ? t('composer.disconnected') : commandError(error))
      throw error
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Composer
      autoModeAvailable
      draftKey={`task:${taskId}`}
      errorMessage={submitError ?? (disconnected ? t('composer.disconnected') : undefined)}
      mode={submitting ? { kind: 'submitting' } : queues ? { kind: 'queue' } : { kind: 'ready' }}
      modelCapability={modelCapability}
      modelConfigId={modelConfigId}
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
              setSubmitError(null)
              void client.connect().catch((error) => setSubmitError(commandError(error)))
            }
          : undefined
      }
      onSubmit={submit}
      submitAriaLabel={queues ? t('composer.queueMessage') : t('composer.sendMessage')}
      submitLabel={queues ? t('composer.queue') : t('composer.send')}
    />
  )
}

function commandError(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
