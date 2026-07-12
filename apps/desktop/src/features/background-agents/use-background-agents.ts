import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import type { TaskProjection } from '@/generated/daemon-protocol'
import { createTaskCommandMetadata, requireAcceptedCommand } from '@/shared/daemon/task-command'
import { useDaemonClient } from '@/shared/tauri/react'

const backgroundAgentQueryKeys = {
  all: ['background-agents'] as const,
  list: () => [...backgroundAgentQueryKeys.all, 'list'] as const,
}

export function useBackgroundAgents() {
  const daemonClient = useDaemonClient()
  const queryClient = useQueryClient()
  const listQuery = useQuery({
    queryFn: async () => {
      const response = await daemonClient.listTasks()
      return response.tasks.filter((task) => task.parent?.attachment === 'detached')
    },
    queryKey: backgroundAgentQueryKeys.list(),
  })

  async function invalidateAgents() {
    await queryClient.invalidateQueries({ queryKey: backgroundAgentQueryKeys.all })
  }

  const pauseMutation = useMutation({
    mutationFn: (task: TaskProjection) => stopTask(task, 'safe_point'),
    onSuccess: invalidateAgents,
  })
  const resumeMutation = useMutation({
    mutationFn: async (task: TaskProjection) => {
      const frame = await daemonClient.request({
        indeterminateTools: [],
        metadata: createTaskCommandMetadata(task.taskId, task.streamVersion, 'continue'),
        taskId: task.taskId,
        type: 'continue_task',
      })
      return requireAcceptedCommand(frame, task.taskId)
    },
    onSuccess: invalidateAgents,
  })
  const cancelMutation = useMutation({
    mutationFn: (task: TaskProjection) => stopTask(task, 'force'),
    onSuccess: invalidateAgents,
  })
  const sendInputMutation = useMutation({
    mutationFn: async ({ input, task }: { input: string; task: TaskProjection }) => {
      const frame = await daemonClient.request({
        attachments: [],
        content: input,
        contextReferences: [],
        metadata: createTaskCommandMetadata(task.taskId, task.streamVersion, 'background-input'),
        permissionMode: 'default',
        taskId: task.taskId,
        type: 'submit_message',
      })
      return requireAcceptedCommand(frame, task.taskId)
    },
    onSuccess: invalidateAgents,
  })
  const archiveMutation = useMutation({
    mutationFn: (task: TaskProjection) =>
      daemonClient.setTaskArchived(task.taskId, task.streamVersion, true),
    onSuccess: invalidateAgents,
  })
  const deleteMutation = useMutation({
    mutationFn: (task: TaskProjection) => daemonClient.removeTask(task.taskId, task.streamVersion),
    onSuccess: invalidateAgents,
  })

  async function stopTask(task: TaskProjection, mode: 'force' | 'safe_point') {
    const frame = await daemonClient.request({
      metadata: createTaskCommandMetadata(task.taskId, task.streamVersion, `stop-${mode}`),
      mode,
      taskId: task.taskId,
      type: 'stop_run',
    })
    return requireAcceptedCommand(frame, task.taskId)
  }

  return {
    archiveMutation,
    cancelMutation,
    deleteMutation,
    listQuery,
    pauseMutation,
    resumeMutation,
    sendInputMutation,
  }
}
