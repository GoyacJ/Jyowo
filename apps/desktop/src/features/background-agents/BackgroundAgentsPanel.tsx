import { Archive, Pause, Play, Send, Trash2, X } from 'lucide-react'
import { useState } from 'react'

import { cn } from '@/shared/lib/utils'
import type { BackgroundAgentIdRequest, BackgroundAgentRecord } from '@/shared/tauri/commands'
import { Button } from '@/shared/ui/button'

import { useBackgroundAgents } from './use-background-agents'

const stateLabels: Record<BackgroundAgentRecord['state'], string> = {
  archived: '已归档',
  cancelled: '已取消',
  cancelling: '取消中',
  failed: '失败',
  interrupted: '已中断',
  paused: '已暂停',
  queued: '排队中',
  recoverable: '可恢复',
  running: '运行中',
  succeeded: '已完成',
  waiting_for_input: '等待输入',
  waiting_for_permission: '等待权限',
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
    <section className="mx-auto flex w-full max-w-5xl flex-col gap-4">
      <h1 className="font-semibold text-2xl">后台 Agent</h1>

      {listQuery.isLoading ? (
        <p className="text-muted-foreground text-sm">正在加载后台 Agent。</p>
      ) : null}

      {listQuery.isError ? (
        <p className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          后台 Agent 无法加载。
        </p>
      ) : null}

      {!listQuery.isLoading && !listQuery.isError && agents.length === 0 ? (
        <p className="rounded-md border border-dashed border-border bg-surface px-4 py-6 text-center text-muted-foreground text-sm">
          暂无后台 Agent。
        </p>
      ) : null}

      {agents.length > 0 ? (
        <div className="grid gap-3">
          {agents.map((agent) => (
            <article
              aria-label={agent.title}
              className={cn(
                'rounded-md border border-border bg-background p-4',
                agent.backgroundAgentId === selectedBackgroundAgentId && 'border-primary',
              )}
              key={agent.backgroundAgentId}
            >
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0 space-y-1">
                  <h2 className="break-words font-medium text-base">{agent.title}</h2>
                  <div className="flex flex-wrap gap-x-3 gap-y-1 text-muted-foreground text-xs">
                    <span>{stateLabels[agent.state]}</span>
                    <span>{agent.conversationId}</span>
                    {agent.parentRunId ? <span>{agent.parentRunId}</span> : null}
                    {agent.backgroundAgentId === selectedBackgroundAgentId ? (
                      <span>已打开</span>
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
                        暂停
                      </Button>
                      <Button
                        disabled={cancelMutation.isPending}
                        onClick={() => cancelMutation.mutate(agentRequest(agent))}
                        size="sm"
                        type="button"
                        variant="outline"
                      >
                        <X data-icon className="size-4" />
                        取消
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
                      恢复
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
                      归档
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
                      删除
                    </Button>
                  ) : null}
                </div>
              </div>

              {agent.state === 'waiting_for_input' ? (
                <div className="mt-3 flex flex-wrap items-end gap-2">
                  <label className="flex min-w-64 flex-1 flex-col gap-1 text-sm">
                    <span className="font-medium">输入</span>
                    <input
                      className="h-9 rounded-md border border-border bg-background px-3 outline-none focus-visible:ring-2 focus-visible:ring-ring"
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
                    发送输入
                  </Button>
                  {!agent.pendingInputRequestId ? (
                    <p className="basis-full text-muted-foreground text-xs">
                      等待输入请求尚未恢复。
                    </p>
                  ) : null}
                </div>
              ) : null}
            </article>
          ))}
        </div>
      ) : null}
    </section>
  )
}
