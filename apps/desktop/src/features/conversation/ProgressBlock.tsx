import { LoaderCircle } from 'lucide-react'

type ProgressStatus = 'blocked' | 'failed' | 'queued' | 'running' | 'success'

export interface ProgressBlockProps {
  label: string
  status: ProgressStatus
  time?: string
}

const progressStatusCopy = {
  blocked: 'Blocked',
  failed: 'Failed',
  queued: 'Queued',
  running: 'Running',
  success: 'Complete',
} satisfies Record<ProgressStatus, string>

export function ProgressBlock({ label, status, time }: ProgressBlockProps) {
  return (
    <section
      aria-label="Work progress"
      className="mt-3 flex items-center justify-between gap-3 text-sm"
    >
      <div className="flex min-w-0 items-center gap-3">
        <LoaderCircle
          aria-hidden="true"
          className={`size-4 shrink-0 text-muted-foreground ${status === 'running' ? 'animate-spin' : ''}`}
        />
        <div className="min-w-0">
          <p className="truncate font-medium">Working: {label}</p>
          {time ? <p className="text-muted-foreground text-xs">{time}</p> : null}
        </div>
      </div>
      <span className="shrink-0 text-muted-foreground text-xs">{progressStatusCopy[status]}</span>
    </section>
  )
}
