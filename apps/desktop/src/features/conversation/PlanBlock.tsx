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
      className="mt-3 rounded-md border border-border bg-surface"
    >
      <div className="flex items-center justify-between border-border border-b px-4 py-1">
        <div className="flex items-center gap-2 font-medium text-sm">
          <ChevronDown className="size-4" />
          {t('plan.title')}
        </div>
        <div className="flex items-center gap-3 text-xs">
          <span>{t('plan.completed', { completed: completedCount, total: totalCount })}</span>
          <span
            aria-label={t('plan.progress')}
            aria-valuemax={100}
            aria-valuemin={0}
            aria-valuenow={progressValue}
            className="h-1 w-20 overflow-hidden rounded-full bg-muted"
            role="progressbar"
          >
            <span className="block h-full bg-success" style={{ width: `${progressValue}%` }} />
          </span>
        </div>
      </div>
      <ul className="px-4 py-1 text-sm">
        {items.map((item) => (
          <li className="flex items-center justify-between gap-3 py-0.5" key={item.label}>
            <span className="flex items-center gap-3">
              {item.status === 'Done' ? (
                <Check className="size-4 text-success" />
              ) : (
                <Circle className="size-4 text-muted-foreground" />
              )}
              {item.label}
            </span>
            <span className="text-muted-foreground text-xs">
              {item.status === 'Done' ? t('plan.done') : t('plan.inProgress')}
            </span>
          </li>
        ))}
      </ul>
    </section>
  )
}
