type EvidenceRefId = string

type ConversationEventRef = {
  cursor: {
    conversationSequence: number
    eventId: string
  }
  eventId: string
}

export type TaskWorkbenchTargetKind =
  | 'artifact'
  | 'audit'
  | 'browser'
  | 'command'
  | 'diff'
  | 'environment'
  | 'file'
  | 'source'
  | 'subagent'

export type TaskWorkbenchTarget = {
  artifact?: {
    artifactId?: string
    artifactKind?: string
    format?: string
    mediaType: string
    preferredSurface?: 'inline' | 'card' | 'workbench'
    preview?: string
    previewBlobId?: string
    size?: number
  }
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

export type TaskWorkbenchViewportMode = 'docked' | 'floating' | 'fullscreen'

export type TaskWorkbenchViewportGeometry = {
  height: number
  width: number
  x: number
  y: number
}

export type TaskWorkbenchResizeEdge = 'e' | 'n' | 'ne' | 'nw' | 's' | 'se' | 'sw' | 'w'

export type TaskWorkbenchSession = {
  activeTabId: string | null
  open: boolean
  previewTabId: string | null
  tabs: TaskWorkbenchTab[]
  viewportGeometry: TaskWorkbenchViewportGeometry | null
  viewportMode: TaskWorkbenchViewportMode
  viewportRestoreMode: Exclude<TaskWorkbenchViewportMode, 'fullscreen'>
}

export const DEFAULT_TASK_WORKBENCH_WIDTH = 400
const MIN_TASK_WORKBENCH_WIDTH = 360
const MAX_TASK_WORKBENCH_WIDTH = 640
export const DEFAULT_TASK_WORKBENCH_VIEWPORT_WIDTH = 560
export const DEFAULT_TASK_WORKBENCH_VIEWPORT_HEIGHT = 400
export const MIN_TASK_WORKBENCH_VIEWPORT_WIDTH = 360
export const MIN_TASK_WORKBENCH_VIEWPORT_HEIGHT = 240
const TASK_WORKBENCH_VIEWPORT_MARGIN = 16

export function createTaskWorkbenchSession(): TaskWorkbenchSession {
  return {
    activeTabId: null,
    open: false,
    previewTabId: null,
    tabs: [],
    viewportGeometry: null,
    viewportMode: 'floating',
    viewportRestoreMode: 'floating',
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

export function setTaskWorkbenchViewportGeometry(
  session: TaskWorkbenchSession,
  viewportGeometry: TaskWorkbenchViewportGeometry,
): TaskWorkbenchSession {
  return { ...session, viewportGeometry }
}

export function setTaskWorkbenchViewportMode(
  session: TaskWorkbenchSession,
  viewportMode: TaskWorkbenchViewportMode,
): TaskWorkbenchSession {
  if (viewportMode === session.viewportMode) return session
  if (viewportMode === 'fullscreen') {
    return {
      ...session,
      viewportMode,
      viewportRestoreMode:
        session.viewportMode === 'fullscreen' ? session.viewportRestoreMode : session.viewportMode,
    }
  }
  return { ...session, viewportMode, viewportRestoreMode: viewportMode }
}

export function defaultTaskWorkbenchViewportGeometry(bounds: {
  height: number
  width: number
}): TaskWorkbenchViewportGeometry {
  const width = Math.min(
    DEFAULT_TASK_WORKBENCH_VIEWPORT_WIDTH,
    Math.max(0, bounds.width - TASK_WORKBENCH_VIEWPORT_MARGIN * 2),
  )
  const height = Math.min(
    DEFAULT_TASK_WORKBENCH_VIEWPORT_HEIGHT,
    Math.max(0, bounds.height - TASK_WORKBENCH_VIEWPORT_MARGIN * 2),
  )
  return clampTaskWorkbenchViewportGeometry(
    {
      height,
      width,
      x: bounds.width - width - TASK_WORKBENCH_VIEWPORT_MARGIN,
      y: TASK_WORKBENCH_VIEWPORT_MARGIN,
    },
    bounds,
  )
}

export function clampTaskWorkbenchViewportGeometry(
  geometry: TaskWorkbenchViewportGeometry,
  bounds: { height: number; width: number },
): TaskWorkbenchViewportGeometry {
  const width = Math.min(
    Math.max(0, bounds.width),
    Math.max(MIN_TASK_WORKBENCH_VIEWPORT_WIDTH, geometry.width),
  )
  const height = Math.min(
    Math.max(0, bounds.height),
    Math.max(MIN_TASK_WORKBENCH_VIEWPORT_HEIGHT, geometry.height),
  )
  return {
    height,
    width,
    x: Math.min(Math.max(0, geometry.x), Math.max(0, bounds.width - width)),
    y: Math.min(Math.max(0, geometry.y), Math.max(0, bounds.height - height)),
  }
}

export function resizeTaskWorkbenchViewportGeometry(
  geometry: TaskWorkbenchViewportGeometry,
  edge: TaskWorkbenchResizeEdge,
  delta: { x: number; y: number },
  bounds: { height: number; width: number },
): TaskWorkbenchViewportGeometry {
  const start = clampTaskWorkbenchViewportGeometry(geometry, bounds)
  const minWidth = Math.min(MIN_TASK_WORKBENCH_VIEWPORT_WIDTH, bounds.width)
  const minHeight = Math.min(MIN_TASK_WORKBENCH_VIEWPORT_HEIGHT, bounds.height)
  let left = start.x
  let right = start.x + start.width
  let top = start.y
  let bottom = start.y + start.height

  if (edge.includes('w')) left = Math.min(Math.max(0, left + delta.x), right - minWidth)
  if (edge.includes('e')) right = Math.max(Math.min(bounds.width, right + delta.x), left + minWidth)
  if (edge.includes('n')) top = Math.min(Math.max(0, top + delta.y), bottom - minHeight)
  if (edge.includes('s')) {
    bottom = Math.max(Math.min(bounds.height, bottom + delta.y), top + minHeight)
  }

  return {
    height: bottom - top,
    width: right - left,
    x: left,
    y: top,
  }
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
