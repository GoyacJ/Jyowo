import { FileText, MessageSquareText } from 'lucide-react'

type ConversationListProps = {
  conversations: string[]
  activeConversation: string
}

export function ConversationList({ activeConversation, conversations }: ConversationListProps) {
  return (
    <div className="mt-6 px-4">
      <div className="mb-2 text-muted-foreground text-xs">Recent conversations</div>
      <ul className="flex flex-col gap-1">
        {conversations.map((conversation) => {
          const isActive = conversation === activeConversation

          return (
            <li key={conversation}>
              <button
                aria-current={isActive ? 'page' : undefined}
                className="relative flex w-full items-center rounded-md px-2 py-2 pr-4 text-left text-xs hover:bg-muted data-[active=true]:bg-accent/10 data-[active=true]:text-foreground"
                data-active={isActive}
                type="button"
              >
                <span className="flex w-full min-w-0 items-center gap-1.5">
                  {isActive ? (
                    <MessageSquareText className="size-3 shrink-0 text-muted-foreground" />
                  ) : (
                    <FileText className="size-3 shrink-0 text-muted-foreground" />
                  )}
                  <span className="min-w-0 truncate">{conversation}</span>
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
