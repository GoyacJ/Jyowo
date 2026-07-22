import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it, vi } from 'vitest'
import { createAppI18n } from '@/shared/i18n/i18n'
import { TaskWorkspaceView, taskWorkspaceLayoutModeForWidth } from './TaskWorkspace'
import type { TaskSnapshot } from './task-store'

const snapshot: TaskSnapshot = {
  projection: {
    archived: false,
    lastGlobalOffset: 0,
    queue: [],
    state: 'completed',
    streamVersion: 0,
    taskId: '01J00000000000000000000981',
    title: 'Repair scheduler recovery',
  },
  snapshotOffset: 0,
  timeline: [],
}

describe('TaskWorkspace task chrome', () => {
  it('uses fullscreen, overlay, and docked modes at the documented boundaries', () => {
    expect(taskWorkspaceLayoutModeForWidth(719)).toBe('fullscreen')
    expect(taskWorkspaceLayoutModeForWidth(720)).toBe('overlay')
    expect(taskWorkspaceLayoutModeForWidth(1039)).toBe('overlay')
    expect(taskWorkspaceLayoutModeForWidth(1040)).toBe('docked')
  })

  it('keeps the reading column without duplicating the task title or normal status', () => {
    render(
      <I18nextProvider i18n={createAppI18n('en-US')}>
        <TaskWorkspaceView connectionState="connected" snapshot={snapshot} />
      </I18nextProvider>,
    )

    expect(screen.getByTestId('task-reading-column')).toHaveClass('max-w-[820px]')
    expect(
      screen.queryByRole('heading', { name: 'Repair scheduler recovery' }),
    ).not.toBeInTheDocument()
    expect(screen.queryByText('Connected')).not.toBeInTheDocument()
    expect(screen.queryByText('Completed')).not.toBeInTheDocument()
  })

  it('does not render normal task and connection states in Chinese', () => {
    render(
      <I18nextProvider i18n={createAppI18n('zh-CN')}>
        <TaskWorkspaceView connectionState="connected" snapshot={snapshot} />
      </I18nextProvider>,
    )

    expect(screen.queryByText('已连接')).not.toBeInTheDocument()
    expect(screen.queryByText('已完成')).not.toBeInTheDocument()
    expect(screen.queryByText('Connected')).not.toBeInTheDocument()
  })

  it('renders an unavailable state without partial task content', () => {
    render(
      <I18nextProvider i18n={createAppI18n('en-US')}>
        <TaskWorkspaceView
          connectionError="Malformed daemon frame"
          connectionState="protocol_error"
          snapshot={null}
        />
      </I18nextProvider>,
    )

    expect(screen.getByRole('alert')).toHaveTextContent('Malformed daemon frame')
  })

  it('shows a recoverable error when projected permission details are unavailable', () => {
    const permissionSnapshot: TaskSnapshot = {
      ...snapshot,
      projection: {
        ...snapshot.projection,
        currentRun: {
          incompleteOutput: false,
          segmentId: '01J00000000000000000000982',
          startedAt: '2026-07-18T00:00:00Z',
          state: 'running',
        },
        pendingPermission: {
          requestId: '01J00000000000000000000983',
          revision: 4,
          route: 'foreground_task',
        },
        state: 'waiting_permission',
      },
    }

    render(
      <I18nextProvider i18n={createAppI18n('zh-CN')}>
        <TaskWorkspaceView
          client={{ connect: vi.fn(), request: vi.fn() }}
          connectionState="connected"
          snapshot={permissionSnapshot}
        />
      </I18nextProvider>,
    )

    expect(screen.getByRole('alert')).toHaveTextContent('权限详情不可用')
    expect(screen.getByRole('alert')).toHaveTextContent('请重启或恢复任务')
    expect(screen.queryByRole('button', { name: 'Allow once' })).not.toBeInTheDocument()
  })
})
