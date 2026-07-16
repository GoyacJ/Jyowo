import { createLazyFileRoute } from '@tanstack/react-router'

import { ScheduledTasksPage } from '@/features/scheduled-tasks/ScheduledTasksPage'

export const Route = createLazyFileRoute('/scheduled-tasks')({
  component: ScheduledTasksPage,
})
