import {
  AlertTriangle,
  Bot,
  FileDiff,
  FileText,
  ImageIcon,
  KeyRound,
  Search,
  Sparkles,
  SquareTerminal,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { TimelineItemProjection } from '@/generated/daemon-protocol'
import { MarkdownMessage } from '@/shared/markdown/MarkdownMessage'

import { ArtifactContainer } from './ArtifactContainer'
import { timelineSummary } from './timeline-summary'
import { UserMessage } from './UserMessage'

export function TimelineEvent({
  item,
  onSelect,
}: {
  item: TimelineItemProjection
  onSelect?: (item: TimelineItemProjection, trigger?: HTMLElement) => void
}) {
  const { t } = useTranslation('tasks')
  const displaySummary = timelineSummary(item, t)
  if (item.kind === 'user_message') {
    const message = <UserMessage content={item.summary} />
    return (
      <TimelineItem item={item}>
        {item.blobId && onSelect ? (
          <button
            aria-label={t('timeline.openPanel', { panel: t('workbench.targetKind.file') })}
            className="ml-auto block rounded-2xl text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onClick={(event) => onSelect(item, event.currentTarget)}
            type="button"
          >
            {message}
          </button>
        ) : (
          message
        )}
      </TimelineItem>
    )
  }

  if (item.kind === 'assistant_text') {
    return (
      <div data-narrative="true">
        <TimelineItem item={item}>
          <div data-incomplete={item.incomplete ? 'true' : undefined}>
            <MarkdownMessage className="text-[15px] text-foreground">
              {item.summary}
            </MarkdownMessage>
          </div>
        </TimelineItem>
      </div>
    )
  }

  if (isArtifact(item)) {
    const canOpen = Boolean(onSelect && supportsWorkbenchTarget(item))
    return (
      <TimelineItem item={item}>
        <ArtifactContainer
          item={item}
          label={t(artifactLabelKey(item.kind))}
          onOpen={canOpen && onSelect ? (trigger) => onSelect(item, trigger) : undefined}
          openLabel={
            canOpen ? t('timeline.openPanel', { panel: t(workbenchLabelKey(item)) }) : undefined
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
          onClick={(event) => onSelect(item, event.currentTarget)}
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

export function isLowValueLifecycleItem(item: TimelineItemProjection) {
  return (
    item.kind === 'notice' &&
    [
      'Run completed',
      'Run started',
      'Task created',
      'Workspace acquired',
      'Workspace released',
    ].includes(item.summary)
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
      className={
        inline
          ? 'whitespace-pre-wrap rounded-sm transition-colors data-[located=true]:bg-accent/60'
          : 'rounded-md transition-colors data-[located=true]:bg-accent/60 data-[located=true]:ring-2 data-[located=true]:ring-ring'
      }
      data-event-id={item.id}
      data-offset={item.globalOffset}
      data-testid="timeline-item"
      tabIndex={-1}
    >
      {children}
    </Component>
  )
}

function isArtifact(item: TimelineItemProjection) {
  return (
    ['artifact', 'command', 'diff', 'file', 'image', 'permission', 'error'].includes(item.kind) ||
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
  if (kind === 'command') return <SquareTerminal aria-hidden="true" className={className} />
  if (kind === 'diff') return <FileDiff aria-hidden="true" className={className} />
  if (kind === 'artifact' || kind === 'file') {
    return <FileText aria-hidden="true" className={className} />
  }
  if (kind === 'image') return <ImageIcon aria-hidden="true" className={className} />
  if (kind === 'permission') return <KeyRound aria-hidden="true" className={className} />
  return <AlertTriangle aria-hidden="true" className="mt-1 size-4 shrink-0 text-destructive" />
}

function RowIcon({ kind }: { kind: TimelineItemProjection['kind'] }) {
  const className = 'mt-0.5 size-4 shrink-0'
  if (kind === 'tool_activity') return <Search aria-hidden="true" className={className} />
  if (kind === 'subagent') return <Bot aria-hidden="true" className={className} />
  return <Sparkles aria-hidden="true" className={className} />
}

function isWorkbenchRow(item: TimelineItemProjection) {
  return item.kind === 'subagent'
}

function supportsWorkbenchTarget(item: TimelineItemProjection) {
  if (item.kind === 'subagent') return true
  return Boolean(item.blobId && ['diff', 'file', 'artifact', 'image'].includes(item.kind))
}

function workbenchLabelKey(item: TimelineItemProjection) {
  if (item.kind === 'diff') return 'workbench.tabs.changes'
  if (item.kind === 'command') return 'workbench.tabs.commands'
  if (item.kind === 'file') return 'workbench.targetKind.file'
  if (item.kind === 'artifact') return 'workbench.targetKind.artifact'
  if (item.kind === 'subagent') return 'workbench.tabs.agents'
  if (item.kind === 'image') return 'workbench.tabs.sources'
  if (item.kind === 'notice' && item.summary.toLowerCase().startsWith('workspace')) {
    return 'workbench.tabs.environment'
  }
  return 'workbench.tabs.audit'
}
