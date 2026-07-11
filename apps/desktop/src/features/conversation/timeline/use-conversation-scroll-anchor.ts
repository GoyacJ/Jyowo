import { useCallback, useLayoutEffect, useRef, useState } from 'react'

import {
  createScrollFollowMode,
  isNearBottom,
  preservedScrollTop,
  shouldAutoFollowOnAnchorChange,
  shouldShowJumpToLatest,
} from './conversation-scroll-controller'

function scrollToEnd(endElement: HTMLElement | null) {
  endElement?.scrollIntoView({ block: 'end' })
}

export function useConversationScrollAnchor(
  latestAnchorKey: string | null,
  options: {
    isStreamingUpdate?: boolean
    prependAnchorKey?: number | string
    streamingScrollTick?: number | string
  } = {},
) {
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const endRef = useRef<HTMLDivElement | null>(null)
  const followRef = useRef(createScrollFollowMode())
  const lastAnchorKeyRef = useRef<string | null>(null)
  const previousPrependRef = useRef<{
    key: number | string | undefined
    scrollHeight: number
    scrollTop: number
  } | null>(null)
  const [showJumpToLatest, setShowJumpToLatest] = useState(false)

  useLayoutEffect(() => {
    const viewport = viewportRef.current
    if (!viewport) return
    const previous = previousPrependRef.current
    if (previous && previous.key !== options.prependAnchorKey) {
      viewport.scrollTop = preservedScrollTop({
        nextScrollHeight: viewport.scrollHeight,
        previousScrollHeight: previous.scrollHeight,
        previousScrollTop: previous.scrollTop,
      })
    }
    previousPrependRef.current = {
      key: options.prependAnchorKey,
      scrollHeight: viewport.scrollHeight,
      scrollTop: viewport.scrollTop,
    }
  }, [options.prependAnchorKey])

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
    if (previousPrependRef.current) {
      previousPrependRef.current.scrollHeight = viewport.scrollHeight
      previousPrependRef.current.scrollTop = viewport.scrollTop
    }
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
