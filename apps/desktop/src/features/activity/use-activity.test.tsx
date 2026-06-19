import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it } from 'vitest'

import type { CommandClient } from '@/shared/tauri/commands'
import { createMockCommandClient, createRejectedCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { useActivity } from './use-activity'

function renderUseActivity(commandClient: CommandClient = createMockCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  function Probe() {
    const activity = useActivity({ conversationId: 'conversation-001' })

    if (activity.isLoading) {
      return <div>Loading activity</div>
    }

    if (activity.error) {
      return <div>{activity.error.message}</div>
    }

    return (
      <div>
        <span>{activity.items[0]?.label}</span>
        <span>{activity.currentRun?.status}</span>
        {activity.usageSummary ? <span>{activity.usageSummary.inputTokens}</span> : null}
      </div>
    )
  }

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<Probe />, { wrapper: Wrapper })
}

describe('useActivity', () => {
  it('loads activity events through CommandClient and maps them to rail items', async () => {
    renderUseActivity()

    expect(await screen.findByText('run')).toBeInTheDocument()
    expect(screen.getByText('running')).toBeInTheDocument()
  })

  it('exposes the latest completed run usage summary', async () => {
    renderUseActivity(
      createMockCommandClient({
        listActivity: {
          events: [
            {
              id: 'evt-started',
              payload: { sessionId: 'conversation-001' },
              runId: 'run-001',
              sequence: 1,
              source: 'engine',
              timestamp: '2026-06-17T00:00:00.000Z',
              type: 'run.started',
              visibility: 'public',
            },
            {
              id: 'evt-ended',
              payload: {
                reason: 'completed',
                usage: {
                  cacheReadTokens: 3,
                  cacheWriteTokens: 5,
                  costMicros: 260,
                  inputTokens: 11,
                  outputTokens: 7,
                  toolCalls: 2,
                },
              },
              runId: 'run-001',
              sequence: 2,
              source: 'engine',
              timestamp: '2026-06-17T00:00:01.000Z',
              type: 'run.ended',
              visibility: 'public',
            },
          ],
        },
      }),
    )

    expect(await screen.findByText('11')).toBeInTheDocument()
  })

  it('surfaces command errors without raw payload rendering', async () => {
    renderUseActivity(createRejectedCommandClient(new Error('Activity unavailable')))

    expect(await screen.findByText('Activity unavailable')).toBeInTheDocument()
  })
})
