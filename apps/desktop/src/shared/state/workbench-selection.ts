import type { ConversationEventRef } from '@/shared/tauri/commands'

type EvidenceRefId = string

export type TaskWorkbenchTargetKind =
  | 'artifact'
  | 'audit'
  | 'command'
  | 'diff'
  | 'environment'
  | 'file'
  | 'source'
  | 'subagent'

export type TaskWorkbenchTarget = {
  blobId?: string
  kind: TaskWorkbenchTargetKind
  line?: number
  resourceId: string
  sourceEventId?: string
  taskId: string
  title: string
}

export type TaskWorkbenchTab = {
  id: string
  pinned: boolean
  target: TaskWorkbenchTarget
}

export type TaskWorkbenchSession = {
  activeTabId: string | null
  open: boolean
  previewTabId: string | null
  tabs: TaskWorkbenchTab[]
}

export const DEFAULT_TASK_WORKBENCH_WIDTH = 400
export const MIN_TASK_WORKBENCH_WIDTH = 360
export const MAX_TASK_WORKBENCH_WIDTH = 640

export function createTaskWorkbenchSession(): TaskWorkbenchSession {
  return {
    activeTabId: null,
    open: false,
    previewTabId: null,
    tabs: [],
  }
}

export function taskWorkbenchTargetKey(target: TaskWorkbenchTarget) {
  return `${target.kind}:${target.resourceId}`
}

export function openTaskWorkbenchTarget(
  current: TaskWorkbenchSession | undefined,
  target: TaskWorkbenchTarget,
): TaskWorkbenchSession {
  const session = current ?? createTaskWorkbenchSession()
  const id = taskWorkbenchTargetKey(target)
  const existing = session.tabs.find((tab) => tab.id === id)

  if (existing) {
    return {
      ...session,
      activeTabId: id,
      open: true,
      tabs: session.tabs.map((tab) => (tab.id === id ? { ...tab, target } : tab)),
    }
  }

  const nextTab: TaskWorkbenchTab = { id, pinned: false, target }
  if (session.previewTabId) {
    const previewIndex = session.tabs.findIndex((tab) => tab.id === session.previewTabId)
    if (previewIndex >= 0) {
      const tabs = [...session.tabs]
      tabs[previewIndex] = nextTab
      return {
        ...session,
        activeTabId: id,
        open: true,
        previewTabId: id,
        tabs,
      }
    }
  }

  return {
    ...session,
    activeTabId: id,
    open: true,
    previewTabId: id,
    tabs: [...session.tabs, nextTab],
  }
}

export function activateTaskWorkbenchTab(
  session: TaskWorkbenchSession,
  tabId: string,
): TaskWorkbenchSession {
  if (!session.tabs.some((tab) => tab.id === tabId)) return session
  return { ...session, activeTabId: tabId, open: true }
}

export function closeTaskWorkbenchTab(
  session: TaskWorkbenchSession,
  tabId: string,
): TaskWorkbenchSession {
  const index = session.tabs.findIndex((tab) => tab.id === tabId)
  if (index < 0) return session
  const tabs = session.tabs.filter((tab) => tab.id !== tabId)
  const activeTabId =
    session.activeTabId === tabId
      ? (tabs[Math.min(index, tabs.length - 1)]?.id ?? null)
      : session.activeTabId
  return {
    ...session,
    activeTabId,
    open: tabs.length > 0 && session.open,
    previewTabId: session.previewTabId === tabId ? null : session.previewTabId,
    tabs,
  }
}

export function setTaskWorkbenchTabPinned(
  session: TaskWorkbenchSession,
  tabId: string,
  pinned: boolean,
): TaskWorkbenchSession {
  const selected = session.tabs.find((tab) => tab.id === tabId)
  if (!selected || selected.pinned === pinned) return session

  let tabs = session.tabs.map((tab) => (tab.id === tabId ? { ...tab, pinned } : tab))
  let previewTabId = session.previewTabId
  if (pinned && previewTabId === tabId) previewTabId = null
  if (!pinned) {
    tabs = tabs.filter((tab) => tab.id === tabId || tab.id !== previewTabId)
    previewTabId = tabId
  }

  return { ...session, previewTabId, tabs }
}

export function clampTaskWorkbenchWidth(width: number) {
  return Math.min(MAX_TASK_WORKBENCH_WIDTH, Math.max(MIN_TASK_WORKBENCH_WIDTH, width))
}

export type WorkbenchSelection =
  | { kind: 'context' }
  | { kind: 'decision'; conversationId: string; requestId: string }
  | { kind: 'tool'; conversationId: string; toolUseId: string }
  | {
      kind: 'command'
      conversationId: string
      fullOutputRef?: EvidenceRefId
      eventRef?: ConversationEventRef
    }
  | { kind: 'diff'; conversationId: string; changeSetId: string }
  | {
      kind: 'artifact'
      conversationId: string
      artifactId: string
      revisionId?: string
    }
