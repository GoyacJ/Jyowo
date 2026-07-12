import { createLazyFileRoute } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

import { TaskWorkspace } from '@/features/tasks/TaskWorkspace'

export const Route = createLazyFileRoute('/')({
  component: TaskRoute,
})

function TaskRoute() {
  const { t } = useTranslation('shell')
  const { taskId } = Route.useSearch()

  if (!taskId) {
    return (
      <section className="mx-auto flex h-full max-w-[820px] flex-col items-center justify-center text-center">
        <h1 className="font-semibold text-2xl">{t('sidebar.emptyTitle')}</h1>
        <p className="mt-2 text-muted-foreground text-sm">{t('sidebar.emptyDescription')}</p>
      </section>
    )
  }

  return <TaskWorkspace taskId={taskId} />
}
