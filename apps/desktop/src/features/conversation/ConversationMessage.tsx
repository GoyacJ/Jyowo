import type { ReactNode } from 'react'

export interface ConversationMessageProps {
  avatar: string
  author: string
  time: string
  body?: ReactNode
  children?: ReactNode
  elementId?: string
  tone?: 'user' | 'assistant'
}

export function ConversationMessage({
  avatar,
  author,
  body,
  children,
  elementId,
  time,
  tone = 'user',
}: ConversationMessageProps) {
  return (
    <article
      className="grid grid-cols-[36px_minmax(0,1fr)] gap-4 data-[tone=assistant]:border-border data-[tone=assistant]:border-t data-[tone=assistant]:pt-4"
      data-tone={tone}
      id={elementId}
      tabIndex={elementId ? -1 : undefined}
    >
      <div
        className="grid size-8 place-items-center rounded-full bg-accent/20 text-sm data-[tone=assistant]:bg-accent data-[tone=assistant]:text-accent-foreground"
        data-tone={tone}
      >
        {avatar}
      </div>
      <div>
        <div className="flex items-baseline gap-3">
          <span className="font-medium">{author}</span>
          <span className="text-muted-foreground text-xs">{time}</span>
        </div>
        {body ? <div className="mt-2 max-w-3xl text-sm leading-6">{body}</div> : null}
        {children}
      </div>
    </article>
  )
}
