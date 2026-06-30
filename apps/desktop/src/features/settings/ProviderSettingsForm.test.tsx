import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { ProviderSettingsForm } from './ProviderSettingsForm'

function renderProviderSettingsForm() {
  uiStore.getState().setLocale('en-US')
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={createTestCommandClient()}>
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<ProviderSettingsForm />, { wrapper: Wrapper })
}

describe('ProviderSettingsForm', () => {
  it('is reduced to the model matrix page instead of the old provider form', async () => {
    renderProviderSettingsForm()

    expect(await screen.findByRole('heading', { name: 'Models' })).toBeInTheDocument()
    expect(await screen.findByRole('region', { name: 'Model matrix' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Model configuration' })).not.toBeInTheDocument()
  })
})
