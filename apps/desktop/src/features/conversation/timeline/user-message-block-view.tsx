import { useTranslation } from 'react-i18next'

import type { UserMessageBlock } from './conversation-blocks'

export function UserMessageBlockView({ block }: { block: UserMessageBlock }) {
  const { t } = useTranslation('conversation')
  const userLabel = t('userAuthor')

  return (
    <article className="grid grid-cols-[36px_minmax(0,1fr)] gap-4">
      <div className="grid size-8 place-items-center rounded-full bg-muted font-medium text-xs">
        {userLabel.charAt(0)}
      </div>
      <div>
        <div className="flex items-baseline gap-3">
          <span className="font-medium">{userLabel}</span>
          <span className="text-muted-foreground text-xs">{statusLabel(block.status, t)}</span>
        </div>
        <p className="mt-2 whitespace-pre-wrap text-sm leading-6">{block.body}</p>
        {block.errorMessage ? (
          <p className="mt-2 text-destructive text-xs">{block.errorMessage}</p>
        ) : null}
      </div>
    </article>
  )
}

function statusLabel(
  status: UserMessageBlock['status'],
  t: (key: 'messageStatus.sending' | 'messageStatus.sent' | 'messageStatus.failed') => string,
) {
  switch (status) {
    case 'sending':
      return t('messageStatus.sending')
    case 'failed':
      return t('messageStatus.failed')
    case 'sent':
      return t('messageStatus.sent')
  }
}
