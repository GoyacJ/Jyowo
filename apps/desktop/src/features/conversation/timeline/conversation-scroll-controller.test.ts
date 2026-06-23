import { describe, expect, it } from 'vitest'

import {
  blockScrollAnchorKey,
  isNearBottom,
  shouldAutoFollowOnAnchorChange,
} from './conversation-scroll-controller'

describe('conversationScrollController', () => {
  it('builds stable anchor keys without body length', () => {
    expect(
      blockScrollAnchorKey({
        id: 'message:1',
        kind: 'assistantStreaming',
        status: 'streaming',
      }),
    ).toBe('message:1:streaming:assistantStreaming')
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
})
