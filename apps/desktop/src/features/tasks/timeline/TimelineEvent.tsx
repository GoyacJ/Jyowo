import {
  AlertTriangle,
  Bot,
  FileDiff,
  ImageIcon,
  KeyRound,
  Search,
  Sparkles,
  SquareTerminal,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { TimelineItemProjection } from '@/generated/daemon-protocol'

import { ArtifactContainer } from './ArtifactContainer'
import { timelineSummary } from './timeline-summary'
import { UserMessage } from './UserMessage'

export function TimelineEvent({
  item,
  onSelect,
}: {
  item: TimelineItemProjection
  onSelect?: (item: TimelineItemProjection) => void
}) {
  const { t } = useTranslation('tasks')
  const displaySummary = timelineSummary(item, t)
  if (item.kind === 'user_message') {
    return (
      <TimelineItem item={item}>
        <UserMessage content={item.summary} />
      </TimelineItem>
    )
  }

  if (item.kind === 'assistant_text') {
    return (
      <div className="text-[15px] leading-7 text-foreground" data-narrative="true">
        <TimelineItem item={item} inline>
          <span data-incomplete={item.incomplete ? 'true' : undefined}>{item.summary}</span>
        </TimelineItem>
      </div>
    )
  }

  if (isArtifact(item)) {
    return (
      <TimelineItem item={item}>
        <ArtifactContainer
          item={item}
          label={t(artifactLabelKey(item.kind))}
          onOpen={onSelect ? () => onSelect(item) : undefined}
          openLabel={
            onSelect ? t('timeline.openPanel', { panel: t(workbenchLabelKey(item)) }) : undefined
          }
        >
          <div className="flex items-start gap-2.5 text-sm leading-6">
            <ArtifactIcon kind={item.kind} />
            <span className={item.kind === 'command' ? 'font-mono text-[13px]' : undefined}>
              {displaySummary}
            </span>
          </div>
        </ArtifactContainer>
      </TimelineItem>
    )
  }

  const row = (
    <div className="flex items-start gap-2.5 py-1 text-muted-foreground text-sm leading-5">
      <RowIcon kind={item.kind} />
      <span>{displaySummary}</span>
      {item.incomplete ? <span className="sr-only">{t('timeline.incomplete')}</span> : null}
    </div>
  )
  return (
    <TimelineItem item={item}>
      {onSelect && isWorkbenchRow(item) ? (
        <button
          aria-label={t('timeline.openPanel', { panel: t(workbenchLabelKey(item)) })}
          className="w-full rounded-md text-left hover:bg-muted/60"
          onClick={() => onSelect(item)}
          type="button"
        >
          {row}
        </button>
      ) : (
        row
      )}
    </TimelineItem>
  )
}

export function TimelineItem({
  children,
  inline = false,
  item,
}: {
  children: React.ReactNode
  inline?: boolean
  item: TimelineItemProjection
}) {
  const Component = inline ? 'span' : 'div'
  return (
    <Component
      data-offset={item.globalOffset}
      data-testid="timeline-item"
      className={inline ? 'whitespace-pre-wrap' : undefined}
    >
      {children}
    </Component>
  )
}

function isArtifact(item: TimelineItemProjection) {
  return (
    ['command', 'diff', 'image', 'permission', 'error'].includes(item.kind) ||
    item.summary.length > 600
  )
}

function artifactLabelKey(kind: TimelineItemProjection['kind']) {
  const labels: Partial<Record<TimelineItemProjection['kind'], string>> = {
    command: 'timeline.command',
    diff: 'timeline.changes',
    error: 'timeline.actionRequired',
    image: 'timeline.image',
    permission: 'timeline.permission',
  }
  return labels[kind] ?? 'timeline.output'
}

function ArtifactIcon({ kind }: { kind: TimelineItemProjection['kind'] }) {
  const className = 'mt-1 size-4 shrink-0 text-muted-foreground'
  if (kind === 'command') return <SquareTerminal className={className} />
  if (kind === 'diff') return <FileDiff className={className} />
  if (kind === 'image') return <ImageIcon className={className} />
  if (kind === 'permission') return <KeyRound className={className} />
  return <AlertTriangle className="mt-1 size-4 shrink-0 text-destructive" />
}

function RowIcon({ kind }: { kind: TimelineItemProjection['kind'] }) {
  const className = 'mt-0.5 size-4 shrink-0'
  if (kind === 'tool_activity') return <Search className={className} />
  if (kind === 'subagent') return <Bot className={className} />
  return <Sparkles className={className} />
}

function isWorkbenchRow(item: TimelineItemProjection) {
  return ['compaction', 'notice', 'subagent', 'tool_activity'].includes(item.kind)
}

function workbenchLabelKey(item: TimelineItemProjection) {
  if (item.kind === 'diff') return 'workbench.tabs.changes'
  if (item.kind === 'command') return 'workbench.tabs.commands'
  if (item.kind === 'subagent') return 'workbench.tabs.agents'
  if (item.kind === 'image') return 'workbench.tabs.sources'
  if (item.kind === 'notice' && item.summary.toLowerCase().startsWith('workspace')) {
    return 'workbench.tabs.environment'
  }
  return 'workbench.tabs.audit'
}
