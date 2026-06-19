import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { CommandClient } from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { ProviderSettingsForm } from './ProviderSettingsForm'

function renderProviderSettingsForm(commandClient: CommandClient = createMockCommandClient()) {
  return render(
    <CommandClientProvider client={commandClient}>
      <ProviderSettingsForm />
    </CommandClientProvider>,
  )
}

describe('ProviderSettingsForm', () => {
  it('rejects invalid input before calling the backend', async () => {
    const saveProviderSettings = vi.fn()
    const client = {
      ...createMockCommandClient(),
      saveProviderSettings,
    }

    renderProviderSettingsForm(client)

    fireEvent.click(screen.getByRole('button', { name: 'Save provider settings' }))

    expect(await screen.findByText('Model is required.')).toBeInTheDocument()
    expect(screen.getByText('API key is required.')).toBeInTheDocument()
    expect(saveProviderSettings).not.toHaveBeenCalled()
  })

  it('disables submit while backend save is pending', async () => {
    const saveProviderSettings = vi.fn(
      () =>
        new Promise<Awaited<ReturnType<CommandClient['saveProviderSettings']>>>((resolve) => {
          window.setTimeout(
            () =>
              resolve({
                modelId: 'gpt-4o-mini',
                providerId: 'openai',
                secretRef: 'provider/workspace-local/openai/default',
                status: 'saved',
              }),
            25,
          )
        }),
    )
    const client = {
      ...createMockCommandClient(),
      saveProviderSettings,
    }

    renderProviderSettingsForm(client)

    fireEvent.change(screen.getByLabelText('Model'), { target: { value: 'gpt-4o-mini' } })
    fireEvent.change(screen.getByLabelText('API key'), { target: { value: 'provider-test-token' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save provider settings' }))

    expect(screen.getByRole('button', { name: 'Saving provider settings' })).toBeDisabled()
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Save provider settings' })).toBeEnabled(),
    )
  })

  it('surfaces backend errors without keeping the submitted key visible', async () => {
    const client = {
      ...createMockCommandClient(),
      saveProviderSettings: vi.fn().mockRejectedValue(new Error('Provider health check failed')),
    }

    renderProviderSettingsForm(client)

    fireEvent.change(screen.getByLabelText('Model'), { target: { value: 'gpt-4o-mini' } })
    fireEvent.change(screen.getByLabelText('API key'), { target: { value: 'provider-test-token' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save provider settings' }))

    expect(await screen.findByText('Provider settings could not be saved.')).toBeInTheDocument()
    expect(screen.getByLabelText('API key')).toHaveValue('')
    expect(screen.queryByText('provider-test-token')).not.toBeInTheDocument()
  })

  it('shows saved secret reference and masks the raw key after save', async () => {
    const rawKey = 'provider-test-token'
    const client = {
      ...createMockCommandClient(),
      saveProviderSettings: vi.fn().mockResolvedValue({
        modelId: 'gpt-4o-mini',
        providerId: 'openai',
        secretRef: 'provider/workspace-local/openai/default',
        status: 'saved',
      }),
    }

    renderProviderSettingsForm(client)

    fireEvent.change(screen.getByLabelText('Model'), { target: { value: 'gpt-4o-mini' } })
    fireEvent.change(screen.getByLabelText('API key'), { target: { value: rawKey } })
    fireEvent.click(screen.getByRole('button', { name: 'Save provider settings' }))

    expect(await screen.findByText('Provider saved.')).toBeInTheDocument()
    expect(screen.getByText('provider/workspace-local/openai/default')).toBeInTheDocument()
    expect(screen.getByText('Stored as secret reference')).toBeInTheDocument()
    expect(screen.getByLabelText('API key')).toHaveValue('')
    expect(screen.queryByDisplayValue(rawKey)).not.toBeInTheDocument()
    expect(screen.queryByText(rawKey)).not.toBeInTheDocument()
  })
})
