import { describe, expect, it } from 'vitest'

import { coalesceTimelineActions } from './use-conversation-event-stream'

describe('coalesceTimelineActions', () => {
  it('merges consecutive worktree refetch signals into one update', () => {
    expect(
      coalesceTimelineActions([
        { type: 'worktreeRefreshRequested', immediate: false },
        { type: 'worktreeRefreshRequested', immediate: true },
      ]),
    ).toEqual([{ type: 'worktreeRefreshRequested', immediate: true }])
  })

  it('keeps gap ordering between refetch updates', () => {
    expect(
      coalesceTimelineActions([
        { type: 'worktreeRefreshRequested', immediate: false },
        { type: 'markGap', afterCursor: null },
        { type: 'worktreeRefreshRequested', immediate: true },
      ]),
    ).toEqual([
      { type: 'worktreeRefreshRequested', immediate: false },
      { type: 'markGap', afterCursor: null },
      { type: 'worktreeRefreshRequested', immediate: true },
    ])
  })

  it('collapses one frame of streaming updates but keeps terminal urgency', () => {
    expect(
      coalesceTimelineActions([
        ...Array.from({ length: 100 }, () => ({
          type: 'worktreeRefreshRequested' as const,
          immediate: false,
        })),
        { type: 'worktreeRefreshRequested', immediate: true },
      ]),
    ).toEqual([{ type: 'worktreeRefreshRequested', immediate: true }])
  })
})
