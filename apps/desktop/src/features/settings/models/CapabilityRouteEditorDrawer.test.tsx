import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'

import { CapabilityRouteEditorDrawer } from './CapabilityRouteEditorDrawer'
import type { CapabilityRouteRow } from './model-settings-view-model'

function renderDrawer(row: CapabilityRouteRow = imageRoute) {
  uiStore.getState().setLocale('en-US')
  const onSave = vi.fn()
  const onClear = vi.fn()
  render(
    <AppI18nProvider>
      <CapabilityRouteEditorDrawer
        onClear={onClear}
        onOpenChange={vi.fn()}
        onSave={onSave}
        open
        route={row}
      />
    </AppI18nProvider>,
  )
  return { onClear, onSave }
}

describe('CapabilityRouteEditorDrawer', () => {
  it('lists eligible targets and disabled unavailable targets with backend reasons', () => {
    renderDrawer()

    const dialog = screen.getByRole('dialog', { name: 'Edit image generation route' })
    expect(within(dialog).getByLabelText('Primary OpenAI')).toBeEnabled()
    expect(within(dialog).getByLabelText('Backup OpenAI')).toBeDisabled()
    expect(dialog).toHaveTextContent('Operation IDs: images.generate')
    expect(dialog).toHaveTextContent('Backend says this profile cannot run image generation')
  })

  it('saves the selected backend route target', () => {
    const { onSave } = renderDrawer()

    fireEvent.click(screen.getByRole('button', { name: 'Save route' }))

    expect(onSave).toHaveBeenCalledWith({
      kind: 'image_generation',
      configId: 'cfg-openai',
      providerId: 'openai',
      operationIds: ['images.generate'],
      enabled: true,
    })
  })

  it('clears the existing backend route target', () => {
    const { onClear } = renderDrawer()

    fireEvent.click(screen.getByRole('button', { name: 'Clear route' }))

    expect(onClear).toHaveBeenCalledWith({
      kind: 'image_generation',
      configId: 'cfg-openai',
      providerId: 'openai',
    })
  })

  it('uses the shared centered dialog placement', () => {
    renderDrawer()

    const dialog = screen.getByRole('dialog', { name: 'Edit image generation route' })
    expect(dialog).not.toHaveClass('right-4')
    expect(dialog).not.toHaveClass('top-4')
    expect(dialog).not.toHaveClass('translate-x-0')
    expect(dialog).not.toHaveClass('translate-y-0')
  })
})

const imageRoute: CapabilityRouteRow = {
  kind: 'image_generation',
  savedRoute: {
    kind: 'image_generation',
    configId: 'cfg-openai',
    providerId: 'openai',
    operationIds: ['images.generate'],
    enabled: true,
  },
  selectedTarget: {
    configId: 'cfg-openai',
    providerId: 'openai',
    modelId: 'gpt-4.1',
    displayName: 'Primary OpenAI',
    providerDisplayName: 'OpenAI',
    operationIds: ['images.generate'],
    execution: 'sync',
    costRisk: 'medium',
    health: {
      status: 'online',
      latencyMs: 118,
      timeoutMs: 10000,
      checkedAt: '2026-06-30T10:00:00Z',
    },
  },
  eligibleTargets: [
    {
      configId: 'cfg-openai',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      displayName: 'Primary OpenAI',
      providerDisplayName: 'OpenAI',
      operationIds: ['images.generate'],
      execution: 'sync',
      costRisk: 'medium',
      health: {
        status: 'online',
        latencyMs: 118,
        timeoutMs: 10000,
        checkedAt: '2026-06-30T10:00:00Z',
      },
    },
  ],
  unavailableTargets: [
    {
      configId: 'cfg-backup',
      providerId: 'openai',
      modelId: 'gpt-4.1',
      displayName: 'Backup OpenAI',
      operationId: 'images.generate',
      reason: 'Backend says this profile cannot run image generation',
    },
  ],
}
