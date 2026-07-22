import type { Meta, StoryObj } from '@storybook/react-vite'
import { createMemoryHistory, createRouter, RouterProvider } from '@tanstack/react-router'
import { useState } from 'react'

import { taskStoreFor } from '@/features/tasks/use-task'
import type { ClientRequest } from '@/generated/daemon-protocol'
import { routeTree } from '@/routeTree.gen'
import type { DaemonClient } from '@/shared/daemon/client'
import { DaemonClientProvider } from '@/shared/tauri/react'

const selectedTaskId = '01J00000000000000000000991'
const selectedRunSegmentId = '01J00000000000000000000992'
const selectedTaskSnapshot = {
  projection: {
    archived: false,
    currentRun: {
      incompleteOutput: false,
      segmentId: selectedRunSegmentId,
      startedAt: '2026-07-18T00:00:00Z',
      state: 'running' as const,
    },
    lastGlobalOffset: 1,
    queue: [],
    state: 'running' as const,
    streamVersion: 1,
    taskId: selectedTaskId,
    title: '优化右侧任务区域',
  },
  snapshotOffset: 1,
  timeline: [
    {
      globalOffset: 1,
      id: 'event-1',
      incomplete: false,
      kind: 'user_message' as const,
      runSegmentId: selectedRunSegmentId,
      summary: '修复右侧任务区域的反馈和控制。',
    },
  ],
}
const storyDaemonClient = {
  connect: async () => ({}) as never,
  listTasks: async () => ({ tasks: [selectedTaskSnapshot.projection], type: 'task_list' as const }),
  loadTask: async () => selectedTaskSnapshot,
  loadTaskEvents: async () => ({ events: [], nextBeforeOffset: null, taskId: selectedTaskId }),
  request: async (request: ClientRequest) => ({
    message: {
      commandId: 'metadata' in request ? request.metadata.commandId : selectedTaskId,
      committedOffset: 2,
      streamVersion: 2,
      taskId: selectedTaskId,
      type: 'command_accepted' as const,
    },
    protocolVersion: 7,
  }),
  subscribe: async () => async () => undefined,
} as unknown as DaemonClient

function createStoryRouter(initialEntry = '/') {
  return createRouter({
    routeTree,
    defaultPreload: 'intent',
    history: createMemoryHistory({ initialEntries: [initialEntry] }),
    scrollRestoration: true,
  })
}

function ConversationWorkspaceShellStory() {
  const [router] = useState(createStoryRouter)

  return (
    <DaemonClientProvider client={storyDaemonClient}>
      <RouterProvider router={router} />
    </DaemonClientProvider>
  )
}

function SelectedConversationWorkspaceShellStory() {
  const [router] = useState(() => {
    taskStoreFor(selectedTaskId).getState().replaceSnapshot(selectedTaskSnapshot)

    return createStoryRouter(`/?taskId=${selectedTaskId}`)
  })

  return (
    <DaemonClientProvider client={storyDaemonClient}>
      <RouterProvider router={router} />
    </DaemonClientProvider>
  )
}

const meta = {
  title: 'App/Shell',
  component: ConversationWorkspaceShellStory,
  parameters: {
    layout: 'fullscreen',
  },
} satisfies Meta<typeof ConversationWorkspaceShellStory>

export default meta

type Story = StoryObj<typeof meta>

export const ConversationWorkspace: Story = {}

export const SelectedConversation: Story = {
  render: () => <SelectedConversationWorkspaceShellStory />,
}
