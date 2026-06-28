import { ChevronDown, ChevronRight, Wrench } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import type { ToolAttempt } from '@/shared/tauri/commands'

type ToolEvidenceCounts = {
  completedCount: number
  failedCount: number
  runningCount: number
  waitingPermissionCount: number
}

export function ToolEvidenceSummary({
  attempts,
  completedGroupOpen = false,
  onCompletedGroupToggle,
}: {
  attempts: ToolAttempt[]
  completedGroupOpen?: boolean
  onCompletedGroupToggle?: () => void
}) {
  const { t } = useTranslation('conversation')
  const counts = getToolEvidenceCounts(attempts)
  const canToggleCompleted = counts.completedCount > 0 && onCompletedGroupToggle !== undefined
  const chips = [
    counts.completedCount > 0
      ? {
          key: 'completed',
          label: t('timeline.toolEvidenceSummary.completed', {
            count: counts.completedCount,
          }),
        }
      : null,
    counts.failedCount > 0
      ? {
          key: 'failed',
          label: t('timeline.toolEvidenceSummary.failed', { count: counts.failedCount }),
          tone: 'destructive' as const,
        }
      : null,
    counts.runningCount > 0
      ? {
          key: 'running',
          label: t('timeline.toolEvidenceSummary.running', { count: counts.runningCount }),
        }
      : null,
    counts.waitingPermissionCount > 0
      ? {
          key: 'waitingPermission',
          label: t('timeline.toolEvidenceSummary.waitingPermission', {
            count: counts.waitingPermissionCount,
          }),
        }
      : null,
  ].filter((chip): chip is { key: string; label: string; tone?: 'destructive' } => Boolean(chip))

  const content = (
    <>
      <Wrench className="size-3.5 shrink-0" />
      <span className="shrink-0">{t('timeline.tools')}</span>
      {chips.length > 0 ? (
        chips.map((chip) => (
          <span
            className={cn(
              'rounded-sm bg-muted px-1.5 py-0.5',
              chip.tone === 'destructive' ? 'text-destructive' : null,
            )}
            key={chip.key}
          >
            {chip.label}
          </span>
        ))
      ) : (
        <span>{t('timeline.tools')}</span>
      )}
      {canToggleCompleted ? (
        completedGroupOpen ? (
          <ChevronDown className="size-3.5 shrink-0" />
        ) : (
          <ChevronRight className="size-3.5 shrink-0" />
        )
      ) : null}
    </>
  )

  const className = 'flex min-w-0 flex-wrap items-center gap-1.5 text-muted-foreground text-xs'

  if (canToggleCompleted) {
    return (
      <button
        aria-expanded={completedGroupOpen}
        className={cn(className, 'text-left hover:text-foreground')}
        onClick={onCompletedGroupToggle}
        type="button"
      >
        {content}
      </button>
    )
  }

  return <div className={className}>{content}</div>
}

function getToolEvidenceCounts(attempts: ToolAttempt[]): ToolEvidenceCounts {
  return attempts.reduce<ToolEvidenceCounts>(
    (counts, attempt) => {
      if (attempt.status === 'completed') {
        counts.completedCount += 1
      }
      if (attempt.status === 'failed' || attempt.status === 'denied') {
        counts.failedCount += 1
      }
      if (attempt.status === 'running') {
        counts.runningCount += 1
      }
      if (attempt.status === 'waitingPermission' || attempt.permission?.status === 'pending') {
        counts.waitingPermissionCount += 1
      }
      return counts
    },
    {
      completedCount: 0,
      failedCount: 0,
      runningCount: 0,
      waitingPermissionCount: 0,
    },
  )
}
