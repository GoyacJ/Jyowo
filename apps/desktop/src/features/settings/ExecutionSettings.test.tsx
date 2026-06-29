import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { ExecutionSettings } from './ExecutionSettings'

const agentCapabilities = {
  agentTeamsAvailable: false,
  agentTeamsEnabled: false,
  backgroundAgentsAvailable: false,
  backgroundAgentsEnabled: false,
  subagentsAvailable: false,
  subagentsEnabled: false,
  unavailableReasons: [],
}

function renderExecutionSettings(commandClient: CommandClient = createMockCommandClient()) {
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
    }))
    const commandClient = {
      ...createMockCommandClient(),
      getExecutionSettings: async () => ({
        agentCapabilities,
        autoModeAvailable: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default' as const,
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
      })
    })
  })

  it('loads and saves context compression trigger ratio', async () => {
    const setExecutionSettings = vi.fn(async () => ({
      agentCapabilities,
      autoModeAvailable: false,
      contextCompressionTriggerRatio: 0.75,
      permissionMode: 'default' as const,
    }))
    const commandClient = {
      ...createMockCommandClient(),
      getExecutionSettings: async () => ({
        agentCapabilities,
        autoModeAvailable: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default' as const,
      }),
      setExecutionSettings,
    } satisfies CommandClient

    renderExecutionSettings(commandClient)

    const ratioInput = await screen.findByLabelText(/上下文压缩触发比例/i)
    expect(ratioInput).toHaveValue(80)

    fireEvent.change(ratioInput, { target: { value: '75' } })
    fireEvent.click(screen.getByRole('button', { name: /保存执行设置/i }))

    await waitFor(() => {
      expect(setExecutionSettings).toHaveBeenCalledWith({
        agentTeamsEnabled: false,
        backgroundAgentsEnabled: false,
        contextCompressionTriggerRatio: 0.75,
        permissionMode: 'default',
        subagentsEnabled: false,
      })
    })
  })

  it('disables auto mode when unavailable', async () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createMockCommandClient({
        executionSettings: {
          agentCapabilities,
          autoModeAvailable: false,
          contextCompressionTriggerRatio: 0.8,
          permissionMode: 'default',
        },
      }),
    )

    expect(await screen.findByText(/Auto approval is unavailable/i)).toBeInTheDocument()
    expect(screen.getByLabelText(/Auto/i)).toBeDisabled()
  })
})
