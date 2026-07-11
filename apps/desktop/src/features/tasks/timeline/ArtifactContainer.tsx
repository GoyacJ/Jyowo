import { ChevronDown } from 'lucide-react'
import type { ReactNode } from 'react'

import type { TimelineItemProjection } from '@/generated/daemon-protocol'

export function ArtifactContainer({
  children,
  item,
  label,
  onOpen,
  openLabel,
}: {
  children: ReactNode
  item: TimelineItemProjection
  label: string
  onOpen?: () => void
  openLabel?: string
}) {
  return (
    <section
      className="overflow-hidden rounded-xl border border-border/80 bg-artifact"
      data-artifact="true"
    >
      <div className="flex min-h-9 items-center justify-between gap-3 border-border/70 border-b px-3 text-muted-foreground text-xs">
        <span className="font-medium text-foreground">{label}</span>
        <span className="flex items-center gap-2">
          {item.incomplete ? <span>Incomplete</span> : null}
          {onOpen ? (
            <button
              aria-label={openLabel}
              className="rounded px-1.5 py-0.5 font-medium text-foreground hover:bg-muted"
              onClick={onOpen}
              type="button"
            >
              Open
            </button>
          ) : null}
        </span>
      </div>
      <div className="px-3 py-3">{children}</div>
      <details className="group border-border/70 border-t px-3 py-2 text-muted-foreground text-xs">
        <summary className="flex cursor-pointer list-none items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
          <ChevronDown
            aria-hidden="true"
            className="size-3 transition-transform group-open:rotate-180"
          />
          Details
        </summary>
        <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 font-mono">
          <dt>Offset</dt>
          <dd>{item.globalOffset}</dd>
          <dt>Event</dt>
          <dd className="truncate">{item.id}</dd>
        </dl>
      </details>
    </section>
  )
}
