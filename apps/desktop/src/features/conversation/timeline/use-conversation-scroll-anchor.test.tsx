import { renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { useConversationScrollAnchor } from './use-conversation-scroll-anchor'

describe('useConversationScrollAnchor', () => {
  it('preserves only the height added by a prepend after later output was already rendered', () => {
    let scrollHeight = 1_000
    const viewport = {
      clientHeight: 300,
      get scrollHeight() {
        return scrollHeight
      },
      scrollTop: 200,
    } as HTMLDivElement
    const { rerender, result } = renderHook(
      ({ prependAnchorKey, streamingScrollTick }) =>
        useConversationScrollAnchor('latest', { prependAnchorKey, streamingScrollTick }),
      {
        initialProps: { prependAnchorKey: 'first-a', streamingScrollTick: 'tick-a' },
      },
    )
    result.current.viewportRef.current = viewport
    rerender({ prependAnchorKey: 'first-b', streamingScrollTick: 'tick-b' })

    scrollHeight = 1_200
    rerender({ prependAnchorKey: 'first-b', streamingScrollTick: 'tick-c' })

    scrollHeight = 1_300
    rerender({ prependAnchorKey: 'first-c', streamingScrollTick: 'tick-d' })

    expect(viewport.scrollTop).toBe(300)
  })
})
