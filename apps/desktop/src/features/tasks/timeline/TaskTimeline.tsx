import { useConversationScrollAnchor } from '@/features/conversation/timeline/use-conversation-scroll-anchor'
import type { RunProjection, TimelineItemProjection } from '@/generated/daemon-protocol'
import { Button } from '@/shared/ui/button'

import { RunSegment } from './RunSegment'
import { TimelineEvent } from './TimelineEvent'

export function TaskTimeline({
  currentRun,
  items,
}: {
  currentRun?: RunProjection | null
  items: TimelineItemProjection[]
}) {
  const orderedItems = [...items].sort(
    (left, right) => left.globalOffset - right.globalOffset || left.id.localeCompare(right.id),
  )
  const latest = orderedItems.at(-1)
  const first = orderedItems.at(0)
  const { endRef, jumpToLatest, onScroll, showJumpToLatest, viewportRef } =
    useConversationScrollAnchor(latest ? `${latest.id}:${latest.incomplete}` : null, {
      prependAnchorKey: first?.id,
      isStreamingUpdate: latest?.incomplete,
      streamingScrollTick: latest?.incomplete ? `${latest.id}:${latest.summary.length}` : undefined,
    })

  return (
    <div className="relative min-h-0 flex-1">
      <div
        className="h-full overflow-y-auto overscroll-contain px-1 pb-28"
        data-testid="task-timeline-viewport"
        onScroll={onScroll}
        ref={viewportRef}
      >
        <div className="space-y-8">{renderBlocks(orderedItems, currentRun)}</div>
        <div aria-hidden="true" ref={endRef} />
      </div>
      {showJumpToLatest ? (
        <Button
          className="absolute bottom-4 left-1/2 -translate-x-1/2 rounded-full shadow-lg"
          onClick={jumpToLatest}
          size="sm"
          type="button"
          variant="outline"
        >
          Jump to latest
        </Button>
      ) : null}
    </div>
  )
}

function renderBlocks(items: TimelineItemProjection[], currentRun?: RunProjection | null) {
  const blocks: React.ReactNode[] = []
  let index = 0
  while (index < items.length) {
    const item = items[index]
    if (!item) break
    if (!item.runSegmentId || item.kind === 'user_message') {
      blocks.push(<TimelineEvent item={item} key={item.id} />)
      index += 1
      continue
    }

    const segmentItems: TimelineItemProjection[] = []
    const segmentId = item.runSegmentId
    while (items[index]?.runSegmentId === segmentId && items[index]?.kind !== 'user_message') {
      segmentItems.push(items[index] as TimelineItemProjection)
      index += 1
    }
    blocks.push(
      <RunSegment
        items={segmentItems}
        key={`${segmentId}:${segmentItems[0]?.globalOffset}`}
        run={currentRun}
        segmentId={segmentId}
      />,
    )
  }
  return blocks
}
