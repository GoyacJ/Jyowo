import { useStore } from 'zustand'
import { createStore } from 'zustand/vanilla'

type ThemeMode = 'light' | 'dark' | 'system'

export interface UiState {
  activeRunConversationId: string | undefined
  activeRunId: string | undefined
  theme: ThemeMode
  sidebarCollapsed: boolean
  contextPanelCollapsed: boolean
  activityRailCollapsed: boolean
  activityRailExpanded: boolean
  inspectorOpen: boolean
  clearActiveRun: () => void
  setActiveRun: (activeRun: { conversationId: string; runId: string }) => void
  setTheme: (theme: ThemeMode) => void
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
