import { load, type Store } from '@tauri-apps/plugin-store'

export const UI_PREFERENCES_STORE_PATH = 'ui-preferences.json'

export type UiThemePreference = 'system' | 'light' | 'dark'

export type UiPreferences = {
  theme: UiThemePreference
  sidebarCollapsed: boolean
  chatComposerHeight: number
  contextPanelWidth: number
}

const UI_PREFERENCES_DEFAULTS: UiPreferences = {
  theme: 'light',
  sidebarCollapsed: false,
  chatComposerHeight: 160,
  contextPanelWidth: 320,
}

let storePromise: Promise<Store> | undefined

export function loadUiPreferencesStore() {
  storePromise ??= load(UI_PREFERENCES_STORE_PATH, {
    autoSave: 100,
    defaults: { ...UI_PREFERENCES_DEFAULTS },
    overrideDefaults: true,
  })

  return storePromise
}

export async function readUiPreferences(): Promise<UiPreferences> {
  const store = await loadUiPreferencesStore()
  const [theme, sidebarCollapsed, chatComposerHeight, contextPanelWidth] = await Promise.all([
    store.get<UiThemePreference>('theme'),
    store.get<boolean>('sidebarCollapsed'),
    store.get<number>('chatComposerHeight'),
    store.get<number>('contextPanelWidth'),
  ])

  return {
    theme: isUiThemePreference(theme) ? theme : UI_PREFERENCES_DEFAULTS.theme,
    sidebarCollapsed:
      typeof sidebarCollapsed === 'boolean'
        ? sidebarCollapsed
        : UI_PREFERENCES_DEFAULTS.sidebarCollapsed,
    chatComposerHeight:
      typeof chatComposerHeight === 'number'
        ? chatComposerHeight
        : UI_PREFERENCES_DEFAULTS.chatComposerHeight,
    contextPanelWidth:
      typeof contextPanelWidth === 'number'
        ? contextPanelWidth
        : UI_PREFERENCES_DEFAULTS.contextPanelWidth,
  }
}

export async function writeUiPreferences(preferences: Partial<UiPreferences>) {
  const store = await loadUiPreferencesStore()
  const entries = Object.entries(preferences) as Array<
    [keyof UiPreferences, UiPreferences[keyof UiPreferences]]
  >

  await Promise.all(entries.map(([key, value]) => store.set(key, value)))
}

function isUiThemePreference(value: unknown): value is UiThemePreference {
  return value === 'system' || value === 'light' || value === 'dark'
}
