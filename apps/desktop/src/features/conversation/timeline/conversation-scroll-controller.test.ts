import { describe, expect, it } from 'vitest'

import {
  isNearBottom,
  preservedScrollTop,
  shouldAutoFollowOnAnchorChange,
  turnScrollAnchorKey,
} from './conversation-scroll-controller'

describe('conversationScrollController', () => {
  it('builds stable anchor keys from turn id and assistant work state', () => {
    expect(
      turnScrollAnchorKey({
        id: 'turn:user-1',
        assistant: { status: 'running', segments: [{}, {}] },
      }),
    ).toBe('turn:user-1:running:2')
  })

  it('does not auto follow streaming anchor changes when follow mode is disabled', () => {
    expect(
      shouldAutoFollowOnAnchorChange({
        followLatest: false,
        anchorChanged: true,
        isStreamingUpdate: true,
      }),
    ).toBe(false)
  })

  it('detects near-bottom scroll position', () => {
    const viewport = {
      scrollHeight: 1000,
      scrollTop: 910,
      clientHeight: 80,
    } as HTMLElement

    expect(isNearBottom(viewport, 96)).toBe(true)
  })

  it('preserves the visible row when older history is prepended', () => {
    expect(
      preservedScrollTop({
        previousScrollHeight: 1_000,
        nextScrollHeight: 1_420,
        previousScrollTop: 240,
      }),
    ).toBe(660)
  })
})
