import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createRejectedTestCommandClient, createTestCommandClient } from '@/testing/command-client'
import { SystemStatusPage } from './SystemStatusPage'

function renderSystemStatus(commandClient = createTestCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  })

  return render(
    <CommandClientProvider client={commandClient}>
      <QueryClientProvider client={queryClient}>
        <SystemStatusPage />
      </QueryClientProvider>
    </CommandClientProvider>,
  )
}

describe('SystemStatusPage', () => {
  it('renders a loading state before IPC resolves', () => {
    renderSystemStatus(
      createTestCommandClient({
        delayMs: 50,
      }),
    )

    expect(screen.getByText('Loading Jyowo')).toBeInTheDocument()
  })

  it('renders app info and harness status from the command client', async () => {
    renderSystemStatus()

    expect(await screen.findByRole('heading', { name: 'Jyowo' })).toBeInTheDocument()
    expect(screen.getByText('0.1.0')).toBeInTheDocument()
    expect(screen.getByText('tauri2-react')).toBeInTheDocument()
    expect(screen.getByText('in-process')).toBeInTheDocument()
    expect(screen.getByText('available')).toBeInTheDocument()
  })

  it('renders a normalized error state', async () => {
    renderSystemStatus(createRejectedTestCommandClient(new Error('IPC unavailable')))

    expect(await screen.findByText('IPC unavailable')).toBeInTheDocument()
  })
})
