import { Clock, Wrench } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import type { ToolAttempt } from '@/shared/tauri/commands'

export function ToolInvocationCard({
  attempt,
  onClick,
}: {
  attempt: ToolAttempt
  onClick?: () => void
}) {
  const { t } = useTranslation('conversation')
  const statusLabel = t(`timeline.toolStatus.${attempt.status}`)
  const originLabel = attempt.origin ? t(`timeline.toolOrigin.${attempt.origin}`) : null
  const interactive = Boolean(onClick)
  const className = cn(
    'w-full rounded-md border border-border px-3 py-2 text-left transition-colors',
    interactive && 'hover:bg-muted/50 focus-visible:ring-2 focus-visible:ring-ring',
    attempt.status === 'failed' && 'border-destructive/30',
  )
  const content = (
    <>
      <div className="flex items-center gap-2">
        <Wrench className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="truncate font-medium text-sm">{attempt.toolName}</span>
        <ToolStatusBadge status={attempt.status} label={statusLabel} />
      </div>

      <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-muted-foreground text-xs">
        {originLabel ? <span>{originLabel}</span> : null}
        {attempt.durationMs ? (
          <span className="inline-flex items-center gap-1">
            <Clock className="size-3" />
            {t('timeline.durationMs', { ms: attempt.durationMs })}
          </span>
        ) : null}
        {attempt.outputSummary ? <span className="truncate">{attempt.outputSummary}</span> : null}
        {attempt.failurePhase ? (
          <span className="text-destructive">
            {t(`timeline.failurePhase.${attempt.failurePhase}`)}
          </span>
        ) : null}
      </div>

      {attempt.affectedTargets && attempt.affectedTargets.length > 0 ? (
        <div className="mt-1.5 flex flex-wrap gap-1">
          {attempt.affectedTargets.slice(0, 3).map((target) => (
            <span key={target} className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs">
              {target}
            </span>
          ))}
          {attempt.affectedTargets.length > 3 ? (
            <span className="text-muted-foreground text-xs">
              +{attempt.affectedTargets.length - 3}
            </span>
          ) : null}
        </div>
      ) : null}
    </>
  )

  if (!interactive) {
    return (
      <div className={className} data-tool-invocation-id={attempt.id}>
        {content}
      </div>
    )
  }

  return (
    <button
      className={className}
      data-tool-invocation-id={attempt.id}
      onClick={onClick}
      type="button"
    >
      {content}
    </button>
  )
}

function ToolStatusBadge({ status, label }: { status: ToolAttempt['status']; label: string }) {
  const colors: Record<string, string> = {
    queued: 'bg-muted text-muted-foreground',
    waitingPermission: 'bg-yellow-100 text-yellow-800',
    running: 'bg-blue-100 text-blue-800',
    completed: 'bg-green-100 text-green-800',
    failed: 'bg-red-100 text-red-800',
    denied: 'bg-red-100 text-red-800',
  }
  return (
    <span
      className={cn(
        'ml-auto rounded px-1.5 py-0.5 font-medium text-xs',
        colors[status] ?? 'bg-muted',
      )}
    >
      {label}
    </span>
  )
}
