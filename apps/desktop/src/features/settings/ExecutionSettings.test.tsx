import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { AgentCapabilities, CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { ExecutionSettings } from './ExecutionSettings'

function createAgentCapabilities(overrides: Partial<AgentCapabilities> = {}): AgentCapabilities {
  return {
    agentTeamsAvailable: false,
    agentTeamsEnabled: false,
    backgroundAgentsAvailable: false,
    backgroundAgentsEnabled: false,
    subagentsAvailable: false,
    subagentsEnabled: false,
    unavailableReasons: [],
    ...overrides,
  }
}

const agentCapabilities = createAgentCapabilities()

function createExecutionSettings(
  agentCapabilityOverrides: Partial<ReturnType<typeof createAgentCapabilities>> = {},
) {
  return {
    agentCapabilities: createAgentCapabilities(agentCapabilityOverrides),
    autoModeAvailable: false,
    contextCompressionTriggerRatio: 0.8,
    permissionMode: 'default' as const,
    scope: 'global' as const,
    toolProfile: 'full' as const,
  }
}

const availableAgentCapabilities = createAgentCapabilities({
  agentTeamsAvailable: true,
  backgroundAgentsAvailable: true,
  backgroundAgentsEnabled: false,
  subagentsAvailable: true,
})

function renderExecutionSettings(commandClient: CommandClient = createTestCommandClient()) {
  return render(
    <CommandClientProvider client={commandClient}>
      <AppI18nProvider>
        <ExecutionSettings />
      </AppI18nProvider>
    </CommandClientProvider>,
  )
}

describe('ExecutionSettings', () => {
  afterEach(() => {
    uiStore.getState().setLocale('zh-CN')
  })

  it('loads and saves permission mode', async () => {
    const setExecutionSettings = vi.fn(async () => ({
      agentCapabilities,
      autoModeAvailable: false,
      contextCompressionTriggerRatio: 0.8,
      permissionMode: 'bypass_permissions' as const,
      scope: 'global' as const,
      toolProfile: 'full' as const,
    }))
    const commandClient = {
      ...createTestCommandClient(),
      getExecutionSettings: async () => ({
        agentCapabilities,
        autoModeAvailable: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default' as const,
        scope: 'global' as const,
        toolProfile: 'full' as const,
      }),
      setExecutionSettings,
    } satisfies CommandClient

    renderExecutionSettings(commandClient)

    expect(await screen.findByLabelText(/标准/i)).toBeChecked()

    fireEvent.click(screen.getByLabelText(/绕过/i))

    await waitFor(() => {
      expect(setExecutionSettings).toHaveBeenCalledWith({
        agentTeamsEnabled: false,
        backgroundAgentsEnabled: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'bypass_permissions',
        subagentsEnabled: false,
        toolProfile: 'full',
      })
    })
    expect(screen.queryByText('默认授权模式已保存。')).not.toBeInTheDocument()
  })

  it('loads and saves context compression trigger ratio', async () => {
    const setExecutionSettings = vi.fn(async () => ({
      agentCapabilities,
      autoModeAvailable: false,
      contextCompressionTriggerRatio: 0.75,
      permissionMode: 'default' as const,
      scope: 'global' as const,
      toolProfile: 'full' as const,
    }))
    const commandClient = {
      ...createTestCommandClient(),
      getExecutionSettings: async () => ({
        agentCapabilities,
        autoModeAvailable: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default' as const,
        scope: 'global' as const,
        toolProfile: 'full' as const,
      }),
      setExecutionSettings,
    } satisfies CommandClient

    renderExecutionSettings(commandClient)

    const ratioInput = await screen.findByLabelText(/上下文压缩触发比例/i)
    expect(ratioInput).toHaveValue(80)

    fireEvent.change(ratioInput, { target: { value: '75' } })
    fireEvent.click(screen.getByRole('button', { name: /保存默认模式/i }))

    await waitFor(() => {
      expect(setExecutionSettings).toHaveBeenCalledWith({
        agentTeamsEnabled: false,
        backgroundAgentsEnabled: false,
        contextCompressionTriggerRatio: 0.75,
        permissionMode: 'default',
        subagentsEnabled: false,
        toolProfile: 'full',
      })
    })
  })

  it('loads and saves tool profile', async () => {
    uiStore.getState().setLocale('en-US')

    const setExecutionSettings = vi.fn(async () => ({
      agentCapabilities,
      autoModeAvailable: false,
      contextCompressionTriggerRatio: 0.8,
      permissionMode: 'default' as const,
      scope: 'global' as const,
      toolProfile: 'minimal' as const,
    }))
    const commandClient = {
      ...createTestCommandClient(),
      getExecutionSettings: async () => ({
        agentCapabilities,
        autoModeAvailable: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default' as const,
        scope: 'global' as const,
        toolProfile: 'full' as const,
      }),
      setExecutionSettings,
    } satisfies CommandClient

    renderExecutionSettings(commandClient)

    expect(await screen.findByLabelText(/Full/i)).toBeChecked()

    fireEvent.click(screen.getByLabelText(/Minimal/i))

    await waitFor(() => {
      expect(setExecutionSettings).toHaveBeenCalledWith({
        agentTeamsEnabled: false,
        backgroundAgentsEnabled: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default',
        subagentsEnabled: false,
        toolProfile: 'minimal',
      })
    })
  })

  it('disables auto mode when unavailable', async () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createTestCommandClient({
        executionSettings: {
          agentCapabilities,
          autoModeAvailable: false,
          contextCompressionTriggerRatio: 0.8,
          permissionMode: 'default',
          scope: 'global',
          toolProfile: 'full',
        },
      }),
    )

    expect(await screen.findByText(/Auto approval is unavailable/i)).toBeInTheDocument()
    expect(screen.getByDisplayValue('auto')).toBeDisabled()
  })

  it('shows a loading state before execution settings arrive', () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createTestCommandClient({
        delayMs: 50,
        executionSettings: createExecutionSettings(availableAgentCapabilities),
      }),
    )

    expect(screen.getByText('Loading default permission mode…')).toBeInTheDocument()
  })

  it('keeps settings labeled as global even if a stale project scope is returned', async () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createTestCommandClient({
        executionSettings: {
          ...createExecutionSettings(availableAgentCapabilities),
          scope: 'project',
        },
      }),
    )

    expect(await screen.findByText('Global defaults')).toBeInTheDocument()
    expect(screen.queryByText('Project overrides')).not.toBeInTheDocument()
  })

  it('renders available agent switches off with dependents disabled', async () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createTestCommandClient({
        executionSettings: createExecutionSettings(availableAgentCapabilities),
      }),
    )

    expect(await screen.findByRole('switch', { name: 'Subagents' })).toBeEnabled()
    expect(screen.getByRole('switch', { name: 'Subagents' })).not.toBeChecked()
    expect(screen.getByRole('switch', { name: 'Agent teams' })).toBeDisabled()
    expect(screen.getByRole('switch', { name: 'Agent teams' })).not.toBeChecked()
    expect(screen.getByRole('switch', { name: 'Background agents' })).toBeDisabled()
    expect(screen.getByRole('switch', { name: 'Background agents' })).not.toBeChecked()
  })

  it('renders available agent switches on', async () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createTestCommandClient({
        executionSettings: createExecutionSettings({
          ...availableAgentCapabilities,
          agentTeamsEnabled: true,
          backgroundAgentsEnabled: true,
          subagentsEnabled: true,
        }),
      }),
    )

    expect(await screen.findByRole('switch', { name: 'Subagents' })).toBeChecked()
    expect(screen.getByRole('switch', { name: 'Agent teams' })).toBeChecked()
    expect(screen.getByRole('switch', { name: 'Background agents' })).toBeChecked()
  })

  it('disables unavailable agent switches and renders backend reasons', async () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createTestCommandClient({
        executionSettings: createExecutionSettings({
          unavailableReasons: [
            {
              capability: 'subagents',
              message: 'runtime store closed',
              type: 'runtimeStoreUnavailable',
            },
          ],
        }),
      }),
    )

    expect(await screen.findByRole('switch', { name: 'Subagents' })).toBeDisabled()
    expect(screen.getByText('Runtime store unavailable: runtime store closed')).toBeInTheDocument()
  })

  it('saves agent switch changes through setExecutionSettings', async () => {
    uiStore.getState().setLocale('en-US')

    const setExecutionSettings = vi.fn(async () =>
      createExecutionSettings({
        ...availableAgentCapabilities,
        subagentsEnabled: true,
      }),
    )
    const commandClient = {
      ...createTestCommandClient(),
      getExecutionSettings: async () => createExecutionSettings(availableAgentCapabilities),
      setExecutionSettings,
    } satisfies CommandClient

    renderExecutionSettings(commandClient)

    fireEvent.click(await screen.findByRole('switch', { name: 'Subagents' }))

    await waitFor(() => {
      expect(setExecutionSettings).toHaveBeenCalledWith({
        agentTeamsEnabled: false,
        backgroundAgentsEnabled: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default',
        subagentsEnabled: true,
        toolProfile: 'full',
      })
    })
    expect(screen.getByRole('switch', { name: 'Subagents' })).toBeChecked()
  })

  it('disabling subagents atomically disables dependent capabilities', async () => {
    uiStore.getState().setLocale('en-US')

    const enabledCapabilities = createAgentCapabilities({
      ...availableAgentCapabilities,
      agentTeamsEnabled: true,
      backgroundAgentsEnabled: true,
      subagentsEnabled: true,
    })
    const setExecutionSettings = vi.fn(async () =>
      createExecutionSettings({
        ...availableAgentCapabilities,
        agentTeamsEnabled: false,
        backgroundAgentsEnabled: false,
        subagentsEnabled: false,
      }),
    )
    const commandClient = {
      ...createTestCommandClient(),
      getExecutionSettings: async () => createExecutionSettings(enabledCapabilities),
      setExecutionSettings,
    } satisfies CommandClient

    renderExecutionSettings(commandClient)
    fireEvent.click(await screen.findByRole('switch', { name: 'Subagents' }))

    await waitFor(() => {
      expect(setExecutionSettings).toHaveBeenCalledWith({
        agentTeamsEnabled: false,
        backgroundAgentsEnabled: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default',
        subagentsEnabled: false,
        toolProfile: 'full',
      })
    })
  })

  it('keeps dependent capability switches disabled until subagents are enabled', async () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createTestCommandClient({
        executionSettings: createExecutionSettings(availableAgentCapabilities),
      }),
    )

    expect(await screen.findByRole('switch', { name: 'Subagents' })).toBeEnabled()
    expect(screen.getByRole('switch', { name: 'Agent teams' })).toBeDisabled()
    expect(screen.getByRole('switch', { name: 'Background agents' })).toBeDisabled()
  })

  it('refetches backend truth and shows a safe error when saving agent switches fails', async () => {
    uiStore.getState().setLocale('en-US')

    const getExecutionSettings = vi.fn(async () =>
      createExecutionSettings(availableAgentCapabilities),
    )
    const commandClient = {
      ...createTestCommandClient(),
      getExecutionSettings,
      setExecutionSettings: vi.fn(async () => {
        throw new Error('write denied')
      }),
    } satisfies CommandClient

    renderExecutionSettings(commandClient)

    fireEvent.click(await screen.findByRole('switch', { name: 'Subagents' }))

    await waitFor(() => {
      expect(getExecutionSettings).toHaveBeenCalledTimes(2)
    })
    expect(screen.getByText('Execution settings could not be saved.')).toBeInTheDocument()
    expect(document.body.textContent).not.toContain('write denied')
    expect(screen.getByRole('switch', { name: 'Subagents' })).not.toBeChecked()
  })

  it('restores the previous switch state when save and backend refetch both fail', async () => {
    uiStore.getState().setLocale('en-US')

    const getExecutionSettings = vi
      .fn()
      .mockResolvedValueOnce(createExecutionSettings(availableAgentCapabilities))
      .mockRejectedValueOnce(new Error('/Users/goya/.ssh/id_ed25519 leaked path'))
    const commandClient = {
      ...createTestCommandClient(),
      getExecutionSettings,
      setExecutionSettings: vi.fn(async () => {
        throw new Error('/Users/goya/.ssh/id_ed25519 leaked path')
      }),
    } satisfies CommandClient

    renderExecutionSettings(commandClient)

    fireEvent.click(await screen.findByRole('switch', { name: 'Subagents' }))

    await waitFor(() => {
      expect(getExecutionSettings).toHaveBeenCalledTimes(2)
    })
    expect(screen.getByText('Execution settings could not be saved.')).toBeInTheDocument()
    expect(document.body.textContent).not.toContain('/Users/goya/.ssh/id_ed25519')
    expect(screen.getByRole('switch', { name: 'Subagents' })).not.toBeChecked()
  })

  it('uses backend enabled false after an attempted switch save', async () => {
    uiStore.getState().setLocale('en-US')

    const setExecutionSettings = vi.fn(async () =>
      createExecutionSettings(availableAgentCapabilities),
    )
    const commandClient = {
      ...createTestCommandClient(),
      getExecutionSettings: async () => createExecutionSettings(availableAgentCapabilities),
      setExecutionSettings,
    } satisfies CommandClient

    renderExecutionSettings(commandClient)

    fireEvent.click(await screen.findByRole('switch', { name: 'Subagents' }))

    await waitFor(() => {
      expect(setExecutionSettings).toHaveBeenCalled()
    })
    expect(screen.getByRole('switch', { name: 'Subagents' })).not.toBeChecked()
  })

  it('labels settings as the default permission mode without leaking translation keys', async () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createTestCommandClient({
        executionSettings: {
          agentCapabilities,
          autoModeAvailable: false,
          contextCompressionTriggerRatio: 0.8,
          permissionMode: 'default',
          scope: 'global',
          toolProfile: 'full',
        },
      }),
    )

    expect(
      await screen.findByRole('heading', { name: 'Default Permission Mode' }),
    ).toBeInTheDocument()
    expect(document.body.textContent).not.toContain('execution.mode.default.description')
  })
})
