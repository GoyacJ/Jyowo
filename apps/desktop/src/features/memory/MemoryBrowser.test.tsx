import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { DaemonClient } from '@/shared/daemon/client'
import { DaemonClientProvider } from '@/shared/tauri/react'

import { MemoryBrowser } from './MemoryBrowser'
import type { MemoryItemSummary } from './memory-types'

const workspaceRoot = '/workspace/active'
const memoryItems: { items: MemoryItemSummary[]; type: 'memory_items' } = {
  type: 'memory_items',
  items: [
    {
      contentHash: '0'.repeat(64),
      contentPreview: 'Prefers concise Chinese responses',
      deleted: false,
      id: '01HZ0000000000000000000001',
      kind: 'user_preference',
      source: 'user_input',
      tags: ['tone'],
      updatedAt: '2026-06-17T00:00:00.000Z',
      visibility: 'tenant',
    },
    {
      contentHash: '1'.repeat(64),
      contentPreview: 'Project uses Tauri and React',
      deleted: false,
      id: '01HZ0000000000000000000002',
      kind: 'project_fact',
      source: 'agent_derived',
      tags: [],
      updatedAt: '2026-06-17T00:00:00.000Z',
      visibility: 'private',
    },
  ],
}

function createMemoryDaemonClient(
  options: {
    memoryExport?: Record<string, unknown>
    memoryItem?: Record<string, unknown>
    memoryItems?: { items: MemoryItemSummary[]; type?: 'memory_items' }
  } = {},
) {
  return {
    deleteMemoryItem: vi.fn().mockResolvedValue({ memoryId: '', type: 'memory_deleted' }),
    exportMemoryItems: vi.fn().mockResolvedValue({
      auditHash: '0'.repeat(64),
      exportedAt: '2026-06-17T00:00:00.000Z',
      format: 'json',
      includeHashes: true,
      includeMetadata: true,
      includeRawContent: false,
      itemCount: 0,
      path: '/tmp/memory.json',
      scope: 'visible',
      type: 'memory_exported',
      ...options.memoryExport,
    }),
    getMemoryItem: vi.fn().mockResolvedValue({ type: 'memory_item', ...options.memoryItem }),
    listMemoryItems: vi.fn().mockResolvedValue({
      items: options.memoryItems?.items ?? [],
      type: 'memory_items',
    }),
    updateMemoryItem: vi.fn(),
  } as unknown as DaemonClient
}

function renderMemoryBrowser(daemonClient: DaemonClient = createMemoryDaemonClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <DaemonClientProvider client={daemonClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </DaemonClientProvider>
    )
  }

  return render(
    <Wrapper>
      <MemoryBrowser workspaceRoot={workspaceRoot} />
    </Wrapper>,
  )
}

describe('MemoryBrowser', () => {
  it('renders an empty state when no memory items are visible', async () => {
    renderMemoryBrowser(
      createMemoryDaemonClient({
        memoryExport: {
          auditHash: '0'.repeat(64),
          exportedAt: '2026-06-17T00:00:00.000Z',
          format: 'json',
          includeHashes: true,
          includeMetadata: true,
          includeRawContent: false,
          itemCount: 0,
          path: '.jyowo/runtime/exports/memory-empty.json',
          scope: 'visible',
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
      createMemoryDaemonClient({
        memoryItem: {
          item: {
            accessCount: 2,
            confidence: 0.9,
            content: 'Prefers concise Chinese responses',
            contentHash: '0'.repeat(64),
            createdAt: '2026-06-17T00:00:00.000Z',
            deleted: false,
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
    const longList: { items: MemoryItemSummary[]; type: 'memory_items' } = {
      type: 'memory_items',
      items: Array.from({ length: 24 }, (_, index) => ({
        contentHash: String(index).repeat(64).slice(0, 64).padEnd(64, '0'),
        contentPreview: `Memory item ${index + 1}`,
        deleted: false,
        id: `01HZ00000000000000000000${String(index + 1).padStart(2, '0')}`,
        kind: 'reference',
        source: 'imported',
        tags: [],
        updatedAt: '2026-06-17T00:00:00.000Z',
        visibility: 'tenant',
      })),
    }

    renderMemoryBrowser(createMemoryDaemonClient({ memoryItems: longList }))

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
        contentHash: '0'.repeat(64),
        createdAt: '2026-06-17T00:00:00.000Z',
        deleted: false,
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
        contentHash: '0'.repeat(64),
        createdAt: '2026-06-17T00:00:00.000Z',
        deleted: false,
        id: '01HZ0000000000000000000001',
        kind: 'user_preference',
        source: 'user_input',
        tags: ['tone'],
        updatedAt: '2026-06-17T00:00:00.000Z',
        visibility: 'tenant',
      },
    })
    const client = {
      ...createMemoryDaemonClient(),
      getMemoryItem,
      listMemoryItems,
      updateMemoryItem,
    } as unknown as DaemonClient

    renderMemoryBrowser(client)

    const card = await screen.findByRole('article', {
      name: 'Memory 01HZ0000000000000000000001',
    })
    fireEvent.click(within(card).getByRole('button', { name: 'Inspect memory item' }))
    const editor = await screen.findByLabelText('Memory content')
    fireEvent.change(editor, { target: { value: 'Prefers terse Chinese responses' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save memory item' }))

    await waitFor(() =>
      expect(updateMemoryItem).toHaveBeenCalledWith(workspaceRoot, {
        content: 'Prefers terse Chinese responses',
        id: '01HZ0000000000000000000001',
      }),
    )
    expect(listMemoryItems).toHaveBeenCalledTimes(2)
  })

  it('requires confirmation before deleting a memory item', async () => {
    const deleteMemoryItem = vi.fn().mockResolvedValue({
      memoryId: '01HZ0000000000000000000001',
      type: 'memory_deleted',
    })
    const client = {
      ...createMemoryDaemonClient({ memoryItems }),
      deleteMemoryItem,
    } as unknown as DaemonClient

    renderMemoryBrowser(client)

    const card = await screen.findByRole('article', {
      name: 'Memory 01HZ0000000000000000000001',
    })
    fireEvent.click(within(card).getByRole('button', { name: 'Delete memory item' }))
    expect(deleteMemoryItem).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Confirm memory deletion' }))

    await waitFor(() =>
      expect(deleteMemoryItem).toHaveBeenCalledWith(workspaceRoot, {
        id: '01HZ0000000000000000000001',
      }),
    )
  })

  it('exports visible memory items through the backend', async () => {
    const exportMemoryItems = vi.fn().mockResolvedValue({
      auditHash: '1'.repeat(64),
      exportedAt: '2026-06-17T00:00:00.000Z',
      format: 'json',
      includeHashes: true,
      includeMetadata: true,
      includeRawContent: false,
      itemCount: 2,
      path: '.jyowo/runtime/exports/memory-export.json',
      scope: 'visible',
    })
    const client = {
      ...createMemoryDaemonClient({ memoryItems }),
      exportMemoryItems,
    } as unknown as DaemonClient

    renderMemoryBrowser(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Export memory items' }))

    await waitFor(() =>
      expect(exportMemoryItems).toHaveBeenCalledWith(workspaceRoot, {
        explicitUserAction: true,
        format: 'json',
        includeHashes: true,
        includeMetadata: true,
        includeRawContent: false,
        scope: 'visible',
      }),
    )
    expect(
      await screen.findByText(
        'Export saved: 2 memory items to .jyowo/runtime/exports/memory-export.json.',
      ),
    ).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Download memory export' })).not.toBeInTheDocument()
  })
})
