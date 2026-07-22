import { Archive, Pause, Play, Send, Trash2, X } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { TaskProjection, TaskState } from '@/generated/daemon-protocol'
import { cn } from '@/shared/lib/utils'
import { Button } from '@/shared/ui/button'
import { Card } from '@/shared/ui/card'
import { EmptyState } from '@/shared/ui/empty-state'
import { Input } from '@/shared/ui/input'
import { Section, SectionTitle } from '@/shared/ui/section'

import { useBackgroundAgents } from './use-background-agents'

const stateLabels: Record<TaskState, string> = {
  completed: 'state.succeeded',
  failed: 'state.failed',
  idle: 'state.queued',
  interrupted: 'state.interrupted',
  running: 'state.running',
  waiting_permission: 'state.waitingForPermission',
  waiting_input: 'state.waitingForInput',
  yielding: 'state.running',
}

function canArchive(task: TaskProjection) {
  return !task.archived && ['completed', 'failed', 'interrupted'].includes(task.state)
}

export function BackgroundAgentsPanel({
  selectedBackgroundAgentId,
}: {
  selectedBackgroundAgentId?: string
}) {
  const { t } = useTranslation('backgroundAgents')
  const {
    archiveMutation,
    cancelMutation,
    deleteMutation,
    listQuery,
    pauseMutation,
    resumeMutation,
    sendInputMutation,
  } = useBackgroundAgents()
  const [inputDrafts, setInputDrafts] = useState<Record<string, string>>({})
  const agents = selectedBackgroundAgentId
    ? [...(listQuery.data ?? [])].sort((left, right) => {
        if (left.taskId === selectedBackgroundAgentId) return -1
        if (right.taskId === selectedBackgroundAgentId) return 1
        return 0
      })
    : (listQuery.data ?? [])

  function submitInput(task: TaskProjection) {
    const input = inputDrafts[task.taskId]?.trim()
    if (input) sendInputMutation.mutate({ input, task })
  }

  return (
    <Section className="mx-auto w-full max-w-5xl">
      <SectionTitle>{t('title')}</SectionTitle>
      {listQuery.isLoading ? <p className="text-muted-foreground text-sm">{t('loading')}</p> : null}
      {listQuery.isError ? (
        <p className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {t('loadError')}
        </p>
      ) : null}
      {!listQuery.isLoading && !listQuery.isError && agents.length === 0 ? (
        <EmptyState>{t('empty')}</EmptyState>
      ) : null}
      {agents.length > 0 ? (
        <div className="grid gap-3">
          {agents.map((task) => (
            <Card
              aria-label={task.title}
              className={cn(
                'bg-background p-4',
                task.taskId === selectedBackgroundAgentId && 'border-primary',
              )}
              key={task.taskId}
              role="article"
            >
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0 space-y-1">
                  <h2 className="break-words font-medium text-base">{task.title}</h2>
                  <div className="flex flex-wrap gap-x-3 gap-y-1 text-muted-foreground text-xs">
                    <span>{task.archived ? t('state.archived') : t(stateLabels[task.state])}</span>
                    <span>{task.parent?.parentTaskId}</span>
                    <span>{task.parent?.parentSegmentId}</span>
                    {task.taskId === selectedBackgroundAgentId ? (
                      <span>{t('selected')}</span>
                    ) : null}
                  </div>
                </div>
                <div className="flex flex-wrap justify-end gap-2">
                  {['running', 'yielding', 'waiting_permission', 'waiting_input'].includes(
                    task.state,
                  ) ? (
                    <>
                      <Button
                        disabled={pauseMutation.isPending}
                        onClick={() => pauseMutation.mutate(task)}
                        size="sm"
                        type="button"
                        variant="outline"
                      >
                        <Pause data-icon className="size-4" />
                        {t('actions.pause')}
                      </Button>
                      <Button
                        disabled={cancelMutation.isPending}
                        onClick={() => cancelMutation.mutate(task)}
                        size="sm"
                        type="button"
                        variant="outline"
                      >
                        <X data-icon className="size-4" />
                        {t('actions.cancel')}
                      </Button>
                    </>
                  ) : null}
                  {task.state === 'interrupted' && !task.archived ? (
                    <Button
                      disabled={resumeMutation.isPending}
                      onClick={() => resumeMutation.mutate(task)}
                      size="sm"
                      type="button"
                      variant="outline"
                    >
                      <Play data-icon className="size-4" />
                      {t('actions.resume')}
                    </Button>
                  ) : null}
                  {canArchive(task) ? (
                    <Button
                      disabled={archiveMutation.isPending}
                      onClick={() => archiveMutation.mutate(task)}
                      size="sm"
                      type="button"
                      variant="outline"
                    >
                      <Archive data-icon className="size-4" />
                      {t('actions.archive')}
                    </Button>
                  ) : null}
                  {task.archived ? (
                    <Button
                      disabled={deleteMutation.isPending}
                      onClick={() => deleteMutation.mutate(task)}
                      size="sm"
                      type="button"
                      variant="outline"
                    >
                      <Trash2 data-icon className="size-4" />
                      {t('actions.delete')}
                    </Button>
                  ) : null}
                </div>
              </div>
              {task.state === 'interrupted' && !task.archived ? (
                <div className="mt-3 flex flex-wrap items-end gap-2">
                  <label
                    className="flex min-w-64 flex-1 flex-col gap-1 text-sm"
                    htmlFor={`background-agent-input-${task.taskId}`}
                  >
                    <span className="font-medium">{t('input.label')}</span>
                    <Input
                      id={`background-agent-input-${task.taskId}`}
                      onChange={(event) =>
                        setInputDrafts((current) => ({
                          ...current,
                          [task.taskId]: event.target.value,
                        }))
                      }
                      value={inputDrafts[task.taskId] ?? ''}
                    />
                  </label>
                  <Button
                    disabled={sendInputMutation.isPending}
                    onClick={() => submitInput(task)}
                    size="sm"
                    type="button"
                  >
                    <Send data-icon className="size-4" />
                    {t('input.send')}
                  </Button>
                </div>
              ) : null}
            </Card>
          ))}
        </div>
      ) : null}
    </Section>
  )
}
