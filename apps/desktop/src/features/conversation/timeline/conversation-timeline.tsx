import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '@/shared/state/ui-store'
import type { ConversationEventRef, ConversationTurn } from '@/shared/tauri/commands'
import { turnScrollAnchorKey } from './conversation-scroll-controller'
import { ConversationTurnRow } from './conversation-turn-row'
import { useConversationScrollAnchor } from './use-conversation-scroll-anchor'

const estimatedTurnHeightPx = 180
const composerReservePx = 112
const virtualListThreshold = 24

export function ConversationTimeline({
  onOpenDetails,
  onPermissionResolve,
  onReviewContinue,
  title,
  turns,
}: {
  turns: ConversationTurn[]
  title: string
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
    confirmationText?: string
  }) => void
  onReviewContinue?: (prompt: string) => void
}) {
  const { t } = useTranslation('conversation')
  const timelineTurns = turns
  const latestTurn = timelineTurns.at(-1)
  const latestAnchorKey = latestTurn ? turnScrollAnchorKey(latestTurn) : null
  const streamingScrollTick =
    latestTurn?.assistant?.status === 'running'
      ? latestTurn.assistant.segments.reduce(
          (size, segment) => size + JSON.stringify(segment).length,
          0,
        )
      : undefined
  const { endRef, jumpToLatest, onScroll, showJumpToLatest, viewportRef } =
    useConversationScrollAnchor(latestAnchorKey, { streamingScrollTick })
  const timelineScrollRequest = useUiStore((state) => state.timelineScrollRequest)
  const clearTimelineScrollRequest = useUiStore((state) => state.clearTimelineScrollRequest)
  const listRef = useRef<HTMLDivElement | null>(null)
  const useVirtualList = timelineTurns.length >= virtualListThreshold
  const rowVirtualizer = useVirtualizer({
    count: useVirtualList ? timelineTurns.length : 0,
    estimateSize: () => estimatedTurnHeightPx,
    getScrollElement: () => viewportRef.current,
    overscan: 6,
  })

  useEffect(() => {
    if (!timelineScrollRequest) {
      return
    }

    let frameId: number | undefined
    let timeoutId: number | undefined
    const scheduleHighlightRetry = (remainingFrames: number) => {
      frameId = window.requestAnimationFrame(() => {
        if (highlightTarget()) {
          return
        }
        if (remainingFrames > 0) {
          scheduleHighlightRetry(remainingFrames - 1)
          return
        }
        clearTimelineScrollRequest()
      })
    }
    const highlightTarget = () => {
      const root = viewportRef.current
      if (!root) {
        return false
      }

      const target = findTimelineScrollTarget(timelineScrollRequest.anchorId, root)
      if (!target) {
        return false
      }

      target.scrollIntoView({ behavior: 'smooth', block: 'center' })
      target.classList.add('ring-2', 'ring-ring', 'ring-offset-2', 'ring-offset-background')
      timeoutId = window.setTimeout(() => {
        target.classList.remove('ring-2', 'ring-ring', 'ring-offset-2', 'ring-offset-background')
        clearTimelineScrollRequest()
      }, 1600)
      return true
    }

    if (!highlightTarget()) {
      const turnIndex = findTimelineScrollTurnIndex(timelineScrollRequest.anchorId, timelineTurns)
      if (useVirtualList && turnIndex !== null) {
        rowVirtualizer.scrollToIndex(turnIndex, { align: 'center' })
      }
      scheduleHighlightRetry(5)
    }

    return () => {
      if (frameId !== undefined) {
        window.cancelAnimationFrame(frameId)
      }
      if (timeoutId !== undefined) {
        window.clearTimeout(timeoutId)
      }
    }
  }, [
    clearTimelineScrollRequest,
    rowVirtualizer,
    timelineScrollRequest,
    timelineTurns,
    useVirtualList,
    viewportRef,
  ])

  return (
    <section className="relative mx-auto grid h-full min-h-0 w-full max-w-[900px] grid-rows-[auto_minmax(0,1fr)]">
      <header className="pt-3 pb-4">
        <h1 className="font-semibold text-2xl tracking-normal">{title}</h1>
      </header>
      <div className="min-h-0 overflow-auto pr-1" onScroll={onScroll} ref={viewportRef}>
        {timelineTurns.length > 0 ? (
          useVirtualList ? (
            <div
              className="relative pb-28"
              data-testid="conversation-timeline-scroll-content"
              ref={listRef}
              style={{ height: `${rowVirtualizer.getTotalSize() + composerReservePx}px` }}
            >
              {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                const turn = timelineTurns[virtualRow.index]
                if (!turn) {
                  return null
                }

                return (
                  <div
                    className="absolute top-0 left-0 w-full pb-5"
                    data-index={virtualRow.index}
                    key={turn.id}
                    ref={rowVirtualizer.measureElement}
                    style={{ transform: `translateY(${virtualRow.start}px)` }}
                  >
                    <ConversationTurnRow
                      onOpenDetails={onOpenDetails}
                      onPermissionResolve={onPermissionResolve}
                      onReviewContinue={onReviewContinue}
                      turn={turn}
                    />
                  </div>
                )
              })}
              <div aria-hidden="true" ref={endRef} />
            </div>
          ) : (
            <div className="grid gap-5 pb-28" data-testid="conversation-timeline-scroll-content">
              {timelineTurns.map((turn) => (
                <ConversationTurnRow
                  key={turn.id}
                  onOpenDetails={onOpenDetails}
                  onPermissionResolve={onPermissionResolve}
                  onReviewContinue={onReviewContinue}
                  turn={turn}
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

function findTimelineScrollTurnIndex(anchorId: string, turns: ConversationTurn[]) {
  if (anchorId.startsWith('permission:')) {
    const requestId = anchorId.slice('permission:'.length)
    const index = turns.findIndex((turn) => turnHasPermissionRequest(turn, requestId))
    return index >= 0 ? index : null
  }

  const index = turns.findIndex((turn) => turn.id === anchorId)
  return index >= 0 ? index : null
}

function turnHasPermissionRequest(turn: ConversationTurn, requestId: string) {
  return (
    turn.assistant?.segments.some(
      (segment) =>
        segment.kind === 'toolGroup' &&
        segment.attempts.some((attempt) => attempt.permission?.requestId === requestId),
    ) ?? false
  )
}

function findTimelineScrollTarget(anchorId: string, root: ParentNode) {
  if (anchorId.startsWith('permission:')) {
    const requestId = anchorId.slice('permission:'.length)
    return (
      Array.from(root.querySelectorAll<HTMLElement>('[data-permission-request-id]')).find(
        (element) => element.dataset.permissionRequestId === requestId,
      ) ?? null
    )
  }

  return (
    Array.from(root.querySelectorAll<HTMLElement>('[id]')).find(
      (element) => element.id === `conversation-turn-${anchorId}`,
    ) ?? null
  )
}
