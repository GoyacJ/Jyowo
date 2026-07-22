import { describe, expect, it } from 'vitest'

import {
  clampTaskWorkbenchViewportGeometry,
  closeTaskWorkbenchTab,
  createTaskWorkbenchSession,
  defaultTaskWorkbenchViewportGeometry,
  openTaskWorkbenchTarget,
  resizeTaskWorkbenchViewportGeometry,
  setTaskWorkbenchTabPinned,
  setTaskWorkbenchViewportMode,
  type TaskWorkbenchTarget,
} from './workbench-selection'

describe('task workbench session', () => {
  it('reuses one preview tab while browsing objects', () => {
    const first = openTaskWorkbenchTarget(undefined, target('diff', 'diff-1'))
    const second = openTaskWorkbenchTarget(first, target('command', 'command-1'))

    expect(second.tabs).toHaveLength(1)
    expect(second.tabs[0]?.target.kind).toBe('command')
    expect(second.previewTabId).toBe('command:command-1')
    expect(second.activeTabId).toBe('command:command-1')
  })

  it('keeps pinned tabs and focuses an already open object', () => {
    const preview = openTaskWorkbenchTarget(undefined, target('diff', 'diff-1'))
    const pinned = setTaskWorkbenchTabPinned(preview, 'diff:diff-1', true)
    const withCommand = openTaskWorkbenchTarget(pinned, target('command', 'command-1'))
    const reopened = openTaskWorkbenchTarget(withCommand, target('diff', 'diff-1', 'Updated diff'))

    expect(reopened.tabs).toHaveLength(2)
    expect(reopened.tabs[0]).toMatchObject({ id: 'diff:diff-1', pinned: true })
    expect(reopened.tabs[0]?.target.title).toBe('Updated diff')
    expect(reopened.activeTabId).toBe('diff:diff-1')
  })

  it('selects the adjacent tab and closes the session with its final tab', () => {
    const first = openTaskWorkbenchTarget(undefined, target('diff', 'diff-1'))
    const pinned = setTaskWorkbenchTabPinned(first, 'diff:diff-1', true)
    const second = openTaskWorkbenchTarget(pinned, target('command', 'command-1'))
    const afterCommand = closeTaskWorkbenchTab(second, 'command:command-1')
    const empty = closeTaskWorkbenchTab(afterCommand, 'diff:diff-1')

    expect(afterCommand.activeTabId).toBe('diff:diff-1')
    expect(afterCommand.open).toBe(true)
    expect(empty).toEqual({
      ...createTaskWorkbenchSession(),
      tabs: [],
    })
  })

  it('uses an unpinned tab as the only replaceable preview', () => {
    const first = openTaskWorkbenchTarget(undefined, target('diff', 'diff-1'))
    const pinnedFirst = setTaskWorkbenchTabPinned(first, 'diff:diff-1', true)
    const second = openTaskWorkbenchTarget(pinnedFirst, target('command', 'command-1'))
    const unpinnedFirst = setTaskWorkbenchTabPinned(second, 'diff:diff-1', false)

    expect(unpinnedFirst.previewTabId).toBe('diff:diff-1')
    expect(unpinnedFirst.tabs).toEqual([
      expect.objectContaining({ id: 'diff:diff-1', pinned: false }),
    ])
  })

  it('restores the previous viewport mode after expanded fullscreen', () => {
    const floating = createTaskWorkbenchSession()
    const fullscreen = setTaskWorkbenchViewportMode(floating, 'fullscreen')
    const restored = setTaskWorkbenchViewportMode(fullscreen, fullscreen.viewportRestoreMode)

    expect(fullscreen).toMatchObject({
      viewportMode: 'fullscreen',
      viewportRestoreMode: 'floating',
    })
    expect(restored.viewportMode).toBe('floating')
  })

  it('places, clamps, and resizes the floating viewport inside the task workspace', () => {
    const bounds = { height: 700, width: 1200 }
    expect(defaultTaskWorkbenchViewportGeometry(bounds)).toEqual({
      height: 400,
      width: 560,
      x: 624,
      y: 16,
    })
    expect(
      clampTaskWorkbenchViewportGeometry({ height: 600, width: 900, x: 800, y: 500 }, bounds),
    ).toEqual({ height: 600, width: 900, x: 300, y: 100 })
    expect(
      resizeTaskWorkbenchViewportGeometry(
        { height: 400, width: 560, x: 624, y: 16 },
        'nw',
        { x: -100, y: -100 },
        bounds,
      ),
    ).toEqual({ height: 416, width: 660, x: 524, y: 0 })
  })
})

function target(
  kind: TaskWorkbenchTarget['kind'],
  resourceId: string,
  title = resourceId,
): TaskWorkbenchTarget {
  return { kind, resourceId, taskId: 'task-1', title }
}
