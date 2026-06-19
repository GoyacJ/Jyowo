import type { ReactNode } from 'react'

export interface ConversationCanvasProps {
  title: string
  children: ReactNode
}

export function ConversationCanvas({ children, title }: ConversationCanvasProps) {
  return (
    <section className="mx-auto flex min-h-full w-full max-w-5xl flex-col">
      <div className="flex-1 pt-3 pb-4">
        <h1 className="font-semibold text-2xl tracking-normal">{title}</h1>
        <div className="mt-6 grid gap-5">{children}</div>
      </div>
    </section>
  )
}
