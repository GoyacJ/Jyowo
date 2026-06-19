import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { type EvalCase, EvalLab } from './EvalLab'

const evalCases: EvalCase[] = [
  {
    id: 'eval-safe-tooling',
    lastRun: {
      completedAt: '2026-06-17T00:00:00.000Z',
      failed: 1,
      passed: 4,
      status: 'failed',
    },
    title: 'Safe tool routing',
  },
  {
    id: 'eval-context-recall',
    lastRun: {
      completedAt: '2026-06-17T01:00:00.000Z',
      failed: 0,
      passed: 6,
      status: 'passed',
    },
    title: 'Context recall',
  },
]

describe('EvalLab', () => {
  it('renders eval cases and result previews as a support workflow', () => {
    render(<EvalLab cases={evalCases} />)

    expect(screen.getByRole('heading', { name: 'Eval lab' })).toBeInTheDocument()
    expect(screen.queryByRole('main')).not.toBeInTheDocument()

    const failedCase = screen.getByRole('article', { name: 'Safe tool routing' })
    expect(within(failedCase).getByText('4 passed')).toBeInTheDocument()
    expect(within(failedCase).getByText('1 failed')).toBeInTheDocument()
    expect(within(failedCase).getByText('failed')).toBeInTheDocument()

    const passedCase = screen.getByRole('article', { name: 'Context recall' })
    expect(within(passedCase).getByText('6 passed')).toBeInTheDocument()
    expect(within(passedCase).getByText('passed')).toBeInTheDocument()
  })

  it('emits a run intent for the selected eval case', () => {
    const onRunCase = vi.fn()

    render(<EvalLab cases={evalCases} onRunCase={onRunCase} />)

    fireEvent.click(screen.getByRole('button', { name: 'Run Safe tool routing' }))

    expect(onRunCase).toHaveBeenCalledWith('eval-safe-tooling')
  })

  it('shows unavailable and failure states without leaking raw errors', () => {
    const rawError = 'provider failed with Authorization Bearer sk-secret-value'
    const { rerender } = render(<EvalLab cases={[]} unavailable />)

    expect(screen.getByText('Eval runtime is not connected.')).toBeInTheDocument()

    rerender(<EvalLab cases={evalCases} errorMessage={rawError} />)

    expect(screen.getByRole('alert')).toHaveTextContent('Eval results could not be loaded.')
    expect(screen.queryByText(rawError)).not.toBeInTheDocument()
    expect(screen.queryByText(/sk-secret-value/)).not.toBeInTheDocument()
  })
})
