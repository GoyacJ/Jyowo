import { useTranslation } from 'react-i18next'
import type { ConversationEventRef, ConversationTurn } from '@/shared/tauri/commands'
import { AssistantWorkView } from './assistant-work-view'

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
    <article className="grid gap-4">
      <section className="flex justify-end">
        <div className="max-w-[78%] rounded-md bg-primary px-4 py-3 text-primary-foreground">
          <div className="mb-1 font-medium text-xs opacity-80">{t('userAuthor')}</div>
          <p className="whitespace-pre-wrap text-sm leading-6">{turn.user.body}</p>
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
