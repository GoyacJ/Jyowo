import { ChevronsDown, ChevronsUp } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  ActivityItem,
  type ActivityRailItem,
  type CurrentRunStatus,
  getActivityStatusClass,
} from './ActivityItem'

export type { ActivityRailItem } from './ActivityItem'

type ActivityRailProps = {
  collapsed?: boolean
  currentRun?: CurrentRunStatus
  errorMessage?: string
  expanded?: boolean
  items: ActivityRailItem[]
  loading?: boolean
  onCollapse?: () => void
  onExpand?: () => void
  onViewAll?: () => void
}

const compactActivityLimit = 3

export function ActivityRail({
  collapsed = false,
  currentRun,
  errorMessage,
  expanded = false,
  items,
  loading = false,
  onCollapse,
  onExpand,
  onViewAll,
}: ActivityRailProps) {
  const { t } = useTranslation(['activity', 'common'])
  const visibleItems = expanded ? items : items.slice(0, compactActivityLimit)

  if (collapsed) {
    return (
      <section
        aria-label={t('activity:title')}
        className="border-border border-t bg-background"
        data-collapsed="true"
        data-expanded="false"
      >
        <div className="flex h-full items-center gap-4 px-5 text-sm">
          {onExpand ? (
            <button
              aria-label={t('activity:expand')}
              className="text-muted-foreground hover:text-foreground"
              onClick={onExpand}
              type="button"
            >
              <ChevronsDown className="size-4" />
            </button>
          ) : null}
          <span className="font-medium">{t('activity:title')}</span>
        </div>
      </section>
    )
  }

  return (
    <section
      aria-label={t('activity:title')}
      className="border-border border-t bg-background"
      data-collapsed="false"
      data-expanded={expanded}
    >
      <div className="flex h-full items-center gap-4 px-5 text-sm">
        {onCollapse ? (
          <button
            aria-label={t('activity:collapse')}
            className="text-muted-foreground hover:text-foreground"
            onClick={onCollapse}
            type="button"
          >
            <ChevronsUp className="size-4" />
          </button>
        ) : (
          <ChevronsUp aria-hidden="true" className="size-4 text-muted-foreground" />
        )}
        <span className="font-medium">{t('activity:title')}</span>
        {currentRun ? (
          <span className="rounded-md border border-border bg-surface px-3 py-1 text-muted-foreground text-xs">
            {currentRun.label === 'Current run' ? t('activity:currentRun') : currentRun.label}
            <span
              className={`ml-3 ${getActivityStatusClass(currentRun.status)}`}
              data-testid="current-run-status"
            >
              {t(`common:status.${currentRun.status}`)}
            </span>
          </span>
        ) : null}
        {loading ? (
          <span className="min-w-0 flex-1 text-muted-foreground">{t('activity:loading')}</span>
        ) : errorMessage ? (
          <span className="min-w-0 flex-1 text-destructive">{errorMessage}</span>
        ) : (
          <ul
            className={
              expanded
                ? 'grid max-h-full min-w-0 flex-1 grid-cols-[repeat(auto-fit,minmax(190px,1fr))] gap-2 overflow-y-auto'
                : 'flex min-w-0 flex-1 items-center gap-4 overflow-hidden'
            }
          >
            {visibleItems.map((item) => (
              <ActivityItem item={item} key={item.id} />
            ))}
          </ul>
        )}
        {onViewAll ? (
          <button
            className="text-muted-foreground hover:text-foreground"
            onClick={onViewAll}
            type="button"
          >
            {t('activity:viewAll')}
          </button>
        ) : null}
      </div>
    </section>
  )
}
