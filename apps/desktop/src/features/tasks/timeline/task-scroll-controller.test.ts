import { describe, expect, it } from 'vitest'

import {
  createScrollFollowMode,
  isNearBottom,
  nextScrollFollowState,
  preservedScrollTop,
  restoredScrollTopFromVirtualAnchor,
} from './task-scroll-controller'

describe('task scroll controller', () => {
  it('uses a narrow bottom threshold', () => {
    expect(createScrollFollowMode()).toEqual({
      mode: 'following',
      nearBottomThresholdPx: 24,
    })
  })

  it('treats 24px as near bottom and 25px as outside the resume boundary', () => {
    const viewport = {
      clientHeight: 300,
      scrollHeight: 1_000,
      scrollTop: 676,
    } as HTMLElement

    expect(isNearBottom(viewport, 24)).toBe(true)
    viewport.scrollTop = 675
    expect(isNearBottom(viewport, 24)).toBe(false)
  })

  it('pauses on an upward user scroll and resumes at the bottom', () => {
    expect(
      nextScrollFollowState({
        current: 'following',
        isProgrammatic: false,
        nearBottom: false,
        nextScrollTop: 180,
        previousScrollTop: 240,
      }),
    ).toBe('paused')
    expect(
      nextScrollFollowState({
        current: 'paused',
        isProgrammatic: false,
        nearBottom: true,
        nextScrollTop: 900,
        previousScrollTop: 850,
      }),
    ).toBe('following')
  })

  it('keeps an upward user scroll paused inside the bottom threshold', () => {
    expect(
      nextScrollFollowState({
        current: 'following',
        isProgrammatic: false,
        nearBottom: true,
        nextScrollTop: 890,
        previousScrollTop: 900,
      }),
    ).toBe('paused')
  })

  it('does not treat programmatic restoration as user intent', () => {
    expect(
      nextScrollFollowState({
        current: 'following',
        isProgrammatic: true,
        nearBottom: false,
        nextScrollTop: 180,
        previousScrollTop: 240,
      }),
    ).toBe('following')
  })

  it('preserves the viewport when history is prepended', () => {
    expect(
      preservedScrollTop({
        nextScrollHeight: 1_400,
        previousScrollHeight: 1_000,
        previousScrollTop: 250,
      }),
    ).toBe(650)
  })

  it('restores an unmounted virtual block from its stable measured start and offset', () => {
    expect(
      restoredScrollTopFromVirtualAnchor({
        anchorOffset: -18,
        virtualBlockStart: 2_400,
      }),
    ).toBe(2_418)
  })
})
