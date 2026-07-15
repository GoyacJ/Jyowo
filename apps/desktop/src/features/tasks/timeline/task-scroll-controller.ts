export type ScrollFollowState = 'following' | 'paused'

export type ScrollFollowMode = {
  mode: ScrollFollowState
  nearBottomThresholdPx: number
}

export function createScrollFollowMode(thresholdPx = 24): ScrollFollowMode {
  return { mode: 'following', nearBottomThresholdPx: thresholdPx }
}

export function isNearBottom(viewport: HTMLElement, thresholdPx: number) {
  return viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight <= thresholdPx
}

export function nextScrollFollowState(input: {
  current: ScrollFollowState
  isProgrammatic: boolean
  nearBottom: boolean
  nextScrollTop: number
  previousScrollTop: number
}) {
  if (input.isProgrammatic) return input.current
  if (input.nextScrollTop < input.previousScrollTop - 1) return 'paused'
  if (input.nearBottom) return 'following'
  return input.current === 'paused' ? 'paused' : input.current
}

export function preservedScrollTop(input: {
  nextScrollHeight: number
  previousScrollHeight: number
  previousScrollTop: number
}) {
  return input.previousScrollTop + Math.max(0, input.nextScrollHeight - input.previousScrollHeight)
}

export function restoredScrollTopFromVirtualAnchor(input: {
  anchorOffset: number
  virtualBlockStart: number
}) {
  return Math.max(0, input.virtualBlockStart - input.anchorOffset)
}
