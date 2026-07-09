import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { BackgroundAgentsPanel } from '@/features/background-agents/BackgroundAgentsPanel'
import { AppI18nProvider } from '@/shared/i18n/i18n'
import type { BackgroundAgentRecord, CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import {
  agentOrchestrationBackgroundAgentsResponse,
  createRejectedTestCommandClient,
  createTestCommandClient,
} from '@/testing/command-client'

const agentStates = [
  'running',
  'waiting_for_input',
  'interrupted',
  'succeeded',
  'archived',
] satisfies BackgroundAgentRecord['state'][]

function backgroundAgent(
  state: BackgroundAgentRecord['state'],
  index: number,
): BackgroundAgentRecord {
  return {
    backgroundAgentId: `bg-agent-${index}`,
    conversationId: `conversation-${index}`,
    createdAt: '2026-06-30T00:00:00.000Z',
    parentRunId: `run-${index}`,
    ...(state === 'waiting_for_input' ? { pendingInputRequestId: `input-request-${index}` } : {}),
    state,
    title: `Background job ${index}`,
    updatedAt: '2026-06-30T00:01:00.000Z',
  }
}

function renderPanel(
  commandClient: CommandClient = createTestCommandClient(),
  selectedBackgroundAgentId?: string,
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<BackgroundAgentsPanel selectedBackgroundAgentId={selectedBackgroundAgentId} />, {
    wrapper: Wrapper,
  })
}

describe('BackgroundAgentsPanel', () => {
  it('renders loading, empty, and error states', async () => {
    renderPanel(
      createTestCommandClient({
        backgroundAgents: { agents: [] },
        delayMs: 50,
      }),
    )
    expect(screen.getByText('正在加载后台 Agent。')).toBeInTheDocument()

    renderPanel(createTestCommandClient({ backgroundAgents: { agents: [] } }))
    expect(await screen.findByText('暂无后台 Agent。')).toBeInTheDocument()

    renderPanel(createRejectedTestCommandClient(new Error('raw secret path')))
    expect(await screen.findByText('后台 Agent 无法加载。')).toBeInTheDocument()
    expect(screen.queryByText(/raw secret path/)).not.toBeInTheDocument()
  })

  it('renders running, waiting, interrupted, terminal, and archived states', async () => {
    renderPanel(
      createTestCommandClient({
        backgroundAgents: {
          agents: agentStates.map((state, index) => backgroundAgent(state, index + 1)),
        },
      }),
    )

    expect(await screen.findByText('Background job 1')).toBeInTheDocument()
    expect(screen.getByText('运行中')).toBeInTheDocument()
    expect(screen.getByText('等待输入')).toBeInTheDocument()
    expect(screen.getByText('已中断')).toBeInTheDocument()
    expect(screen.getByText('已完成')).toBeInTheDocument()
    expect(screen.getByText('已归档')).toBeInTheDocument()
    expect(screen.getByText('conversation-1')).toBeInTheDocument()
    expect(screen.getByText('run-1')).toBeInTheDocument()
  })

  it('opens the selected background agent from route search', async () => {
    renderPanel(
      createTestCommandClient({
        backgroundAgents: {
          agents: [backgroundAgent('running', 1), backgroundAgent('running', 2)],
        },
      }),
      'bg-agent-2',
    )

    expect(await screen.findByText('Background job 2')).toBeInTheDocument()
    expect(screen.getByText('已打开')).toBeInTheDocument()
  })

  it('renders durable background agent state from command-client fixtures', async () => {
    const commandClient = createTestCommandClient({
      backgroundAgents: agentOrchestrationBackgroundAgentsResponse,
    })

    renderPanel(commandClient, 'bg-agent-runtime-recovery')

    expect(await screen.findByText('Recovered background run')).toBeInTheDocument()
    expect(screen.getByText('已打开')).toBeInTheDocument()
    expect(screen.getByText('已中断')).toBeInTheDocument()
    expect(screen.getByText('Runtime orchestration background run')).toBeInTheDocument()

    const detail = await commandClient.getBackgroundAgent({
      backgroundAgentId: 'bg-agent-runtime-recovery',
      conversationId: 'conversation-agent-orchestration',
    })
    expect(detail.agent.parentRunId).toBe('run-agent-recovery')
  })

  it('supports lifecycle actions and background input submission', async () => {
    const pauseBackgroundAgent = vi.fn(async () => ({
      agent: { ...backgroundAgent('paused', 1), state: 'paused' as const },
    }))
    const resumeBackgroundAgent = vi.fn(async () => ({
      agent: { ...backgroundAgent('running', 2), state: 'running' as const },
    }))
    const sendBackgroundAgentInput = vi.fn(async () => ({
      agent: { ...backgroundAgent('running', 3), state: 'running' as const },
    }))
    const cancelBackgroundAgent = vi.fn(async () => ({
      agent: { ...backgroundAgent('cancelled', 1), state: 'cancelled' as const },
    }))
    const archiveBackgroundAgent = vi.fn(async () => ({
      agent: { ...backgroundAgent('archived', 4), state: 'archived' as const },
    }))
    const deleteBackgroundAgent = vi.fn(async () => ({
      backgroundAgentId: 'bg-agent-5',
      status: 'deleted' as const,
    }))
    const commandClient = {
      ...createTestCommandClient({
        backgroundAgents: {
          agents: [
            backgroundAgent('running', 1),
            backgroundAgent('interrupted', 2),
            backgroundAgent('waiting_for_input', 3),
            backgroundAgent('succeeded', 4),
            backgroundAgent('archived', 5),
          ],
        },
      }),
      archiveBackgroundAgent,
      cancelBackgroundAgent,
      deleteBackgroundAgent,
      pauseBackgroundAgent,
      resumeBackgroundAgent,
      sendBackgroundAgentInput,
    }

    renderPanel(commandClient)

    const running = await screen.findByRole('article', { name: 'Background job 1' })
    fireEvent.click(within(running).getByRole('button', { name: '暂停' }))
    fireEvent.click(within(running).getByRole('button', { name: '取消' }))

    const interrupted = screen.getByRole('article', { name: 'Background job 2' })
    fireEvent.click(within(interrupted).getByRole('button', { name: '恢复' }))

    const waiting = screen.getByRole('article', { name: 'Background job 3' })
    fireEvent.change(within(waiting).getByLabelText('输入'), {
      target: { value: 'Continue safely' },
    })
    fireEvent.click(within(waiting).getByRole('button', { name: '发送输入' }))

    const terminal = screen.getByRole('article', { name: 'Background job 4' })
    fireEvent.click(within(terminal).getByRole('button', { name: '归档' }))

    const archived = screen.getByRole('article', { name: 'Background job 5' })
    fireEvent.click(within(archived).getByRole('button', { name: '删除' }))

    await waitFor(() => {
      expect(pauseBackgroundAgent).toHaveBeenCalledWith({
        backgroundAgentId: 'bg-agent-1',
        conversationId: 'conversation-1',
      })
      expect(cancelBackgroundAgent).toHaveBeenCalledWith({
        backgroundAgentId: 'bg-agent-1',
        conversationId: 'conversation-1',
      })
      expect(resumeBackgroundAgent).toHaveBeenCalledWith({
        backgroundAgentId: 'bg-agent-2',
        conversationId: 'conversation-2',
      })
      expect(sendBackgroundAgentInput).toHaveBeenCalledWith({
        backgroundAgentId: 'bg-agent-3',
        conversationId: 'conversation-3',
        input: 'Continue safely',
        requestId: 'input-request-3',
      })
      expect(archiveBackgroundAgent).toHaveBeenCalledWith({
        backgroundAgentId: 'bg-agent-4',
        conversationId: 'conversation-4',
      })
      expect(deleteBackgroundAgent).toHaveBeenCalledWith({
        backgroundAgentId: 'bg-agent-5',
        conversationId: 'conversation-5',
      })
    })
  })
})
