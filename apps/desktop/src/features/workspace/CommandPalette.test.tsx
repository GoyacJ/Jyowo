import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { CommandPalette } from './CommandPalette'

describe('CommandPalette', () => {
  it('opens from keyboard and exposes product-facing commands', () => {
    const onAction = vi.fn()

    render(<CommandPalette onAction={onAction} />)

    fireEvent.keyDown(window, { key: 'k', metaKey: true })

    expect(screen.getByRole('dialog', { name: 'Command palette' })).toBeInTheDocument()
    expect(screen.getByRole('combobox', { name: 'Search commands' })).toHaveFocus()
    expect(screen.getByRole('option', { name: 'New conversation' })).toBeInTheDocument()
    expect(screen.getByRole('option', { name: 'Open artifact' })).toBeInTheDocument()
    expect(screen.getByRole('option', { name: 'Open evals' })).toBeInTheDocument()
    expect(screen.queryByRole('option', { name: 'Search files' })).not.toBeInTheDocument()
    expect(screen.queryByRole('option', { name: 'View activity' })).not.toBeInTheDocument()
    expect(screen.getByRole('option', { name: 'Settings' })).toBeInTheDocument()
    expect(screen.queryByText(/reducer|router|cache|plugin|store/i)).not.toBeInTheDocument()
  })

  it('runs the selected command from keyboard', async () => {
    const onAction = vi.fn()

    render(<CommandPalette onAction={onAction} />)

    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    const searchInput = screen.getByRole('combobox', { name: 'Search commands' })
    fireEvent.change(searchInput, { target: { value: 'settings' } })
    fireEvent.keyDown(searchInput, { key: 'Enter' })

    await waitFor(() => {
      expect(onAction).toHaveBeenCalledWith('settings')
    })
    expect(screen.queryByRole('dialog', { name: 'Command palette' })).not.toBeInTheDocument()
  })

  it('restores focus when dismissed from keyboard', async () => {
    render(
      <>
        <button type="button">Before palette</button>
        <CommandPalette />
      </>,
    )

    const previousFocus = screen.getByRole('button', { name: 'Before palette' })
    previousFocus.focus()
    fireEvent.keyDown(window, { key: 'k', metaKey: true })

    expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument()
    expect(screen.getByRole('combobox', { name: 'Search commands' })).toHaveFocus()

    fireEvent.keyDown(document, { key: 'Escape' })

    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Command palette' })).not.toBeInTheDocument()
    })
    expect(previousFocus).toHaveFocus()
  })
})
