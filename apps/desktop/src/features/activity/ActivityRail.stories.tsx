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

export const Loading: Story = {
  args: {
    activeRunId: 'run-starting',
  },
}

export const Empty: Story = {
  args: {
    activeRunId: undefined,
  },
}

export const Ready: Story = {
  args: {
    activeRunId: undefined,
  },
}

export const ErrorState: Story = {
  name: 'Error',
  args: {
    activeRunId: 'run-003',
    errorMessage: 'Command failed during verification.',
  },
}

export const Compact: Story = {}

export const Idle: Story = Empty

export const Running: Story = Loading

export const Failed: Story = ErrorState

function StoryFrame({ children }: { children: ReactNode }) {
  return (
    <main className="flex min-h-screen items-end bg-background text-foreground">
      <div className="h-8 w-full">{children}</div>
    </main>
  )
}
