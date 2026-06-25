import { useCallback, useLayoutEffect, useRef, useState } from 'react'

import {
  createScrollFollowMode,
  isNearBottom,
  shouldAutoFollowOnAnchorChange,
  shouldShowJumpToLatest,
} from './conversation-scroll-controller'

function scrollToEnd(endElement: HTMLElement | null) {
  endElement?.scrollIntoView({ block: 'end' })
}

export function useConversationScrollAnchor(
  latestAnchorKey: string | null,
  options: { isStreamingUpdate?: boolean; streamingScrollTick?: number } = {},
) {
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const endRef = useRef<HTMLDivElement | null>(null)
  const followRef = useRef(createScrollFollowMode())
  const lastAnchorKeyRef = useRef<string | null>(null)
  const [showJumpToLatest, setShowJumpToLatest] = useState(false)

  useLayoutEffect(() => {
    if (options.streamingScrollTick === undefined || !followRef.current.followLatest) {
      return
    }

    scrollToEnd(endRef.current)
  }, [options.streamingScrollTick])

  useLayoutEffect(() => {
    if (options.streamingScrollTick !== undefined) {
      return
    }

    const anchorChanged = Boolean(latestAnchorKey && latestAnchorKey !== lastAnchorKeyRef.current)
    lastAnchorKeyRef.current = latestAnchorKey

    if (
      !shouldAutoFollowOnAnchorChange({
        followLatest: followRef.current.followLatest,
        anchorChanged,
        isStreamingUpdate: options.isStreamingUpdate ?? false,
      })
    ) {
      if (anchorChanged) {
        setShowJumpToLatest(shouldShowJumpToLatest(followRef.current.followLatest))
      }
      return
    }

    scrollToEnd(endRef.current)
    setShowJumpToLatest(false)
  }, [latestAnchorKey, options.isStreamingUpdate, options.streamingScrollTick])

  const onScroll = useCallback(() => {
    const viewport = viewportRef.current
    if (!viewport) {
      followRef.current.followLatest = true
      return
    }

    followRef.current.followLatest = isNearBottom(viewport, followRef.current.nearBottomThresholdPx)
    if (followRef.current.followLatest) {
      setShowJumpToLatest(false)
    }
  }, [])

  const jumpToLatest = useCallback(() => {
    followRef.current.followLatest = true
    scrollToEnd(endRef.current)
    setShowJumpToLatest(false)
  }, [])

  return {
    endRef,
    jumpToLatest,
    onScroll,
    showJumpToLatest,
    viewportRef,
  }
}
