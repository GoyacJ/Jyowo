import '@testing-library/jest-dom/vitest'

import { QueryClient } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { AppProviders } from '@/app/providers'
import { createMockCommandClient, createRejectedCommandClient } from '@/shared/tauri/mock-client'
import { SystemStatusPage } from './SystemStatusPage'

function renderSystemStatus(commandClient = createMockCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  })

  return render(
    <AppProviders commandClient={commandClient} queryClient={queryClient}>
      <SystemStatusPage />
    </AppProviders>,
  )
}

describe('SystemStatusPage', () => {
  it('renders a loading state before IPC resolves', () => {
    renderSystemStatus(
      createMockCommandClient({
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
    renderSystemStatus(createRejectedCommandClient(new Error('IPC unavailable')))

    expect(await screen.findByText('IPC unavailable')).toBeInTheDocument()
  })
})
