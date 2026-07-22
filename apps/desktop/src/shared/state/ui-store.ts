import { useStore } from 'zustand'
import { createStore } from 'zustand/vanilla'

import { type AppLocale, DEFAULT_APP_LOCALE } from '@/shared/i18n/locales'
import { clampSidebarWidth, DEFAULT_SIDEBAR_WIDTH } from '@/shared/state/sidebar-layout'
import type {
  TaskWorkbenchSession,
  TaskWorkbenchTarget,
  TaskWorkbenchViewportGeometry,
  TaskWorkbenchViewportMode,
  WorkbenchSelection,
} from '@/shared/state/workbench-selection'
import {
  activateTaskWorkbenchTab,
  clampTaskWorkbenchWidth,
  closeTaskWorkbenchTab,
  DEFAULT_TASK_WORKBENCH_WIDTH,
  openTaskWorkbenchTarget,
  setTaskWorkbenchTabPinned,
  setTaskWorkbenchViewportGeometry,
  setTaskWorkbenchViewportMode,
} from '@/shared/state/workbench-selection'

type ThemeMode = 'light' | 'dark' | 'system'
export type SidebarSection = 'pinned' | 'projects' | 'conversations'
export type SidebarSections = Record<SidebarSection, boolean>

type TimelineScrollRequest = {
  anchorId: string
  nonce: number
}

export type TimelineVisibleAnchor = {
  key: string
  offset: number
  virtualIndex?: number
  virtualStart?: number
}

export type TimelineScrollSession = {
  hasNewContent: boolean
  mode: 'following' | 'paused'
  newItemCount: number
  scrollTop: number
  showJumpToLatest: boolean
  visibleAnchor: TimelineVisibleAnchor | null
}

export interface UiState {
  activeRunConversationId: string | undefined
  activeRunId: string | undefined
  activeRunsByConversation: Record<string, string>
  evidenceDisclosureOpen: Record<string, boolean>
  theme: ThemeMode
  locale: AppLocale
  sidebarCollapsed: boolean
  sidebarWidth: number
  sidebarSections: SidebarSections
  expandedProjects: Record<string, boolean>
  contextPanelCollapsed: boolean
  inspectorOpen: boolean
  workbenchSelection: WorkbenchSelection | null
  taskWorkbenchByTaskId: Record<string, TaskWorkbenchSession>
  taskWorkbenchSummaryCollapsed: boolean
  taskWorkbenchWidth: number
  timelineScrollByTaskId: Record<string, TimelineScrollSession>
  timelineScrollRequest: TimelineScrollRequest | null
  clearActiveRun: (conversationId?: string) => void
  resetEvidenceDisclosure: () => void
  setEvidenceDisclosureOpen: (id: string, open: boolean) => void
  setActiveRun: (activeRun: { conversationId: string; runId: string }) => void
  setTheme: (theme: ThemeMode) => void
  setLocale: (locale: AppLocale) => void
  setSidebarCollapsed: (sidebarCollapsed: boolean) => void
  setSidebarWidth: (sidebarWidth: number) => void
  setSidebarSectionExpanded: (section: SidebarSection, expanded: boolean) => void
  setProjectExpanded: (path: string, expanded: boolean) => void
  setContextPanelCollapsed: (contextPanelCollapsed: boolean) => void
  setInspectorOpen: (inspectorOpen: boolean) => void
  setWorkbenchSelection: (selection: WorkbenchSelection | null) => void
  openTaskWorkbench: (target: TaskWorkbenchTarget) => void
  closeTaskWorkbench: (taskId: string) => void
  activateTaskWorkbenchTab: (taskId: string, tabId: string) => void
  closeTaskWorkbenchTab: (taskId: string, tabId: string) => void
  setTaskWorkbenchTabPinned: (taskId: string, tabId: string, pinned: boolean) => void
  setTaskWorkbenchViewportGeometry: (
    taskId: string,
    geometry: TaskWorkbenchViewportGeometry,
  ) => void
  setTaskWorkbenchViewportMode: (taskId: string, mode: TaskWorkbenchViewportMode) => void
  setTaskWorkbenchSummaryCollapsed: (collapsed: boolean) => void
  setTaskWorkbenchWidth: (width: number) => void
  requestTimelineScroll: (anchorId: string) => void
  setTimelineScrollSession: (taskId: string, session: TimelineScrollSession) => void
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
    sidebarWidth: DEFAULT_SIDEBAR_WIDTH,
    sidebarSections: {
      pinned: true,
      projects: true,
      conversations: true,
    },
    expandedProjects: {},
    contextPanelCollapsed: true,
    inspectorOpen: false,
    workbenchSelection: null,
    taskWorkbenchByTaskId: {},
    taskWorkbenchSummaryCollapsed: false,
    taskWorkbenchWidth: DEFAULT_TASK_WORKBENCH_WIDTH,
    timelineScrollByTaskId: {},
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
    setSidebarWidth: (sidebarWidth) => set({ sidebarWidth: clampSidebarWidth(sidebarWidth) }),
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
    openTaskWorkbench: (target) =>
      set((state) => ({
        taskWorkbenchByTaskId: {
          ...state.taskWorkbenchByTaskId,
          [target.taskId]: openTaskWorkbenchTarget(
            state.taskWorkbenchByTaskId[target.taskId],
            target,
          ),
        },
      })),
    closeTaskWorkbench: (taskId) =>
      set((state) => {
        const session = state.taskWorkbenchByTaskId[taskId]
        if (!session) return state
        return {
          taskWorkbenchByTaskId: {
            ...state.taskWorkbenchByTaskId,
            [taskId]: { ...session, open: false },
          },
        }
      }),
    activateTaskWorkbenchTab: (taskId, tabId) =>
      set((state) => {
        const session = state.taskWorkbenchByTaskId[taskId]
        if (!session) return state
        return {
          taskWorkbenchByTaskId: {
            ...state.taskWorkbenchByTaskId,
            [taskId]: activateTaskWorkbenchTab(session, tabId),
          },
        }
      }),
    closeTaskWorkbenchTab: (taskId, tabId) =>
      set((state) => {
        const session = state.taskWorkbenchByTaskId[taskId]
        if (!session) return state
        return {
          taskWorkbenchByTaskId: {
            ...state.taskWorkbenchByTaskId,
            [taskId]: closeTaskWorkbenchTab(session, tabId),
          },
        }
      }),
    setTaskWorkbenchTabPinned: (taskId, tabId, pinned) =>
      set((state) => {
        const session = state.taskWorkbenchByTaskId[taskId]
        if (!session) return state
        return {
          taskWorkbenchByTaskId: {
            ...state.taskWorkbenchByTaskId,
            [taskId]: setTaskWorkbenchTabPinned(session, tabId, pinned),
          },
        }
      }),
    setTaskWorkbenchViewportGeometry: (taskId, geometry) =>
      set((state) => {
        const session = state.taskWorkbenchByTaskId[taskId]
        if (!session) return state
        return {
          taskWorkbenchByTaskId: {
            ...state.taskWorkbenchByTaskId,
            [taskId]: setTaskWorkbenchViewportGeometry(session, geometry),
          },
        }
      }),
    setTaskWorkbenchViewportMode: (taskId, mode) =>
      set((state) => {
        const session = state.taskWorkbenchByTaskId[taskId]
        if (!session) return state
        return {
          taskWorkbenchByTaskId: {
            ...state.taskWorkbenchByTaskId,
            [taskId]: setTaskWorkbenchViewportMode(session, mode),
          },
        }
      }),
    setTaskWorkbenchSummaryCollapsed: (taskWorkbenchSummaryCollapsed) =>
      set({ taskWorkbenchSummaryCollapsed }),
    setTaskWorkbenchWidth: (taskWorkbenchWidth) =>
      set({ taskWorkbenchWidth: clampTaskWorkbenchWidth(taskWorkbenchWidth) }),
    requestTimelineScroll: (anchorId) =>
      set((state) => ({
        timelineScrollRequest: {
          anchorId,
          nonce: (state.timelineScrollRequest?.nonce ?? 0) + 1,
        },
      })),
    setTimelineScrollSession: (taskId, session) =>
      set((state) => ({
        timelineScrollByTaskId: {
          ...state.timelineScrollByTaskId,
          [taskId]: session,
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
