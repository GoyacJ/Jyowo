import { useVirtualizer } from '@tanstack/react-virtual'
import { useTranslation } from 'react-i18next'
import type { RunProjection, TimelineItemProjection } from '@/generated/daemon-protocol'
import { Button } from '@/shared/ui/button'

import { RunSegment } from './RunSegment'
import { TimelineEvent } from './TimelineEvent'
import { timelineSummary } from './timeline-summary'
import { useTaskScrollAnchor } from './use-task-scroll-anchor'

const estimatedBlockHeightPx = 240
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
      showHeader: boolean
      statusItems: TimelineItemProjection[]
    }

export function TaskTimeline({
  currentRun,
  items,
  onSelectItem,
}: {
  currentRun?: RunProjection | null
  items: TimelineItemProjection[]
  onSelectItem?: (item: TimelineItemProjection) => void
}) {
  const { t } = useTranslation('tasks')
  const orderedItems = [...items].sort(
    (left, right) => left.globalOffset - right.globalOffset || left.id.localeCompare(right.id),
  )
  const latest = orderedItems.at(-1)
  const first = orderedItems.at(0)
  const blocks = createBlocks(coalesceAssistantItems(orderedItems))
  const { endRef, jumpToLatest, onScroll, showJumpToLatest, viewportRef } = useTaskScrollAnchor(
    latest ? `${latest.id}:${latest.incomplete}` : null,
    {
      prependAnchorKey: first?.id,
      isStreamingUpdate: latest?.incomplete,
      streamingScrollTick: latest?.incomplete ? `${latest.id}:${latest.summary.length}` : undefined,
    },
  )
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
  const visibleRows =
    virtualRows.length > 0
      ? virtualRows.map((row) => ({ index: row.index, key: row.key, start: row.start }))
      : blocks.slice(0, 8).map((block, index) => ({
          index,
          key: block.key,
          start: index * estimatedBlockHeightPx,
        }))

  return (
    <div className="relative min-h-0 flex-1">
      <p aria-live="polite" className="sr-only" role="status">
        {latest
          ? t('timeline.update', { summary: timelineSummary(latest, t) })
          : t('timeline.noActivity')}
      </p>
      <div
        className="h-full overflow-y-auto overscroll-contain px-1 pb-28"
        data-testid="task-timeline-viewport"
        onScroll={onScroll}
        ref={viewportRef}
      >
        {useVirtualList ? (
          <div
            className="relative"
            data-testid="task-timeline-scroll-content"
            style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
          >
            {visibleRows.map((virtualRow) => {
              const block = blocks[virtualRow.index]
              if (!block) return null
              return (
                <div
                  className="absolute top-0 left-0 w-full pb-8"
                  data-index={virtualRow.index}
                  key={block.key}
                  ref={rowVirtualizer.measureElement}
                  style={{ transform: `translateY(${virtualRow.start}px)` }}
                >
                  <TimelineBlockView
                    block={block}
                    currentRun={currentRun}
                    onSelectItem={onSelectItem}
                  />
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
          <div className="space-y-8" data-testid="task-timeline-scroll-content">
            {blocks.map((block) => (
              <TimelineBlockView
                block={block}
                currentRun={currentRun}
                key={block.key}
                onSelectItem={onSelectItem}
              />
            ))}
            <div aria-hidden="true" ref={endRef} />
          </div>
        )}
      </div>
      {showJumpToLatest ? (
        <Button
          className="absolute bottom-4 left-1/2 -translate-x-1/2 rounded-full shadow-lg"
          onClick={jumpToLatest}
          size="sm"
          type="button"
          variant="outline"
        >
          {t('timeline.jumpToLatest')}
        </Button>
      ) : null}
    </div>
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
        showHeader: start === 0,
        statusItems: segmentItems,
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
  currentRun,
  onSelectItem,
}: {
  block: TimelineBlock
  currentRun?: RunProjection | null
  onSelectItem?: (item: TimelineItemProjection) => void
}) {
  if (block.kind === 'item') return <TimelineEvent item={block.item} onSelect={onSelectItem} />
  return (
    <RunSegment
      items={block.items}
      onSelectItem={onSelectItem}
      run={currentRun}
      segmentId={block.segmentId}
      showHeader={block.showHeader}
      statusItems={block.statusItems}
    />
  )
}
