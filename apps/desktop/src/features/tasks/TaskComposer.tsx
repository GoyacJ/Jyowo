import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Composer, type ComposerSubmitPayload } from '@/features/conversation/Composer'
import { referenceKey } from '@/features/conversation/composer/ReferenceCombobox'
import type { TaskState, TypedUlid } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import type {
  AttachmentInputModality,
  AttachmentReference,
  ListReferenceCandidatesResponse,
} from '@/shared/tauri/commands'
import { createTaskCommandMetadata, requireAcceptedCommand } from './task-command'
import type { TaskConnectionState } from './task-store'

const queueStates = new Set<TaskState>(['running', 'waiting_permission', 'yielding'])

export function TaskComposer({
  client,
  connectionState,
  onCommandAccepted,
  onCreateAttachmentFromPath,
  onListReferenceCandidates,
  onPickAttachmentPath,
  streamVersion,
  taskId,
  taskState,
}: {
  client: Pick<DaemonClient, 'connect' | 'request'>
  connectionState: TaskConnectionState
  onCommandAccepted?: (streamVersion: number) => void
  onCreateAttachmentFromPath?: (path: string) => Promise<{ attachment: AttachmentReference }>
  onListReferenceCandidates?: () => Promise<ListReferenceCandidatesResponse>
  onPickAttachmentPath?: (modalities: AttachmentInputModality[]) => Promise<string | null>
  streamVersion: number
  taskId: TypedUlid
  taskState: TaskState
}) {
  const { t } = useTranslation('tasks')
  const [submitting, setSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)
  const queues = queueStates.has(taskState)
  const disconnected = connectionState === 'disconnected' || connectionState === 'protocol_error'

  async function submit(payload: ComposerSubmitPayload) {
    if (submitting) return
    setSubmitting(true)
    setSubmitError(null)
    try {
      const frame = await client.request({
        attachments: (payload.attachments ?? []).map((attachment) => attachment.blobRef.id),
        content: payload.prompt,
        contextReferences: (payload.contextReferences ?? []).map(referenceKey),
        metadata: createTaskCommandMetadata(taskId, streamVersion, 'submit'),
        taskId,
        type: 'submit_message',
      })
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
      draftKey={`task:${taskId}`}
      errorMessage={submitError ?? (disconnected ? t('composer.disconnected') : undefined)}
      mode={submitting ? { kind: 'submitting' } : queues ? { kind: 'queue' } : { kind: 'ready' }}
      onCreateAttachmentFromPath={
        onCreateAttachmentFromPath ? (path) => onCreateAttachmentFromPath(path) : undefined
      }
      onListReferenceCandidates={onListReferenceCandidates}
      onPickAttachmentPath={onPickAttachmentPath}
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
