export type ScrollFollowMode = {
  followLatest: boolean
  nearBottomThresholdPx: number
}

export function createScrollFollowMode(thresholdPx = 96): ScrollFollowMode {
  return {
    followLatest: true,
    nearBottomThresholdPx: thresholdPx,
  }
}

function distanceFromBottom(viewport: HTMLElement) {
  return viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight
}

export function isNearBottom(viewport: HTMLElement, thresholdPx: number) {
  return distanceFromBottom(viewport) < thresholdPx
}

export function turnScrollAnchorKey(turn: {
  id: string
  assistant?: { status?: string; segments?: unknown[] }
}) {
  return `${turn.id}:${turn.assistant?.status ?? ''}:${turn.assistant?.segments?.length ?? 0}`
}

export function shouldAutoFollowOnAnchorChange(input: {
  followLatest: boolean
  anchorChanged: boolean
  isStreamingUpdate: boolean
}) {
  if (!input.anchorChanged) {
    return false
  }

  if (input.isStreamingUpdate) {
    return input.followLatest
  }

  return input.followLatest
}

export function shouldShowJumpToLatest(followLatest: boolean) {
  return !followLatest
}

export function preservedScrollTop(input: {
  previousScrollHeight: number
  previousScrollTop: number
  nextScrollHeight: number
}) {
  return input.previousScrollTop + Math.max(0, input.nextScrollHeight - input.previousScrollHeight)
}
