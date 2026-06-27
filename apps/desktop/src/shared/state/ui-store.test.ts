import { describe, expect, it } from 'vitest'

import { createUiStore } from './ui-store'

describe('ui-store', () => {
  it('stores local UI state only', () => {
    const store = createUiStore()

    store.getState().setSidebarCollapsed(true)
    store.getState().setContextPanelCollapsed(true)
    store.getState().setActiveRun({ conversationId: 'conversation-001', runId: 'run-001' })
    store.getState().setInspectorOpen(false)
    store.getState().setTheme('dark')
    store.getState().setLocale('en-US')

    expect(store.getState().activeRunConversationId).toBe('conversation-001')
    expect(store.getState().activeRunId).toBe('run-001')
    expect(store.getState().sidebarCollapsed).toBe(true)
    expect(store.getState().contextPanelCollapsed).toBe(true)
    expect(store.getState().inspectorOpen).toBe(false)
    expect(store.getState().theme).toBe('dark')
    expect(store.getState().locale).toBe('en-US')

    store.getState().clearActiveRun()

    expect(store.getState().activeRunConversationId).toBeUndefined()
    expect(store.getState().activeRunId).toBeUndefined()
    expect(store.getState()).not.toHaveProperty('appInfo')
    expect(store.getState()).not.toHaveProperty('healthcheck')
    expect(store.getState()).not.toHaveProperty('conversations')
    expect(store.getState()).not.toHaveProperty('runs')
    expect(store.getState()).not.toHaveProperty('tools')
    expect(store.getState()).not.toHaveProperty('servers')
    expect(store.getState()).not.toHaveProperty('secrets')
  })

  it('tracks active runs by conversation', () => {
    const store = createUiStore()

    store.getState().setActiveRun({ conversationId: 'conversation-001', runId: 'run-001' })
    store.getState().setActiveRun({ conversationId: 'conversation-002', runId: 'run-002' })

    expect(store.getState().activeRunsByConversation).toEqual({
      'conversation-001': 'run-001',
      'conversation-002': 'run-002',
    })

    store.getState().clearActiveRun('conversation-001')

    expect(store.getState().activeRunsByConversation).toEqual({
      'conversation-002': 'run-002',
    })
  })

  it('does not expand conversation context when a run becomes active', () => {
    const store = createUiStore()

    store.getState().setContextPanelCollapsed(true)
    store.getState().setActiveRun({ conversationId: 'conversation-001', runId: 'run-001' })

    expect(store.getState().contextPanelCollapsed).toBe(true)
  })

  it('tracks timeline scroll requests by turn anchor id', () => {
    const store = createUiStore()

    store.getState().requestTimelineScroll('segment:permission:001')

    expect(store.getState().timelineScrollRequest).toEqual({
      anchorId: 'segment:permission:001',
      nonce: 1,
    })
  })
})
