import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { CommandClient, ListMemoryItemsResponse } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { MemoryBrowser } from './MemoryBrowser'

const memoryItems: ListMemoryItemsResponse = {
  items: [
    {
      contentPreview: 'Prefers concise Chinese responses',
      id: '01HZ0000000000000000000001',
      kind: 'user_preference',
      source: 'user_input',
      tags: ['tone'],
      updatedAt: '2026-06-17T00:00:00.000Z',
      visibility: 'tenant',
    },
    {
      contentPreview: 'Project uses Tauri and React',
      id: '01HZ0000000000000000000002',
      kind: 'project_fact',
      source: 'agent_derived',
      tags: [],
      updatedAt: '2026-06-17T00:00:00.000Z',
      visibility: 'private',
    },
  ],
}

function renderMemoryBrowser(commandClient: CommandClient = createTestCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(
    <Wrapper>
      <MemoryBrowser />
    </Wrapper>,
  )
}

describe('MemoryBrowser', () => {
  it('renders an empty state when no memory items are visible', async () => {
    renderMemoryBrowser(
      createTestCommandClient({
        memoryExport: {
          exportedAt: '2026-06-17T00:00:00.000Z',
          format: 'json',
          itemCount: 0,
          path: '.jyowo/runtime/exports/memory-empty.json',
        },
        memoryItems: { items: [] },
      }),
    )

    expect(await screen.findByText('No memory items available.')).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: /memory/i })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Export memory items' }))
    expect(
      await screen.findByText(
        'Export saved: 0 memory items to .jyowo/runtime/exports/memory-empty.json.',
      ),
    ).toBeInTheDocument()
  })

  it('lists visible memory items and inspects the selected item', async () => {
    renderMemoryBrowser(
      createTestCommandClient({
        memoryItem: {
          item: {
            accessCount: 2,
            confidence: 0.9,
            content: 'Prefers concise Chinese responses',
            createdAt: '2026-06-17T00:00:00.000Z',
            id: '01HZ0000000000000000000001',
            kind: 'user_preference',
            source: 'user_input',
            tags: ['tone'],
            updatedAt: '2026-06-17T00:00:00.000Z',
            visibility: 'tenant',
          },
        },
        memoryItems,
      }),
    )

    const card = await screen.findByRole('article', {
      name: 'Memory 01HZ0000000000000000000001',
    })

    expect(within(card).getByText('tenant')).toBeInTheDocument()
    expect(within(card).getByText('user_preference')).toBeInTheDocument()
    fireEvent.click(within(card).getByRole('button', { name: 'Inspect memory item' }))

    const detail = await screen.findByRole('region', { name: 'Memory detail' })
    expect(
      within(detail).getByDisplayValue('Prefers concise Chinese responses'),
    ).toBeInTheDocument()
    expect(within(detail).getByText('source: user_input')).toBeInTheDocument()
  })

  it('keeps long memory lists stable', async () => {
    const longList: ListMemoryItemsResponse = {
      items: Array.from({ length: 24 }, (_, index) => ({
        contentPreview: `Memory item ${index + 1}`,
        id: `01HZ00000000000000000000${String(index + 1).padStart(2, '0')}`,
        kind: 'reference',
        source: 'imported',
        tags: [],
        updatedAt: '2026-06-17T00:00:00.000Z',
        visibility: 'tenant',
      })),
    }

    renderMemoryBrowser(createTestCommandClient({ memoryItems: longList }))

    expect(await screen.findByText('Memory item 1')).toBeInTheDocument()
    expect(screen.getByText('Memory item 24')).toBeInTheDocument()
  })

  it('edits a selected memory item and refreshes the list', async () => {
    const listMemoryItems = vi.fn().mockResolvedValue(memoryItems)
    const getMemoryItem = vi.fn().mockResolvedValue({
      item: {
        accessCount: 0,
        confidence: 1,
        content: 'Prefers concise Chinese responses',
        createdAt: '2026-06-17T00:00:00.000Z',
        id: '01HZ0000000000000000000001',
        kind: 'user_preference',
        source: 'user_input',
        tags: ['tone'],
        updatedAt: '2026-06-17T00:00:00.000Z',
        visibility: 'tenant',
      },
    })
    const updateMemoryItem = vi.fn().mockResolvedValue({
      item: {
        accessCount: 0,
        confidence: 1,
        content: 'Prefers terse Chinese responses',
        createdAt: '2026-06-17T00:00:00.000Z',
        id: '01HZ0000000000000000000001',
        kind: 'user_preference',
        source: 'user_input',
        tags: ['tone'],
        updatedAt: '2026-06-17T00:00:00.000Z',
        visibility: 'tenant',
      },
    })
    const client = {
      ...createTestCommandClient(),
      getMemoryItem,
      listMemoryItems,
      updateMemoryItem,
    } satisfies CommandClient

    renderMemoryBrowser(client)

    const card = await screen.findByRole('article', {
      name: 'Memory 01HZ0000000000000000000001',
    })
    fireEvent.click(within(card).getByRole('button', { name: 'Inspect memory item' }))
    const editor = await screen.findByLabelText('Memory content')
    fireEvent.change(editor, { target: { value: 'Prefers terse Chinese responses' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save memory item' }))

    await waitFor(() =>
      expect(updateMemoryItem).toHaveBeenCalledWith({
        content: 'Prefers terse Chinese responses',
        id: '01HZ0000000000000000000001',
      }),
    )
    expect(listMemoryItems).toHaveBeenCalledTimes(2)
  })

  it('requires confirmation before deleting a memory item', async () => {
    const deleteMemoryItem = vi.fn().mockResolvedValue({
      id: '01HZ0000000000000000000001',
      status: 'deleted',
    })
    const client = {
      ...createTestCommandClient({ memoryItems }),
      deleteMemoryItem,
    } satisfies CommandClient

    renderMemoryBrowser(client)

    const card = await screen.findByRole('article', {
      name: 'Memory 01HZ0000000000000000000001',
    })
    fireEvent.click(within(card).getByRole('button', { name: 'Delete memory item' }))
    expect(deleteMemoryItem).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Confirm memory deletion' }))

    await waitFor(() => expect(deleteMemoryItem).toHaveBeenCalledWith('01HZ0000000000000000000001'))
  })

  it('exports visible memory items through the backend', async () => {
    const exportMemoryItems = vi.fn().mockResolvedValue({
      exportedAt: '2026-06-17T00:00:00.000Z',
      format: 'json',
      itemCount: 2,
      path: '.jyowo/runtime/exports/memory-export.json',
    })
    const client = {
      ...createTestCommandClient({ memoryItems }),
      exportMemoryItems,
    } satisfies CommandClient

    renderMemoryBrowser(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Export memory items' }))

    await waitFor(() => expect(exportMemoryItems).toHaveBeenCalled())
    expect(
      await screen.findByText(
        'Export saved: 2 memory items to .jyowo/runtime/exports/memory-export.json.',
      ),
    ).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Download memory export' })).not.toBeInTheDocument()
  })
})
