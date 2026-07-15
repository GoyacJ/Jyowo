import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import type { TimelineItemProjection } from '@/generated/daemon-protocol'

import { RunSegment } from './RunSegment'
import { isLowValueLifecycleItem, TimelineEvent } from './TimelineEvent'
import { timelineSummary } from './timeline-summary'
import { toolActivitySummary } from './tool-activity-summary'
import { useTaskScrollAnchor, type VirtualAnchorAdapter } from './use-task-scroll-anchor'

const estimatedBlockHeightPx = 180
const segmentChunkSize = 16
const virtualListThreshold = 24

type TimelineBlock =
  | {
      item: TimelineItemProjection
      key: string
      kind: 'item'
    }
  | {
      items: TimelineItemProjection[]
      key: string
      kind: 'segment'
      segmentId: string
    }

export function TaskTimeline({
  focusRequest,
  items,
  onSelectItem,
  taskId,
}: {
  focusRequest?: { eventId: string; nonce: number } | null
  items: TimelineItemProjection[]
  onSelectItem?: (item: TimelineItemProjection, trigger?: HTMLElement) => void
  taskId?: string
}) {
  const { t } = useTranslation('tasks')
  const orderedItems = [...items].sort(
    (left, right) => left.globalOffset - right.globalOffset || left.id.localeCompare(right.id),
  )
  const conversationItems = orderedItems.filter((item) => !isLowValueLifecycleItem(item))
  const latest = conversationItems.at(-1)
  const first = conversationItems.at(0)
  const blocks = createBlocks(coalesceAssistantItems(conversationItems))
  const virtualAnchorAdapterRef = useRef<VirtualAnchorAdapter>({ resolve: () => null })
  const {
    contentRef,
    endRef,
    onKeyDown,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onScroll,
    onTouchEnd,
    onTouchMove,
    onTouchStart,
    onWheel,
    pauseFollowing,
    runProgrammaticScroll,
    viewportRef,
  } = useTaskScrollAnchor(latest?.id ?? null, {
    prependAnchorKey: first?.id,
    streamingScrollTick: latest?.incomplete ? `${latest.id}:${latest.summary.length}` : undefined,
    taskId,
    virtualAnchorAdapter: virtualAnchorAdapterRef.current,
  })
  const useVirtualList = blocks.length >= virtualListThreshold
  const rowVirtualizer = useVirtualizer({
    count: useVirtualList ? blocks.length : 0,
    estimateSize: () => estimatedBlockHeightPx,
    getItemKey: (index) => blocks[index]?.key ?? index,
    getScrollElement: () => viewportRef.current,
    initialRect: { height: 900, width: 820 },
    overscan: 4,
  })
  const virtualRows = rowVirtualizer.getVirtualItems()
  virtualAnchorAdapterRef.current.resolve = (key, fallbackIndex) => {
    const index = blocks.findIndex((block) => block.key === key)
    const resolvedIndex = index >= 0 ? index : fallbackIndex
    if (resolvedIndex === undefined || resolvedIndex < 0 || resolvedIndex >= blocks.length) {
      return null
    }
    const mounted = rowVirtualizer
      .getVirtualItems()
      .find((virtualItem) => virtualItem.index === resolvedIndex)
    if (mounted) return { index: resolvedIndex, start: mounted.start }
    const measured = rowVirtualizer
      .takeSnapshot()
      .find((virtualItem) => virtualItem.index === resolvedIndex)
    if (measured) return { index: resolvedIndex, start: measured.start }
    const offset = rowVirtualizer.getOffsetForIndex(resolvedIndex, 'start')?.[0]
    return offset === undefined ? null : { index: resolvedIndex, start: offset }
  }
  const visibleRows =
    virtualRows.length > 0
      ? virtualRows.map((row) => ({ index: row.index, key: row.key, start: row.start }))
      : blocks.slice(0, 8).map((block, index) => ({
          index,
          key: block.key,
          start: index * estimatedBlockHeightPx,
        }))
  const handledFocusNonceRef = useRef<number | null>(null)

  useEffect(() => {
    if (!focusRequest) return
    if (handledFocusNonceRef.current === focusRequest.nonce) return
    const blockIndex = blocks.findIndex((block) => blockContainsEvent(block, focusRequest.eventId))
    if (blockIndex < 0) return
    handledFocusNonceRef.current = focusRequest.nonce
    let cancelled = false
    let highlightTimer: number | undefined
    const focusFrames = new Set<number>()
    const focusTarget = (attempt: number) => {
      const frame = requestAnimationFrame(() => {
        focusFrames.delete(frame)
        if (cancelled) return
        const viewport = viewportRef.current
        const target = viewport ? findTimelineEvent(viewport, focusRequest.eventId) : null
        if (!target) {
          if (attempt < 3) focusTarget(attempt + 1)
          return
        }
        const disclosure = target.closest('details')
        if (disclosure) disclosure.open = true
        runProgrammaticScroll(() => target.scrollIntoView({ block: 'center' }))
        pauseFollowing()
        target.focus({ preventScroll: true })
        target.dataset.located = 'true'
        highlightTimer = window.setTimeout(() => {
          delete target.dataset.located
        }, 1_600)
      })
      focusFrames.add(frame)
    }
    runProgrammaticScroll(() => {
      if (useVirtualList) rowVirtualizer.scrollToIndex(blockIndex, { align: 'center' })
    })
    focusTarget(0)
    return () => {
      cancelled = true
      for (const frame of focusFrames) cancelAnimationFrame(frame)
      if (highlightTimer !== undefined) window.clearTimeout(highlightTimer)
    }
  }, [
    blocks,
    focusRequest,
    pauseFollowing,
    rowVirtualizer,
    runProgrammaticScroll,
    useVirtualList,
    viewportRef,
  ])

  return (
    <div className="relative min-h-0 min-w-0 flex-1">
      <p aria-live="polite" className="sr-only" role="status">
        <span key={latest?.id}>
          {latest
            ? t('timeline.update', {
                summary:
                  latest.kind === 'tool_activity' && latest.tool
                    ? toolActivitySummary(latest, t)
                    : timelineSummary(latest, t),
              })
            : t('timeline.noActivity')}
        </span>
      </p>
      <section
        aria-label={t('timeline.label')}
        className="h-full overflow-y-auto overscroll-contain px-1 pb-28"
        data-testid="task-timeline-viewport"
        onKeyDown={onKeyDown}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onScroll={onScroll}
        onTouchEnd={onTouchEnd}
        onTouchMove={onTouchMove}
        onTouchStart={onTouchStart}
        onWheel={onWheel}
        ref={viewportRef}
      >
        {useVirtualList ? (
          <div
            className="relative"
            data-testid="task-timeline-scroll-content"
            ref={contentRef}
            style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
          >
            {visibleRows.map((virtualRow) => {
              const block = blocks[virtualRow.index]
              if (!block) return null
              return (
                <div
                  className="absolute top-0 left-0 w-full pb-5"
                  data-index={virtualRow.index}
                  data-timeline-block={block.key}
                  key={block.key}
                  ref={rowVirtualizer.measureElement}
                  style={{ transform: `translateY(${virtualRow.start}px)` }}
                >
                  <TimelineBlockView block={block} onSelectItem={onSelectItem} />
                </div>
              )
            })}
            <div
              aria-hidden="true"
              className="absolute left-0 w-full"
              ref={endRef}
              style={{ top: `${rowVirtualizer.getTotalSize()}px` }}
            />
          </div>
        ) : (
          <div className="space-y-5" data-testid="task-timeline-scroll-content" ref={contentRef}>
            {blocks.map((block) => (
              <div data-timeline-block={block.key} key={block.key}>
                <TimelineBlockView block={block} onSelectItem={onSelectItem} />
              </div>
            ))}
            <div aria-hidden="true" ref={endRef} />
          </div>
        )}
      </section>
    </div>
  )
}

function blockContainsEvent(block: TimelineBlock, eventId: string) {
  return block.kind === 'item'
    ? block.item.id === eventId
    : block.items.some((item) => item.id === eventId)
}

function findTimelineEvent(viewport: HTMLElement, eventId: string) {
  return Array.from(viewport.querySelectorAll<HTMLElement>('[data-event-id]')).find(
    (element) => element.dataset.eventId === eventId,
  )
}

function createBlocks(items: TimelineItemProjection[]): TimelineBlock[] {
  const blocks: TimelineBlock[] = []
  let index = 0
  while (index < items.length) {
    const item = items[index]
    if (!item) break
    if (!item.runSegmentId || item.kind === 'user_message') {
      blocks.push({ item, key: item.id, kind: 'item' })
      index += 1
      continue
    }

    const segmentItems: TimelineItemProjection[] = []
    const segmentId = item.runSegmentId
    while (items[index]?.runSegmentId === segmentId && items[index]?.kind !== 'user_message') {
      segmentItems.push(items[index] as TimelineItemProjection)
      index += 1
    }
    for (let start = 0; start < segmentItems.length; ) {
      const end = Math.min(start + segmentChunkSize, segmentItems.length)
      const chunk = segmentItems.slice(start, end)
      blocks.push({
        items: chunk,
        key: `${segmentId}:${chunk[0]?.globalOffset}`,
        kind: 'segment',
        segmentId,
      })
      start = end
    }
  }
  return blocks
}

function coalesceAssistantItems(items: TimelineItemProjection[]) {
  const coalesced: TimelineItemProjection[] = []
  for (const item of items) {
    const previous = coalesced.at(-1)
    if (
      item.kind === 'assistant_text' &&
      previous?.kind === 'assistant_text' &&
      item.runSegmentId === previous.runSegmentId &&
      item.semanticGroupId === previous.semanticGroupId
    ) {
      previous.summary += item.summary
      previous.incomplete = item.incomplete
      continue
    }
    coalesced.push({ ...item })
  }
  return coalesced
}

function TimelineBlockView({
  block,
  onSelectItem,
}: {
  block: TimelineBlock
  onSelectItem?: (item: TimelineItemProjection, trigger?: HTMLElement) => void
}) {
  if (block.kind === 'item') return <TimelineEvent item={block.item} onSelect={onSelectItem} />
  return <RunSegment items={block.items} onSelectItem={onSelectItem} segmentId={block.segmentId} />
}
