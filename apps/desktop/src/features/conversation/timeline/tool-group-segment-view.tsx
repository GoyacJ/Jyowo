import { useTranslation } from 'react-i18next'
import type { ConversationEventRef, ToolGroupSegment } from '@/shared/tauri/commands'
import { ToolAttemptRow } from './tool-attempt-row'

export function ToolGroupSegmentView({
  conversationId,
  onOpenDetails,
  onPermissionResolve,
  segment,
  turnId,
}: {
  conversationId: string
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
  segment: ToolGroupSegment
  turnId: string
}) {
  const { t } = useTranslation('conversation')

  return (
    <section className="rounded-md border border-border bg-surface">
      <div className="border-border border-b px-3 py-2 font-medium text-sm">
        {t('timeline.tools')}
      </div>
      <div className="divide-y divide-border">
        {segment.attempts.map((attempt) => (
          <ToolAttemptRow
            attempt={attempt}
            conversationId={conversationId}
            key={attempt.id}
            onOpenDetails={onOpenDetails}
            onPermissionResolve={onPermissionResolve}
            turnId={turnId}
          />
        ))}
      </div>
    </section>
  )
}
