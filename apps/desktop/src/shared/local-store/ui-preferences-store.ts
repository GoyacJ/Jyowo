import { load, type Store } from '@tauri-apps/plugin-store'

import { type AppLocale, DEFAULT_APP_LOCALE, isAppLocale } from '@/shared/i18n/locales'
import {
  clampTaskWorkbenchWidth,
  DEFAULT_TASK_WORKBENCH_WIDTH,
} from '@/shared/state/workbench-selection'

export const UI_PREFERENCES_STORE_PATH = 'ui-preferences.json'

export type UiThemePreference = 'system' | 'light' | 'dark'

export type SidebarSectionsPreference = {
  pinned: boolean
  projects: boolean
  conversations: boolean
}

export type UiPreferences = {
  theme: UiThemePreference
  locale: AppLocale
  sidebarCollapsed: boolean
  sidebarSections: SidebarSectionsPreference
  expandedProjects: Record<string, boolean>
  taskWorkbenchWidth: number
  chatComposerHeight: number
  contextPanelWidth: number
}

const UI_PREFERENCES_DEFAULTS: UiPreferences = {
  theme: 'system',
  locale: DEFAULT_APP_LOCALE,
  sidebarCollapsed: false,
  sidebarSections: {
    pinned: true,
    projects: true,
    conversations: true,
  },
  expandedProjects: {},
  taskWorkbenchWidth: DEFAULT_TASK_WORKBENCH_WIDTH,
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
    sidebarSections,
    expandedProjects,
    taskWorkbenchWidth,
    legacyTaskWorkbenchMode,
    chatComposerHeight,
    contextPanelWidth,
  ] = await Promise.all([
    store.get<UiThemePreference>('theme'),
    store.get<AppLocale>('locale'),
    store.get<boolean>('sidebarCollapsed'),
    store.get<SidebarSectionsPreference>('sidebarSections'),
    store.get<Record<string, boolean>>('expandedProjects'),
    store.get<number>('taskWorkbenchWidth'),
    store.get<string>('taskWorkbenchMode'),
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
    sidebarSections: isSidebarSectionsPreference(sidebarSections)
      ? sidebarSections
      : { ...UI_PREFERENCES_DEFAULTS.sidebarSections },
    expandedProjects: isExpandedProjectsPreference(expandedProjects) ? expandedProjects : {},
    taskWorkbenchWidth:
      typeof taskWorkbenchWidth === 'number' && Number.isFinite(taskWorkbenchWidth)
        ? clampTaskWorkbenchWidth(taskWorkbenchWidth)
        : legacyTaskWorkbenchWidth(legacyTaskWorkbenchMode),
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

function legacyTaskWorkbenchWidth(value: unknown) {
  return value === 'collaboration' ? 560 : UI_PREFERENCES_DEFAULTS.taskWorkbenchWidth
}

function isSidebarSectionsPreference(value: unknown): value is SidebarSectionsPreference {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const candidate = value as Partial<SidebarSectionsPreference>
  return (
    typeof candidate.pinned === 'boolean' &&
    typeof candidate.projects === 'boolean' &&
    typeof candidate.conversations === 'boolean'
  )
}

function isExpandedProjectsPreference(value: unknown): value is Record<string, boolean> {
  return (
    !!value &&
    typeof value === 'object' &&
    !Array.isArray(value) &&
    Object.values(value).every((expanded) => typeof expanded === 'boolean')
  )
}
