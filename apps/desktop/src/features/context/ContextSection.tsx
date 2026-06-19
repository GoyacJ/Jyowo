import { type ReactNode, useId } from 'react'

type ContextSectionProps = {
  action?: ReactNode
  children: ReactNode
  title: string
}

export function ContextSection({ action, children, title }: ContextSectionProps) {
  const titleId = useId()

  return (
    <section aria-labelledby={titleId} className="border-border border-t pt-4">
      <div className="mb-3 flex items-center justify-between">
        <h2 className="font-normal text-muted-foreground text-sm" id={titleId}>
          {title}
        </h2>
        {action}
      </div>
      {children}
    </section>
  )
}
