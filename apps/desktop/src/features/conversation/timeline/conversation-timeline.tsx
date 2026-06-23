import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { useUiStore } from '@/shared/state/ui-store'
import { ConversationBlockRow } from './conversation-block-row'
import type { ConversationBlock } from './conversation-blocks'
import { blockScrollAnchorKey, useConversationScrollAnchor } from './use-conversation-scroll-anchor'

const estimatedBlockHeightPx = 120
const virtualListThreshold = 24

export function ConversationTimeline({
  blocks,
  title,
  onPermissionResolve,
  onReviewContinue,
}: {
  blocks: ConversationBlock[]
  title: string
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
  onReviewContinue?: (prompt: string) => void
}) {
  const { t } = useTranslation('conversation')
  const latestBlock = blocks.at(-1)
  const latestAnchorKey = latestBlock ? blockScrollAnchorKey(latestBlock) : null
  const streamingScrollTick =
    latestBlock?.kind === 'assistantStreaming' ? latestBlock.body.length : undefined
  const { endRef, jumpToLatest, onScroll, showJumpToLatest, viewportRef } =
    useConversationScrollAnchor(latestAnchorKey, { streamingScrollTick })
  const timelineScrollRequest = useUiStore((state) => state.timelineScrollRequest)
  const clearTimelineScrollRequest = useUiStore((state) => state.clearTimelineScrollRequest)
  const listRef = useRef<HTMLDivElement | null>(null)
  const useVirtualList = blocks.length >= virtualListThreshold
  const rowVirtualizer = useVirtualizer({
    count: useVirtualList ? blocks.length : 0,
    estimateSize: () => estimatedBlockHeightPx,
    getScrollElement: () => viewportRef.current,
    overscan: 6,
  })

  useEffect(() => {
    if (!timelineScrollRequest) {
      return
    }

    const target = document.getElementById(`conversation-block-${timelineScrollRequest.blockId}`)
    target?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    target?.classList.add('ring-2', 'ring-ring', 'ring-offset-2', 'ring-offset-background')
    const timeoutId = window.setTimeout(() => {
      target?.classList.remove('ring-2', 'ring-ring', 'ring-offset-2', 'ring-offset-background')
      clearTimelineScrollRequest()
    }, 1600)

    return () => {
      window.clearTimeout(timeoutId)
    }
  }, [clearTimelineScrollRequest, timelineScrollRequest])

  return (
    <section className="relative mx-auto grid h-full min-h-0 w-full max-w-[900px] grid-rows-[auto_minmax(0,1fr)]">
      <header className="pt-3 pb-4">
        <h1 className="font-semibold text-2xl tracking-normal">{title}</h1>
      </header>
      <div className="min-h-0 overflow-auto pr-1" onScroll={onScroll} ref={viewportRef}>
        {blocks.length > 0 ? (
          useVirtualList ? (
            <div
              className="relative pb-4"
              ref={listRef}
              style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
            >
              {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                const block = blocks[virtualRow.index]
                if (!block) {
                  return null
                }

                return (
                  <div
                    className="absolute top-0 left-0 w-full pb-5"
                    data-index={virtualRow.index}
                    key={block.id}
                    ref={rowVirtualizer.measureElement}
                    style={{ transform: `translateY(${virtualRow.start}px)` }}
                  >
                    <ConversationBlockRow
                      block={block}
                      onPermissionResolve={onPermissionResolve}
                      onReviewContinue={onReviewContinue}
                    />
                  </div>
                )
              })}
              <div aria-hidden="true" ref={endRef} />
            </div>
          ) : (
            <div className="grid gap-5 pb-4">
              {blocks.map((block) => (
                <ConversationBlockRow
                  block={block}
                  key={block.id}
                  onPermissionResolve={onPermissionResolve}
                  onReviewContinue={onReviewContinue}
                />
              ))}
              <div aria-hidden="true" ref={endRef} />
            </div>
          )
        ) : (
          <div className="flex min-h-full items-center justify-center py-16 text-center">
            <div>
              <h2 className="font-semibold text-xl">{t('timeline.emptyTitle')}</h2>
              <p className="mt-2 text-muted-foreground text-sm">{t('timeline.emptyDescription')}</p>
            </div>
          </div>
        )}
      </div>
      {showJumpToLatest ? (
        <button
          className="absolute right-4 bottom-4 rounded-md border border-border bg-surface px-3 py-1.5 text-sm shadow-card"
          onClick={jumpToLatest}
          type="button"
        >
          {t('timeline.jumpToLatest')}
        </button>
      ) : null}
    </section>
  )
}
