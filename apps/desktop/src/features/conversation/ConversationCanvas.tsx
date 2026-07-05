import type { ReactNode } from 'react'

export interface ConversationCanvasProps {
  title: string
  children: ReactNode
  actions?: ReactNode
  rightPanel?: ReactNode
  rightPanelWidth?: number
}

export function ConversationCanvas({
  actions,
  children,
  rightPanel,
  rightPanelWidth = 320,
  title,
}: ConversationCanvasProps) {
  return (
    <section
      className="grid h-full min-h-0 w-full"
      style={{
        gridTemplateColumns: rightPanel ? `minmax(0,1fr) ${rightPanelWidth}px` : 'minmax(0,1fr)',
      }}
    >
      <div className="min-h-0 min-w-0">
        <header className="flex items-center justify-between gap-3 pt-3 pb-4">
          <h1 className="min-w-0 truncate font-semibold text-2xl tracking-normal">{title}</h1>
          {actions ? <div className="flex shrink-0 items-center gap-2">{actions}</div> : null}
        </header>
        <div className="grid h-[calc(100%-64px)] min-h-0 gap-5">{children}</div>
      </div>
      {rightPanel}
    </section>
  )
}
