import '@testing-library/jest-dom/vitest'

import { QueryClient } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import App from './App'

describe('App', () => {
  it('renders the index route through providers and router', async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
      },
    })

    render(<App commandClient={createMockCommandClient()} queryClient={queryClient} />)

    expect(await screen.findByRole('heading', { name: 'Jyowo' })).toBeInTheDocument()
    expect(screen.getAllByText('tauri2-react')[0]).toBeInTheDocument()
    expect(screen.getAllByText('jyowo_harness_sdk')[0]).toBeInTheDocument()
    expect(screen.getByText('available')).toBeInTheDocument()
  })
})
