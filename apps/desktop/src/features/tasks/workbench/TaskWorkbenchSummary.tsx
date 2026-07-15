import {
  Bot,
  ChevronDown,
  ChevronUp,
  FileDiff,
  FileText,
  FolderGit2,
  ImageIcon,
} from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import type {
  TaskEventEnvelope,
  TaskProjection,
  TimelineItemProjection,
} from '@/generated/daemon-protocol'
import { cn } from '@/shared/lib/utils'
import { useUiStore } from '@/shared/state/ui-store'
import type { TaskWorkbenchTarget } from '@/shared/state/workbench-selection'
import { taskWorkbenchTargetKey } from '@/shared/state/workbench-selection'
import { type TaskWorkbenchSummaryItem, taskWorkbenchSummaryItems } from './task-workbench-summary'

export function TaskWorkbenchSummary({
  events,
  onOpen,
  projection,
  timeline,
}: {
  events: TaskEventEnvelope[]
  onOpen: (target: TaskWorkbenchTarget, trigger: HTMLElement) => void
  projection: TaskProjection
  timeline: TimelineItemProjection[]
}) {
  const { t } = useTranslation('tasks')
  const collapsed = useUiStore((state) => state.taskWorkbenchSummaryCollapsed)
  const setCollapsed = useUiStore((state) => state.setTaskWorkbenchSummaryCollapsed)
  const session = useUiStore((state) => state.taskWorkbenchByTaskId[projection.taskId])
  const entries = taskWorkbenchSummaryItems({
    events,
    labels: { subagents: t('workbench.summary.item.subagents') },
    projection,
    timeline,
  })
  const activeTarget = session?.tabs.find((tab) => tab.id === session.activeTabId)?.target
  const groups = groupEntries(entries)

  return (
    <aside
      aria-label={t('workbench.summary.label')}
      className="task-workbench-summary z-20 flex min-h-0 shrink-0 flex-col overflow-hidden rounded-2xl border border-border/80 bg-surface-raised/95 shadow-lg backdrop-blur-xl"
      data-collapsed={collapsed}
    >
      <header className="task-workbench-summary-header flex h-11 shrink-0 items-center justify-between px-4">
        <span className="font-medium text-xs text-muted-foreground">
          {t('workbench.summary.group.environment')}
        </span>
        <button
          aria-label={collapsed ? t('workbench.summary.expand') : t('workbench.summary.collapse')}
          className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => setCollapsed(!collapsed)}
          type="button"
        >
          {collapsed ? (
            <ChevronDown aria-hidden="true" className="size-4" />
          ) : (
            <ChevronUp aria-hidden="true" className="size-4" />
          )}
        </button>
      </header>
      {collapsed ? null : (
        <div className="task-workbench-summary-content min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {entries.length === 0 ? (
            <p className="px-2 py-3 text-muted-foreground text-xs">
              {t('workbench.summary.empty')}
            </p>
          ) : null}
          {groups.map(([group, groupItems], index) => (
            <section
              className={index > 0 ? 'mt-2 border-border/70 border-t pt-2' : undefined}
              key={group}
            >
              {group !== 'environment' ? (
                <h3 className="px-2 py-1.5 font-medium text-[11px] text-muted-foreground">
                  {t(`workbench.summary.group.${group}`)}
                </h3>
              ) : null}
              <div className="space-y-0.5">
                {groupItems.map((entry) => (
                  <SummaryRow
                    active={
                      activeTarget && entry.target
                        ? taskWorkbenchTargetKey(activeTarget) ===
                          taskWorkbenchTargetKey(entry.target)
                        : false
                    }
                    entry={entry}
                    key={entry.id}
                    onOpen={onOpen}
                  />
                ))}
              </div>
            </section>
          ))}
        </div>
      )}
    </aside>
  )
}

function SummaryRow({
  active,
  entry,
  onOpen,
}: {
  active: boolean
  entry: TaskWorkbenchSummaryItem
  onOpen: (target: TaskWorkbenchTarget, trigger: HTMLElement) => void
}) {
  const { t } = useTranslation('tasks')
  return (
    <SummaryRowContainer active={active} entry={entry} onOpen={onOpen}>
      <SummaryIcon entry={entry} />
      <span className="min-w-0">
        <span className="block truncate text-[13px]">{summaryTitle(entry, t)}</span>
        {showSummaryDetail(entry) ? (
          <span className="block truncate text-[10px] text-muted-foreground">{entry.detail}</span>
        ) : null}
      </span>
      <span
        className={cn(
          'shrink-0 text-[11px] tabular-nums text-muted-foreground',
          entry.status === 'running' && 'text-state-running',
          entry.status === 'failed' && 'text-state-failed',
        )}
      >
        {summaryMeta(entry, t)}
      </span>
    </SummaryRowContainer>
  )
}

function SummaryRowContainer({
  active,
  children,
  entry,
  onOpen,
}: {
  active: boolean
  children: ReactNode
  entry: TaskWorkbenchSummaryItem
  onOpen: (target: TaskWorkbenchTarget, trigger: HTMLElement) => void
}) {
  const className = cn(
    'grid min-h-10 w-full grid-cols-[28px_minmax(0,1fr)_auto] items-center gap-2 rounded-xl px-2 py-1.5 text-left transition-colors aria-current:bg-muted',
    entry.id === 'environment' && 'bg-muted/65',
  )
  if (!entry.target) return <div className={className}>{children}</div>
  return (
    <button
      aria-current={active ? 'true' : undefined}
      className={`${className} hover:bg-muted/80`}
      onClick={(event) => onOpen(entry.target as TaskWorkbenchTarget, event.currentTarget)}
      type="button"
    >
      {children}
    </button>
  )
}

function SummaryIcon({ entry }: { entry: TaskWorkbenchSummaryItem }) {
  const className = cn(
    'flex size-7 items-center justify-center rounded-lg border border-border/70 bg-background/45 text-muted-foreground',
    entry.status === 'running' && 'text-state-running',
    entry.status === 'failed' && 'text-state-failed',
  )
  const iconClassName = 'size-3.5'
  if (entry.id === 'changes') {
    return (
      <span className={className}>
        <FileDiff aria-hidden="true" className={iconClassName} />
      </span>
    )
  }
  if (entry.id === 'sources') {
    return (
      <span className={className}>
        <ImageIcon aria-hidden="true" className={iconClassName} />
      </span>
    )
  }
  if (entry.id === 'artifacts') {
    return (
      <span className={className}>
        <FileText aria-hidden="true" className={iconClassName} />
      </span>
    )
  }
  if (entry.id === 'subagents') {
    return (
      <span className={className}>
        <Bot aria-hidden="true" className={iconClassName} />
      </span>
    )
  }
  return (
    <span className={className}>
      <FolderGit2 aria-hidden="true" className={iconClassName} />
    </span>
  )
}

function groupEntries(entries: TaskWorkbenchSummaryItem[]) {
  const order = ['environment', 'sources', 'subagents'] as const
  return order
    .map((group) => [group, entries.filter((entry) => entry.group === group)] as const)
    .filter(([, groupEntries]) => groupEntries.length > 0)
}

function statusLabel(status: TaskWorkbenchSummaryItem['status'], t: (key: string) => string) {
  return status === 'running'
    ? t('workbench.summary.status.running')
    : status === 'failed'
      ? t('workbench.summary.status.failed')
      : ''
}

function summaryMeta(
  entry: TaskWorkbenchSummaryItem,
  t: (key: string, options?: Record<string, unknown>) => string,
) {
  if (entry.id === 'changes') {
    const stats = changeStats(entry.detail)
    if (stats) {
      return (
        <span className="flex items-center gap-1 font-medium tabular-nums">
          <span className="text-state-completed">+{stats.insertions}</span>
          <span className="text-state-failed">-{stats.deletions}</span>
        </span>
      )
    }
  }
  if (entry.id === 'subagents') {
    return [
      entry.runningCount
        ? t('workbench.summary.meta.runningCount', { count: entry.runningCount })
        : '',
      entry.failedCount
        ? t('workbench.summary.meta.failedRatio', {
            count: entry.failedCount,
            total: entry.count ?? entry.failedCount,
          })
        : '',
    ]
      .filter(Boolean)
      .join(' · ')
  }
  return [entry.count, statusLabel(entry.status, t)]
    .filter((value) => value !== undefined && value !== '')
    .join(' · ')
}

function summaryTitle(
  entry: TaskWorkbenchSummaryItem,
  t: (key: string, options?: Record<string, unknown>) => string,
) {
  if (entry.group === 'sources' && entry.detail) return entry.detail
  return t(`workbench.summary.item.${entry.id}`)
}

function showSummaryDetail(entry: TaskWorkbenchSummaryItem) {
  if (!entry.detail || entry.group === 'sources') return false
  return entry.id !== 'changes' || changeStats(entry.detail) === null
}

function changeStats(detail: string) {
  const insertions = detail.match(/(\d[\d,]*)\s+insertions?/i)?.[1]
  const deletions = detail.match(/(\d[\d,]*)\s+deletions?/i)?.[1]
  return insertions && deletions ? { deletions, insertions } : null
}
