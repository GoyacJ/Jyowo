import { Check, ChevronDown, Circle } from 'lucide-react'
import { useTranslation } from 'react-i18next'

export interface PlanItem {
  label: string
  status: 'Done' | 'In progress'
}

export interface PlanBlockProps {
  items: PlanItem[]
  completedCount: number
  totalCount: number
}

export function PlanBlock({ completedCount, items, totalCount }: PlanBlockProps) {
  const { t } = useTranslation('conversation')
  const progressValue =
    totalCount > 0 ? Math.min(100, Math.max(0, Math.round((completedCount / totalCount) * 100))) : 0

  return (
    <section
      aria-label={t('plan.title')}
      className="mt-3 rounded-md border border-border bg-surface shadow-sm hover:shadow-card transition-[box-shadow,transform] duration-200"
    >
      <div className="flex items-center justify-between border-border border-b px-4 py-2">
        <div className="flex items-center gap-2 font-semibold text-sm text-foreground/90">
          <ChevronDown className="size-4 text-muted-foreground" />
          {t('plan.title')}
        </div>
        <div className="flex items-center gap-3 text-xs">
          <span className="text-muted-foreground font-medium">
            {t('plan.completed', { completed: completedCount, total: totalCount })}
          </span>
          <span
            aria-label={t('plan.progress')}
            aria-valuemax={100}
            aria-valuemin={0}
            aria-valuenow={progressValue}
            className="h-1.5 w-20 overflow-hidden rounded-full bg-muted/60"
            role="progressbar"
          >
            <span
              className="block h-full bg-gradient-to-r from-success/80 to-success transition-[width] duration-300"
              style={{ width: `${progressValue}%` }}
            />
          </span>
        </div>
      </div>
      <ul className="px-3 py-1.5 text-sm space-y-0.5">
        {items.map((item) => (
          <li
            className="flex items-center justify-between gap-3 py-1 px-2 rounded-md hover:bg-muted/40 transition-colors duration-150"
            key={item.label}
          >
            <span className="flex items-center gap-3">
              {item.status === 'Done' ? (
                <Check className="size-4 text-success" />
              ) : (
                <span className="relative flex size-4 items-center justify-center">
                  <span className="animate-ping absolute inline-flex h-3 w-3 rounded-full bg-primary/30 opacity-75"></span>
                  <Circle className="relative size-4 text-primary" />
                </span>
              )}
              <span
                className={
                  item.status === 'Done'
                    ? 'text-muted-foreground/80 line-through'
                    : 'text-foreground font-medium'
                }
              >
                {item.label}
              </span>
            </span>
            <span
              className={
                item.status === 'Done'
                  ? 'text-muted-foreground/60 text-xs'
                  : 'text-primary/90 text-xs font-semibold'
              }
            >
              {item.status === 'Done' ? t('plan.done') : t('plan.inProgress')}
            </span>
          </li>
        ))}
      </ul>
    </section>
  )
}
