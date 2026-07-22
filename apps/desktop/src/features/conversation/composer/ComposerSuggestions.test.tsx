import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { Composer } from '../Composer'

const referenceCandidates = {
  artifacts: [{ id: 'artifact-001', label: 'Build notes' }],
  conversations: [],
  files: [
    {
      label: 'Composer.tsx',
      path: 'apps/desktop/src/features/conversation/Composer.tsx',
    },
  ],
  memories: [],
  mcpServers: [],
  skills: [],
  tools: [],
}

describe('Composer suggestions', () => {
  it('keeps a slash query open, filters commands, and inserts with Tab', async () => {
    render(<Composer onSubmit={vi.fn()} />)
    const editor = screen.getByRole('textbox', { name: 'Message input' })

    fireEvent.change(editor, { target: { value: '/rev' } })

    expect(await screen.findByRole('option', { name: 'Review' })).toBeInTheDocument()
    expect(screen.queryByRole('option', { name: 'Plan' })).not.toBeInTheDocument()
    expect(editor).toHaveAttribute('aria-expanded', 'true')

    fireEvent.keyDown(editor, { key: 'Tab' })

    expect(editor).toHaveValue('/review ')
    expect(screen.queryByRole('listbox', { name: 'Slash commands' })).not.toBeInTheDocument()
  })

  it('uses the inline reference token as search without rendering another input', async () => {
    render(
      <Composer
        onListReferenceCandidates={vi.fn().mockResolvedValue(referenceCandidates)}
        onSubmit={vi.fn()}
      />,
    )
    const editor = screen.getByRole('textbox', { name: 'Message input' })

    fireEvent.change(editor, { target: { value: '@comp' } })

    expect(
      await screen.findByRole('listbox', { name: 'Reference project object' }),
    ).toBeInTheDocument()
    expect(screen.getByRole('option', { name: 'Composer.tsx' })).toBeInTheDocument()
    expect(screen.queryByRole('combobox', { name: 'Search references' })).not.toBeInTheDocument()
    expect(editor).toHaveAttribute('aria-controls', 'composer-reference-combobox-listbox')
  })

  it('replaces a reference token at the cursor and preserves surrounding text', async () => {
    render(
      <Composer
        onListReferenceCandidates={vi.fn().mockResolvedValue(referenceCandidates)}
        onSubmit={vi.fn()}
      />,
    )
    const editor = screen.getByRole('textbox', { name: 'Message input' }) as HTMLTextAreaElement

    fireEvent.change(editor, { target: { value: 'Use @comp today' } })
    editor.setSelectionRange(9, 9)
    fireEvent.select(editor)
    fireEvent.click(await screen.findByRole('option', { name: 'Composer.tsx' }))

    expect(editor).toHaveValue('Use today')
    expect(screen.getByText('Composer.tsx')).toBeInTheDocument()
    await waitFor(() => expect(editor).toHaveFocus())
  })

  it('focuses the dedicated search input when opened from the toolbar', async () => {
    render(
      <Composer
        onListReferenceCandidates={vi.fn().mockResolvedValue(referenceCandidates)}
        onSubmit={vi.fn()}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Reference project object' }))

    const search = await screen.findByRole('combobox', { name: 'Search references' })
    await waitFor(() => expect(search).toHaveFocus())
    expect(screen.getByTestId('composer-suggestion-panel')).toHaveClass('absolute', 'w-full')
  })

  it('closes an open suggestion panel when clicking outside the composer', async () => {
    render(<Composer onSubmit={vi.fn()} />)
    const editor = screen.getByRole('textbox', { name: 'Message input' })
    fireEvent.change(editor, { target: { value: '/' } })
    expect(await screen.findByRole('listbox', { name: 'Slash commands' })).toBeInTheDocument()

    fireEvent.pointerDown(document.body)

    expect(screen.queryByRole('listbox', { name: 'Slash commands' })).not.toBeInTheDocument()
  })
})
