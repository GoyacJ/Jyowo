import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'

import { CapabilityRoutesPanel } from './CapabilityRoutesPanel'
import type { CapabilityRouteRow } from './model-settings-view-model'

function renderPanel(rows: CapabilityRouteRow[]) {
  uiStore.getState().setLocale('en-US')
  const onConfigure = vi.fn()
  render(
    <AppI18nProvider>
      <CapabilityRoutesPanel
        onConfigure={onConfigure}
        routeSection={{ status: 'ready', data: rows }}
      />
    </AppI18nProvider>,
  )
  return { onConfigure }
}

describe('CapabilityRoutesPanel', () => {
  it('lists route kinds and configured route state from backend route options', () => {
    renderPanel(routeRows)

    expect(screen.getByRole('table', { name: 'Capability route table' })).toBeInTheDocument()
    expect(
      screen.getByRole('row', {
        name: /Image generation.*Primary OpenAI.*Online.*Sync.*Medium/,
      }),
    ).toBeInTheDocument()
    expect(
      screen.getByRole('row', { name: /Video generation.*Not configured/ }),
    ).toBeInTheDocument()
    expect(screen.getByRole('row', { name: /Speech to text.*Not configured/ })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: /Text to speech.*Not configured/ })).toBeInTheDocument()
    expect(
      screen.getByRole('row', { name: /Music generation.*Not configured/ }),
    ).toBeInTheDocument()
  })

  it('opens the route editor for unconfigured routes', () => {
    const { onConfigure } = renderPanel(routeRows)
    const videoRow = within(screen.getByRole('row', { name: /Video generation/ }))

    fireEvent.click(videoRow.getByRole('button', { name: 'Configure video generation' }))

    expect(onConfigure).toHaveBeenCalledWith(routeRows[1])
  })

  it('keeps route query failures local to the route surface', () => {
    uiStore.getState().setLocale('en-US')
    render(
      <AppI18nProvider>
        <CapabilityRoutesPanel
          onConfigure={vi.fn()}
          routeSection={{ status: 'error', safeMessage: 'Route options unavailable' }}
        />
      </AppI18nProvider>,
    )

    expect(screen.getByRole('alert')).toHaveTextContent('Capability routes could not be loaded.')
    expect(screen.getByRole('alert')).toHaveTextContent('Route options unavailable')
  })

  it('renders a loading state while backend route queries are pending', () => {
    uiStore.getState().setLocale('en-US')
    render(
      <AppI18nProvider>
        <CapabilityRoutesPanel onConfigure={vi.fn()} routeSection={{ status: 'loading' }} />
      </AppI18nProvider>,
    )

    expect(screen.getByRole('status')).toHaveTextContent('Loading capability routes...')
  })

  it('renders an unavailable fallback when route state cannot be built', () => {
    uiStore.getState().setLocale('en-US')
    render(
      <AppI18nProvider>
        <CapabilityRoutesPanel onConfigure={vi.fn()} routeSection={{ status: 'unavailable' }} />
      </AppI18nProvider>,
    )

    expect(screen.getByRole('status')).toHaveTextContent('Capability routes unavailable')
  })

  it('renders an empty state when backend returns no route options', () => {
    renderPanel([])

    expect(screen.getByRole('heading', { name: 'Capability Routes' })).toBeInTheDocument()
    expect(screen.getByText('No capability route options are available.')).toBeInTheDocument()
    expect(screen.queryByRole('table', { name: 'Capability route table' })).not.toBeInTheDocument()
  })
})

const routeRows: CapabilityRouteRow[] = [
  {
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
    unavailableTargets: [],
  },
  routeRow('video_generation'),
  routeRow('speech_to_text'),
  routeRow('text_to_speech'),
  routeRow('music_generation'),
]

function routeRow(kind: CapabilityRouteRow['kind']): CapabilityRouteRow {
  return {
    kind,
    savedRoute: null,
    selectedTarget: null,
    eligibleTargets: [
      {
        configId: 'cfg-openai',
        providerId: 'openai',
        modelId: 'gpt-4.1',
        displayName: 'Primary OpenAI',
        providerDisplayName: 'OpenAI',
        operationIds: [`${kind}.run`],
        execution:
          kind === 'video_generation' || kind === 'music_generation' ? 'async_job' : 'sync',
        costRisk: kind === 'speech_to_text' ? 'low' : 'high',
        health: { status: 'never_checked' },
      },
    ],
    unavailableTargets: [],
  }
}
