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

export function getActivityStatusClass(status: ActivityStatus) {
  return statusClasses[status]
}

export function ActivityItem({ item }: ActivityItemProps) {
  return (
    <li>
      <span className="rounded-md border border-border bg-surface px-3 py-1 font-mono text-muted-foreground text-xs">
        {item.label}
        <span className={`ml-3 ${statusClasses[item.status]}`}>{statusLabels[item.status]}</span>
        <span className="ml-6">{item.time}</span>
      </span>
    </li>
  )
}
