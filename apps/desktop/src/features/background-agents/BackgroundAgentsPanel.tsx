import { Archive, Pause, Play, Send, Trash2, X } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/shared/lib/utils'
import type { BackgroundAgentIdRequest, BackgroundAgentRecord } from '@/shared/tauri/commands'
import { Button } from '@/shared/ui/button'
import { Card } from '@/shared/ui/card'
import { EmptyState } from '@/shared/ui/empty-state'
import { Input } from '@/shared/ui/input'
import { Section, SectionTitle } from '@/shared/ui/section'

import { useBackgroundAgents } from './use-background-agents'

const stateLabels: Record<BackgroundAgentRecord['state'], string> = {
  archived: 'state.archived',
  cancelled: 'state.cancelled',
  cancelling: 'state.cancelling',
  failed: 'state.failed',
  interrupted: 'state.interrupted',
  paused: 'state.paused',
  queued: 'state.queued',
  recoverable: 'state.recoverable',
  running: 'state.running',
  succeeded: 'state.succeeded',
  waiting_for_input: 'state.waitingForInput',
  waiting_for_permission: 'state.waitingForPermission',
}

function agentRequest(agent: BackgroundAgentRecord): BackgroundAgentIdRequest {
  return {
    backgroundAgentId: agent.backgroundAgentId,
    conversationId: agent.conversationId,
  }
}

function canArchive(state: BackgroundAgentRecord['state']) {
  return ['cancelled', 'failed', 'interrupted', 'recoverable', 'succeeded'].includes(state)
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
    ? [...(listQuery.data?.agents ?? [])].sort((left, right) => {
        if (left.backgroundAgentId === selectedBackgroundAgentId) {
          return -1
        }
        if (right.backgroundAgentId === selectedBackgroundAgentId) {
          return 1
        }
        return 0
      })
    : (listQuery.data?.agents ?? [])

  function submitInput(agent: BackgroundAgentRecord) {
    const input = inputDrafts[agent.backgroundAgentId]?.trim()

    if (!input) {
      return
    }

    if (!agent.pendingInputRequestId) {
      return
    }

    sendInputMutation.mutate({
      ...agentRequest(agent),
      input,
      requestId: agent.pendingInputRequestId,
    })
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
          {agents.map((agent) => (
            <Card
              aria-label={agent.title}
              className={cn(
                'bg-background p-4',
                agent.backgroundAgentId === selectedBackgroundAgentId && 'border-primary',
              )}
              key={agent.backgroundAgentId}
              role="article"
            >
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0 space-y-1">
                  <h2 className="break-words font-medium text-base">{agent.title}</h2>
                  <div className="flex flex-wrap gap-x-3 gap-y-1 text-muted-foreground text-xs">
                    <span>{t(stateLabels[agent.state])}</span>
                    <span>{agent.conversationId}</span>
                    {agent.parentRunId ? <span>{agent.parentRunId}</span> : null}
                    {agent.backgroundAgentId === selectedBackgroundAgentId ? (
                      <span>{t('selected')}</span>
                    ) : null}
                  </div>
                </div>

                <div className="flex flex-wrap justify-end gap-2">
                  {agent.state === 'running' || agent.state === 'queued' ? (
                    <>
                      <Button
                        disabled={pauseMutation.isPending}
                        onClick={() => pauseMutation.mutate(agentRequest(agent))}
                        size="sm"
                        type="button"
                        variant="outline"
                      >
                        <Pause data-icon className="size-4" />
                        {t('actions.pause')}
                      </Button>
                      <Button
                        disabled={cancelMutation.isPending}
                        onClick={() => cancelMutation.mutate(agentRequest(agent))}
                        size="sm"
                        type="button"
                        variant="outline"
                      >
                        <X data-icon className="size-4" />
                        {t('actions.cancel')}
                      </Button>
                    </>
                  ) : null}

                  {['interrupted', 'paused', 'recoverable'].includes(agent.state) ? (
                    <Button
                      disabled={resumeMutation.isPending}
                      onClick={() => resumeMutation.mutate(agentRequest(agent))}
                      size="sm"
                      type="button"
                      variant="outline"
                    >
                      <Play data-icon className="size-4" />
                      {t('actions.resume')}
                    </Button>
                  ) : null}

                  {canArchive(agent.state) ? (
                    <Button
                      disabled={archiveMutation.isPending}
                      onClick={() => archiveMutation.mutate(agentRequest(agent))}
                      size="sm"
                      type="button"
                      variant="outline"
                    >
                      <Archive data-icon className="size-4" />
                      {t('actions.archive')}
                    </Button>
                  ) : null}

                  {agent.state === 'archived' ? (
                    <Button
                      disabled={deleteMutation.isPending}
                      onClick={() => deleteMutation.mutate(agentRequest(agent))}
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

              {agent.state === 'waiting_for_input' ? (
                <div className="mt-3 flex flex-wrap items-end gap-2">
                  <label
                    className="flex min-w-64 flex-1 flex-col gap-1 text-sm"
                    htmlFor={`background-agent-input-${agent.backgroundAgentId}`}
                  >
                    <span className="font-medium">{t('input.label')}</span>
                    <Input
                      id={`background-agent-input-${agent.backgroundAgentId}`}
                      onChange={(event) =>
                        setInputDrafts((current) => ({
                          ...current,
                          [agent.backgroundAgentId]: event.target.value,
                        }))
                      }
                      value={inputDrafts[agent.backgroundAgentId] ?? ''}
                    />
                  </label>
                  <Button
                    disabled={sendInputMutation.isPending || !agent.pendingInputRequestId}
                    onClick={() => submitInput(agent)}
                    size="sm"
                    type="button"
                  >
                    <Send data-icon className="size-4" />
                    {t('input.send')}
                  </Button>
                  {!agent.pendingInputRequestId ? (
                    <p className="basis-full text-muted-foreground text-xs">
                      {t('input.pendingRequestMissing')}
                    </p>
                  ) : null}
                </div>
              ) : null}
            </Card>
          ))}
        </div>
      ) : null}
    </Section>
  )
}
