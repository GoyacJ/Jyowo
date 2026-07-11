import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/')({
  validateSearch: (search): TaskSearch => ({
    taskId:
      typeof search.taskId === 'string' && search.taskId.trim().length > 0
        ? search.taskId
        : undefined,
  }),
})

type TaskSearch = {
  taskId?: string
}
