import {
  Bot,
  Check,
  ChevronRight,
  CircleAlert,
  FileText,
  Globe2,
  ImageIcon,
  LoaderCircle,
  PencilLine,
  Search,
  SquareTerminal,
  Wrench,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { TimelineToolOperation, TimelineToolStatus } from '@/generated/daemon-protocol'
import { cn } from '@/shared/lib/utils'

import { isBrowserToolItem, TimelineItem } from './TimelineEvent'
import {
  type ToolTimelineItem,
  toolActivityGroupSummary,
  toolActivitySummary,
} from './tool-activity-summary'

export function ToolActivityGroup({
  items,
  onSelectItem,
}: {
  items: ToolTimelineItem[]
  onSelectItem?: (item: ToolTimelineItem, trigger?: HTMLElement) => void
}) {
  const { t } = useTranslation('tasks')
  const hasFailure = items.some((item) => ['denied', 'failed'].includes(item.tool.status))
  const hasActive = items.some((item) => ['requested', 'running'].includes(item.tool.status))
  const hasCommand = items.some(
    (item) => item.tool.operation === 'command' && (item.tool.command || item.tool.output),
  )
  const hasBrowser = items.some(isBrowserToolItem)
  const label = toolActivityGroupSummary(items, t)

  return (
    <details
      className="group/tool text-sm"
      open={hasFailure || hasActive || hasCommand || hasBrowser || undefined}
    >
      <summary
        className={cn(
          'flex w-fit cursor-pointer select-none items-center gap-2 rounded-md py-1 text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
          hasFailure && 'text-destructive',
        )}
      >
        <ChevronRight
          aria-hidden="true"
          className="size-3.5 shrink-0 transition-transform group-open/tool:rotate-90"
        />
        <ToolOperationIcon operation={groupOperation(items)} />
        <span>{label}</span>
        {hasActive ? (
          <LoaderCircle aria-hidden="true" className="size-3.5 shrink-0 animate-spin" />
        ) : null}
      </summary>
      <div className="mt-1.5 ml-[6px] space-y-1 border-border/70 border-l pl-5">
        {items.map((item) => (
          <TimelineItem item={item} key={item.id}>
            {onSelectItem && isOpenableToolItem(item) ? (
              <button
                aria-label={t('timeline.openPanel', {
                  panel: t(
                    isBrowserToolItem(item)
                      ? 'workbench.targetKind.browser'
                      : 'workbench.targetKind.command',
                  ),
                })}
                className="w-full rounded-md px-1 text-left hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                onClick={(event) => onSelectItem(item, event.currentTarget)}
                type="button"
              >
                <ToolDetail item={item} />
              </button>
            ) : (
              <ToolDetail item={item} />
            )}
          </TimelineItem>
        ))}
      </div>
    </details>
  )
}

function ToolDetail({ item }: { item: ToolTimelineItem }) {
  const { t } = useTranslation('tasks')
  const showCommand =
    item.tool.operation === 'command' && Boolean(item.tool.command || item.tool.output)
  return (
    <div className="py-1">
      <div className="flex min-w-0 flex-1 items-start gap-2 text-left">
        <ToolStatusIcon status={item.tool.status} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-foreground/90">{toolActivitySummary(item, t)}</div>
          {item.tool.resultSummary && !showCommand ? (
            <div className="truncate text-muted-foreground text-xs">{item.tool.resultSummary}</div>
          ) : null}
        </div>
        {item.tool.durationMs !== undefined && item.tool.durationMs !== null ? (
          <span className="shrink-0 text-muted-foreground text-xs">
            {formatDuration(item.tool.durationMs)}
          </span>
        ) : null}
      </div>
      {showCommand ? <CommandTranscript item={item} /> : null}
    </div>
  )
}

function CommandTranscript({ item }: { item: ToolTimelineItem }) {
  const { t } = useTranslation('tasks')
  const transcript = [item.tool.command ? `$ ${item.tool.command}` : null, item.tool.output]
    .filter(Boolean)
    .join('\n')
  return (
    <section
      aria-label={t('timeline.tool.commandOutput')}
      className="mt-2 overflow-hidden rounded-lg border border-border/80 bg-background/70"
    >
      <div className="border-border/70 border-b px-2.5 py-1.5 font-medium text-muted-foreground text-xs">
        {t('timeline.tool.shell')}
      </div>
      <pre className="max-h-64 overflow-auto px-2.5 py-2 font-mono text-[12px] text-foreground/85 leading-5">
        {transcript}
      </pre>
    </section>
  )
}

function isOpenableToolItem(item: ToolTimelineItem) {
  return (
    isBrowserToolItem(item) ||
    (item.tool.operation === 'command' && Boolean(item.tool.command || item.tool.output))
  )
}

function groupOperation(items: ToolTimelineItem[]): TimelineToolOperation {
  const first = items[0]?.tool.operation ?? 'other'
  return items.every((item) => item.tool.operation === first) ? first : 'other'
}

function ToolOperationIcon({ operation }: { operation: TimelineToolOperation }) {
  const className = 'size-4 shrink-0'
  if (operation === 'read') return <FileText aria-hidden="true" className={className} />
  if (operation === 'edit') return <PencilLine aria-hidden="true" className={className} />
  if (operation === 'search') return <Search aria-hidden="true" className={className} />
  if (operation === 'command') return <SquareTerminal aria-hidden="true" className={className} />
  if (operation === 'browse') return <Globe2 aria-hidden="true" className={className} />
  if (operation === 'generate') return <ImageIcon aria-hidden="true" className={className} />
  if (operation === 'delegate') return <Bot aria-hidden="true" className={className} />
  return <Wrench aria-hidden="true" className={className} />
}

function ToolStatusIcon({ status }: { status: TimelineToolStatus }) {
  const className = 'mt-0.5 size-3.5 shrink-0'
  if (status === 'requested' || status === 'running') {
    return <LoaderCircle aria-hidden="true" className={`${className} animate-spin`} />
  }
  if (status === 'failed' || status === 'denied') {
    return <CircleAlert aria-hidden="true" className={`${className} text-destructive`} />
  }
  return <Check aria-hidden="true" className={className} />
}

function formatDuration(durationMs: number) {
  if (durationMs < 1_000) return `${durationMs}ms`
  return `${(durationMs / 1_000).toFixed(durationMs < 10_000 ? 1 : 0)}s`
}
