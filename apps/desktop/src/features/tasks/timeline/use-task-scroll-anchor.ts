import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'

import {
  type TimelineScrollSession,
  type TimelineVisibleAnchor,
  uiStore,
} from '@/shared/state/ui-store'

import {
  createScrollFollowMode,
  isNearBottom,
  nextScrollFollowState,
  preservedScrollTop,
  restoredScrollTopFromVirtualAnchor,
} from './task-scroll-controller'

export type VirtualAnchorAdapter = {
  resolve: (key: string, index?: number) => { index: number; start: number } | null
}

export function useTaskScrollAnchor(
  latestAnchorKey: string | null,
  options: {
    prependAnchorKey?: number | string
    streamingScrollTick?: number | string
    taskId?: string
    virtualAnchorAdapter?: VirtualAnchorAdapter
  } = {},
) {
  const taskId = options.taskId ?? '__default_timeline__'
  const persistTaskState = options.taskId !== undefined
  const initialSession = persistTaskState
    ? uiStore.getState().timelineScrollByTaskId[taskId]
    : undefined
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const contentRef = useRef<HTMLDivElement | null>(null)
  const endRef = useRef<HTMLDivElement | null>(null)
  const followRef = useRef({
    ...createScrollFollowMode(),
    mode: initialSession?.mode ?? 'following',
  })
  const activeTaskIdRef = useRef(taskId)
  const restoredTaskIdRef = useRef<string | null>(null)
  const lastAnchorKeyRef = useRef<string | null>(null)
  const lastStreamingTickRef = useRef<number | string | undefined>(undefined)
  const lastScrollTopRef = useRef(0)
  const programmaticRef = useRef(false)
  const releaseProgrammaticFrameRef = useRef<number | null>(null)
  const resizeFrameRef = useRef<number | null>(null)
  const visibleAnchorRef = useRef<TimelineVisibleAnchor | null>(
    initialSession?.visibleAnchor ?? null,
  )
  const previousPrependRef = useRef<{
    key: number | string | undefined
    scrollHeight: number
    scrollTop: number
  } | null>(null)
  const indicatorRef = useRef({
    hasNewContent: initialSession?.hasNewContent ?? false,
    newItemCount: initialSession?.newItemCount ?? 0,
  })
  const showJumpToLatestRef = useRef(initialSession?.showJumpToLatest ?? false)
  const pointerYRef = useRef<number | null>(null)
  const touchYRef = useRef<number | null>(null)
  const [indicator, setIndicator] = useState(indicatorRef.current)
  const [showJumpToLatest, setShowJumpToLatest] = useState(showJumpToLatestRef.current)

  const updateIndicator = useCallback(
    (
      next:
        | { hasNewContent: boolean; newItemCount: number }
        | ((current: { hasNewContent: boolean; newItemCount: number }) => {
            hasNewContent: boolean
            newItemCount: number
          }),
    ) => {
      const value = typeof next === 'function' ? next(indicatorRef.current) : next
      indicatorRef.current = value
      setIndicator(value)
    },
    [],
  )

  const updateShowJumpToLatest = useCallback((value: boolean) => {
    showJumpToLatestRef.current = value
    setShowJumpToLatest(value)
  }, [])

  const runProgrammaticScroll = useCallback((operation: () => void) => {
    programmaticRef.current = true
    operation()
    if (releaseProgrammaticFrameRef.current !== null) {
      cancelAnimationFrame(releaseProgrammaticFrameRef.current)
    }
    releaseProgrammaticFrameRef.current = requestAnimationFrame(() => {
      programmaticRef.current = false
      releaseProgrammaticFrameRef.current = null
      const viewport = viewportRef.current
      if (viewport) lastScrollTopRef.current = viewport.scrollTop
    })
  }, [])

  const scrollToEnd = useCallback(() => {
    runProgrammaticScroll(() => {
      const viewport = viewportRef.current
      if (viewport) viewport.scrollTop = viewport.scrollHeight
      endRef.current?.scrollIntoView({ block: 'end' })
    })
  }, [runProgrammaticScroll])

  const clearIndicator = useCallback(() => {
    updateIndicator({ hasNewContent: false, newItemCount: 0 })
    updateShowJumpToLatest(false)
  }, [updateIndicator, updateShowJumpToLatest])

  const interruptProgrammaticScroll = useCallback(() => {
    if (releaseProgrammaticFrameRef.current !== null) {
      cancelAnimationFrame(releaseProgrammaticFrameRef.current)
      releaseProgrammaticFrameRef.current = null
    }
    programmaticRef.current = false
    const viewport = viewportRef.current
    if (viewport) lastScrollTopRef.current = viewport.scrollTop
  }, [])

  const pauseFollowing = useCallback(() => {
    const viewport = viewportRef.current
    if (!viewport || followRef.current.mode === 'paused') return
    interruptProgrammaticScroll()
    followRef.current.mode = 'paused'
    visibleAnchorRef.current = captureVisibleAnchor(viewport, options.virtualAnchorAdapter)
    updateShowJumpToLatest(true)
  }, [interruptProgrammaticScroll, options.virtualAnchorAdapter, updateShowJumpToLatest])

  useLayoutEffect(() => {
    const taskChanged = activeTaskIdRef.current !== taskId
    if (!taskChanged && restoredTaskIdRef.current === taskId) return
    const previousTaskId = activeTaskIdRef.current
    const viewport = viewportRef.current
    if (taskChanged && persistTaskState) {
      uiStore.getState().setTimelineScrollSession(
        previousTaskId,
        createTimelineScrollSession({
          followMode: followRef.current.mode,
          indicator: indicatorRef.current,
          scrollTop: lastScrollTopRef.current,
          showJumpToLatest: showJumpToLatestRef.current,
          visibleAnchor: visibleAnchorRef.current,
        }),
      )
    }

    activeTaskIdRef.current = taskId
    restoredTaskIdRef.current = taskId
    previousPrependRef.current = null
    lastAnchorKeyRef.current = latestAnchorKey
    lastStreamingTickRef.current = options.streamingScrollTick
    const restored = uiStore.getState().timelineScrollByTaskId[taskId]
    followRef.current.mode = restored?.mode ?? 'following'
    visibleAnchorRef.current = restored?.visibleAnchor ?? null
    lastScrollTopRef.current = restored?.scrollTop ?? 0
    updateIndicator({
      hasNewContent: restored?.hasNewContent ?? false,
      newItemCount: restored?.newItemCount ?? 0,
    })
    updateShowJumpToLatest(restored?.showJumpToLatest ?? false)
    if (!viewport) return
    runProgrammaticScroll(() => {
      restoreViewport(viewport, restored, options.virtualAnchorAdapter)
    })
  }, [
    latestAnchorKey,
    options.streamingScrollTick,
    options.virtualAnchorAdapter,
    persistTaskState,
    runProgrammaticScroll,
    taskId,
    updateIndicator,
    updateShowJumpToLatest,
  ])

  useLayoutEffect(() => {
    const viewport = viewportRef.current
    if (!viewport) return
    const previous = previousPrependRef.current
    if (previous && previous.key !== options.prependAnchorKey) {
      runProgrammaticScroll(() => {
        viewport.scrollTop = preservedScrollTop({
          nextScrollHeight: viewport.scrollHeight,
          previousScrollHeight: previous.scrollHeight,
          previousScrollTop: previous.scrollTop,
        })
      })
    }
    previousPrependRef.current = {
      key: options.prependAnchorKey,
      scrollHeight: viewport.scrollHeight,
      scrollTop: viewport.scrollTop,
    }
  })

  useLayoutEffect(() => {
    const anchorChanged = Boolean(latestAnchorKey && latestAnchorKey !== lastAnchorKeyRef.current)
    lastAnchorKeyRef.current = latestAnchorKey
    if (!anchorChanged) return
    if (followRef.current.mode === 'following') {
      scrollToEnd()
      clearIndicator()
      return
    }
    updateIndicator((current) => ({
      ...current,
      hasNewContent: true,
      newItemCount: current.newItemCount + 1,
    }))
    updateShowJumpToLatest(true)
  }, [clearIndicator, latestAnchorKey, scrollToEnd, updateIndicator, updateShowJumpToLatest])

  useLayoutEffect(() => {
    const tick = options.streamingScrollTick
    if (tick === undefined || tick === lastStreamingTickRef.current) return
    lastStreamingTickRef.current = tick
    if (followRef.current.mode === 'following') {
      scrollToEnd()
      clearIndicator()
      return
    }
    updateIndicator((current) => ({ ...current, hasNewContent: true }))
    updateShowJumpToLatest(true)
  }, [
    clearIndicator,
    options.streamingScrollTick,
    scrollToEnd,
    updateIndicator,
    updateShowJumpToLatest,
  ])

  useEffect(() => {
    if (typeof ResizeObserver === 'undefined') return
    const viewport = viewportRef.current
    const content = contentRef.current
    if (!viewport || !content) return

    const observer = new ResizeObserver(() => {
      if (resizeFrameRef.current !== null) cancelAnimationFrame(resizeFrameRef.current)
      resizeFrameRef.current = requestAnimationFrame(() => {
        resizeFrameRef.current = null
        const currentViewport = viewportRef.current
        if (!currentViewport) return
        if (followRef.current.mode === 'following') {
          scrollToEnd()
          return
        }
        const anchor = visibleAnchorRef.current
        if (!anchor) return
        const node = findTimelineBlock(currentViewport, anchor.key)
        if (!node) {
          const virtualAnchor = options.virtualAnchorAdapter?.resolve(
            anchor.key,
            anchor.virtualIndex,
          )
          if (!virtualAnchor) return
          runProgrammaticScroll(() => {
            currentViewport.scrollTop = restoredScrollTopFromVirtualAnchor({
              anchorOffset: anchor.offset,
              virtualBlockStart: virtualAnchor.start,
            })
          })
          visibleAnchorRef.current = {
            ...anchor,
            virtualIndex: virtualAnchor.index,
            virtualStart: virtualAnchor.start,
          }
          return
        }
        const nextOffset =
          node.getBoundingClientRect().top - currentViewport.getBoundingClientRect().top
        const delta = nextOffset - anchor.offset
        if (Math.abs(delta) < 1) return
        runProgrammaticScroll(() => {
          currentViewport.scrollTop += delta
        })
      })
    })
    observer.observe(viewport)
    observer.observe(content)
    return () => observer.disconnect()
  }, [options.virtualAnchorAdapter, runProgrammaticScroll, scrollToEnd])

  useEffect(
    () => () => {
      const viewport = viewportRef.current
      if (persistTaskState) {
        uiStore.getState().setTimelineScrollSession(
          activeTaskIdRef.current,
          createTimelineScrollSession({
            followMode: followRef.current.mode,
            indicator: indicatorRef.current,
            scrollTop: viewport?.scrollTop ?? lastScrollTopRef.current,
            showJumpToLatest: showJumpToLatestRef.current,
            visibleAnchor: visibleAnchorRef.current,
          }),
        )
      }
      if (releaseProgrammaticFrameRef.current !== null) {
        cancelAnimationFrame(releaseProgrammaticFrameRef.current)
      }
      if (resizeFrameRef.current !== null) cancelAnimationFrame(resizeFrameRef.current)
    },
    [persistTaskState],
  )

  const onScroll = useCallback(() => {
    const viewport = viewportRef.current
    if (!viewport) return
    const nearBottom = isNearBottom(viewport, followRef.current.nearBottomThresholdPx)
    const nextMode = nextScrollFollowState({
      current: followRef.current.mode,
      isProgrammatic: programmaticRef.current,
      nearBottom,
      nextScrollTop: viewport.scrollTop,
      previousScrollTop: lastScrollTopRef.current,
    })
    followRef.current.mode = nextMode
    lastScrollTopRef.current = viewport.scrollTop
    if (previousPrependRef.current) {
      previousPrependRef.current.scrollHeight = viewport.scrollHeight
      previousPrependRef.current.scrollTop = viewport.scrollTop
    }
    if (nextMode === 'paused') {
      visibleAnchorRef.current = captureVisibleAnchor(viewport, options.virtualAnchorAdapter)
      updateShowJumpToLatest(true)
      return
    }
    visibleAnchorRef.current = null
    clearIndicator()
  }, [clearIndicator, options.virtualAnchorAdapter, updateShowJumpToLatest])

  const onWheel = useCallback(
    (event: React.WheelEvent<HTMLDivElement>) => {
      if (event.deltaY < 0) pauseFollowing()
    },
    [pauseFollowing],
  )

  const onPointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      pointerYRef.current = event.clientY
      interruptProgrammaticScroll()
    },
    [interruptProgrammaticScroll],
  )

  const onPointerMove = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      const previousY = pointerYRef.current
      pointerYRef.current = event.clientY
      if (previousY !== null && event.clientY > previousY + 1) pauseFollowing()
    },
    [pauseFollowing],
  )

  const onPointerUp = useCallback(() => {
    pointerYRef.current = null
  }, [])

  const onTouchStart = useCallback(
    (event: React.TouchEvent<HTMLDivElement>) => {
      touchYRef.current = event.touches[0]?.clientY ?? null
      interruptProgrammaticScroll()
    },
    [interruptProgrammaticScroll],
  )

  const onTouchMove = useCallback(
    (event: React.TouchEvent<HTMLDivElement>) => {
      const nextY = event.touches[0]?.clientY
      const previousY = touchYRef.current
      if (nextY === undefined) return
      touchYRef.current = nextY
      if (previousY !== null && nextY > previousY + 1) pauseFollowing()
    },
    [pauseFollowing],
  )

  const onTouchEnd = useCallback(() => {
    touchYRef.current = null
  }, [])

  const onKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (['ArrowUp', 'Home', 'PageUp'].includes(event.key)) pauseFollowing()
    },
    [pauseFollowing],
  )

  const jumpToLatest = useCallback(() => {
    followRef.current.mode = 'following'
    visibleAnchorRef.current = null
    scrollToEnd()
    clearIndicator()
  }, [clearIndicator, scrollToEnd])

  return {
    contentRef,
    endRef,
    hasNewContent: indicator.hasNewContent,
    jumpToLatest,
    newItemCount: indicator.newItemCount,
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
    showJumpToLatest,
    viewportRef,
  }
}

function captureVisibleAnchor(
  viewport: HTMLElement,
  virtualAnchorAdapter?: VirtualAnchorAdapter,
): TimelineVisibleAnchor | null {
  const viewportTop = viewport.getBoundingClientRect().top
  const blocks = Array.from(viewport.querySelectorAll<HTMLElement>('[data-timeline-block]'))
  const block = blocks.find(
    (candidate) => candidate.getBoundingClientRect().bottom > viewportTop + 1,
  )
  const key = block?.dataset.timelineBlock
  if (!block || !key) return null
  const virtualIndex = parseOptionalIndex(block.dataset.index)
  const virtual = virtualAnchorAdapter?.resolve(key, virtualIndex)
  return {
    key,
    offset: block.getBoundingClientRect().top - viewportTop,
    ...(virtual
      ? { virtualIndex: virtual.index, virtualStart: virtual.start }
      : virtualIndex === undefined
        ? {}
        : { virtualIndex }),
  }
}

function findTimelineBlock(viewport: HTMLElement, key: string) {
  return Array.from(viewport.querySelectorAll<HTMLElement>('[data-timeline-block]')).find(
    (candidate) => candidate.dataset.timelineBlock === key,
  )
}

function parseOptionalIndex(value: string | undefined) {
  if (value === undefined) return undefined
  const index = Number(value)
  return Number.isInteger(index) ? index : undefined
}

function restoreViewport(
  viewport: HTMLElement,
  session: TimelineScrollSession | undefined,
  virtualAnchorAdapter?: VirtualAnchorAdapter,
) {
  if (!session) {
    viewport.scrollTop = viewport.scrollHeight
    return
  }
  const anchor = session.visibleAnchor
  if (!anchor) {
    viewport.scrollTop = session.scrollTop
    return
  }
  const node = findTimelineBlock(viewport, anchor.key)
  if (node) {
    viewport.scrollTop =
      session.scrollTop +
      (node.getBoundingClientRect().top - viewport.getBoundingClientRect().top - anchor.offset)
    return
  }
  const virtual = virtualAnchorAdapter?.resolve(anchor.key, anchor.virtualIndex)
  viewport.scrollTop = virtual
    ? restoredScrollTopFromVirtualAnchor({
        anchorOffset: anchor.offset,
        virtualBlockStart: virtual.start,
      })
    : session.scrollTop
}

function createTimelineScrollSession(input: {
  followMode: 'following' | 'paused'
  indicator: { hasNewContent: boolean; newItemCount: number }
  scrollTop: number
  showJumpToLatest: boolean
  visibleAnchor: TimelineVisibleAnchor | null
}): TimelineScrollSession {
  return {
    hasNewContent: input.indicator.hasNewContent,
    mode: input.followMode,
    newItemCount: input.indicator.newItemCount,
    scrollTop: input.scrollTop,
    showJumpToLatest: input.showJumpToLatest,
    visibleAnchor: input.visibleAnchor,
  }
}
