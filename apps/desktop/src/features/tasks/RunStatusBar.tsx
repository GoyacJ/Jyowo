import { Clock3, GitPullRequestArrow, ListTodo, LoaderCircle } from 'lucide-react'
import { useEffect, useState } from 'react'

import type { TaskProjection, TimelineItemProjection } from '@/generated/daemon-protocol'
import { cn } from '@/shared/lib/utils'

export function RunStatusBar({
  items,
  now,
  projection,
}: {
  items: TimelineItemProjection[]
  now?: number
  projection: TaskProjection
}) {
  const clock = useClock(now)
  const run = projection.currentRun
  if (!run || !['running', 'waiting_permission', 'yielding'].includes(run.state)) return null

  const segmentItems = items.filter((item) => item.runSegmentId === run.segmentId)
  const currentStep = segmentItems.at(-1)?.summary ?? statusLabel(run.state)
  const queueCount = projection.queue.filter(
    (item) => item.state === 'queued' || item.state === 'promoting',
  ).length
  const changeSummary = [...segmentItems].reverse().find((item) => item.kind === 'diff')?.summary

  return (
    <section
      aria-label="Current run status"
      className="flex min-h-9 items-center gap-4 border-border border-t bg-surface/75 px-4 text-xs backdrop-blur-sm"
    >
      <span aria-atomic="true" aria-live="polite" className="sr-only" role="status">
        {currentStep}, {queueCount} queued, {changeSummary ?? 'No file changes'}
      </span>
      <span
        className={cn(
          'flex min-w-0 flex-1 items-center gap-2 font-medium',
          run.state === 'waiting_permission'
            ? 'text-state-waiting'
            : run.state === 'yielding'
              ? 'text-state-yielding'
              : 'text-state-running',
        )}
      >
        <LoaderCircle aria-hidden="true" className="size-3.5 shrink-0 animate-spin" />
        <span className="truncate">{currentStep}</span>
      </span>
      <StatusDatum icon={<Clock3 />} label={formatElapsed(run.startedAt, clock)} />
      <StatusDatum icon={<ListTodo />} label={`${queueCount} queued`} />
      <StatusDatum icon={<GitPullRequestArrow />} label={changeSummary ?? 'No file changes'} />
    </section>
  )
}

function StatusDatum({ icon, label }: { icon: React.ReactElement; label: string }) {
  return (
    <span className="flex shrink-0 items-center gap-1.5 text-muted-foreground">
      <span aria-hidden="true" className="[&_svg]:size-3.5">
        {icon}
      </span>
      {label}
    </span>
  )
}

function useClock(fixedNow?: number) {
  const [now, setNow] = useState(() => fixedNow ?? Date.now())
  useEffect(() => {
    if (fixedNow !== undefined) {
      setNow(fixedNow)
      return
    }
    const timer = window.setInterval(() => setNow(Date.now()), 1_000)
    return () => window.clearInterval(timer)
  }, [fixedNow])
  return fixedNow ?? now
}

function formatElapsed(startedAt: string, now: number) {
  const started = Date.parse(startedAt)
  if (!Number.isFinite(started)) return '—'
  const seconds = Math.max(0, Math.floor((now - started) / 1_000))
  if (seconds < 60) return `${seconds}s`
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`
}

function statusLabel(state: string) {
  if (state === 'waiting_permission') return 'Waiting for permission'
  if (state === 'yielding') return 'Preparing to yield'
  return 'Running'
}
