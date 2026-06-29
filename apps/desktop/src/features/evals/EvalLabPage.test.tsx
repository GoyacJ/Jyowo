import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createRejectedTestCommandClient, createTestCommandClient } from '@/testing/command-client'

import { EvalLabPage } from './EvalLabPage'

function renderEvalLabPage(commandClient: CommandClient = createTestCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<EvalLabPage />, { wrapper: Wrapper })
}

describe('EvalLabPage', () => {
  it('loads eval cases from the command client and runs a selected case', async () => {
    const commandClient = createTestCommandClient()
    const listEvalCases = vi.fn(commandClient.listEvalCases)
    const runEvalCase = vi.fn(commandClient.runEvalCase)
    const trackedClient = {
      ...commandClient,
      listEvalCases,
      runEvalCase,
    } satisfies CommandClient

    renderEvalLabPage(trackedClient)

    expect(await screen.findByRole('article', { name: 'Regression smoke' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Run Regression smoke' }))

    await waitFor(() => {
      expect(listEvalCases).toHaveBeenCalled()
      expect(runEvalCase).toHaveBeenCalledWith('regression-smoke')
    })
  })

  it('renders eval loading and error states without raw provider details', async () => {
    const { unmount } = renderEvalLabPage(createTestCommandClient({ delayMs: 10 }))

    expect(screen.getByText('Loading eval cases')).toBeInTheDocument()
    expect(await screen.findByRole('article', { name: 'Regression smoke' })).toBeInTheDocument()

    unmount()

    renderEvalLabPage(
      createRejectedTestCommandClient(
        new Error('provider failed with Authorization Bearer secret'),
      ),
    )

    expect(await screen.findByRole('alert')).toHaveTextContent('Eval results could not be loaded.')
    expect(screen.queryByText(/Authorization Bearer/)).not.toBeInTheDocument()
  })
})
