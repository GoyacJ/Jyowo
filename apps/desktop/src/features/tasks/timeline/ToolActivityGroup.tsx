import {
  Bot,
  ChevronDown,
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

import {
  type ArtifactDescriptor,
  artifactDescriptorFromTimelineItem,
} from '@/features/artifacts/model'
import { useArtifactResource } from '@/features/artifacts/resource'
import { DiffPanel } from '@/features/tasks/workbench/DiffPanel'
import type {
  TimelineItemProjection,
  TimelineToolOperation,
  TimelineToolStatus,
} from '@/generated/daemon-protocol'
import { cn } from '@/shared/lib/utils'

import { isBrowserToolItem, TimelineItem } from './TimelineEvent'
import {
  type ToolTimelineItem,
  toolActivityGroupSummary,
  toolActivitySummary,
} from './tool-activity-summary'

export function ToolActivityGroup({
  artifacts = [],
  items,
  onSelectItem,
}: {
  artifacts?: TimelineItemProjection[]
  items: ToolTimelineItem[]
  onSelectItem?: (item: ToolTimelineItem, trigger?: HTMLElement) => void
}) {
  const { t } = useTranslation('tasks')
  const hasFailure = items.some((item) => ['denied', 'failed'].includes(item.tool.status))
  const hasActive = items.some((item) => ['requested', 'running'].includes(item.tool.status))
  const fileArtifacts = artifacts.flatMap((item) => {
    const artifact = artifactDescriptorFromTimelineItem(item)
    return artifact ? [{ artifact, item }] : []
  })
  const label = toolActivityGroupSummary(items, t)

  return (
    <details className="group/tool text-sm" open={hasActive || undefined}>
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
            {onSelectItem && isOpenableToolItem(item, fileArtifacts) ? (
              <ToolDetail
                item={item}
                openLabel={t('timeline.openPanel', {
                  panel: t(openableTargetLabel(item, fileArtifacts)),
                })}
                onOpen={(trigger) => onSelectItem(selectionItem(item, fileArtifacts), trigger)}
              />
            ) : (
              <ToolDetail item={item} />
            )}
          </TimelineItem>
        ))}
        <FileChangeGroup artifacts={fileArtifacts} />
      </div>
    </details>
  )
}

function ToolDetail({
  item,
  openLabel,
  onOpen,
}: {
  item: ToolTimelineItem
  openLabel?: string
  onOpen?: (trigger: HTMLButtonElement) => void
}) {
  const { t } = useTranslation('tasks')
  const showCommand =
    item.tool.operation === 'command' && Boolean(item.tool.command || item.tool.output)
  const header = (
    <div className="flex min-w-0 flex-1 items-start gap-2 text-left">
      <ToolStatusIcon operation={item.tool.operation} status={item.tool.status} />
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
  )
  return (
    <div className="py-1">
      {onOpen ? (
        <button
          aria-label={openLabel}
          className="w-full rounded-md px-1 text-left hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={(event) => onOpen(event.currentTarget)}
          type="button"
        >
          {header}
        </button>
      ) : (
        header
      )}
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

type FileArtifact = { artifact: ArtifactDescriptor; item: TimelineItemProjection }

function isOpenableToolItem(item: ToolTimelineItem, artifacts: FileArtifact[]) {
  return (
    isBrowserToolItem(item) ||
    (item.tool.operation === 'command' && Boolean(item.tool.command || item.tool.output)) ||
    Boolean(linkedArtifact(item, artifacts))
  )
}

function linkedArtifact(item: ToolTimelineItem, artifacts: FileArtifact[]) {
  return artifacts.find(({ artifact }) => artifact.sourceToolUseId === item.tool.toolUseId)
}

function selectionItem(item: ToolTimelineItem, artifacts: FileArtifact[]): ToolTimelineItem {
  const linked = linkedArtifact(item, artifacts)
  if (!linked) return item
  return {
    ...item,
    blobId: linked.item.blobId,
    contentBlocks: linked.item.contentBlocks,
    globalOffset: item.globalOffset,
    id: item.id,
    kind: linked.item.kind,
    runSegmentId: item.runSegmentId,
    summary: linked.item.summary,
  }
}

function openableTargetLabel(item: ToolTimelineItem, artifacts: FileArtifact[]) {
  if (isBrowserToolItem(item)) return 'workbench.targetKind.browser'
  const linked = linkedArtifact(item, artifacts)
  if (linked?.artifact.artifactKind === 'diff' || linked?.artifact.artifactKind === 'patch') {
    return 'workbench.targetKind.diff'
  }
  if (linked) return 'workbench.targetKind.file'
  return 'workbench.targetKind.command'
}

function FileChangeGroup({ artifacts }: { artifacts: FileArtifact[] }) {
  const { t } = useTranslation('tasks')
  const diffs = artifacts.filter(({ artifact }) =>
    ['diff', 'patch'].includes(artifact.artifactKind ?? ''),
  )
  if (diffs.length === 0) return null
  return (
    <details className="group/changes pt-1" open>
      <summary className="flex w-fit cursor-pointer list-none items-center gap-2 rounded-md py-1 text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <ChevronDown
          aria-hidden="true"
          className="size-3.5 transition-transform group-open/changes:rotate-180"
        />
        <PencilLine aria-hidden="true" className="size-4" />
        <span>{t('timeline.tool.editedFiles', { count: diffs.length })}</span>
      </summary>
      <div className="mt-1.5 space-y-2">
        {diffs.map(({ artifact, item }) => (
          <InlineDiffArtifact artifact={artifact} key={item.id} />
        ))}
      </div>
    </details>
  )
}

function InlineDiffArtifact({ artifact }: { artifact: ArtifactDescriptor }) {
  const resource = useArtifactResource(artifact, undefined, 'card')
  return (
    <DiffPanel
      error={resource.error}
      loading={resource.loading}
      missing={resource.missing}
      onRetry={resource.retry}
      surface="inline"
      text={resource.text}
    />
  )
}

function groupOperation(items: ToolTimelineItem[]): TimelineToolOperation {
  const first = items[0]?.tool.operation ?? 'other'
  return items.every((item) => item.tool.operation === first) ? first : 'other'
}

function ToolOperationIcon({
  className = 'size-4 shrink-0',
  operation,
}: {
  className?: string
  operation: TimelineToolOperation
}) {
  if (operation === 'read') return <FileText aria-hidden="true" className={className} />
  if (operation === 'edit') return <PencilLine aria-hidden="true" className={className} />
  if (operation === 'search') return <Search aria-hidden="true" className={className} />
  if (operation === 'command') return <SquareTerminal aria-hidden="true" className={className} />
  if (operation === 'browse') return <Globe2 aria-hidden="true" className={className} />
  if (operation === 'generate') return <ImageIcon aria-hidden="true" className={className} />
  if (operation === 'delegate') return <Bot aria-hidden="true" className={className} />
  return <Wrench aria-hidden="true" className={className} />
}

function ToolStatusIcon({
  operation,
  status,
}: {
  operation: TimelineToolOperation
  status: TimelineToolStatus
}) {
  const className = 'mt-0.5 size-3.5 shrink-0'
  if (status === 'requested' || status === 'running') {
    return <LoaderCircle aria-hidden="true" className={`${className} animate-spin`} />
  }
  if (status === 'failed' || status === 'denied') {
    return <CircleAlert aria-hidden="true" className={`${className} text-destructive`} />
  }
  return <ToolOperationIcon className={className} operation={operation} />
}

function formatDuration(durationMs: number) {
  if (durationMs < 1_000) return `${durationMs}ms`
  return `${(durationMs / 1_000).toFixed(durationMs < 10_000 ? 1 : 0)}s`
}
