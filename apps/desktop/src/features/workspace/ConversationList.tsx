import { Plus, Text, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/shared/lib/utils'
import { ScrollArea } from '@/shared/ui/scroll-area'

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
  disabled?: boolean
  errorMessage?: string
  isLoading?: boolean
  onDeleteConversation: (conversationId: string) => void
  onNewConversation: () => void
  onSelectConversation: (conversationId: string) => void
}

export function ConversationList({
  activeConversationId,
  conversations,
  disabled = false,
  errorMessage,
  isLoading = false,
  onDeleteConversation,
  onNewConversation,
  onSelectConversation,
}: ConversationListProps) {
  const { t } = useTranslation('shell')

  return (
    <div className="mt-5 flex min-h-0 flex-1 flex-col px-3">
      <div className="mb-2 flex shrink-0 items-center justify-between gap-2">
        <div className="text-muted-foreground text-xs">{t('conversations.recent')}</div>
        <button
          aria-label={t('actions.newConversation')}
          className="grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
          disabled={disabled}
          onClick={onNewConversation}
          title={t('actions.newConversation')}
          type="button"
        >
          <Plus className="size-4" strokeWidth={1.75} />
        </button>
      </div>
      {disabled ? (
        <div className="shrink-0 rounded-md px-2 py-2 text-muted-foreground text-xs">
          {t('conversations.projectRequired')}
        </div>
      ) : null}
      {!disabled && isLoading ? (
        <div className="shrink-0 rounded-md px-2 py-2 text-muted-foreground text-xs">
          {t('conversations.loading')}
        </div>
      ) : null}
      {!disabled && !isLoading && errorMessage ? (
        <div className="shrink-0 rounded-md px-2 py-2 text-destructive text-xs">{errorMessage}</div>
      ) : null}
      {!disabled && !isLoading && !errorMessage && conversations.length === 0 ? (
        <div className="shrink-0 rounded-md px-2 py-2 text-muted-foreground text-xs">
          {t('conversations.empty')}
        </div>
      ) : null}
      <ScrollArea className="min-h-0 flex-1">
        <ul className="flex flex-col gap-1 pr-0.5">
          {conversations.map((conversation) => {
            const isActive = conversation.id === activeConversationId
            const title = conversation.isEmpty
              ? t('conversations.defaultTitle')
              : conversation.title
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
                    <span className="flex w-full min-w-0 gap-2">
                      <Text
                        aria-hidden="true"
                        className={cn(
                          'mt-0.5 size-3.5 shrink-0',
                          isActive ? 'text-foreground' : 'text-muted-foreground/80',
                        )}
                        strokeWidth={isActive ? 2 : 1.5}
                      />
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
                    className="mt-1 grid size-6 shrink-0 place-items-center rounded-md text-muted-foreground opacity-0 transition-opacity hover:bg-background hover:text-destructive focus-visible:opacity-100 group-hover:opacity-100"
                    onClick={() => onDeleteConversation(conversation.id)}
                    title={t('conversations.delete', { title })}
                    type="button"
                  >
                    <X className="size-3.5" strokeWidth={1.75} />
                  </button>
                </div>
              </li>
            )
          })}
        </ul>
      </ScrollArea>
    </div>
  )
}
