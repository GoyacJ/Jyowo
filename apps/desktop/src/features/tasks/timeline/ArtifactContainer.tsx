import { ChevronDown } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

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
  onOpen?: (trigger: HTMLButtonElement) => void
  openLabel?: string
}) {
  const { t } = useTranslation('tasks')
  return (
    <section
      className="overflow-hidden rounded-xl border border-border/80 bg-artifact"
      data-artifact="true"
    >
      <div className="flex min-h-9 items-center justify-between gap-3 border-border/70 border-b px-3 text-muted-foreground text-xs">
        <span className="font-medium text-foreground">{label}</span>
        <span className="flex items-center gap-2">
          {item.incomplete ? <span>{t('timeline.incomplete')}</span> : null}
          {onOpen ? (
            <button
              aria-label={openLabel}
              className="min-h-7 rounded-md px-2 font-medium text-foreground hover:bg-muted"
              onClick={(event) => onOpen(event.currentTarget)}
              type="button"
            >
              {t('timeline.open')}
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
          {t('timeline.details')}
        </summary>
        <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 font-mono">
          <dt>{t('timeline.offset')}</dt>
          <dd>{item.globalOffset}</dd>
          <dt>{t('timeline.event')}</dt>
          <dd className="truncate">{item.id}</dd>
        </dl>
      </details>
    </section>
  )
}
