import { ChevronDown, ChevronRight, type LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import { cn } from '@/shared/lib/utils'

export type EvidenceDisclosureProps = {
  id: string
  icon: LucideIcon
  title: ReactNode
  meta?: ReactNode
  open: boolean
  forcedOpen?: boolean
  onOpenChange?: (open: boolean) => void
  actions?: ReactNode
  children: ReactNode
}

export function EvidenceDisclosure({
  actions,
  children,
  forcedOpen = false,
  icon: Icon,
  id,
  meta,
  onOpenChange,
  open,
  title,
}: EvidenceDisclosureProps) {
  const expanded = forcedOpen || open
  const Chevron = expanded ? ChevronDown : ChevronRight

  return (
    <section
      className="grid min-w-0 gap-2 rounded-md border border-border bg-card/40 px-2.5 py-2"
      data-evidence-disclosure-id={id}
    >
      <div className="flex min-w-0 items-center gap-2">
        <button
          aria-expanded={expanded}
          className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-sm text-left text-muted-foreground text-xs hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() => {
            if (forcedOpen) {
              return
            }
            onOpenChange?.(!expanded)
          }}
          type="button"
        >
          <Icon className="size-3.5 shrink-0" data-testid="evidence-disclosure-icon" />
          <span
            className="min-w-0 flex-1 truncate text-foreground"
            data-testid="evidence-disclosure-title"
          >
            {title}
          </span>
          {meta ? (
            <span
              className="max-w-[40%] shrink truncate tabular-nums"
              data-testid="evidence-disclosure-meta"
            >
              {meta}
            </span>
          ) : null}
          <Chevron
            className={cn('size-3.5 shrink-0', forcedOpen ? 'text-muted-foreground/60' : null)}
            data-testid="evidence-disclosure-chevron"
          />
        </button>
        {actions ? <div className="flex shrink-0 items-center gap-1">{actions}</div> : null}
      </div>
      {expanded ? (
        <div className="min-w-0 overflow-hidden pl-5" data-testid="evidence-disclosure-body">
          {children}
        </div>
      ) : null}
    </section>
  )
}
