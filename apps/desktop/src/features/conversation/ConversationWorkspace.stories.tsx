import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { ConversationWorkspace } from './ConversationWorkspace'

const withProviders = ((StoryComponent) => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })

  return (
    <CommandClientProvider client={createTestCommandClient()}>
      <QueryClientProvider client={queryClient}>
        <main className="h-screen bg-background p-6 text-foreground">
          <StoryComponent />
        </main>
      </QueryClientProvider>
    </CommandClientProvider>
  )
}) satisfies Decorator

const meta = {
  title: 'Conversation/Workspace',
  component: ConversationWorkspace,
  decorators: [withProviders],
  parameters: {
    layout: 'fullscreen',
  },
} satisfies Meta<typeof ConversationWorkspace>

export default meta

type Story = StoryObj<typeof meta>

export const Ready: Story = {}
