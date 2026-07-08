import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { StatusBadge } from './status-badge'

describe('StatusBadge', () => {
  it('maps product status variants to semantic token classes', () => {
    render(
      <div>
        <StatusBadge tone="success">Ready</StatusBadge>
        <StatusBadge tone="warning">Pending</StatusBadge>
        <StatusBadge tone="destructive">Failed</StatusBadge>
        <StatusBadge tone="info">Running</StatusBadge>
        <StatusBadge tone="neutral">Idle</StatusBadge>
      </div>,
    )

    expect(screen.getByText('Ready')).toHaveClass('bg-success/12', 'text-success')
    expect(screen.getByText('Pending')).toHaveClass('bg-warning/12', 'text-warning')
    expect(screen.getByText('Failed')).toHaveClass('bg-destructive/12', 'text-destructive')
    expect(screen.getByText('Running')).toHaveClass('bg-info/12', 'text-info')
    expect(screen.getByText('Idle')).toHaveClass('bg-secondary', 'text-secondary-foreground')
  })
})
