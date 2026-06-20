import {
  ChevronRight,
  ListChecks,
  type LucideIcon,
  Play,
  SquareArrowOutUpRight,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

type NextActionListProps = {
  actions: string[]
  onNextAction?: (action: string) => void
}

function getActionIcon(action: string): LucideIcon {
  const normalized = action.toLowerCase()

  if (normalized.includes('continue')) {
    return Play
  }

  if (normalized.includes('open') || normalized.includes('artifact')) {
    return SquareArrowOutUpRight
  }

  return ListChecks
}

export function NextActionList({ actions, onNextAction }: NextActionListProps) {
  const { t } = useTranslation('context')

  if (actions.length === 0) {
    return <p className="text-muted-foreground text-sm">{t('noNextActions')}</p>
  }

  return (
    <ul aria-label={t('nextActions')} className="space-y-2">
      {actions.map((action) => {
        const Icon = getActionIcon(action)
        const content = (
          <>
            <span className="flex min-w-0 items-center gap-2.5">
              <Icon aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
              <span className="min-w-0 truncate">{action}</span>
            </span>
            <ChevronRight aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
          </>
        )

        return (
          <li key={action}>
            {onNextAction ? (
              <button
                className="flex w-full items-center justify-between gap-3 rounded-md border border-border bg-surface px-3 py-2 text-left text-sm hover:bg-muted"
                onClick={() => onNextAction(action)}
                type="button"
              >
                {content}
              </button>
            ) : (
              <div className="flex w-full items-center justify-between gap-3 rounded-md border border-border bg-surface px-3 py-2 text-sm">
                {content}
              </div>
            )}
          </li>
        )
      })}
    </ul>
  )
}
