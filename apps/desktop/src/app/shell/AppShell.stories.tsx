import type { Meta, StoryObj } from '@storybook/react-vite'
import { createMemoryHistory, createRouter, RouterProvider } from '@tanstack/react-router'
import { useState } from 'react'

import { routeTree } from '@/routeTree.gen'

function createStoryRouter() {
  return createRouter({
    routeTree,
    defaultPreload: 'intent',
    history: createMemoryHistory({ initialEntries: ['/'] }),
    scrollRestoration: true,
  })
}

function ConversationWorkspaceShellStory() {
  const [router] = useState(createStoryRouter)

  return <RouterProvider router={router} />
}

const meta = {
  title: 'App/Shell',
  component: ConversationWorkspaceShellStory,
  parameters: {
    layout: 'fullscreen',
  },
} satisfies Meta<typeof ConversationWorkspaceShellStory>

export default meta

type Story = StoryObj<typeof meta>

export const ConversationWorkspace: Story = {}
