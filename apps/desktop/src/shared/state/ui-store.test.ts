import { describe, expect, it } from 'vitest'

import { createUiStore } from './ui-store'

describe('ui-store', () => {
  it('stores local UI state only', () => {
    const store = createUiStore()

    store.getState().setSidebarCollapsed(true)
    store.getState().setInspectorOpen(false)
    store.getState().setTheme('dark')

    expect(store.getState().sidebarCollapsed).toBe(true)
    expect(store.getState().inspectorOpen).toBe(false)
    expect(store.getState().theme).toBe('dark')
    expect(store.getState()).not.toHaveProperty('appInfo')
    expect(store.getState()).not.toHaveProperty('healthcheck')
  })
})
