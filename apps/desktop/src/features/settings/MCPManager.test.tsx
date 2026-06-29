import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type {
  CommandClient,
  McpDiagnosticBatchPayload,
  McpServerSummary,
} from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { MCPManager } from './MCPManager'

function renderMCPManager(
  commandClient: CommandClient = createMockCommandClient(),
  onOpenPlugin?: (pluginId: string) => void,
) {
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
      <MCPManager onOpenPlugin={onOpenPlugin} />
    </Wrapper>,
  )
}

function mcpServer(overrides: Partial<McpServerSummary> = {}): McpServerSummary {
  return {
    displayName: 'Workspace GitHub',
    enabled: true,
    exposedToolCount: 2,
    id: 'github',
    manageable: true,
    origin: 'workspace',
    scope: 'global',
    status: 'ready',
    transport: 'stdio',
    ...overrides,
  }
}

describe('MCPManager', () => {
  it('renders the loading state while MCP servers load', () => {
    renderMCPManager({
      ...createMockCommandClient({ mcpDiagnostics: { events: [] } }),
      listMcpServers: vi.fn(() => new Promise<never>(() => undefined)),
    })

    expect(screen.getByText('Loading MCP servers.')).toBeInTheDocument()
  })

  it('renders a sanitized failure state when MCP servers cannot load', async () => {
    renderMCPManager({
      ...createMockCommandClient({ mcpDiagnostics: { events: [] } }),
      listMcpServers: vi.fn().mockRejectedValue(new Error('Authorization=Bearer mcp-secret-token')),
    })

    expect(await screen.findByText('MCP servers could not be loaded.')).toBeInTheDocument()
    expect(screen.queryByText(/mcp-secret-token/)).not.toBeInTheDocument()
  })

  it('renders an empty support surface when no MCP servers are configured', async () => {
    renderMCPManager(
      createMockCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { servers: [] },
      }),
    )

    expect(await screen.findByText('No MCP servers configured.')).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: /mcp/i })).not.toBeInTheDocument()
  })

  it('shows server status, origin, tool count, scope, and transport', async () => {
    renderMCPManager(
      createMockCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: {
          servers: [
            mcpServer({
              lastDiagnostic: 'MCP server connection recovered.',
              lastDiagnosticAt: '2026-06-17T00:00:00.000Z',
              lastDiagnosticSeverity: 'info',
            }),
            mcpServer({
              displayName: 'Plugin Context',
              enabled: true,
              exposedToolCount: 1,
              id: 'plugin-context',
              manageable: false,
              origin: 'plugin',
              scope: 'session',
              sourcePluginId: 'formatter@1.0.0',
              status: 'ready',
              transport: 'http',
            }),
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
    expect(within(card).getByText('Last diagnostic')).toBeInTheDocument()
    expect(within(card).getByText('MCP server connection recovered.')).toBeInTheDocument()
    expect(screen.getByText('Servers')).toBeInTheDocument()
    expect(screen.getByText('From plugins')).toBeInTheDocument()
    expect(
      within(await screen.findByRole('article', { name: 'Plugin Context' })).getByText('Read-only'),
    ).toBeInTheDocument()
  })

  it('opens the source plugin for read-only MCP servers', async () => {
    const openPlugin = vi.fn()
    renderMCPManager(
      createMockCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: {
          servers: [
            mcpServer({
              displayName: 'Plugin Context',
              id: 'plugin-context',
              manageable: false,
              origin: 'plugin',
              sourcePluginId: 'formatter@1.0.0',
            }),
          ],
        },
      }),
      openPlugin,
    )

    const card = await screen.findByRole('article', { name: 'Plugin Context' })
    fireEvent.click(
      within(card).getByRole('button', { name: 'View source plugin formatter@1.0.0' }),
    )

    expect(openPlugin).toHaveBeenCalledWith('formatter@1.0.0')
  })

  it('rejects invalid config before calling the backend', async () => {
    const saveMcpServer = vi.fn()
    const client = {
      ...createMockCommandClient({ mcpDiagnostics: { events: [] }, mcpServers: { servers: [] } }),
      saveMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(screen.getByRole('button', { name: 'Add server' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Save MCP server' }))

    expect(await screen.findByText('Server name is required.')).toBeInTheDocument()
    expect(screen.queryByLabelText('Server id')).not.toBeInTheDocument()
    expect(screen.getByText('Command is required.')).toBeInTheDocument()
    expect(saveMcpServer).not.toHaveBeenCalled()
  })

  it('renders a sanitized connection failure without leaking raw backend details', async () => {
    const rawError = 'spawn failed: Authorization=Bearer mcp-secret-token'
    const client = {
      ...createMockCommandClient({ mcpDiagnostics: { events: [] }, mcpServers: { servers: [] } }),
      saveMcpServer: vi.fn().mockRejectedValue(new Error(rawError)),
    }

    renderMCPManager(client)

    fireEvent.click(screen.getByRole('button', { name: 'Add server' }))
    fireEvent.change(await screen.findByLabelText('Server name'), {
      target: { value: 'Workspace GitHub' },
    })
    fireEvent.change(screen.getByLabelText('Command'), { target: { value: 'node' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add argument' }))
    fireEvent.change(screen.getByLabelText('Argument'), { target: { value: 'mcp-server' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save MCP server' }))

    expect(await screen.findByText('MCP server could not be saved.')).toBeInTheDocument()
    expect(screen.queryByText(rawError)).not.toBeInTheDocument()
    expect(screen.queryByText(/mcp-secret-token/)).not.toBeInTheDocument()
  })

  it('toggles a workspace-managed server', async () => {
    const setMcpServerEnabled = vi.fn().mockResolvedValue({ server: mcpServer({ enabled: false }) })
    const client = {
      ...createMockCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { servers: [mcpServer()] },
      }),
      setMcpServerEnabled,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('switch', { name: 'Disable Workspace GitHub' }))

    await waitFor(() => expect(setMcpServerEnabled).toHaveBeenCalledWith('github', false))
  })

  it('restarts a workspace-managed server', async () => {
    const restartMcpServer = vi.fn().mockResolvedValue({ server: mcpServer() })
    const client = {
      ...createMockCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { servers: [mcpServer()] },
      }),
      restartMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Restart Workspace GitHub' }))

    await waitFor(() => expect(restartMcpServer).toHaveBeenCalledWith('github'))
  })

  it('deletes a configured server and refreshes the list', async () => {
    const listMcpServers = vi
      .fn()
      .mockResolvedValueOnce({
        servers: [mcpServer()],
      })
      .mockResolvedValueOnce({ servers: [] })
    const deleteMcpServer = vi.fn().mockResolvedValue({ id: 'github', status: 'deleted' })
    const client = {
      ...createMockCommandClient({ mcpDiagnostics: { events: [] } }),
      deleteMcpServer,
      listMcpServers,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Delete Workspace GitHub' }))

    await waitFor(() => expect(deleteMcpServer).toHaveBeenCalledWith('github'))
    expect(await screen.findByText('No MCP servers configured.')).toBeInTheDocument()
  })

  it('submits a stdio server payload from the dialog', async () => {
    const saveMcpServer = vi.fn().mockResolvedValue({ server: mcpServer({ status: 'configured' }) })
    const client = {
      ...createMockCommandClient({ mcpDiagnostics: { events: [] }, mcpServers: { servers: [] } }),
      saveMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(screen.getByRole('button', { name: 'Add server' }))
    fireEvent.change(await screen.findByLabelText('Server name'), {
      target: { value: 'Workspace GitHub' },
    })
    fireEvent.change(screen.getByLabelText('Command'), { target: { value: 'node' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add argument' }))
    fireEvent.change(screen.getByLabelText('Argument'), { target: { value: 'mcp-server' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add argument' }))
    fireEvent.change(screen.getAllByLabelText('Argument')[1], { target: { value: '--stdio' } })
    fireEvent.click(screen.getAllByRole('button', { name: 'Remove argument' })[1])
    fireEvent.click(screen.getByRole('button', { name: 'Add argument' }))
    fireEvent.change(screen.getAllByLabelText('Argument')[1], { target: { value: '--stdio' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add inherited env var' }))
    fireEvent.change(screen.getByLabelText('Inherited env var'), {
      target: { value: 'GITHUB_TOKEN' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Add inline env var' }))
    fireEvent.change(screen.getByLabelText('Inline env name'), { target: { value: 'LOG_LEVEL' } })
    fireEvent.change(screen.getByLabelText('Inline env value'), { target: { value: 'info' } })
    fireEvent.change(screen.getByLabelText('Working directory'), {
      target: { value: '.' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save MCP server' }))

    await waitFor(() =>
      expect(saveMcpServer).toHaveBeenCalledWith({
        displayName: 'Workspace GitHub',
        enabled: true,
        id: 'workspace-github',
        scope: 'global',
        transport: {
          args: ['mcp-server', '--stdio'],
          command: 'node',
          env: [{ key: 'LOG_LEVEL', value: 'info' }],
          inheritEnv: ['GITHUB_TOKEN'],
          kind: 'stdio',
          workingDir: '.',
        },
      }),
    )
  })

  it('submits an HTTP server payload without raw bearer tokens', async () => {
    const saveMcpServer = vi
      .fn()
      .mockResolvedValue({ server: mcpServer({ id: 'remote-context', transport: 'http' }) })
    const client = {
      ...createMockCommandClient({ mcpDiagnostics: { events: [] }, mcpServers: { servers: [] } }),
      saveMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(screen.getByRole('button', { name: 'Add server' }))
    fireEvent.change(await screen.findByLabelText('Server name'), {
      target: { value: 'Remote Context' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'HTTP' }))
    fireEvent.change(screen.getByLabelText('URL'), {
      target: { value: 'https://mcp.example.com/mcp' },
    })
    fireEvent.change(screen.getByLabelText('Bearer token env var'), {
      target: { value: 'MCP_BEARER_TOKEN' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Add header' }))
    fireEvent.change(screen.getByLabelText('Header name'), { target: { value: 'X-Workspace' } })
    fireEvent.change(screen.getByLabelText('Header value'), { target: { value: 'jyowo' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add header from env var' }))
    fireEvent.change(screen.getByLabelText('Env header name'), { target: { value: 'X-Api-Key' } })
    fireEvent.change(screen.getByLabelText('Env header variable'), {
      target: { value: 'MCP_CONTEXT7_TOKEN' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save MCP server' }))

    await waitFor(() =>
      expect(saveMcpServer).toHaveBeenCalledWith({
        displayName: 'Remote Context',
        enabled: true,
        id: 'remote-context',
        scope: 'global',
        transport: {
          bearerTokenEnvVar: 'MCP_BEARER_TOKEN',
          headers: [{ key: 'X-Workspace', value: 'jyowo' }],
          headersFromEnv: [{ key: 'X-Api-Key', envVar: 'MCP_CONTEXT7_TOKEN' }],
          kind: 'http',
          url: 'https://mcp.example.com/mcp',
        },
      }),
    )
    expect(JSON.stringify(saveMcpServer.mock.calls)).not.toContain('mcp-secret-token')
  })

  it('loads editable config details before configuring a server', async () => {
    const getMcpServerConfig = vi.fn().mockResolvedValue({
      server: {
        displayName: 'Workspace GitHub',
        enabled: true,
        id: 'github',
        scope: 'global',
        transport: {
          args: ['mcp-server'],
          command: 'node',
          env: [{ key: 'LOG_LEVEL', value: 'info' }],
          inheritEnv: ['GITHUB_TOKEN'],
          kind: 'stdio',
          workingDir: '.',
        },
      },
    })
    const saveMcpServer = vi.fn().mockResolvedValue({ server: mcpServer({ status: 'configured' }) })
    const client = {
      ...createMockCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { servers: [mcpServer()] },
      }),
      getMcpServerConfig,
      saveMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Configure Workspace GitHub' }))

    await waitFor(() => expect(getMcpServerConfig).toHaveBeenCalledWith('github'))
    expect(await screen.findByDisplayValue('mcp-server')).toBeInTheDocument()
    expect(screen.getByDisplayValue('LOG_LEVEL')).toBeInTheDocument()
    expect(screen.getByDisplayValue('GITHUB_TOKEN')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Save MCP server' }))

    await waitFor(() =>
      expect(saveMcpServer).toHaveBeenCalledWith(
        expect.objectContaining({
          id: 'github',
          transport: expect.objectContaining({
            args: ['mcp-server'],
            env: [{ key: 'LOG_LEVEL', value: 'info' }],
            inheritEnv: ['GITHUB_TOKEN'],
          }),
        }),
      ),
    )
  })

  it('renders live diagnostics without leaking raw payload details', async () => {
    let emitBatch: ((batch: McpDiagnosticBatchPayload) => void) | undefined
    const client = {
      ...createMockCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { servers: [mcpServer()] },
        subscribeMcpDiagnostics: {
          replayEvents: [],
          subscriptionId: 'mcp-diagnostic-subscription-001',
        },
      }),
      listenMcpDiagnosticBatches: vi.fn(
        async (onBatch: (batch: McpDiagnosticBatchPayload) => void) => {
          emitBatch = onBatch
          return () => undefined
        },
      ),
    }

    renderMCPManager(client)

    await waitFor(() => expect(client.listenMcpDiagnosticBatches).toHaveBeenCalled())
    act(() => {
      emitBatch?.({
        events: [
          {
            eventType: 'oauth_refresh',
            id: 'mcp-diagnostic-002',
            serverId: 'github',
            severity: 'info',
            summary: 'OAuth refresh completed.',
            timestamp: '2026-06-17T00:00:00.000Z',
          },
        ],
        phase: 'live',
        subscriptionId: 'mcp-diagnostic-subscription-001',
      })
    })

    expect(await screen.findByText('OAuth refresh completed.')).toBeInTheDocument()
    expect(screen.getByText('OAuth refresh')).toBeInTheDocument()
    expect(screen.queryByText(/mcp-secret-token/)).not.toBeInTheDocument()
  })
})
