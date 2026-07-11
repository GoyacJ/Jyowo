import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { ClientRequest, QueueItemProjection, TypedUlid } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'

import { createTaskCommandMetadata, requireAcceptedCommand } from '../task-command'
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
  items,
  onCommandAccepted,
  taskId,
}: {
  client: Pick<DaemonClient, 'request'>
  expectedStreamVersion: number
  items: QueueItemProjection[]
  onCommandAccepted?: (streamVersion: number) => void
  taskId: TypedUlid
}) {
  const { t } = useTranslation('tasks')
  const [busyItemIds, setBusyItemIds] = useState<Set<TypedUlid>>(new Set())
  const [latestById, setLatestById] = useState<Record<TypedUlid, QueueItemProjection>>({})
  const [announcement, setAnnouncement] = useState('')

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

  if (activeItems.length === 0) return null

  async function send(item: QueueItemProjection, request: QueueCommandRequest) {
    setBusyItemIds((current) => new Set(current).add(item.queueItemId))
    setAnnouncement('')
    try {
      const frame = await client.request(request)
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
        setAnnouncement(t('queue.staleConflict'))
        return
      }
      const accepted = requireAcceptedCommand(frame, taskId)
      onCommandAccepted?.(accepted.streamVersion)
    } finally {
      setBusyItemIds((current) => {
        const next = new Set(current)
        next.delete(item.queueItemId)
        return next
      })
    }
  }

  return (
    <section
      aria-label={t('queue.label')}
      className="mb-2 overflow-hidden rounded-lg border border-border bg-muted/20"
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
            busy={busyItemIds.has(item.queueItemId)}
            item={item}
            key={item.queueItemId}
            onDelete={() =>
              send(item, {
                expectedRevision: item.revision,
                metadata: createTaskCommandMetadata(
                  taskId,
                  expectedStreamVersion,
                  `delete:${item.queueItemId}:${item.revision}`,
                ),
                queueItemId: item.queueItemId,
                taskId,
                type: 'delete_queued_message',
              })
            }
            onEdit={(content) =>
              send(item, {
                attachments: item.attachments,
                content,
                contextReferences: item.contextReferences,
                expectedRevision: item.revision,
                metadata: createTaskCommandMetadata(
                  taskId,
                  expectedStreamVersion,
                  `edit:${item.queueItemId}:${item.revision}`,
                ),
                queueItemId: item.queueItemId,
                taskId,
                type: 'edit_queued_message',
              })
            }
            onForcePromote={async () => {
              if (!window.confirm(t('queue.forceConfirmation'))) return
              await send(item, {
                expectedRevision: item.revision,
                metadata: createTaskCommandMetadata(
                  taskId,
                  expectedStreamVersion,
                  `force:${item.queueItemId}:${item.revision}`,
                ),
                mode: 'force_stop',
                queueItemId: item.queueItemId,
                taskId,
                type: 'promote_queued_message',
              })
            }}
            onSafePromote={() =>
              send(item, {
                expectedRevision: item.revision,
                metadata: createTaskCommandMetadata(
                  taskId,
                  expectedStreamVersion,
                  `safe:${item.queueItemId}:${item.revision}`,
                ),
                mode: 'safe_point',
                queueItemId: item.queueItemId,
                taskId,
                type: 'promote_queued_message',
              })
            }
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
    item.contextReferences.join('\0') === replacement.contextReferences.join('\0')
  )
}
