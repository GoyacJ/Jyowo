import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import { createMockCommandClient, createRejectedCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { ProviderSettingsForm } from './ProviderSettingsForm'

const meta = {
  title: 'Settings/ProviderSettingsForm',
  component: ProviderSettingsForm,
  parameters: {
    layout: 'centered',
  },
} satisfies Meta<typeof ProviderSettingsForm>

export default meta

type Story = StoryObj<typeof meta>

const withClient =
  (client: ReturnType<typeof createMockCommandClient>): Decorator =>
  (StoryComponent) => (
    <StoryFrame>
      <CommandClientProvider client={client}>
        <StoryComponent />
      </CommandClientProvider>
    </StoryFrame>
  )

export const Ready: Story = {
  decorators: [withClient(createMockCommandClient())],
}

export const SlowSave: Story = {
  decorators: [withClient(createMockCommandClient({ delayMs: 1200 }))],
}

export const SaveFailure: Story = {
  decorators: [withClient(createRejectedCommandClient(new Error('provider save rejected')))],
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="w-[760px] bg-background p-6 text-foreground">{children}</main>
}
