import { useTranslation } from 'react-i18next'
import type { ConversationEventRef, ConversationTurn } from '@/shared/tauri/commands'
import { AssistantWorkView } from './assistant-work-view'
import { UserAttachmentStrip } from './user-attachment-strip'

export function ConversationTurnView({
  turn,
  onOpenDetails,
  onPermissionResolve,
  onReviewContinue,
}: {
  turn: ConversationTurn
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
  onReviewContinue?: (prompt: string) => void
}) {
  const { t } = useTranslation('conversation')

  return (
    <article aria-label={t('turnLabel')} className="grid gap-4">
      <section className="flex justify-end">
        <div className="flex max-w-[92%] flex-col items-end gap-2 sm:max-w-[78%]">
          <UserAttachmentStrip attachments={turn.user.attachments ?? []} />
          <div className="w-full rounded-md border border-border bg-muted px-4 py-3 text-foreground">
            <div className="mb-1 text-muted-foreground text-xs">{t('userAuthor')}</div>
            <p className="whitespace-pre-wrap text-sm leading-6">{turn.user.body}</p>
          </div>
        </div>
      </section>
      {turn.assistant ? (
        <AssistantWorkView
          assistant={turn.assistant}
          conversationId={turn.conversationId}
          onOpenDetails={onOpenDetails}
          onPermissionResolve={onPermissionResolve}
          onReviewContinue={onReviewContinue}
          turnId={turn.id}
        />
      ) : null}
    </article>
  )
}
