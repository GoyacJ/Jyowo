import { ChevronDown, ChevronRight, type LucideIcon } from 'lucide-react'
import { cn } from '@/shared/lib/utils'
import type { ProcessStep } from '@/shared/tauri/commands'

export type ProcessStatusRowProps = {
  collapsible?: boolean
  countLabel?: string
  durationMs?: number
  icon: LucideIcon
  onToggle?: () => void
  open?: boolean
  status: ProcessStep['status']
  title: string
}

export function ProcessStatusRow({
  collapsible = false,
  countLabel,
  durationMs,
  icon: Icon,
  onToggle,
  open = false,
  status,
  title,
}: ProcessStatusRowProps) {
  const content = (
    <>
      <Icon className="size-3.5 shrink-0" />
      <StatusDot status={status} />
      <span className="min-w-0 truncate">{title}</span>
      {countLabel ? <span className="shrink-0">{countLabel}</span> : null}
      {durationMs !== undefined ? (
        <span className="shrink-0 tabular-nums">{formatDuration(durationMs)}</span>
      ) : null}
      {collapsible ? (
        open ? (
          <ChevronDown className="size-3.5 shrink-0" />
        ) : (
          <ChevronRight className="size-3.5 shrink-0" />
        )
      ) : null}
    </>
  )

  const className = cn(
    'flex h-7 max-w-full items-center gap-1.5 text-xs leading-5',
    status === 'failed' ? 'text-destructive' : 'text-muted-foreground',
  )

  if (collapsible) {
    return (
      <button
        aria-expanded={open}
        className={cn(className, 'text-left hover:text-foreground')}
        onClick={onToggle}
        type="button"
      >
        {content}
      </button>
    )
  }

  return <div className={className}>{content}</div>
}

function StatusDot({ status }: { status: ProcessStep['status'] }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        'size-1.5 shrink-0 rounded-full bg-muted-foreground',
        status === 'running' ? 'bg-warning' : null,
        status === 'failed' ? 'bg-destructive' : null,
        status === 'complete' ? 'bg-success' : null,
      )}
    />
  )
}

function formatDuration(durationMs: number) {
  if (durationMs < 1000) {
    return `${durationMs} ms`
  }

  return `${Math.round(durationMs / 1000)}s`
}
