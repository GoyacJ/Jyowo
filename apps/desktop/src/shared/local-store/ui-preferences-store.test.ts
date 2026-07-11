import { beforeEach, describe, expect, it, vi } from 'vitest'

const storeFixture = vi.hoisted(() => {
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
  load: storeFixture.load,
}))

async function importUiPreferencesStore() {
  return import('./ui-preferences-store')
}

describe('ui-preferences-store', () => {
  beforeEach(() => {
    storeFixture.state.clear()
    storeFixture.get.mockClear()
    storeFixture.set.mockClear()
    storeFixture.load.mockClear()
    vi.resetModules()
  })

  it('loads the Tauri store with local UI defaults', async () => {
    const { UI_PREFERENCES_STORE_PATH, readUiPreferences } = await importUiPreferencesStore()

    await expect(readUiPreferences()).resolves.toEqual({
      theme: 'system',
      locale: 'zh-CN',
      sidebarCollapsed: false,
      taskWorkbenchMode: 'closed',
      chatComposerHeight: 160,
      contextPanelWidth: 320,
    })

    expect(storeFixture.load).toHaveBeenCalledWith(UI_PREFERENCES_STORE_PATH, {
      autoSave: 100,
      defaults: {
        theme: 'system',
        locale: 'zh-CN',
        sidebarCollapsed: false,
        taskWorkbenchMode: 'closed',
        chatComposerHeight: 160,
        contextPanelWidth: 320,
      },
      overrideDefaults: true,
    })
  })

  it('falls back when stored values are invalid', async () => {
    storeFixture.state.set('theme', 'blue')
    storeFixture.state.set('locale', 'pirate')
    storeFixture.state.set('sidebarCollapsed', 'yes')
    storeFixture.state.set('taskWorkbenchMode', 'wide')
    storeFixture.state.set('chatComposerHeight', 'tall')
    storeFixture.state.set('contextPanelWidth', 'wide')

    const { readUiPreferences } = await importUiPreferencesStore()

    await expect(readUiPreferences()).resolves.toEqual({
      theme: 'system',
      locale: 'zh-CN',
      sidebarCollapsed: false,
      taskWorkbenchMode: 'closed',
      chatComposerHeight: 160,
      contextPanelWidth: 320,
    })
  })

  it('writes partial UI preferences to the Tauri store', async () => {
    const { readUiPreferences, writeUiPreferences } = await importUiPreferencesStore()

    await writeUiPreferences({
      theme: 'dark',
      locale: 'en-US',
      sidebarCollapsed: true,
      taskWorkbenchMode: 'collaboration',
      contextPanelWidth: 420,
    })

    expect(storeFixture.set).toHaveBeenCalledWith('theme', 'dark')
    expect(storeFixture.set).toHaveBeenCalledWith('locale', 'en-US')
    expect(storeFixture.set).toHaveBeenCalledWith('sidebarCollapsed', true)
    expect(storeFixture.set).toHaveBeenCalledWith('taskWorkbenchMode', 'collaboration')
    expect(storeFixture.set).toHaveBeenCalledWith('contextPanelWidth', 420)
    await expect(readUiPreferences()).resolves.toMatchObject({
      theme: 'dark',
      locale: 'en-US',
      sidebarCollapsed: true,
      contextPanelWidth: 420,
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
