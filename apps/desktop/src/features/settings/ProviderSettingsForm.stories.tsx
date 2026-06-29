import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

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
  (client: CommandClient): Decorator =>
  (StoryComponent) => (
    <StoryFrame>
      <CommandClientProvider client={client}>
        <StoryComponent />
      </CommandClientProvider>
    </StoryFrame>
  )

export const Ready: Story = {
  decorators: [withClient(createTestCommandClient())],
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="w-[760px] bg-background p-6 text-foreground">{children}</main>
}
