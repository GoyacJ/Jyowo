import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { WorkspaceSearch } from './WorkspaceSearch'

describe('WorkspaceSearch', () => {
  it('renders the workspace search input and reports changes', () => {
    const onChange = vi.fn()

    render(<WorkspaceSearch onChange={onChange} value="" />)

    const input = screen.getByRole('searchbox', { name: 'Search' })

    expect(input).toHaveAttribute('placeholder', 'Search')

    fireEvent.change(input, { target: { value: 'auth' } })

    expect(onChange).toHaveBeenCalled()
  })
})
