import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import { ActivityRail } from './ActivityRail'

const meta = {
  title: 'Activity/Rail',
  component: ActivityRail,
  parameters: {
    layout: 'fullscreen',
  },
  args: {
    activeRunId: 'run-001',
  },
  decorators: [
    ((StoryComponent) => (
      <StoryFrame>
        <StoryComponent />
      </StoryFrame>
    )) satisfies Decorator,
  ],
} satisfies Meta<typeof ActivityRail>

export default meta

type Story = StoryObj<typeof meta>

export const Compact: Story = {}

export const Idle: Story = {
  args: {
    activeRunId: undefined,
  },
}

export const Running: Story = {
  args: {
    activeRunId: 'run-002',
  },
}

export const Failed: Story = {
  args: {
    activeRunId: 'run-003',
    errorMessage: 'Command failed during verification.',
  },
}

function StoryFrame({ children }: { children: ReactNode }) {
  return (
    <main className="flex min-h-screen items-end bg-background text-foreground">
      <div className="h-8 w-full">{children}</div>
    </main>
  )
}
