import { CircleAlert } from 'lucide-react'

export interface DecisionCardProps {
  detail: string
  title: string
}

export function DecisionCard({ detail, title }: DecisionCardProps) {
  return (
    <section
      aria-label={`Decision needed: ${title}`}
      className="mt-4 rounded-md border border-border border-l-4 border-l-warning bg-surface/40 hover:bg-surface px-4 py-3.5 shadow-sm hover:shadow-md transition-all duration-200"
    >
      <div className="flex items-start gap-3">
        <div className="mt-0.5 relative flex size-4 shrink-0 items-center justify-center">
          <span className="animate-ping absolute inline-flex h-3 w-3 rounded-full bg-warning/30 opacity-75"></span>
          <CircleAlert aria-hidden="true" className="relative size-4 text-warning" />
        </div>
        <div className="min-w-0">
          <p className="font-semibold text-sm text-foreground/90">{title}</p>
          <p className="mt-1.5 text-muted-foreground text-sm leading-relaxed">{detail}</p>
        </div>
      </div>
    </section>
  )
}
