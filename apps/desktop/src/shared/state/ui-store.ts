import { useStore } from 'zustand'
import { createStore } from 'zustand/vanilla'

import { type AppLocale, DEFAULT_APP_LOCALE } from '@/shared/i18n/locales'
import type { WorkbenchSelection } from '@/shared/state/workbench-selection'

type ThemeMode = 'light' | 'dark' | 'system'

type TimelineScrollRequest = {
  anchorId: string
  nonce: number
}

export interface UiState {
  activeRunConversationId: string | undefined
  activeRunId: string | undefined
  activeRunsByConversation: Record<string, string>
  evidenceDisclosureOpen: Record<string, boolean>
  theme: ThemeMode
  locale: AppLocale
  sidebarCollapsed: boolean
  contextPanelCollapsed: boolean
  inspectorOpen: boolean
  workbenchSelection: WorkbenchSelection | null
  timelineScrollRequest: TimelineScrollRequest | null
  clearActiveRun: (conversationId?: string) => void
  resetEvidenceDisclosure: () => void
  setEvidenceDisclosureOpen: (id: string, open: boolean) => void
  setActiveRun: (activeRun: { conversationId: string; runId: string }) => void
  setTheme: (theme: ThemeMode) => void
  setLocale: (locale: AppLocale) => void
  setSidebarCollapsed: (sidebarCollapsed: boolean) => void
  setContextPanelCollapsed: (contextPanelCollapsed: boolean) => void
  setInspectorOpen: (inspectorOpen: boolean) => void
  setWorkbenchSelection: (selection: WorkbenchSelection | null) => void
  requestTimelineScroll: (anchorId: string) => void
  clearTimelineScrollRequest: () => void
}

export function createUiStore() {
  return createStore<UiState>()((set) => ({
    activeRunConversationId: undefined,
    activeRunId: undefined,
    activeRunsByConversation: {},
    evidenceDisclosureOpen: {},
    theme: 'light',
    locale: DEFAULT_APP_LOCALE,
    sidebarCollapsed: false,
    contextPanelCollapsed: true,
    inspectorOpen: false,
    workbenchSelection: null,
    timelineScrollRequest: null,
    clearActiveRun: (conversationId) =>
      set((state) => {
        if (!conversationId) {
          return {
            activeRunConversationId: undefined,
            activeRunId: undefined,
            activeRunsByConversation: {},
          }
        }

        const { [conversationId]: _removedRunId, ...activeRunsByConversation } =
          state.activeRunsByConversation
        const nextActiveRun = latestActiveRun(activeRunsByConversation)

        return {
          activeRunConversationId: nextActiveRun?.conversationId,
          activeRunId: nextActiveRun?.runId,
          activeRunsByConversation,
        }
      }),
    resetEvidenceDisclosure: () => set({ evidenceDisclosureOpen: {} }),
    setEvidenceDisclosureOpen: (id, open) =>
      set((state) => ({
        evidenceDisclosureOpen: {
          ...state.evidenceDisclosureOpen,
          [id]: open,
        },
      })),
    setActiveRun: (activeRun) =>
      set((state) => ({
        activeRunConversationId: activeRun.conversationId,
        activeRunId: activeRun.runId,
        activeRunsByConversation: {
          ...state.activeRunsByConversation,
          [activeRun.conversationId]: activeRun.runId,
        },
      })),
    setTheme: (theme) => set({ theme }),
    setLocale: (locale) => set({ locale }),
    setSidebarCollapsed: (sidebarCollapsed) => set({ sidebarCollapsed }),
    setContextPanelCollapsed: (contextPanelCollapsed) => set({ contextPanelCollapsed }),
    setInspectorOpen: (inspectorOpen) => set({ inspectorOpen }),
    setWorkbenchSelection: (workbenchSelection) => set({ workbenchSelection }),
    requestTimelineScroll: (anchorId) =>
      set((state) => ({
        timelineScrollRequest: {
          anchorId,
          nonce: (state.timelineScrollRequest?.nonce ?? 0) + 1,
        },
      })),
    clearTimelineScrollRequest: () => set({ timelineScrollRequest: null }),
  }))
}

export const uiStore = createUiStore()

export function useUiStore<T>(selector: (state: UiState) => T) {
  return useStore(uiStore, selector)
}

function latestActiveRun(activeRunsByConversation: Record<string, string>) {
  const [conversationId, runId] = Object.entries(activeRunsByConversation).at(-1) ?? []

  if (!conversationId || !runId) {
    return undefined
  }

  return { conversationId, runId }
}
