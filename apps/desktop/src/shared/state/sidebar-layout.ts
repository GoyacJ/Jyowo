export const COLLAPSED_SIDEBAR_WIDTH = 48
export const DEFAULT_SIDEBAR_WIDTH = 300
export const MAX_SIDEBAR_WIDTH = 420
export const MIN_SIDEBAR_WIDTH = 240

export function clampSidebarWidth(width: number) {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, width))
}
