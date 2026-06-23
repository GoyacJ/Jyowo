type ActivityStatus = 'blocked' | 'failed' | 'queued' | 'redacted' | 'running' | 'success'

export type ActivityRailItem = {
  id: string
  label: string
  status: ActivityStatus
  time: string
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
