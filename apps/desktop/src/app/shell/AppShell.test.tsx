import '@testing-library/jest-dom/vitest'

import { render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { AppShell } from './AppShell'

describe('AppShell', () => {
  it('provides the desktop workbench regions required by the frontend spec', () => {
    render(
      <AppShell>
        <h1>Workbench content</h1>
      </AppShell>,
    )

    expect(screen.getByRole('banner')).toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: 'Primary' })).toBeInTheDocument()
    expect(screen.getByRole('main')).toContainElement(screen.getByText('Workbench content'))
    expect(screen.getByRole('complementary', { name: 'Inspector' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Bottom panel' })).toBeInTheDocument()

    const primaryNavigation = screen.getByRole('navigation', { name: 'Primary' })
    expect(primaryNavigation).not.toHaveClass('hidden')
    expect(within(primaryNavigation).getByText('Runs')).toBeInTheDocument()
    expect(within(primaryNavigation).getByText('MCP')).toBeInTheDocument()
    expect(within(primaryNavigation).getByText('Memory')).toBeInTheDocument()
    expect(screen.getByRole('complementary', { name: 'Inspector' })).not.toHaveClass('hidden')
  })
})
