import { describe, expect, it } from 'vitest'

import { createUiStore } from './ui-store'

describe('ui-store', () => {
  it('stores local UI state only', () => {
    const store = createUiStore()

    store.getState().setSidebarCollapsed(true)
    store.getState().setActivityRailCollapsed(true)
    store.getState().setActivityRailExpanded(true)
    store.getState().setActiveRun({ conversationId: 'conversation-001', runId: 'run-001' })
    store.getState().setInspectorOpen(false)
    store.getState().setTheme('dark')
    store.getState().setLocale('en-US')

    expect(store.getState().activeRunConversationId).toBe('conversation-001')
    expect(store.getState().activeRunId).toBe('run-001')
    expect(store.getState().sidebarCollapsed).toBe(true)
    expect(store.getState().activityRailCollapsed).toBe(true)
    expect(store.getState().activityRailExpanded).toBe(true)
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
})
