import { load, type Store } from '@tauri-apps/plugin-store'

import { type AppLocale, DEFAULT_APP_LOCALE, isAppLocale } from '@/shared/i18n/locales'
import type { TaskWorkbenchMode } from '@/shared/state/workbench-selection'

export const UI_PREFERENCES_STORE_PATH = 'ui-preferences.json'

export type UiThemePreference = 'system' | 'light' | 'dark'

export type UiPreferences = {
  theme: UiThemePreference
  locale: AppLocale
  sidebarCollapsed: boolean
  taskWorkbenchMode: TaskWorkbenchMode
  chatComposerHeight: number
  contextPanelWidth: number
}

const UI_PREFERENCES_DEFAULTS: UiPreferences = {
  theme: 'system',
  locale: DEFAULT_APP_LOCALE,
  sidebarCollapsed: false,
  taskWorkbenchMode: 'closed',
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
  const [
    theme,
    locale,
    sidebarCollapsed,
    taskWorkbenchMode,
    chatComposerHeight,
    contextPanelWidth,
  ] = await Promise.all([
    store.get<UiThemePreference>('theme'),
    store.get<AppLocale>('locale'),
    store.get<boolean>('sidebarCollapsed'),
    store.get<TaskWorkbenchMode>('taskWorkbenchMode'),
    store.get<number>('chatComposerHeight'),
    store.get<number>('contextPanelWidth'),
  ])

  return {
    theme: isUiThemePreference(theme) ? theme : UI_PREFERENCES_DEFAULTS.theme,
    locale: isAppLocale(locale) ? locale : UI_PREFERENCES_DEFAULTS.locale,
    sidebarCollapsed:
      typeof sidebarCollapsed === 'boolean'
        ? sidebarCollapsed
        : UI_PREFERENCES_DEFAULTS.sidebarCollapsed,
    taskWorkbenchMode: isTaskWorkbenchMode(taskWorkbenchMode)
      ? taskWorkbenchMode
      : UI_PREFERENCES_DEFAULTS.taskWorkbenchMode,
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
  if (preferences.theme && typeof window !== 'undefined') {
    window.localStorage.setItem('jyowo-ui-theme', preferences.theme)
  }
}

function isUiThemePreference(value: unknown): value is UiThemePreference {
  return value === 'system' || value === 'light' || value === 'dark'
}

function isTaskWorkbenchMode(value: unknown): value is TaskWorkbenchMode {
  return value === 'closed' || value === 'inspector' || value === 'collaboration'
}
