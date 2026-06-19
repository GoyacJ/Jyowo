import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { UsageSummary, type UsageSummaryModel } from './UsageSummary'

const usage: UsageSummaryModel = {
  cacheReadTokens: 50,
  cacheWriteTokens: 25,
  costMicros: 123_456,
  inputTokens: 1_200,
  outputTokens: 450,
  providerLabel: 'openai',
  toolCalls: 7,
}

describe('UsageSummary', () => {
  it('shows token usage, tool calls, and local cost estimates', () => {
    render(<UsageSummary usage={usage} />)

    expect(screen.getByRole('region', { name: 'Usage summary' })).toBeInTheDocument()
    expect(screen.getByText('1,200')).toBeInTheDocument()
    expect(screen.getByText('450')).toBeInTheDocument()
    expect(screen.getByText('7')).toBeInTheDocument()
    expect(screen.getByText('$0.123456')).toBeInTheDocument()
    expect(screen.getByText('Cache read 50')).toBeInTheDocument()
    expect(screen.getByText('Cache write 25')).toBeInTheDocument()
  })

  it('renders unavailable analytics as non-security telemetry failure', () => {
    render(<UsageSummary unavailable />)

    expect(screen.getByText('Usage analytics unavailable.')).toBeInTheDocument()
    expect(screen.getByText('Execution permissions are unchanged.')).toBeInTheDocument()
  })

  it('does not render raw provider details that may contain secrets', () => {
    render(
      <UsageSummary
        usage={{
          ...usage,
          providerLabel: 'openai sk-proj-secret-value',
        }}
      />,
    )

    expect(screen.getByText('Provider redacted')).toBeInTheDocument()
    expect(screen.queryByText(/sk-proj-secret-value/)).not.toBeInTheDocument()
  })
})
