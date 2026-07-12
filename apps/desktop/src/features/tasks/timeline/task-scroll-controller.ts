export type ScrollFollowMode = {
  followLatest: boolean
  nearBottomThresholdPx: number
}

export function createScrollFollowMode(thresholdPx = 96): ScrollFollowMode {
  return { followLatest: true, nearBottomThresholdPx: thresholdPx }
}

export function isNearBottom(viewport: HTMLElement, thresholdPx: number) {
  return viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < thresholdPx
}

export function shouldAutoFollowOnAnchorChange(input: {
  anchorChanged: boolean
  followLatest: boolean
  isStreamingUpdate: boolean
}) {
  return input.anchorChanged && input.followLatest
}

export function shouldShowJumpToLatest(followLatest: boolean) {
  return !followLatest
}

export function preservedScrollTop(input: {
  nextScrollHeight: number
  previousScrollHeight: number
  previousScrollTop: number
}) {
  return input.previousScrollTop + Math.max(0, input.nextScrollHeight - input.previousScrollHeight)
}
