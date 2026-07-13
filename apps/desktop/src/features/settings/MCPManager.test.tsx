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
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { MCPManager } from './MCPManager'

function renderMCPManager(
  commandClient: CommandClient = createTestCommandClient(),
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
    configLayer: 'global',
    displayName: 'Workspace GitHub',
    effective: true,
    enabled: true,
    exposedToolCount: 2,
    id: 'github',
    manageable: true,
    origin: 'user',
    overridesGlobal: false,
    required: false,
    scope: 'global',
    status: 'ready',
    statusSource: 'settings',
    transport: 'stdio',
    ...overrides,
  }
}

describe('MCPManager', () => {
  it('renders the loading state while MCP servers load', () => {
    renderMCPManager({
      ...createTestCommandClient({ mcpDiagnostics: { events: [] } }),
      listMcpServers: vi.fn(() => new Promise<never>(() => undefined)),
    })

    expect(screen.getByText('Loading MCP servers.')).toBeInTheDocument()
  })

  it('renders a sanitized failure state when MCP servers cannot load', async () => {
    renderMCPManager({
      ...createTestCommandClient({ mcpDiagnostics: { events: [] } }),
      listMcpServers: vi.fn().mockRejectedValue(new Error('Authorization=Bearer mcp-secret-token')),
    })

    expect(await screen.findByText('MCP servers could not be loaded.')).toBeInTheDocument()
    expect(screen.queryByText(/mcp-secret-token/)).not.toBeInTheDocument()
  })

  it('renders an empty support surface when no MCP servers are configured', async () => {
    renderMCPManager(
      createTestCommandClient({
        browserMcpPresets: { presets: [] },
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [] },
      }),
    )

    expect(await screen.findByText('No MCP servers configured.')).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: /mcp/i })).not.toBeInTheDocument()
  })

  it('shows global write controls when no project is active', async () => {
    renderMCPManager(
      createTestCommandClient({
        browserMcpPresets: { presets: [] },
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [] },
        projects: { activePath: null, projects: [] },
      }),
    )

    expect(await screen.findByRole('button', { name: 'Add server' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Global settings' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Project settings' })).toBeDisabled()
    expect(screen.getByText('Select a project to manage project overrides.')).toBeInTheDocument()
    expect(screen.queryByText('Project overrides')).not.toBeInTheDocument()
    expect(await screen.findByText('No MCP servers configured.')).toBeInTheDocument()
  })

  it('switches between global and active-project configuration views', async () => {
    const listMcpServers = vi.fn(async (configLayer: 'global' | 'project') => ({
      configLayer,
      servers:
        configLayer === 'global'
          ? [mcpServer()]
          : [
              mcpServer({
                configLayer: 'project',
                displayName: 'Project GitHub',
                id: 'project-github',
                origin: 'project',
              }),
            ],
    }))
    renderMCPManager({
      ...createTestCommandClient({ mcpDiagnostics: { events: [] } }),
      listMcpServers,
    })

    expect(await screen.findByRole('article', { name: 'Workspace GitHub' })).toBeInTheDocument()
    expect(listMcpServers).toHaveBeenCalledWith('global')

    fireEvent.click(screen.getByRole('button', { name: 'Project settings' }))

    expect(await screen.findByRole('article', { name: 'Project GitHub' })).toBeInTheDocument()
    expect(screen.queryByRole('article', { name: 'Workspace GitHub' })).not.toBeInTheDocument()
    expect(listMcpServers).toHaveBeenCalledWith('project')
  })

  it('does not present plugin servers as inherited global overrides', async () => {
    const pluginServer = mcpServer({
      displayName: 'Plugin Context',
      id: 'plugin-context',
      manageable: false,
      origin: 'plugin',
      sourcePluginId: 'formatter@1.0.0',
      transport: 'inProcess',
    })
    renderMCPManager({
      ...createTestCommandClient({ mcpDiagnostics: { events: [] } }),
      listMcpServers: vi.fn(async (configLayer: 'global' | 'project') => ({
        configLayer,
        servers: configLayer === 'project' ? [pluginServer] : [],
      })),
    })

    const projectSettings = await screen.findByRole('button', { name: 'Project settings' })
    await waitFor(() => expect(projectSettings).toBeEnabled())
    fireEvent.click(projectSettings)

    const card = await screen.findByRole('article', { name: 'Plugin Context' })
    expect(within(card).queryByText('Inherited global configuration')).not.toBeInTheDocument()
    expect(
      within(card).queryByRole('button', { name: 'Override Plugin Context' }),
    ).not.toBeInTheDocument()
  })

  it('copies an inherited global server into a project override with required retained', async () => {
    const inherited = mcpServer({ manageable: false, required: true })
    const getMcpServerConfig = vi.fn().mockResolvedValue({
      server: {
        configLayer: 'global',
        displayName: inherited.displayName,
        effective: true,
        enabled: true,
        id: inherited.id,
        manageable: false,
        overridesGlobal: false,
        required: true,
        scope: 'session',
        transport: {
          args: ['mcp-server'],
          command: 'node',
          env: [{ hasValue: true, key: 'LOG_LEVEL' }],
          inheritEnv: ['PATH'],
          kind: 'stdio',
        },
      },
    })
    const saveMcpServer = vi.fn().mockResolvedValue({
      server: mcpServer({
        configLayer: 'project',
        origin: 'project',
        overridesGlobal: true,
        required: true,
      }),
    })
    const client = {
      ...createTestCommandClient({ mcpDiagnostics: { events: [] } }),
      getMcpServerConfig,
      listMcpServers: vi.fn(async (configLayer: 'global' | 'project') => ({
        configLayer,
        servers: configLayer === 'project' ? [inherited] : [mcpServer()],
      })),
      saveMcpServer,
    }
    renderMCPManager(client)

    const projectSettings = await screen.findByRole('button', { name: 'Project settings' })
    await waitFor(() => expect(projectSettings).toBeEnabled())
    fireEvent.click(projectSettings)
    const card = await screen.findByRole('article', { name: 'Workspace GitHub' })
    fireEvent.click(within(card).getByRole('button', { name: 'Override Workspace GitHub' }))

    await waitFor(() => expect(getMcpServerConfig).toHaveBeenCalledWith('global', 'github'))
    expect(await screen.findByRole('checkbox', { name: 'Required for runs' })).toBeChecked()
    expect(screen.getByLabelText('Runtime scope')).toHaveValue('session')
    expect(screen.getByPlaceholderText('PATH')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Save MCP server' }))

    await waitFor(() =>
      expect(saveMcpServer).toHaveBeenCalledWith(
        expect.objectContaining({
          configLayer: 'project',
          id: 'github',
          required: true,
          scope: 'session',
          transport: expect.objectContaining({
            env: [{ key: 'LOG_LEVEL', preserveExisting: true }],
          }),
        }),
      ),
    )
  })

  it('deletes a project override and reveals the inherited global server', async () => {
    const projectOverride = mcpServer({
      configLayer: 'project',
      origin: 'project',
      overridesGlobal: true,
    })
    const inheritedGlobal = mcpServer({ manageable: false })
    const listMcpServers = vi
      .fn()
      .mockResolvedValueOnce({ configLayer: 'global', servers: [mcpServer()] })
      .mockResolvedValueOnce({ configLayer: 'project', servers: [projectOverride] })
      .mockResolvedValueOnce({ configLayer: 'project', servers: [inheritedGlobal] })
    const deleteMcpServer = vi.fn().mockResolvedValue({
      configLayer: 'project',
      id: 'github',
      status: 'deleted',
    })
    renderMCPManager({
      ...createTestCommandClient({ mcpDiagnostics: { events: [] } }),
      deleteMcpServer,
      listMcpServers,
    })

    const projectSettings = await screen.findByRole('button', { name: 'Project settings' })
    await waitFor(() => expect(projectSettings).toBeEnabled())
    fireEvent.click(projectSettings)
    fireEvent.click(
      await screen.findByRole('button', { name: 'Delete project override Workspace GitHub' }),
    )

    await waitFor(() => expect(deleteMcpServer).toHaveBeenCalledWith('project', 'github'))
    expect(await screen.findByText('Inherited global configuration')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Override Workspace GitHub' })).toBeInTheDocument()
  })

  it('shows server status, origin, tool count, scope, and transport', async () => {
    renderMCPManager(
      createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: {
          configLayer: 'global',
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

    expect(within(card).getByText('Settings check: ready')).toBeInTheDocument()
    expect(within(card).getByText('user')).toBeInTheDocument()
    expect(within(card).getByText('2 tools')).toBeInTheDocument()
    expect(within(card).getByText('Runtime scope: Global')).toBeInTheDocument()
    expect(within(card).getByText('Global configuration')).toBeInTheDocument()
    expect(within(card).getByText('stdio')).toBeInTheDocument()
    expect(within(card).getByText('Last diagnostic')).toBeInTheDocument()
    expect(within(card).getByText('MCP server connection recovered.')).toBeInTheDocument()
    expect(screen.getByText('Servers')).toBeInTheDocument()
    expect(screen.getByText('From plugins')).toBeInTheDocument()
    expect(
      within(await screen.findByRole('article', { name: 'Plugin Context' })).getByText('Read-only'),
    ).toBeInTheDocument()
  })

  it('shows exact browser preset versions when provided', async () => {
    renderMCPManager(
      createTestCommandClient({
        browserMcpPresets: {
          presets: [
            {
              description: 'Browser automation through Playwright MCP.',
              displayName: 'Playwright Browser',
              enabled: false,
              id: 'playwright',
              serverId: 'browser-playwright',
              version: '0.0.78',
            },
            {
              description: 'Browser inspection through Chrome DevTools MCP.',
              displayName: 'Chrome DevTools Browser',
              enabled: false,
              id: 'chrome-devtools',
              serverId: 'browser-chrome-devtools',
              version: '1.5.0',
            },
          ],
        },
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [] },
      }),
    )

    expect(await screen.findByText('0.0.78')).toBeInTheDocument()
    expect(screen.getByText('1.5.0')).toBeInTheDocument()
  })

  it('enables disabled browser MCP presets', async () => {
    const saveBrowserMcpPreset = vi.fn().mockResolvedValue({
      preset: {
        description: 'Browser automation through Playwright MCP.',
        displayName: 'Playwright Browser',
        enabled: true,
        id: 'playwright',
        serverId: 'browser-playwright',
      },
      server: mcpServer({
        displayName: 'Playwright Browser',
        enabled: true,
        id: 'browser-playwright',
        status: 'ready',
      }),
    })
    const client = {
      ...createTestCommandClient({
        browserMcpPresets: {
          presets: [
            {
              description: 'Browser automation through Playwright MCP.',
              displayName: 'Playwright Browser',
              enabled: false,
              id: 'playwright',
              serverId: 'browser-playwright',
            },
          ],
        },
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [] },
      }),
      saveBrowserMcpPreset,
    }

    renderMCPManager(client)

    expect(await screen.findByText('Browser presets')).toBeInTheDocument()
    fireEvent.click(await screen.findByRole('button', { name: 'Add Playwright Browser preset' }))

    await waitFor(() =>
      expect(saveBrowserMcpPreset).toHaveBeenCalledWith({
        enabled: true,
        presetId: 'playwright',
      }),
    )
    expect(JSON.stringify(saveBrowserMcpPreset.mock.calls)).not.toContain('token')
  })

  it('disables enabled browser MCP presets', async () => {
    const saveBrowserMcpPreset = vi.fn().mockResolvedValue({
      preset: {
        description: 'Browser automation through Playwright MCP.',
        displayName: 'Playwright Browser',
        enabled: false,
        id: 'playwright',
        serverId: 'browser-playwright',
      },
      server: mcpServer({
        displayName: 'Playwright Browser',
        enabled: false,
        id: 'browser-playwright',
        status: 'disabled',
      }),
    })
    const client = {
      ...createTestCommandClient({
        browserMcpPresets: {
          presets: [
            {
              description: 'Browser automation through Playwright MCP.',
              displayName: 'Playwright Browser',
              enabled: true,
              id: 'playwright',
              serverId: 'browser-playwright',
            },
          ],
        },
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [] },
      }),
      saveBrowserMcpPreset,
    }

    renderMCPManager(client)

    fireEvent.click(
      await screen.findByRole('button', { name: 'Disable Playwright Browser preset' }),
    )

    await waitFor(() =>
      expect(saveBrowserMcpPreset).toHaveBeenCalledWith({
        enabled: false,
        presetId: 'playwright',
      }),
    )
  })

  it('opens the source plugin for read-only MCP servers', async () => {
    const openPlugin = vi.fn()
    renderMCPManager(
      createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: {
          configLayer: 'global',
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
      ...createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [] },
      }),
      saveMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Add server' }))
    expect(screen.getByRole('checkbox', { name: 'Required for runs' })).not.toBeChecked()
    fireEvent.click(await screen.findByRole('button', { name: 'Save MCP server' }))

    expect(await screen.findByText('Server name is required.')).toBeInTheDocument()
    expect(screen.queryByLabelText('Server id')).not.toBeInTheDocument()
    expect(screen.getByText('Command is required.')).toBeInTheDocument()
    expect(saveMcpServer).not.toHaveBeenCalled()
  })

  it('renders a redacted connection failure without leaking raw backend details', async () => {
    const rawError =
      'spawn failed at /var/folders/run: npx --api-key ctx7sk-secret-token --token=sk_secret sk-proj-secret-token-1234567890'
    const client = {
      ...createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [] },
      }),
      saveMcpServer: vi.fn().mockRejectedValue(new Error(rawError)),
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Add server' }))
    fireEvent.change(await screen.findByLabelText('Server name'), {
      target: { value: 'Workspace GitHub' },
    })
    fireEvent.change(screen.getByLabelText('Command'), { target: { value: 'node' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add argument' }))
    fireEvent.change(screen.getByLabelText('Argument'), { target: { value: 'mcp-server' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save MCP server' }))

    expect(await screen.findByText(/spawn failed at <path>/)).toBeInTheDocument()
    expect(screen.getByText(/--api-key <redacted>/)).toBeInTheDocument()
    expect(screen.getByText(/--token=<redacted>/)).toBeInTheDocument()
    expect(screen.queryByText(rawError)).not.toBeInTheDocument()
    expect(screen.queryByText(/ctx7sk-secret-token/)).not.toBeInTheDocument()
    expect(screen.queryByText(/sk_secret/)).not.toBeInTheDocument()
    expect(screen.queryByText(/sk-proj-secret-token/)).not.toBeInTheDocument()
  })

  it('renders safe backend validation errors when saving a server fails', async () => {
    const client = {
      ...createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [] },
      }),
      saveMcpServer: vi.fn().mockRejectedValue({
        code: 'INVALID_PAYLOAD',
        message: 'transport.args must not contain empty values',
      }),
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Add server' }))
    fireEvent.change(await screen.findByLabelText('Server name'), {
      target: { value: 'Workspace GitHub' },
    })
    fireEvent.change(screen.getByLabelText('Command'), {
      target: { value: 'node' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Add argument' }))
    fireEvent.change(screen.getByLabelText('Argument'), {
      target: { value: 'mcp-server' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save MCP server' }))

    expect(
      await screen.findByText('transport.args must not contain empty values'),
    ).toBeInTheDocument()
    expect(screen.queryByText('MCP server could not be saved.')).not.toBeInTheDocument()
  })

  it('toggles a workspace-managed server', async () => {
    const setMcpServerEnabled = vi.fn().mockResolvedValue({ server: mcpServer({ enabled: false }) })
    const client = {
      ...createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [mcpServer()] },
      }),
      setMcpServerEnabled,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('switch', { name: 'Disable Workspace GitHub' }))

    await waitFor(() => expect(setMcpServerEnabled).toHaveBeenCalledWith('global', 'github', false))
  })

  it('restarts a workspace-managed server', async () => {
    const restartMcpServer = vi.fn().mockResolvedValue({ server: mcpServer() })
    const client = {
      ...createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [mcpServer()] },
      }),
      restartMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Restart Workspace GitHub' }))

    await waitFor(() => expect(restartMcpServer).toHaveBeenCalledWith('global', 'github'))
  })

  it('deletes a configured server and refreshes the list', async () => {
    const listMcpServers = vi
      .fn()
      .mockResolvedValueOnce({
        configLayer: 'global',
        servers: [mcpServer()],
      })
      .mockResolvedValueOnce({ configLayer: 'global', servers: [] })
    const deleteMcpServer = vi
      .fn()
      .mockResolvedValue({ configLayer: 'global', id: 'github', status: 'deleted' })
    const client = {
      ...createTestCommandClient({ mcpDiagnostics: { events: [] } }),
      deleteMcpServer,
      listMcpServers,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Delete Workspace GitHub' }))

    await waitFor(() => expect(deleteMcpServer).toHaveBeenCalledWith('global', 'github'))
    expect(await screen.findByText('No MCP servers configured.')).toBeInTheDocument()
  })

  it('submits a stdio server payload from the dialog', async () => {
    const saveMcpServer = vi.fn().mockResolvedValue({ server: mcpServer({ status: 'configured' }) })
    const client = {
      ...createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [] },
      }),
      saveMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Add server' }))
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
      target: { value: 'PATH' },
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
        configLayer: 'global',
        displayName: 'Workspace GitHub',
        enabled: true,
        id: 'workspace-github',
        required: false,
        scope: 'global',
        transport: {
          args: ['mcp-server', '--stdio'],
          command: 'node',
          env: [{ key: 'LOG_LEVEL', value: 'info' }],
          inheritEnv: ['PATH'],
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
      ...createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [] },
      }),
      saveMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Add server' }))
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
        configLayer: 'global',
        displayName: 'Remote Context',
        enabled: true,
        id: 'remote-context',
        required: false,
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
        configLayer: 'global',
        displayName: 'Workspace GitHub',
        effective: true,
        enabled: true,
        id: 'github',
        manageable: true,
        overridesGlobal: false,
        required: false,
        scope: 'global',
        transport: {
          args: ['mcp-server'],
          command: 'node',
          env: [{ hasValue: true, key: 'LOG_LEVEL' }],
          inheritEnv: ['PATH'],
          kind: 'stdio',
          workingDir: '.',
        },
      },
    })
    const saveMcpServer = vi.fn().mockResolvedValue({ server: mcpServer({ status: 'configured' }) })
    const client = {
      ...createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [mcpServer()] },
      }),
      getMcpServerConfig,
      saveMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Configure Workspace GitHub' }))

    await waitFor(() => expect(getMcpServerConfig).toHaveBeenCalledWith('global', 'github'))
    expect(await screen.findByDisplayValue('mcp-server')).toBeInTheDocument()
    expect(screen.getByDisplayValue('LOG_LEVEL')).toBeInTheDocument()
    expect(screen.queryByDisplayValue('info')).not.toBeInTheDocument()
    expect(screen.getByDisplayValue('PATH')).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Inline env value'), { target: { value: 'info' } })

    fireEvent.click(screen.getByRole('button', { name: 'Save MCP server' }))

    await waitFor(() =>
      expect(saveMcpServer).toHaveBeenCalledWith(
        expect.objectContaining({
          id: 'github',
          transport: expect.objectContaining({
            args: ['mcp-server'],
            env: [{ key: 'LOG_LEVEL', value: 'info' }],
            inheritEnv: ['PATH'],
          }),
        }),
      ),
    )
  })

  it('preserves redacted inline env values when saving unchanged config details', async () => {
    const getMcpServerConfig = vi.fn().mockResolvedValue({
      server: {
        configLayer: 'global',
        displayName: 'Workspace GitHub',
        effective: true,
        enabled: true,
        id: 'github',
        manageable: true,
        overridesGlobal: false,
        required: true,
        scope: 'global',
        transport: {
          args: ['mcp-server'],
          command: 'node',
          env: [{ hasValue: true, key: 'LOG_LEVEL' }],
          inheritEnv: [],
          kind: 'stdio',
        },
      },
    })
    const saveMcpServer = vi.fn().mockResolvedValue({ server: mcpServer({ status: 'configured' }) })
    const client = {
      ...createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [mcpServer()] },
      }),
      getMcpServerConfig,
      saveMcpServer,
    }

    renderMCPManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Configure Workspace GitHub' }))
    await waitFor(() => expect(getMcpServerConfig).toHaveBeenCalledWith('global', 'github'))
    expect(screen.getByDisplayValue('LOG_LEVEL')).toBeInTheDocument()
    expect(screen.queryByDisplayValue('info')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Save MCP server' }))

    await waitFor(() =>
      expect(saveMcpServer).toHaveBeenCalledWith(
        expect.objectContaining({
          id: 'github',
          configLayer: 'global',
          required: true,
          transport: expect.objectContaining({
            env: [{ key: 'LOG_LEVEL', preserveExisting: true }],
          }),
        }),
      ),
    )
    expect(JSON.stringify(saveMcpServer.mock.calls)).not.toContain('info')
  })

  it('renders live diagnostics without leaking raw payload details', async () => {
    let emitBatch: ((batch: McpDiagnosticBatchPayload) => void) | undefined
    const client = {
      ...createTestCommandClient({
        mcpDiagnostics: { events: [] },
        mcpServers: { configLayer: 'global', servers: [mcpServer()] },
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
            plane: 'settings',
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

  it('filters diagnostics by settings and task planes', async () => {
    renderMCPManager(
      createTestCommandClient({
        mcpDiagnostics: {
          events: [
            {
              eventType: 'connection_lost',
              id: 'settings-event',
              plane: 'settings',
              serverId: 'github',
              severity: 'warning',
              summary: 'Settings probe failed.',
              timestamp: '2026-06-17T00:00:00.000Z',
            },
            {
              eventType: 'activation_failed',
              id: 'task-event',
              plane: 'task',
              runId: 'run-1',
              serverId: 'github',
              severity: 'error',
              summary: 'Task activation failed.',
              taskId: 'task-1',
              timestamp: '2026-06-17T00:00:01.000Z',
            },
          ],
        },
        mcpServers: { configLayer: 'global', servers: [mcpServer()] },
      }),
    )

    expect(await screen.findByText('Settings probe failed.')).toBeInTheDocument()
    expect(screen.getByText('Task activation failed.')).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Filter diagnostics by source'), {
      target: { value: 'task' },
    })

    expect(screen.queryByText('Settings probe failed.')).not.toBeInTheDocument()
    expect(screen.getByText('Task activation failed.')).toBeInTheDocument()
    expect(screen.getAllByText('Task runtime')).toHaveLength(2)
  })
})
