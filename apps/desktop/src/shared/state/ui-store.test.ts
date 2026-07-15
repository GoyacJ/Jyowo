import { beforeEach, describe, expect, it } from 'vitest'

import { createUiStore } from './ui-store'

describe('ui-store', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('stores local UI state only', () => {
    const store = createUiStore()

    expect(store.getState().theme).toBe('system')
    expect(store.getState().sidebarSections).toEqual({
      pinned: true,
      projects: true,
      conversations: true,
    })
    expect(store.getState().expandedProjects).toEqual({})

    store.getState().setSidebarCollapsed(true)
    store.getState().setSidebarSectionExpanded('pinned', false)
    store.getState().setProjectExpanded('/repo/alpha', true)
    store.getState().setContextPanelCollapsed(true)
    store.getState().setActiveRun({ conversationId: 'conversation-001', runId: 'run-001' })
    store.getState().setInspectorOpen(false)
    store.getState().setTheme('dark')
    store.getState().setLocale('en-US')

    expect(store.getState().activeRunConversationId).toBe('conversation-001')
    expect(store.getState().activeRunId).toBe('run-001')
    expect(store.getState().sidebarCollapsed).toBe(true)
    expect(store.getState().sidebarSections.pinned).toBe(false)
    expect(store.getState().expandedProjects['/repo/alpha']).toBe(true)
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

  it('uses the synchronous theme preference applied before React mounts', () => {
    localStorage.setItem('jyowo-ui-theme', 'light')

    const store = createUiStore()

    expect(store.getState().theme).toBe('light')
  })

  it('tracks timeline scroll requests by turn anchor id', () => {
    const store = createUiStore()

    store.getState().requestTimelineScroll('segment:permission:001')

    expect(store.getState().timelineScrollRequest).toEqual({
      anchorId: 'segment:permission:001',
      nonce: 1,
    })
  })

  it('isolates timeline position, follow mode, indicator, and anchor by task', () => {
    const store = createUiStore()

    store.getState().setTimelineScrollSession('task-1', {
      hasNewContent: true,
      mode: 'paused',
      newItemCount: 3,
      scrollTop: 420,
      showJumpToLatest: true,
      visibleAnchor: { key: 'block-4', offset: -12, virtualIndex: 4, virtualStart: 960 },
    })
    store.getState().setTimelineScrollSession('task-2', {
      hasNewContent: false,
      mode: 'following',
      newItemCount: 0,
      scrollTop: 900,
      showJumpToLatest: false,
      visibleAnchor: null,
    })

    expect(store.getState().timelineScrollByTaskId['task-1']).toMatchObject({
      mode: 'paused',
      newItemCount: 3,
      scrollTop: 420,
      visibleAnchor: { key: 'block-4', virtualIndex: 4 },
    })
    expect(store.getState().timelineScrollByTaskId['task-2']).toMatchObject({
      mode: 'following',
      scrollTop: 900,
    })
  })

  it('isolates workbench sessions by task and clamps the shared width preference', () => {
    const store = createUiStore()

    store.getState().openTaskWorkbench({
      kind: 'diff',
      resourceId: 'diff-1',
      taskId: 'task-1',
      title: 'First diff',
    })
    store.getState().openTaskWorkbench({
      kind: 'command',
      resourceId: 'command-1',
      taskId: 'task-2',
      title: 'Command',
    })
    store.getState().closeTaskWorkbench('task-1')
    store.getState().setTaskWorkbenchWidth(900)

    expect(store.getState().taskWorkbenchByTaskId['task-1']).toMatchObject({ open: false })
    expect(store.getState().taskWorkbenchByTaskId['task-2']).toMatchObject({
      activeTabId: 'command:command-1',
      open: true,
    })
    expect(store.getState().taskWorkbenchWidth).toBe(640)
  })
})
