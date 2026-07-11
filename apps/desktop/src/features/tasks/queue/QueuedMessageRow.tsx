import { Pencil, SquareArrowUp, Trash2, Zap } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { QueueItemProjection } from '@/generated/daemon-protocol'

import { QueuedMessageEditor } from './QueuedMessageEditor'

export function QueuedMessageRow({
  busy,
  item,
  order,
  onDelete,
  onEdit,
  onForcePromote,
  onSafePromote,
}: {
  busy: boolean
  item: QueueItemProjection
  order: number
  onDelete: () => Promise<boolean>
  onEdit: (content: string) => Promise<boolean>
  onForcePromote: () => Promise<boolean>
  onSafePromote: () => Promise<boolean>
}) {
  const { t } = useTranslation('tasks')
  const [editing, setEditing] = useState(false)
  const locked = busy || item.state === 'promoting'
  const editLabel = t('queue.editLabel', { order })

  useEffect(() => {
    if (item.state === 'promoting') setEditing(false)
  }, [item.state])

  return (
    <li className="border-border/70 border-b px-3 py-2.5 last:border-b-0">
      <div className="flex flex-wrap items-start gap-3">
        <span
          aria-hidden="true"
          className="mt-0.5 grid size-5 shrink-0 place-items-center rounded-full bg-muted font-medium text-[10px] text-muted-foreground"
        >
          {order}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <p className="min-w-0 flex-1 truncate text-sm">{item.content}</p>
            <span className="shrink-0 text-[11px] text-muted-foreground">
              {item.state === 'promoting' ? t('queue.promoting') : t('queue.queued')}
            </span>
          </div>
          {item.attachments.length > 0 || item.contextReferences.length > 0 ? (
            <p className="mt-0.5 text-[11px] text-muted-foreground">
              {t('queue.contextSummary', {
                attachments: item.attachments.length,
                references: item.contextReferences.length,
              })}
            </p>
          ) : null}
        </div>
        <div className="ml-8 flex w-full items-center justify-end gap-0.5 sm:ml-0 sm:w-auto sm:shrink-0">
          <QueueAction disabled={locked} label={editLabel} onClick={() => setEditing(true)}>
            <Pencil className="size-3.5" />
          </QueueAction>
          <QueueAction
            disabled={locked}
            label={t('queue.deleteLabel', { order })}
            onClick={() => void onDelete()}
          >
            <Trash2 className="size-3.5" />
          </QueueAction>
          <QueueAction
            disabled={locked}
            label={t('queue.forceLabel', { order })}
            onClick={() => void onForcePromote()}
          >
            <Zap className="size-3.5" />
          </QueueAction>
          <button
            aria-label={t('queue.safeLabel', { order })}
            className="ml-1 inline-flex items-center gap-1 rounded-md bg-foreground px-2 py-1 font-medium text-background text-xs hover:opacity-90 disabled:opacity-40"
            disabled={locked}
            onClick={() => void onSafePromote()}
            type="button"
          >
            <SquareArrowUp className="size-3.5" />
            {t('queue.runNext')}
          </button>
        </div>
      </div>
      {editing ? (
        <QueuedMessageEditor
          initialValue={item.content}
          label={editLabel}
          onCancel={() => setEditing(false)}
          onSave={async (content) => {
            if (await onEdit(content)) setEditing(false)
          }}
        />
      ) : null}
    </li>
  )
}

function QueueAction({
  children,
  disabled,
  label,
  onClick,
}: {
  children: React.ReactNode
  disabled: boolean
  label: string
  onClick: () => void
}) {
  return (
    <button
      aria-label={label}
      className="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-35"
      disabled={disabled}
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  )
}
