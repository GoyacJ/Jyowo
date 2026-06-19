import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { Composer } from './Composer'

describe('Composer', () => {
  it('submits typed text', () => {
    const onSubmit = vi.fn()

    render(<Composer onSubmit={onSubmit} />)

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Continue the setup' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    expect(onSubmit).toHaveBeenCalledWith('Continue the setup')
  })

  it('blocks empty submit', () => {
    const onSubmit = vi.fn()

    render(<Composer onSubmit={onSubmit} />)

    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('disables input and send while pending', () => {
    render(<Composer onSubmit={vi.fn()} pending />)

    expect(screen.getByPlaceholderText('Ask Jyowo anything about this project...')).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled()
  })

  it('shows retry when an error is present', () => {
    const onRetry = vi.fn()

    render(<Composer errorMessage="Run failed" onRetry={onRetry} onSubmit={vi.fn()} />)

    expect(screen.getByText('Run failed')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(onRetry).toHaveBeenCalledTimes(1)
  })

  it('disables retry while pending', () => {
    render(<Composer errorMessage="Run failed" onRetry={vi.fn()} onSubmit={vi.fn()} pending />)

    expect(screen.getByRole('button', { name: 'Retry' })).toBeDisabled()
  })
})
