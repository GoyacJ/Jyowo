import { useStore } from 'zustand'
import { createStore } from 'zustand/vanilla'

type ThemeMode = 'light' | 'dark' | 'system'

export interface UiState {
  theme: ThemeMode
  sidebarCollapsed: boolean
  inspectorOpen: boolean
  setTheme: (theme: ThemeMode) => void
  setSidebarCollapsed: (sidebarCollapsed: boolean) => void
  setInspectorOpen: (inspectorOpen: boolean) => void
}

export function createUiStore() {
  return createStore<UiState>()((set) => ({
    theme: 'system',
    sidebarCollapsed: false,
    inspectorOpen: true,
    setTheme: (theme) => set({ theme }),
    setSidebarCollapsed: (sidebarCollapsed) => set({ sidebarCollapsed }),
    setInspectorOpen: (inspectorOpen) => set({ inspectorOpen }),
  }))
}

const uiStore = createUiStore()

export function useUiStore<T>(selector: (state: UiState) => T) {
  return useStore(uiStore, selector)
}
