import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { ClientRequest, QueueItemProjection, TypedUlid } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'

import { createTaskCommandMetadata, requireAcceptedCommand } from '../task-command'
import type { TaskCommandExecutor } from '../use-task-command-executor'
import { QueuedMessageRow } from './QueuedMessageRow'

type QueueCommandRequest = Extract<
  ClientRequest,
  {
    type: 'delete_queued_message' | 'edit_queued_message' | 'promote_queued_message'
  }
>

export function QueuedMessages({
  client,
  expectedStreamVersion,
  executeCommand,
  items,
  onCommandAccepted,
  taskId,
}: {
  client: Pick<DaemonClient, 'request'>
  expectedStreamVersion: number
  executeCommand?: TaskCommandExecutor
  items: QueueItemProjection[]
  onCommandAccepted?: (streamVersion: number) => void
  taskId: TypedUlid
}) {
  const { t } = useTranslation('tasks')
  const [busy, setBusy] = useState(false)
  const busyRef = useRef(false)
  const pendingMetadata = useRef(new Map<string, ReturnType<typeof createTaskCommandMetadata>>())
  const [latestById, setLatestById] = useState<Record<TypedUlid, QueueItemProjection>>({})
  const [announcement, setAnnouncement] = useState('')
  const announcedQueueIds = useRef(new Set(items.map((item) => item.queueItemId)))

  useEffect(() => {
    setLatestById((current) => {
      let changed = false
      const next = { ...current }
      for (const item of items) {
        const replacement = next[item.queueItemId]
        if (replacement && isAtLeastAsFresh(item, replacement)) {
          delete next[item.queueItemId]
          changed = true
        }
      }
      return changed ? next : current
    })
  }, [items])

  const activeItems = useMemo(
    () =>
      items
        .map((item) => latestById[item.queueItemId] ?? item)
        .filter((item) => item.state === 'queued' || item.state === 'promoting')
        .sort(
          (left, right) =>
            left.createdGlobalOffset - right.createdGlobalOffset ||
            left.queueItemId.localeCompare(right.queueItemId),
        ),
    [items, latestById],
  )

  useEffect(() => {
    const nextIds = new Set(activeItems.map((item) => item.queueItemId))
    const added = activeItems.filter((item) => !announcedQueueIds.current.has(item.queueItemId))
    announcedQueueIds.current = nextIds
    if (added.length > 0) {
      setAnnouncement(t('queue.added', { count: added.length }))
    }
  }, [activeItems, t])

  if (activeItems.length === 0) return null

  function metadata(operation: string) {
    const existing = pendingMetadata.current.get(operation)
    if (existing) return existing
    const created = createTaskCommandMetadata(taskId, expectedStreamVersion, operation)
    pendingMetadata.current.set(operation, created)
    return created
  }

  async function send(
    item: QueueItemProjection,
    operation: string,
    buildRequest: (
      commandMetadata: ReturnType<typeof createTaskCommandMetadata>,
    ) => QueueCommandRequest,
  ): Promise<boolean> {
    if (busyRef.current) return false
    busyRef.current = true
    setBusy(true)
    setAnnouncement('')
    try {
      const frame = executeCommand
        ? await executeCommand(operation, buildRequest)
        : await client.request(buildRequest(metadata(operation)))
      if (!executeCommand) pendingMetadata.current.delete(operation)
      if (
        frame.message.type === 'command_rejected' &&
        frame.message.reason === 'stale_queue_revision' &&
        frame.message.latestQueueItem
      ) {
        const latestQueueItem = frame.message.latestQueueItem
        setLatestById((current) => ({
          ...current,
          [item.queueItemId]: latestQueueItem,
        }))
        if (typeof frame.message.currentStreamVersion === 'number') {
          onCommandAccepted?.(frame.message.currentStreamVersion)
        }
        setAnnouncement(t('queue.staleConflict'))
        return true
      }
      const accepted = requireAcceptedCommand(frame, taskId)
      onCommandAccepted?.(accepted.streamVersion)
      return true
    } catch (error) {
      setAnnouncement(error instanceof Error ? error.message : String(error))
      return false
    } finally {
      busyRef.current = false
      setBusy(false)
    }
  }

  return (
    <section
      aria-label={t('queue.label')}
      className="mb-2 overflow-hidden rounded-lg border border-border bg-row-muted/40"
    >
      <div className="flex items-center justify-between border-border/70 border-b px-3 py-2">
        <h2 className="font-medium text-xs">{t('queue.title')}</h2>
        <span className="text-[11px] text-muted-foreground">
          {t('queue.count', { count: activeItems.length })}
        </span>
      </div>
      <ol aria-label={t('queue.label')}>
        {activeItems.map((item, index) => (
          <QueuedMessageRow
            busy={busy}
            item={item}
            key={item.queueItemId}
            onDelete={() => {
              const operation = `delete:${item.queueItemId}:${item.revision}`
              return send(item, operation, (commandMetadata) => ({
                expectedRevision: item.revision,
                metadata: commandMetadata,
                queueItemId: item.queueItemId,
                taskId,
                type: 'delete_queued_message',
              }))
            }}
            onEdit={(content) => {
              const operation = `edit:${item.queueItemId}:${item.revision}:${content}`
              return send(item, operation, (commandMetadata) => ({
                attachments: item.attachments,
                content,
                contextReferences: item.contextReferences,
                expectedRevision: item.revision,
                metadata: commandMetadata,
                queueItemId: item.queueItemId,
                taskId,
                type: 'edit_queued_message',
              }))
            }}
            onForcePromote={async () => {
              if (!window.confirm(t('queue.forceConfirmation'))) return false
              const operation = `force:${item.queueItemId}:${item.revision}`
              return send(item, operation, (commandMetadata) => ({
                expectedRevision: item.revision,
                metadata: commandMetadata,
                mode: 'force_stop',
                queueItemId: item.queueItemId,
                taskId,
                type: 'promote_queued_message',
              }))
            }}
            onSafePromote={() => {
              const operation = `safe:${item.queueItemId}:${item.revision}`
              return send(item, operation, (commandMetadata) => ({
                expectedRevision: item.revision,
                metadata: commandMetadata,
                mode: 'safe_point',
                queueItemId: item.queueItemId,
                taskId,
                type: 'promote_queued_message',
              }))
            }}
            order={index + 1}
          />
        ))}
      </ol>
      <p aria-live="polite" className="sr-only" role="status">
        {announcement}
      </p>
    </section>
  )
}

function isAtLeastAsFresh(item: QueueItemProjection, replacement: QueueItemProjection) {
  if (item.revision > replacement.revision) return true
  if (item.revision < replacement.revision) return false
  return (
    item.content === replacement.content &&
    item.state === replacement.state &&
    item.attachments.join('\0') === replacement.attachments.join('\0') &&
    JSON.stringify(item.contextReferences) === JSON.stringify(replacement.contextReferences)
  )
}
