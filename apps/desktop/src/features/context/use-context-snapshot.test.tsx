import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { CommandClient } from '@/shared/tauri/commands'
import { createMockCommandClient, createRejectedCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { useContextSnapshot } from './use-context-snapshot'

function renderUseContextSnapshot(commandClient: CommandClient = createMockCommandClient()) {
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
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<Probe />, { wrapper: Wrapper })
}

function renderDisabledUseContextSnapshot(
  commandClient: CommandClient = createMockCommandClient(),
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
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </CommandClientProvider>
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
    renderUseContextSnapshot(createRejectedCommandClient(new Error('Context unavailable')))

    expect(await screen.findByText('Context unavailable')).toBeInTheDocument()
  })

  it('does not request context when disabled', () => {
    const commandClient = createMockCommandClient()
    const getContextSnapshot = vi.fn(commandClient.getContextSnapshot)
    const trackedClient = {
      ...commandClient,
      getContextSnapshot,
    } satisfies CommandClient

    renderDisabledUseContextSnapshot(trackedClient)

    expect(screen.getByText('No context loaded')).toBeInTheDocument()
    expect(getContextSnapshot).not.toHaveBeenCalled()
  })
})
