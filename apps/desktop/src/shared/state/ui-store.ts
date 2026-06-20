import { useStore } from 'zustand'
import { createStore } from 'zustand/vanilla'

import { type AppLocale, DEFAULT_APP_LOCALE } from '@/shared/i18n/locales'

type ThemeMode = 'light' | 'dark' | 'system'

export interface UiState {
  activeRunConversationId: string | undefined
  activeRunId: string | undefined
  theme: ThemeMode
  locale: AppLocale
  sidebarCollapsed: boolean
  contextPanelCollapsed: boolean
  activityRailCollapsed: boolean
  activityRailExpanded: boolean
  inspectorOpen: boolean
  clearActiveRun: () => void
  setActiveRun: (activeRun: { conversationId: string; runId: string }) => void
  setTheme: (theme: ThemeMode) => void
  setLocale: (locale: AppLocale) => void
  setSidebarCollapsed: (sidebarCollapsed: boolean) => void
  setContextPanelCollapsed: (contextPanelCollapsed: boolean) => void
  setActivityRailCollapsed: (activityRailCollapsed: boolean) => void
  setActivityRailExpanded: (activityRailExpanded: boolean) => void
  setInspectorOpen: (inspectorOpen: boolean) => void
}

export function createUiStore() {
  return createStore<UiState>()((set) => ({
    activeRunConversationId: undefined,
    activeRunId: undefined,
    theme: 'light',
    locale: DEFAULT_APP_LOCALE,
    sidebarCollapsed: false,
    contextPanelCollapsed: false,
    activityRailCollapsed: false,
    activityRailExpanded: false,
    inspectorOpen: true,
    clearActiveRun: () => set({ activeRunConversationId: undefined, activeRunId: undefined }),
    setActiveRun: (activeRun) =>
      set({
        activeRunConversationId: activeRun.conversationId,
        activeRunId: activeRun.runId,
      }),
    setTheme: (theme) => set({ theme }),
    setLocale: (locale) => set({ locale }),
    setSidebarCollapsed: (sidebarCollapsed) => set({ sidebarCollapsed }),
    setContextPanelCollapsed: (contextPanelCollapsed) => set({ contextPanelCollapsed }),
    setActivityRailCollapsed: (activityRailCollapsed) => set({ activityRailCollapsed }),
    setActivityRailExpanded: (activityRailExpanded) => set({ activityRailExpanded }),
    setInspectorOpen: (inspectorOpen) => set({ inspectorOpen }),
  }))
}

export const uiStore = createUiStore()

export function useUiStore<T>(selector: (state: UiState) => T) {
  return useStore(uiStore, selector)
}
