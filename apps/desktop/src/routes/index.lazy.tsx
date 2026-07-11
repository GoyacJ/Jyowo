import { createLazyFileRoute } from '@tanstack/react-router'

import { TaskWorkspace } from '@/features/tasks/TaskWorkspace'

export const Route = createLazyFileRoute('/')({
  component: TaskRoute,
})

function TaskRoute() {
  const { taskId } = Route.useSearch()

  if (!taskId) {
    return (
      <section className="mx-auto flex h-full max-w-[820px] flex-col items-center justify-center text-center">
        <h1 className="font-semibold text-2xl">Choose a task</h1>
        <p className="mt-2 text-muted-foreground text-sm">
          Select or create a task from the sidebar.
        </p>
      </section>
    )
  }

  return <TaskWorkspace taskId={taskId} />
}
