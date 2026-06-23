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

export function blockScrollAnchorKey(block: { id: string; status?: string; kind: string }) {
  return `${block.id}:${block.status ?? ''}:${block.kind}`
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
