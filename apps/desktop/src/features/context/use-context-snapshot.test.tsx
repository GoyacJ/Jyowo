import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { DaemonClient } from '@/shared/daemon/client'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider, DaemonClientProvider } from '@/shared/tauri/react'
import { createRejectedTestCommandClient, createTestCommandClient } from '@/testing/command-client'

import { useContextSnapshot } from './use-context-snapshot'

const daemonClient = {
  getModelRequestPreview: vi.fn(),
} as unknown as DaemonClient

function renderUseContextSnapshot(
  commandClient: CommandClient = createTestCommandClient(),
  client: DaemonClient = daemonClient,
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  function Probe() {
    const contextSnapshot = useContextSnapshot({ conversationId: 'conversation-001' })

    if (contextSnapshot.isLoading) {
      return <div>Loading context</div>
    }

    if (contextSnapshot.error) {
      return <div>{contextSnapshot.error.message}</div>
    }

    return (
      <div>
        <span>{contextSnapshot.context?.project}</span>
        <span>{contextSnapshot.context?.files[0]?.label}</span>
      </div>
    )
  }

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <DaemonClientProvider client={client}>
        <CommandClientProvider client={commandClient}>
          <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
        </CommandClientProvider>
      </DaemonClientProvider>
    )
  }

  return render(<Probe />, { wrapper: Wrapper })
}

function renderDisabledUseContextSnapshot(
  commandClient: CommandClient = createTestCommandClient(),
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  function Probe() {
    const contextSnapshot = useContextSnapshot(
      { conversationId: 'conversation-001' },
      {
        enabled: false,
      },
    )

    return <div>{contextSnapshot.context?.project ?? 'No context loaded'}</div>
  }

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <DaemonClientProvider client={daemonClient}>
        <CommandClientProvider client={commandClient}>
          <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
        </CommandClientProvider>
      </DaemonClientProvider>
    )
  }

  return render(<Probe />, { wrapper: Wrapper })
}

describe('useContextSnapshot', () => {
  it('loads project context through CommandClient', async () => {
    renderUseContextSnapshot()

    expect(await screen.findByText('Desktop App')).toBeInTheDocument()
    expect(screen.getByText('src/')).toBeInTheDocument()
  })

  it('surfaces command errors without turning empty context into data', async () => {
    renderUseContextSnapshot(createRejectedTestCommandClient(new Error('Context unavailable')))

    expect(await screen.findByText('Context unavailable')).toBeInTheDocument()
  })

  it('does not request context when disabled', () => {
    const commandClient = createTestCommandClient()
    const getContextSnapshot = vi.fn(commandClient.getContextSnapshot)
    const trackedClient = {
      ...commandClient,
      getContextSnapshot,
    } satisfies CommandClient

    renderDisabledUseContextSnapshot(trackedClient)

    expect(screen.getByText('No context loaded')).toBeInTheDocument()
    expect(getContextSnapshot).not.toHaveBeenCalled()
  })

  it('loads model request preview through the daemon client', async () => {
    const getModelRequestPreview = vi.fn().mockResolvedValue({
      preview: {
        content_hash: Array.from({ length: 32 }, () => 0),
        policy_decisions: [],
        redacted_count: 0,
        run_id: '01HZ0000000000000000000002',
        sections: [],
        session_id: '01HZ0000000000000000000001',
        token_estimate: 0,
        tool_names: [],
      },
      type: 'model_request_preview',
    })
    const client = { getModelRequestPreview } as unknown as DaemonClient
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })

    function Probe() {
      const snapshot = useContextSnapshot(
        {
          conversationId: '01HZ0000000000000000000001',
          runId: '01HZ0000000000000000000002',
        },
        { workspaceRoot: '/workspace/active' },
      )
      return <div>{snapshot.modelRequestPreview ? 'Preview loaded' : 'No preview'}</div>
    }

    render(
      <DaemonClientProvider client={client}>
        <CommandClientProvider client={createTestCommandClient()}>
          <QueryClientProvider client={queryClient}>
            <Probe />
          </QueryClientProvider>
        </CommandClientProvider>
      </DaemonClientProvider>,
    )

    expect(await screen.findByText('Preview loaded')).toBeInTheDocument()
    expect(getModelRequestPreview).toHaveBeenCalledWith('/workspace/active', {
      run_id: '01HZ0000000000000000000002',
      session_id: '01HZ0000000000000000000001',
      tenant_id: '00000000000000000000000001',
    })
  })
})
