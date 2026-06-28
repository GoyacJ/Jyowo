import { Copy } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { ConversationEventRef, ConversationTurn } from '@/shared/tauri/commands'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'
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
  const { i18n, t } = useTranslation('conversation')
  const timestamp = formatUserMessageTimestamp(turn.user.timestamp, i18n.language)

  return (
    <article aria-label={t('turnLabel')} className="grid gap-4">
      <section className="flex justify-end">
        <div className="flex max-w-[92%] flex-col items-end gap-2 sm:max-w-[78%]">
          <UserAttachmentStrip
            attachments={turn.user.attachments ?? []}
            conversationId={turn.conversationId}
          />
          <div className="w-full rounded-md border border-border bg-muted px-4 py-3 text-foreground">
            <div className="mb-1 text-muted-foreground text-xs">{t('userAuthor')}</div>
            <p className="whitespace-pre-wrap text-sm leading-6">{turn.user.body}</p>
          </div>
          <div className="flex w-full items-center justify-end gap-2 px-1 text-[11px] text-muted-foreground/70">
            <time dateTime={turn.user.timestamp} title={t('userMessage.timestampLabel')}>
              {timestamp}
            </time>
            <TooltipProvider delayDuration={150}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    aria-label={t('userMessage.copy')}
                    className="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground/65 hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    onClick={() => {
                      void navigator.clipboard?.writeText(turn.user.body)
                    }}
                    type="button"
                  >
                    <Copy aria-hidden="true" className="size-3.5" />
                  </button>
                </TooltipTrigger>
                <TooltipContent>{t('userMessage.copy')}</TooltipContent>
              </Tooltip>
            </TooltipProvider>
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

function formatUserMessageTimestamp(timestamp: string, language: string) {
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) {
    return timestamp
  }

  return new Intl.DateTimeFormat(language, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(date)
}
