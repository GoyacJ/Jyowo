import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import { MCPServerCard } from './MCPServerCard'

const meta = {
  title: 'Settings/MCPServerCard',
  component: MCPServerCard,
  parameters: {
    layout: 'centered',
  },
  args: {
    onDelete: () => {},
  },
  decorators: [
    ((StoryComponent) => (
      <StoryFrame>
        <StoryComponent />
      </StoryFrame>
    )) satisfies Decorator,
  ],
} satisfies Meta<typeof MCPServerCard>

export default meta

type Story = StoryObj<typeof meta>

export const Ready: Story = {
  args: {
    server: {
      displayName: 'Workspace GitHub',
      exposedToolCount: 8,
      id: 'workspace-github',
      origin: 'workspace',
      scope: 'global',
      status: 'ready',
      transport: 'stdio',
    },
  },
}

export const Connecting: Story = {
  args: {
    server: {
      displayName: 'Local filesystem tools',
      exposedToolCount: 0,
      id: 'local-filesystem',
      origin: 'managed',
      scope: 'session',
      status: 'connecting',
      transport: 'inProcess',
    },
  },
}

export const Failed: Story = {
  args: {
    server: {
      displayName: 'Design source',
      exposedToolCount: 0,
      id: 'design-source',
      lastError: 'Server failed policy validation before exposing tools.',
      origin: 'user',
      scope: 'agent',
      status: 'failed',
      transport: 'http',
    },
  },
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="w-[620px] bg-background p-6 text-foreground">{children}</main>
}
