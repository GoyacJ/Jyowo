import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { CommandClient, GetPluginDetailResponse, PluginSummary } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { PluginsManager } from './PluginsManager'

const pickPluginPackagePathMock = vi.hoisted(() => vi.fn())

vi.mock('@/shared/tauri/file-dialog', () => ({
  pickPluginPackagePath: pickPluginPackagePathMock,
}))

function pluginSummary(overrides: Partial<PluginSummary> = {}): PluginSummary {
  return {
    id: 'formatter@1.0.0',
    name: 'formatter',
    version: '1.0.0',
    description: 'Formats workspace files.',
    source: 'user',
    trustLevel: 'user_controlled',
    enabled: true,
    state: 'activated',
    capabilities: [
      {
        kind: 'tool',
        name: 'format_file',
        destructive: false,
        registered: true,
      },
    ],
    warnings: [],
    ...overrides,
  }
}

function pluginDetail(summary: PluginSummary = pluginSummary()): GetPluginDetailResponse {
  return {
    plugin: {
      summary,
      manifestOrigin: {
        file: {
          path: '/tmp/formatter-plugin/plugin.json',
        },
      },
      manifestHash: Array.from({ length: 32 }, () => 7),
      manifest: {
        name: 'formatter',
        version: '1.0.0',
      },
      configurationSchema: {
        type: 'object',
        properties: {
          lineWidth: {
            type: 'number',
          },
          formatOnSave: {
            type: 'boolean',
          },
          outputDirectory: {
            type: 'path',
          },
        },
      },
      config: {
        lineWidth: 100,
        formatOnSave: true,
        outputDirectory: 'workspace://formatted',
      },
      registeredCapabilities: summary.capabilities,
      recentEvents: ['loaded', 'deactivated'],
    },
  }
}

function renderPluginsManager(commandClient: CommandClient = createTestCommandClient()) {
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
      <PluginsManager />
    </Wrapper>,
  )
}

describe('PluginsManager', () => {
  beforeEach(() => {
    pickPluginPackagePathMock.mockReset()
  })

  it('renders the loading state while plugins load', () => {
    renderPluginsManager({
      ...createTestCommandClient(),
      listPlugins: vi.fn(() => new Promise<never>(() => undefined)),
    })

    expect(screen.getByText('Loading plugins.')).toBeInTheDocument()
  })

  it('renders a sanitized failure state when plugins cannot load', async () => {
    renderPluginsManager({
      ...createTestCommandClient(),
      listPlugins: vi.fn().mockRejectedValue(new Error('Authorization=Bearer plugin-secret-token')),
    })

    expect(await screen.findByText('Plugins could not be loaded.')).toBeInTheDocument()
    expect(screen.queryByText(/plugin-secret-token/)).not.toBeInTheDocument()
  })

  it('renders an empty support surface when no plugins are installed', async () => {
    renderPluginsManager(
      createTestCommandClient({ plugins: { allowProjectPlugins: false, plugins: [] } }),
    )

    expect(await screen.findByText('No plugins installed.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Install local plugin' })).toBeInTheDocument()
  })

  it('does not render project plugin discovery controls', async () => {
    renderPluginsManager(
      createTestCommandClient({ plugins: { allowProjectPlugins: true, plugins: [] } }),
    )

    expect(await screen.findByText('No plugins installed.')).toBeInTheDocument()
    expect(screen.queryByText('Project plugin discovery')).not.toBeInTheDocument()
    expect(screen.queryByRole('switch', { name: /project plugins/i })).not.toBeInTheDocument()
  })

  it('shows plugin state, trust, source, capabilities, details, and config fields', async () => {
    const summary = pluginSummary()
    renderPluginsManager(
      createTestCommandClient({
        pluginDetail: pluginDetail(summary),
        plugins: {
          allowProjectPlugins: false,
          plugins: [summary],
        },
      }),
    )

    const card = await screen.findByRole('article', { name: 'formatter' })

    expect(within(card).getByText('1.0.0')).toBeInTheDocument()
    expect(within(card).getByText('User')).toBeInTheDocument()
    expect(within(card).getByText('User controlled')).toBeInTheDocument()
    expect(within(card).getByText('Activated')).toBeInTheDocument()
    expect(within(card).getByText('tool: format_file')).toBeInTheDocument()

    fireEvent.click(within(card).getByRole('button', { name: 'View formatter details' }))

    expect(await screen.findByText('/tmp/formatter-plugin/plugin.json')).toBeInTheDocument()
    expect(screen.getByText('07'.repeat(32))).toBeInTheDocument()
    expect(screen.getByDisplayValue('100')).toBeInTheDocument()
    expect(screen.getByDisplayValue('workspace://formatted')).toBeInTheDocument()
    expect(screen.queryByText('apiToken')).not.toBeInTheDocument()
  })

  it('validates an install candidate before installing it', async () => {
    const installPluginFromPath = vi.fn().mockResolvedValue({
      pluginId: 'formatter@1.0.0',
      status: 'installed',
      summary: pluginSummary(),
    })
    const client = {
      ...createTestCommandClient({ plugins: { allowProjectPlugins: false, plugins: [] } }),
      installPluginFromPath,
      validatePluginFromPath: vi.fn().mockResolvedValue({
        sourcePath: '<local-plugin>',
        valid: true,
        summary: pluginSummary(),
        warnings: ['Registers one tool.'],
      }),
    }
    pickPluginPackagePathMock.mockResolvedValue('/tmp/formatter-plugin')

    renderPluginsManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Install local plugin' }))
    expect(await screen.findByText('Registers one tool.')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Confirm install' }))

    await waitFor(() => expect(installPluginFromPath).toHaveBeenCalledWith('/tmp/formatter-plugin'))
  })

  it('does not install a plugin when validation fails', async () => {
    const installPluginFromPath = vi.fn()
    const client = {
      ...createTestCommandClient({ plugins: { allowProjectPlugins: false, plugins: [] } }),
      installPluginFromPath,
      validatePluginFromPath: vi.fn().mockResolvedValue({
        sourcePath: '/tmp/bad-plugin',
        valid: false,
        warnings: [],
        reason: 'trust mismatch',
      }),
    }
    pickPluginPackagePathMock.mockResolvedValue('/tmp/bad-plugin')

    renderPluginsManager(client)

    fireEvent.click(await screen.findByRole('button', { name: 'Install local plugin' }))

    expect(await screen.findByText('trust mismatch')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Confirm install' })).not.toBeInTheDocument()
    expect(installPluginFromPath).not.toHaveBeenCalled()
  })

  it('toggles plugin enablement through the command client', async () => {
    const setPluginEnabled = vi.fn().mockResolvedValue({
      pluginId: 'formatter@1.0.0',
      status: 'disabled',
      summary: pluginSummary({
        enabled: false,
        state: { disabled: { last_state: 'activated' } },
      }),
    })
    renderPluginsManager({
      ...createTestCommandClient({
        plugins: {
          allowProjectPlugins: false,
          plugins: [pluginSummary()],
        },
      }),
      setPluginEnabled,
    })

    fireEvent.click(await screen.findByRole('switch', { name: 'Disable formatter' }))

    await waitFor(() => expect(setPluginEnabled).toHaveBeenCalledWith('formatter@1.0.0', false))
  })

  it('treats non-user plugins as read-only management entries', async () => {
    const summary = pluginSummary({
      source: 'workspace',
      trustLevel: 'admin_trusted',
    })
    renderPluginsManager(
      createTestCommandClient({
        pluginDetail: pluginDetail(summary),
        plugins: {
          allowProjectPlugins: false,
          plugins: [summary],
        },
      }),
    )

    const card = await screen.findByRole('article', { name: 'formatter' })

    expect(within(card).getByRole('button', { name: 'View formatter details' })).toBeInTheDocument()
    expect(
      within(card).queryByRole('switch', { name: 'Disable formatter' }),
    ).not.toBeInTheDocument()
    expect(within(card).queryByRole('button', { name: 'Reload formatter' })).not.toBeInTheDocument()
    expect(
      within(card).queryByRole('button', { name: 'Uninstall formatter' }),
    ).not.toBeInTheDocument()

    fireEvent.click(within(card).getByRole('button', { name: 'View formatter details' }))

    expect(await screen.findByDisplayValue('100')).toBeDisabled()
    expect(screen.queryByRole('button', { name: 'Save config' })).not.toBeInTheDocument()
  })

  it('submits supported config fields without secret values', async () => {
    const updatePluginConfig = vi.fn().mockResolvedValue({
      pluginId: 'formatter@1.0.0',
      status: 'configured',
      summary: pluginSummary(),
    })
    const summary = pluginSummary()
    renderPluginsManager({
      ...createTestCommandClient({
        pluginDetail: pluginDetail(summary),
        plugins: {
          allowProjectPlugins: false,
          plugins: [summary],
        },
      }),
      updatePluginConfig,
    })

    fireEvent.click(
      within(await screen.findByRole('article', { name: 'formatter' })).getByRole('button', {
        name: 'View formatter details',
      }),
    )
    fireEvent.change(await screen.findByLabelText('lineWidth'), { target: { value: '120' } })
    fireEvent.change(screen.getByLabelText('outputDirectory'), {
      target: { value: 'workspace://reports' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save config' }))

    await waitFor(() =>
      expect(updatePluginConfig).toHaveBeenCalledWith('formatter@1.0.0', {
        formatOnSave: true,
        lineWidth: 120,
        outputDirectory: 'workspace://reports',
      }),
    )
    expect(JSON.stringify(updatePluginConfig.mock.calls)).not.toContain('apiToken')
  })

  it('confirms uninstall before removing a plugin', async () => {
    const uninstallPlugin = vi.fn().mockResolvedValue({
      pluginId: 'formatter@1.0.0',
      status: 'uninstalled',
    })
    renderPluginsManager({
      ...createTestCommandClient({
        plugins: {
          allowProjectPlugins: false,
          plugins: [pluginSummary()],
        },
      }),
      uninstallPlugin,
    })

    const card = await screen.findByRole('article', { name: 'formatter' })
    fireEvent.click(within(card).getByRole('button', { name: 'Uninstall formatter' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Confirm uninstall' }))

    await waitFor(() => expect(uninstallPlugin).toHaveBeenCalledWith('formatter@1.0.0'))
  })
})
