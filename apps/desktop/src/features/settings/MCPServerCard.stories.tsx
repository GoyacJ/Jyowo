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
    onConfigure: () => {},
    onDelete: () => {},
    onRestart: () => {},
    onToggle: () => {},
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
      enabled: true,
      exposedToolCount: 8,
      id: 'workspace-github',
      manageable: true,
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
      enabled: true,
      exposedToolCount: 0,
      id: 'local-filesystem',
      manageable: false,
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
      enabled: true,
      exposedToolCount: 0,
      id: 'design-source',
      lastError: 'Server failed policy validation before exposing tools.',
      manageable: true,
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
