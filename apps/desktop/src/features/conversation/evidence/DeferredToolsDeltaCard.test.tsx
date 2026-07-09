import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { DeferredToolsDeltaCard } from './DeferredToolsDeltaCard'

describe('DeferredToolsDeltaCard', () => {
  it('renders added and removed deferred tools with reasons', () => {
    render(
      <DeferredToolsDeltaCard
        change={{
          added: [{ name: 'web_search', hint: 'network task detected' }],
          deferredTotal: 3,
          removed: ['grep'],
          source: 'initial_classification',
        }}
      />,
    )

    expect(screen.getByText('web_search')).toBeInTheDocument()
    expect(screen.getByText('network task detected')).toBeInTheDocument()
    expect(screen.getByText('grep')).toBeInTheDocument()
    expect(screen.getByText('3 deferred')).toBeInTheDocument()
  })
})
