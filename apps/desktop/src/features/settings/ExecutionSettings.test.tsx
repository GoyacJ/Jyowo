import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { ExecutionSettings } from './ExecutionSettings'

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
      autoModeAvailable: false,
      permissionMode: 'bypass_permissions' as const,
    }))
    const commandClient = {
      ...createMockCommandClient(),
      getExecutionSettings: async () => ({
        autoModeAvailable: false,
        permissionMode: 'default' as const,
      }),
      setExecutionSettings,
    } satisfies CommandClient

    renderExecutionSettings(commandClient)

    expect(await screen.findByLabelText(/标准/i)).toBeChecked()

    fireEvent.click(screen.getByLabelText(/绕过/i))

    await waitFor(() => {
      expect(setExecutionSettings).toHaveBeenCalledWith({ permissionMode: 'bypass_permissions' })
    })
  })

  it('disables auto mode when unavailable', async () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createMockCommandClient({
        executionSettings: {
          autoModeAvailable: false,
          permissionMode: 'default',
        },
      }),
    )

    expect(await screen.findByText(/Auto approval is unavailable/i)).toBeInTheDocument()
    expect(screen.getByDisplayValue('auto')).toBeDisabled()
  })

  it('labels settings as the default permission mode without leaking translation keys', async () => {
    uiStore.getState().setLocale('en-US')

    renderExecutionSettings(
      createMockCommandClient({
        executionSettings: {
          autoModeAvailable: false,
          permissionMode: 'default',
        },
      }),
    )

    expect(
      await screen.findByRole('heading', { name: 'Default Permission Mode' }),
    ).toBeInTheDocument()
    expect(document.body.textContent).not.toContain('execution.mode.default.description')
  })
})
