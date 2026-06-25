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
        { type: 'markGap' },
        { type: 'worktreeRefreshRequested', immediate: true },
      ]),
    ).toEqual([
      { type: 'worktreeRefreshRequested', immediate: false },
      { type: 'markGap' },
      { type: 'worktreeRefreshRequested', immediate: true },
    ])
  })
})
