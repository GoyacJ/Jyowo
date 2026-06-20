import { useTranslation } from 'react-i18next'

type ActivityStatus = 'blocked' | 'failed' | 'queued' | 'redacted' | 'running' | 'success'

export type ActivityRailItem = {
  id: string
  label: string
  status: ActivityStatus
  time: string
}

export type CurrentRunStatus = {
  label: string
  status: ActivityStatus
}

type ActivityItemProps = {
  item: ActivityRailItem
}

const statusClasses = {
  blocked: 'text-warning',
  failed: 'text-destructive',
  queued: 'text-muted-foreground',
  redacted: 'text-muted-foreground',
  running: 'text-warning',
  success: 'text-success',
} satisfies Record<ActivityStatus, string>

const statusDotClasses = {
  blocked: 'bg-warning',
  failed: 'bg-destructive',
  queued: 'bg-muted-foreground',
  redacted: 'bg-muted-foreground',
  running: 'bg-warning',
  success: 'bg-success',
} satisfies Record<ActivityStatus, string>

export function getActivityStatusClass(status: ActivityStatus) {
  return statusClasses[status]
}

const translatableActivityLabels = {
  assistant: 'eventLabels.assistant',
  engine: 'eventLabels.engine',
  permission: 'eventLabels.permission',
  run: 'eventLabels.run',
  tool: 'eventLabels.tool',
} satisfies Record<string, string>

export function ActivityItem({ item }: ActivityItemProps) {
  const { t } = useTranslation(['activity', 'common'])
  const labelKey = getActivityLabelKey(item.label)

  return (
    <li className="flex shrink-0 items-center gap-2 font-mono text-xs">
      <span className="text-foreground">{labelKey ? t(`activity:${labelKey}`) : item.label}</span>
      <span
        aria-hidden="true"
        className={`size-1.5 rounded-full ${statusDotClasses[item.status]}`}
      />
      <span className={statusClasses[item.status]}>{t(`common:status.${item.status}`)}</span>
      <span className="text-muted-foreground">{item.time}</span>
    </li>
  )
}

function getActivityLabelKey(label: string): string | undefined {
  return Object.hasOwn(translatableActivityLabels, label)
    ? translatableActivityLabels[label as keyof typeof translatableActivityLabels]
    : undefined
}
