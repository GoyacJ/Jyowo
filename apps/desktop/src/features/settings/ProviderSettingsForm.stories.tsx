import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { ProviderSettingsForm } from './ProviderSettingsForm'

uiStore.getState().setLocale('en-US')

const meta = {
  title: 'Settings/ProviderSettingsForm',
  component: ProviderSettingsForm,
  parameters: {
    layout: 'fullscreen',
  },
} satisfies Meta<typeof ProviderSettingsForm>

export default meta

type Story = StoryObj<typeof meta>

const withClient =
  (client: CommandClient): Decorator =>
  (StoryComponent) => (
    <StoryFrame>
      <CommandClientProvider client={client}>
        <QueryClientProvider
          client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
        >
          <AppI18nProvider>
            <StoryComponent />
          </AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    </StoryFrame>
  )

export const Ready: Story = {
  decorators: [withClient(createTestCommandClient())],
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="min-h-screen bg-background p-4 text-foreground">{children}</main>
}
