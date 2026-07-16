import {
  AlertTriangle,
  Bot,
  CircleAlert,
  FileDiff,
  FileText,
  ImageIcon,
  Info,
  KeyRound,
  Search,
  Sparkles,
  SquareTerminal,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ArtifactRenderer } from '@/features/artifacts/ArtifactRenderer'
import {
  type ArtifactDescriptor,
  type ContentBlock,
  timelineContentBlocks,
} from '@/features/artifacts/model'
import type { TimelineItemProjection } from '@/generated/daemon-protocol'
import { MarkdownMessage } from '@/shared/markdown/MarkdownMessage'

import { ArtifactContainer } from './ArtifactContainer'
import { timelineSummary } from './timeline-summary'
import { toolActivitySummary } from './tool-activity-summary'
import { UserMessage } from './UserMessage'

export function TimelineEvent({
  item,
  onSelect,
}: {
  item: TimelineItemProjection
  onSelect?: (item: TimelineItemProjection, trigger?: HTMLElement) => void
}) {
  const blocks = timelineContentBlocks(item)
  const artifactCount = blocks.filter((block) => block.type === 'artifact').length
  const content = (
    <TimelineItem item={item}>
      <div className="space-y-3">
        {blocks.map((block, index) => (
          <ContentBlockView
            artifactCount={artifactCount}
            block={block}
            blockIndex={index}
            item={item}
            key={contentBlockKey(block, index)}
            onSelect={onSelect}
          />
        ))}
      </div>
    </TimelineItem>
  )
  return item.kind === 'assistant_text' ? <div data-narrative="true">{content}</div> : content
}

function ContentBlockView({
  artifactCount,
  block,
  blockIndex,
  item,
  onSelect,
}: {
  artifactCount: number
  block: ContentBlock
  blockIndex: number
  item: TimelineItemProjection
  onSelect?: (item: TimelineItemProjection, trigger?: HTMLElement) => void
}) {
  const { t } = useTranslation('tasks')
  if (block.type === 'text') {
    if (item.kind === 'user_message') return <UserMessage content={block.text} />
    if (block.format === 'markdown') {
      return (
        <div data-incomplete={item.incomplete ? 'true' : undefined}>
          <MarkdownMessage className="text-[15px] text-foreground">{block.text}</MarkdownMessage>
        </div>
      )
    }
    return (
      <div
        className="whitespace-pre-wrap break-words text-[15px] text-foreground"
        data-incomplete={item.incomplete ? 'true' : undefined}
      >
        {block.text}
      </div>
    )
  }
  if (block.type === 'artifact') {
    const selectionItem = itemForArtifact(item, block.artifact, blockIndex, artifactCount)
    const kind = block.artifact.artifactKind ?? item.kind
    const canOpen = Boolean(
      onSelect &&
        (item.kind === 'command' ||
          block.artifact.blobId ||
          block.artifact.presentation?.previewBlobId ||
          block.artifact.preview),
    )
    const surface = block.artifact.presentation?.preferredSurface === 'inline' ? 'inline' : 'card'
    const useLegacySummary = ['command', 'diff', 'terminal'].includes(kind)
    return (
      <ArtifactContainer
        item={selectionItem}
        label={t(artifactLabelKey(kind))}
        onOpen={canOpen && onSelect ? (trigger) => onSelect(selectionItem, trigger) : undefined}
        openLabel={
          canOpen
            ? t('timeline.openPanel', { panel: t(artifactWorkbenchLabelKey(kind)) })
            : undefined
        }
      >
        {useLegacySummary ? (
          <div className="flex items-start gap-2.5 text-sm leading-6">
            <ArtifactIcon kind={kind} />
            <span className={kind === 'command' ? 'font-mono text-[13px]' : undefined}>
              {block.artifact.title}
            </span>
          </div>
        ) : (
          <ArtifactRenderer artifact={block.artifact} surface={surface} />
        )}
      </ArtifactContainer>
    )
  }
  if (block.type === 'tool_activity') {
    const toolItem: TimelineItemProjection = {
      ...item,
      contentBlocks: [{ activity: block.activity, type: 'tool_activity' }],
      kind: 'tool_activity',
      tool: block.activity,
    }
    const row = (
      <div className="flex items-start gap-2.5 py-1 text-muted-foreground text-sm leading-5">
        <Search aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
        <span>{toolActivitySummary(toolItem, t)}</span>
        {item.incomplete ? <span className="sr-only">{t('timeline.incomplete')}</span> : null}
      </div>
    )
    const canOpen =
      onSelect &&
      (isBrowserToolItem(toolItem) ||
        (toolItem.tool?.operation === 'command' &&
          Boolean(toolItem.tool.command || toolItem.tool.output)))
    return canOpen && onSelect ? (
      <button
        aria-label={t('timeline.openPanel', {
          panel: t(
            isBrowserToolItem(toolItem)
              ? 'workbench.targetKind.browser'
              : 'workbench.targetKind.command',
          ),
        })}
        className="w-full rounded-md text-left hover:bg-muted/60"
        onClick={(event) => onSelect(toolItem, event.currentTarget)}
        type="button"
      >
        {row}
      </button>
    ) : (
      row
    )
  }

  const text = block.text === item.summary ? timelineSummary(item, t) : block.text
  const notice = (
    <div
      className={`flex items-start gap-2.5 py-1 text-sm leading-5 ${
        block.level === 'error' ? 'text-red-700 dark:text-red-400' : 'text-muted-foreground'
      }`}
      data-notice-level={block.level}
    >
      <NoticeIcon item={item} level={block.level} />
      <span>{text}</span>
      {item.incomplete ? <span className="sr-only">{t('timeline.incomplete')}</span> : null}
    </div>
  )
  return onSelect && item.kind === 'error' ? (
    <button
      aria-label={t('timeline.openPanel', { panel: t('workbench.targetKind.audit') })}
      className="w-full rounded-md text-left hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      onClick={(event) => onSelect(item, event.currentTarget)}
      type="button"
    >
      {notice}
    </button>
  ) : (
    notice
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

function itemForArtifact(
  item: TimelineItemProjection,
  artifact: ArtifactDescriptor,
  blockIndex: number,
  artifactCount: number,
): TimelineItemProjection {
  if (artifactCount === 1) return item
  return {
    ...item,
    blobId: artifact.blobId,
    contentBlocks: [{ artifact, type: 'artifact' }],
    summary: artifact.title,
    semanticGroupId: `${item.semanticGroupId ?? item.id}:artifact:${blockIndex}`,
  }
}

function contentBlockKey(block: ContentBlock, index: number) {
  if (block.type === 'artifact') {
    return `artifact:${block.artifact.artifactId ?? block.artifact.blobId ?? block.artifact.title}:${index}`
  }
  if (block.type === 'tool_activity') return `tool:${block.activity.toolUseId}:${index}`
  return `${block.type}:${index}`
}

function artifactLabelKey(kind: string) {
  if (kind === 'command' || kind === 'terminal') return 'timeline.command'
  if (kind === 'diff') return 'timeline.changes'
  if (kind === 'image' || kind === 'screenshot') return 'timeline.image'
  if (kind === 'file') return 'workbench.targetKind.file'
  return 'timeline.output'
}

function artifactWorkbenchLabelKey(kind: string) {
  if (kind === 'diff') return 'workbench.tabs.changes'
  if (kind === 'command' || kind === 'terminal') return 'workbench.tabs.commands'
  if (kind === 'file') return 'workbench.targetKind.file'
  if (kind === 'image' || kind === 'screenshot') return 'workbench.tabs.sources'
  return 'workbench.targetKind.artifact'
}

function ArtifactIcon({ kind }: { kind: string }) {
  const className = 'mt-1 size-4 shrink-0 text-muted-foreground'
  if (kind === 'command' || kind === 'terminal') {
    return <SquareTerminal aria-hidden="true" className={className} />
  }
  if (kind === 'diff') return <FileDiff aria-hidden="true" className={className} />
  if (kind === 'image' || kind === 'screenshot') {
    return <ImageIcon aria-hidden="true" className={className} />
  }
  return <FileText aria-hidden="true" className={className} />
}

function NoticeIcon({
  item,
  level,
}: {
  item: TimelineItemProjection
  level: Extract<ContentBlock, { type: 'notice' }>['level']
}) {
  const className = 'mt-0.5 size-4 shrink-0'
  if (level === 'error') return <CircleAlert aria-hidden="true" className={className} />
  if (item.kind === 'permission') return <KeyRound aria-hidden="true" className={className} />
  if (level === 'warning') return <AlertTriangle aria-hidden="true" className={className} />
  if (item.kind === 'subagent') return <Bot aria-hidden="true" className={className} />
  if (item.kind === 'tool_activity') return <Search aria-hidden="true" className={className} />
  if (item.kind === 'notice') return <Info aria-hidden="true" className={className} />
  return <Sparkles aria-hidden="true" className={className} />
}

export function isBrowserToolItem(item: TimelineItemProjection) {
  return (
    item.kind === 'tool_activity' &&
    ['BrowserUse', 'BrowserDevTools'].includes(item.tool?.toolName ?? '')
  )
}
