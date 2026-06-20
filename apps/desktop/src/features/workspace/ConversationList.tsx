import { FileText, MessageSquareText } from 'lucide-react'
import { useTranslation } from 'react-i18next'

type ConversationListItem = {
  id: string
  lastMessagePreview?: string
  title: string
  updatedAt: string
}

type ConversationListProps = {
  activeConversationId?: string
  conversations: ConversationListItem[]
  errorMessage?: string
  isLoading?: boolean
  onSelectConversation: (conversationId: string) => void
}

export function ConversationList({
  activeConversationId,
  conversations,
  errorMessage,
  isLoading = false,
  onSelectConversation,
}: ConversationListProps) {
  const { t } = useTranslation('shell')

  return (
    <div className="mt-5 px-3">
      <div className="mb-2 text-muted-foreground text-xs">{t('conversations.recent')}</div>
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

          return (
            <li key={conversation.id}>
              <button
                aria-current={isActive ? 'page' : undefined}
                className="relative flex w-full items-start rounded-md px-2 py-1.5 pr-4 text-left text-xs hover:bg-muted data-[active=true]:bg-accent/10 data-[active=true]:text-foreground"
                data-active={isActive}
                onClick={() => onSelectConversation(conversation.id)}
                type="button"
              >
                <span className="flex w-full min-w-0 gap-1.5">
                  {isActive ? (
                    <MessageSquareText className="mt-0.5 size-3 shrink-0 text-muted-foreground" />
                  ) : (
                    <FileText className="mt-0.5 size-3 shrink-0 text-muted-foreground" />
                  )}
                  <span className="min-w-0">
                    <span className="block truncate">{conversation.title}</span>
                    {conversation.lastMessagePreview ? (
                      <span className="mt-0.5 block truncate text-muted-foreground">
                        {conversation.lastMessagePreview}
                      </span>
                    ) : null}
                  </span>
                </span>
                {isActive ? (
                  <span className="-translate-y-1/2 absolute top-1/2 right-1.5 size-1.5 rounded-full bg-accent" />
                ) : null}
              </button>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
