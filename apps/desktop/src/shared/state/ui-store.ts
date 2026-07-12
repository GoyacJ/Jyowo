import { useStore } from 'zustand'
import { createStore } from 'zustand/vanilla'

import { type AppLocale, DEFAULT_APP_LOCALE } from '@/shared/i18n/locales'
import type {
  TaskWorkbenchMode,
  TaskWorkbenchSelection,
  WorkbenchSelection,
} from '@/shared/state/workbench-selection'

type ThemeMode = 'light' | 'dark' | 'system'
export type SidebarSection = 'pinned' | 'projects' | 'conversations'
export type SidebarSections = Record<SidebarSection, boolean>

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
  sidebarSections: SidebarSections
  expandedProjects: Record<string, boolean>
  contextPanelCollapsed: boolean
  inspectorOpen: boolean
  workbenchSelection: WorkbenchSelection | null
  taskWorkbenchMode: TaskWorkbenchMode
  taskWorkbenchSelection: TaskWorkbenchSelection | null
  timelineScrollRequest: TimelineScrollRequest | null
  clearActiveRun: (conversationId?: string) => void
  resetEvidenceDisclosure: () => void
  setEvidenceDisclosureOpen: (id: string, open: boolean) => void
  setActiveRun: (activeRun: { conversationId: string; runId: string }) => void
  setTheme: (theme: ThemeMode) => void
  setLocale: (locale: AppLocale) => void
  setSidebarCollapsed: (sidebarCollapsed: boolean) => void
  setSidebarSectionExpanded: (section: SidebarSection, expanded: boolean) => void
  setProjectExpanded: (path: string, expanded: boolean) => void
  setContextPanelCollapsed: (contextPanelCollapsed: boolean) => void
  setInspectorOpen: (inspectorOpen: boolean) => void
  setWorkbenchSelection: (selection: WorkbenchSelection | null) => void
  setTaskWorkbenchMode: (mode: TaskWorkbenchMode) => void
  setTaskWorkbenchSelection: (selection: TaskWorkbenchSelection | null) => void
  requestTimelineScroll: (anchorId: string) => void
  clearTimelineScrollRequest: () => void
}

export function createUiStore() {
  return createStore<UiState>()((set) => ({
    activeRunConversationId: undefined,
    activeRunId: undefined,
    activeRunsByConversation: {},
    evidenceDisclosureOpen: {},
    theme: initialTheme(),
    locale: DEFAULT_APP_LOCALE,
    sidebarCollapsed: false,
    sidebarSections: {
      pinned: true,
      projects: true,
      conversations: true,
    },
    expandedProjects: {},
    contextPanelCollapsed: true,
    inspectorOpen: false,
    workbenchSelection: null,
    taskWorkbenchMode: 'closed',
    taskWorkbenchSelection: null,
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
    setSidebarSectionExpanded: (section, expanded) =>
      set((state) => ({
        sidebarSections: {
          ...state.sidebarSections,
          [section]: expanded,
        },
      })),
    setProjectExpanded: (path, expanded) =>
      set((state) => ({
        expandedProjects: {
          ...state.expandedProjects,
          [path]: expanded,
        },
      })),
    setContextPanelCollapsed: (contextPanelCollapsed) => set({ contextPanelCollapsed }),
    setInspectorOpen: (inspectorOpen) => set({ inspectorOpen }),
    setWorkbenchSelection: (workbenchSelection) => set({ workbenchSelection }),
    setTaskWorkbenchMode: (taskWorkbenchMode) => set({ taskWorkbenchMode }),
    setTaskWorkbenchSelection: (taskWorkbenchSelection) => set({ taskWorkbenchSelection }),
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

function initialTheme(): ThemeMode {
  if (typeof window === 'undefined') return 'system'
  try {
    const stored = window.localStorage.getItem('jyowo-ui-theme')
    return stored === 'light' || stored === 'dark' || stored === 'system' ? stored : 'system'
  } catch {
    return 'system'
  }
}
