import { CircleAlert } from 'lucide-react'

export interface DecisionCardProps {
  detail: string
  title: string
}

export function DecisionCard({ detail, title }: DecisionCardProps) {
  return (
    <section
      aria-label={`Decision needed: ${title}`}
      className="mt-4 rounded-md border border-border bg-surface px-4 py-3"
    >
      <div className="flex items-start gap-3">
        <CircleAlert aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-warning" />
        <div className="min-w-0">
          <p className="font-medium text-sm">{title}</p>
          <p className="mt-1 text-muted-foreground text-sm">{detail}</p>
        </div>
      </div>
    </section>
  )
}
