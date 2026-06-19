import { beforeEach, describe, expect, it, vi } from 'vitest'

const storeMock = vi.hoisted(() => {
  const state = new Map<string, unknown>()
  const get = vi.fn(async (key: string) => state.get(key))
  const set = vi.fn(async (key: string, value: unknown) => {
    state.set(key, value)
  })

  return {
    state,
    get,
    set,
    load: vi.fn(async () => ({
      get,
      set,
    })),
  }
})

vi.mock('@tauri-apps/plugin-store', () => ({
  load: storeMock.load,
}))

async function importUiPreferencesStore() {
  return import('./ui-preferences-store')
}

describe('ui-preferences-store', () => {
  beforeEach(() => {
    storeMock.state.clear()
    storeMock.get.mockClear()
    storeMock.set.mockClear()
    storeMock.load.mockClear()
    vi.resetModules()
  })

  it('loads the Tauri store with local UI defaults', async () => {
    const { UI_PREFERENCES_STORE_PATH, readUiPreferences } = await importUiPreferencesStore()

    await expect(readUiPreferences()).resolves.toEqual({
      theme: 'light',
      sidebarCollapsed: false,
      chatComposerHeight: 160,
      contextPanelWidth: 320,
      lastSelectedWorkspaceRef: null,
    })

    expect(storeMock.load).toHaveBeenCalledWith(UI_PREFERENCES_STORE_PATH, {
      autoSave: 100,
      defaults: {
        theme: 'light',
        sidebarCollapsed: false,
        chatComposerHeight: 160,
        contextPanelWidth: 320,
        lastSelectedWorkspaceRef: null,
      },
      overrideDefaults: true,
    })
  })

  it('falls back when stored values are invalid', async () => {
    storeMock.state.set('theme', 'blue')
    storeMock.state.set('sidebarCollapsed', 'yes')
    storeMock.state.set('chatComposerHeight', 'tall')
    storeMock.state.set('contextPanelWidth', 'wide')
    storeMock.state.set('lastSelectedWorkspaceRef', { id: 'workspace-001' })

    const { readUiPreferences } = await importUiPreferencesStore()

    await expect(readUiPreferences()).resolves.toEqual({
      theme: 'light',
      sidebarCollapsed: false,
      chatComposerHeight: 160,
      contextPanelWidth: 320,
      lastSelectedWorkspaceRef: null,
    })
  })

  it('writes partial UI preferences to the Tauri store', async () => {
    const { readUiPreferences, writeUiPreferences } = await importUiPreferencesStore()

    await writeUiPreferences({
      theme: 'dark',
      sidebarCollapsed: true,
      contextPanelWidth: 420,
      lastSelectedWorkspaceRef: 'local:current',
    })

    expect(storeMock.set).toHaveBeenCalledWith('theme', 'dark')
    expect(storeMock.set).toHaveBeenCalledWith('sidebarCollapsed', true)
    expect(storeMock.set).toHaveBeenCalledWith('contextPanelWidth', 420)
    expect(storeMock.set).toHaveBeenCalledWith('lastSelectedWorkspaceRef', 'local:current')
    await expect(readUiPreferences()).resolves.toMatchObject({
      theme: 'dark',
      sidebarCollapsed: true,
      contextPanelWidth: 420,
      lastSelectedWorkspaceRef: 'local:current',
    })
  })

  it('does not expose credential-shaped preferences', async () => {
    const { readUiPreferences } = await importUiPreferencesStore()

    const preferences = await readUiPreferences()

    expect(preferences).not.toHaveProperty('apiKey')
    expect(preferences).not.toHaveProperty('token')
    expect(preferences).not.toHaveProperty('secret')
    expect(preferences).not.toHaveProperty('providerCredentials')
  })
})
