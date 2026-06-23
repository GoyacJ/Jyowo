import { FileText, MessageSquarePlus, MessageSquareText, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

type ConversationListItem = {
  id: string
  isEmpty: boolean
  lastMessagePreview?: string
  title: string
  updatedAt: string
}

type ConversationListProps = {
  activeConversationId?: string
  conversations: ConversationListItem[]
  errorMessage?: string
  isLoading?: boolean
  onDeleteConversation: (conversationId: string) => void
  onNewConversation: () => void
  onSelectConversation: (conversationId: string) => void
}

export function ConversationList({
  activeConversationId,
  conversations,
  errorMessage,
  isLoading = false,
  onDeleteConversation,
  onNewConversation,
  onSelectConversation,
}: ConversationListProps) {
  const { t } = useTranslation('shell')

  return (
    <div className="mt-5 px-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="text-muted-foreground text-xs">{t('conversations.recent')}</div>
        <button
          aria-label={t('actions.newConversation')}
          className="grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={onNewConversation}
          title={t('actions.newConversation')}
          type="button"
        >
          <MessageSquarePlus className="size-3.5" />
        </button>
      </div>
      {isLoading ? (
        <div className="rounded-md px-2 py-2 text-muted-foreground text-xs">
          {t('conversations.loading')}
        </div>
      ) : null}
      {!isLoading && errorMessage ? (
        <div className="rounded-md px-2 py-2 text-destructive text-xs">{errorMessage}</div>
      ) : null}
      {!isLoading && !errorMessage && conversations.length === 0 ? (
        <div className="rounded-md px-2 py-2 text-muted-foreground text-xs">
          {t('conversations.empty')}
        </div>
      ) : null}
      <ul className="flex flex-col gap-1">
        {conversations.map((conversation) => {
          const isActive = conversation.id === activeConversationId
          const title = conversation.isEmpty ? t('conversations.defaultTitle') : conversation.title
          const lastMessagePreview = conversation.isEmpty
            ? t('conversations.defaultPreview')
            : conversation.lastMessagePreview

          return (
            <li key={conversation.id}>
              <div
                className="group flex w-full items-start gap-1 rounded-md pr-1 hover:bg-muted data-[active=true]:bg-accent/10 data-[active=true]:text-foreground"
                data-active={isActive}
              >
                <button
                  aria-current={isActive ? 'page' : undefined}
                  className="flex min-w-0 flex-1 items-start rounded-md px-2 py-1.5 text-left text-xs"
                  onClick={() => onSelectConversation(conversation.id)}
                  type="button"
                >
                  <span className="flex w-full min-w-0 gap-1.5">
                    <span
                      aria-hidden="true"
                      className="mt-1.5 size-1.5 shrink-0 rounded-full bg-transparent data-[active=true]:bg-accent"
                      data-active={isActive}
                    />
                    {isActive ? (
                      <MessageSquareText className="mt-0.5 size-3 shrink-0 text-muted-foreground" />
                    ) : (
                      <FileText className="mt-0.5 size-3 shrink-0 text-muted-foreground" />
                    )}
                    <span className="min-w-0">
                      <span className="block truncate">{title}</span>
                      {lastMessagePreview ? (
                        <span className="mt-0.5 block truncate text-muted-foreground">
                          {lastMessagePreview}
                        </span>
                      ) : null}
                    </span>
                  </span>
                </button>
                <button
                  aria-label={t('conversations.delete', { title })}
                  className="mt-1 grid size-6 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-background hover:text-destructive"
                  onClick={() => onDeleteConversation(conversation.id)}
                  title={t('conversations.delete', { title })}
                  type="button"
                >
                  <Trash2 className="size-3.5" />
                </button>
              </div>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
