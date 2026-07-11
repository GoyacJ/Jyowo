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

import type { TimelineItemProjection } from '@/generated/daemon-protocol'

import { ArtifactContainer } from './ArtifactContainer'
import { UserMessage } from './UserMessage'

export function TimelineEvent({ item }: { item: TimelineItemProjection }) {
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
        <ArtifactContainer item={item} label={artifactLabel(item.kind)}>
          <div className="flex items-start gap-2.5 text-sm leading-6">
            <ArtifactIcon kind={item.kind} />
            <span className={item.kind === 'command' ? 'font-mono text-[13px]' : undefined}>
              {item.summary}
            </span>
          </div>
        </ArtifactContainer>
      </TimelineItem>
    )
  }

  return (
    <TimelineItem item={item}>
      <div className="flex items-start gap-2.5 py-1 text-muted-foreground text-sm leading-5">
        <RowIcon kind={item.kind} />
        <span>{item.summary}</span>
        {item.incomplete ? <span className="sr-only">Incomplete</span> : null}
      </div>
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

function artifactLabel(kind: TimelineItemProjection['kind']) {
  const labels: Partial<Record<TimelineItemProjection['kind'], string>> = {
    command: 'Command',
    diff: 'Changes',
    error: 'Action required',
    image: 'Image',
    permission: 'Permission',
  }
  return labels[kind] ?? 'Output'
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
