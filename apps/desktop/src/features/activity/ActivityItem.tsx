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

const statusLabels = {
  blocked: 'Blocked',
  failed: 'Failed',
  queued: 'Queued',
  redacted: 'Redacted',
  running: 'Running',
  success: 'Success',
} satisfies Record<ActivityStatus, string>

export function getActivityStatusLabel(status: ActivityStatus) {
  return statusLabels[status]
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

export function ActivityItem({ item }: ActivityItemProps) {
  return (
    <li className="flex shrink-0 items-center gap-2 font-mono text-xs">
      <span className="text-foreground">{item.label}</span>
      <span
        aria-hidden="true"
        className={`size-1.5 rounded-full ${statusDotClasses[item.status]}`}
      />
      <span className={statusClasses[item.status]}>{statusLabels[item.status]}</span>
      <span className="text-muted-foreground">{item.time}</span>
    </li>
  )
}
