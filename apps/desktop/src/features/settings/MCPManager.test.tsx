import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { CommandClient } from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { MCPManager } from './MCPManager'

function renderMCPManager(commandClient: CommandClient = createMockCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
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
      <MCPManager />
    </Wrapper>,
  )
}

describe('MCPManager', () => {
  it('renders an empty support surface when no MCP servers are configured', async () => {
    renderMCPManager(
      createMockCommandClient({
        mcpServers: { servers: [] },
      }),
    )

    expect(await screen.findByText('No MCP servers configured.')).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: /mcp/i })).not.toBeInTheDocument()
  })

  it('shows server status, origin, tool count, scope, and transport', async () => {
    renderMCPManager(
      createMockCommandClient({
        mcpServers: {
          servers: [
            {
              displayName: 'Workspace GitHub',
              exposedToolCount: 2,
              id: 'github',
              origin: 'workspace',
              scope: 'global',
              status: 'ready',
              transport: 'stdio',
            },
          ],
        },
      }),
    )

    const card = await screen.findByRole('article', { name: 'Workspace GitHub' })

    expect(within(card).getByText('ready')).toBeInTheDocument()
    expect(within(card).getByText('workspace')).toBeInTheDocument()
    expect(within(card).getByText('2 tools')).toBeInTheDocument()
    expect(within(card).getByText('global')).toBeInTheDocument()
    expect(within(card).getByText('stdio')).toBeInTheDocument()
  })

  it('rejects invalid config before calling the backend', async () => {
    const saveMcpServer = vi.fn()
    const client = {
      ...createMockCommandClient({ mcpServers: { servers: [] } }),
      saveMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Save MCP server' }))

    expect(await screen.findByText('Server name is required.')).toBeInTheDocument()
    expect(screen.getByText('Server id is required.')).toBeInTheDocument()
    expect(screen.getByText('Command is required.')).toBeInTheDocument()
    expect(saveMcpServer).not.toHaveBeenCalled()
  })

  it('renders a sanitized connection failure without leaking raw backend details', async () => {
    const rawError = 'spawn failed: Authorization=Bearer mcp-secret-token'
    const client = {
      ...createMockCommandClient({ mcpServers: { servers: [] } }),
      saveMcpServer: vi.fn().mockRejectedValue(new Error(rawError)),
    }

    renderMCPManager(client)

    fireEvent.change(await screen.findByLabelText('Server name'), {
      target: { value: 'Workspace GitHub' },
    })
    fireEvent.change(screen.getByLabelText('Server id'), { target: { value: 'github' } })
    fireEvent.change(screen.getByLabelText('Command'), { target: { value: 'node' } })
    fireEvent.change(screen.getByLabelText('Arguments'), { target: { value: 'mcp-server' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save MCP server' }))

    expect(await screen.findByText('MCP server could not be saved.')).toBeInTheDocument()
    expect(screen.queryByText(rawError)).not.toBeInTheDocument()
    expect(screen.queryByText(/mcp-secret-token/)).not.toBeInTheDocument()
  })

  it('deletes a configured server and refreshes the list', async () => {
    const listMcpServers = vi
      .fn()
      .mockResolvedValueOnce({
        servers: [
          {
            displayName: 'Workspace GitHub',
            exposedToolCount: 2,
            id: 'github',
            origin: 'workspace',
            scope: 'global',
            status: 'ready',
            transport: 'stdio',
          },
        ],
      })
      .mockResolvedValueOnce({ servers: [] })
    const deleteMcpServer = vi.fn().mockResolvedValue({ id: 'github', status: 'deleted' })
    const client = {
      ...createMockCommandClient(),
      deleteMcpServer,
      listMcpServers,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Delete Workspace GitHub' }))

    await waitFor(() => expect(deleteMcpServer).toHaveBeenCalledWith('github'))
    expect(await screen.findByText('No MCP servers configured.')).toBeInTheDocument()
  })
})
