import { describe, expect, it } from 'vitest'

import { createMockConversationState, mockConversationRuntime } from './mock-conversation-runtime'

describe('mockConversationRuntime', () => {
  it('appends submitted user messages in order', () => {
    const initialState = createMockConversationState()

    const firstState = mockConversationRuntime.submitMessage(initialState, 'Continue the setup')
    const secondState = mockConversationRuntime.submitMessage(firstState, 'Add tests')

    expect(secondState.messages.map((message) => message.body)).toEqual([
      "Let's scaffold the desktop app with Tauri + React + TypeScript.\nUse Vite for the renderer. Keep it minimal and ready for local AI features.",
      "I'll set up the foundation with a clean project structure, dev scripts, and base app shell.",
      'Continue the setup',
      'Add tests',
    ])
  })

  it('produces the plan once and keeps repeated action idempotent', () => {
    const initialState = createMockConversationState()

    const plannedState = mockConversationRuntime.producePlan(initialState)
    const repeatedState = mockConversationRuntime.producePlan(plannedState)

    expect(plannedState.planItems).toHaveLength(5)
    expect(repeatedState.planItems).toEqual(plannedState.planItems)
    expect(repeatedState.messages).toEqual(plannedState.messages)
  })

  it('marks activity running, completes work, produces artifact, and requests review', () => {
    const initialState = createMockConversationState()
    const plannedState = mockConversationRuntime.producePlan(initialState)
    const runningState = mockConversationRuntime.markActivityRunning(plannedState)
    const completedState = mockConversationRuntime.completeWork(runningState)
    const artifactState = mockConversationRuntime.produceArtifactSummary(completedState)
    const reviewState = mockConversationRuntime.requestReview(artifactState)

    expect(runningState.activityItems[0]).toMatchObject({
      label: 'start_run',
      status: 'running',
    })
    expect(completedState.activityItems[0]).toMatchObject({
      label: 'start_run',
      status: 'success',
    })
    expect(completedState.planItems.every((item) => item.status === 'completed')).toBe(true)
    expect(artifactState.artifacts[0]?.title).toBe('Desktop foundation created')
    expect(reviewState.reviewRequest).toMatchObject({
      continueActionLabel: 'Continue',
      title: 'Review generated foundation',
    })
  })

  it('does not expose production security decision actions', () => {
    expect(mockConversationRuntime).not.toHaveProperty('approvePermission')
    expect(mockConversationRuntime).not.toHaveProperty('denyPermission')
    expect(mockConversationRuntime).not.toHaveProperty('executeCommand')
    expect(mockConversationRuntime).not.toHaveProperty('readSecret')
  })
})
