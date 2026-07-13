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
    viewConfigLayer: 'global',
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
      configLayer: 'global',
      displayName: 'Workspace GitHub',
      effective: true,
      enabled: true,
      exposedToolCount: 8,
      id: 'workspace-github',
      manageable: true,
      origin: 'workspace',
      overridesGlobal: false,
      required: false,
      scope: 'global',
      status: 'ready',
      statusSource: 'settings',
      transport: 'stdio',
    },
  },
}

export const Connecting: Story = {
  args: {
    server: {
      configLayer: 'global',
      displayName: 'Local filesystem tools',
      effective: true,
      enabled: true,
      exposedToolCount: 0,
      id: 'local-filesystem',
      manageable: false,
      origin: 'managed',
      overridesGlobal: false,
      required: false,
      scope: 'session',
      status: 'connecting',
      statusSource: 'settings',
      transport: 'inProcess',
    },
  },
}

export const Failed: Story = {
  args: {
    server: {
      configLayer: 'global',
      displayName: 'Design source',
      effective: true,
      enabled: true,
      exposedToolCount: 0,
      id: 'design-source',
      lastError: 'Server failed policy validation before exposing tools.',
      manageable: true,
      origin: 'user',
      overridesGlobal: false,
      required: false,
      scope: 'agent',
      status: 'failed',
      statusSource: 'settings',
      transport: 'http',
    },
  },
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="w-[620px] bg-background p-6 text-foreground">{children}</main>
}
