import { useTranslation } from 'react-i18next'
import type { ConversationEventRef, ToolAttempt } from '@/shared/tauri/commands'
import { PermissionInlinePanel } from './permission-inline-panel'

export function ToolAttemptRow({
  attempt,
  conversationId,
  onOpenDetails,
  onPermissionResolve,
  turnId,
}: {
  attempt: ToolAttempt
  conversationId: string
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
  turnId: string
}) {
  const { t } = useTranslation('conversation')
  const eventRef = attempt.eventRefs?.[0]
  const executionStatus = t(`timeline.toolStatus.${attempt.status}`)

  return (
    <div className="grid gap-2 px-3 py-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="font-medium text-sm">{attempt.toolName}</div>
          <div className="text-muted-foreground text-xs">
            {t('timeline.executionStatus', { status: executionStatus })}
          </div>
        </div>
        {eventRef ? (
          <button
            className="rounded-md border border-border px-2 py-1 text-xs"
            onClick={() => onOpenDetails?.(eventRef)}
            type="button"
          >
            {t('timeline.details')}
          </button>
        ) : null}
      </div>
      {attempt.permission ? (
        <PermissionInlinePanel
          conversationId={conversationId}
          onResolve={onPermissionResolve}
          permission={attempt.permission}
          turnId={turnId}
        />
      ) : null}
      {attempt.failureSummary ? (
        <p className="rounded-md bg-destructive/10 px-3 py-2 text-destructive text-sm">
          {attempt.failureSummary}
        </p>
      ) : null}
    </div>
  )
}
