import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import { ActivityRail, type ActivityRailItem } from './ActivityRail'

const meta = {
  title: 'Activity/Rail',
  component: ActivityRail,
  parameters: {
    layout: 'fullscreen',
  },
  args: {
    currentRun: {
      label: 'Run run-001',
      status: 'running',
    },
    items: [
      { id: 'activity-queued', label: 'queued', status: 'queued', time: '10:20' },
      { id: 'activity-running', label: 'tool.exec', status: 'running', time: '10:21' },
      { id: 'activity-success', label: 'cargo test', status: 'success', time: '10:23' },
      { id: 'activity-failed', label: 'storybook', status: 'failed', time: '10:24' },
      { id: 'activity-blocked', label: 'permission', status: 'blocked', time: '10:25' },
      { id: 'activity-redacted', label: 'payload', status: 'redacted', time: '10:26' },
    ] satisfies ActivityRailItem[],
    onCollapse: () => {},
    onExpand: () => {},
    onViewAll: () => {},
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

export const Expanded: Story = {
  args: {
    expanded: true,
  },
}

export const Collapsed: Story = {
  args: {
    collapsed: true,
  },
}

export const Loading: Story = {
  args: {
    loading: true,
  },
}

export const Failed: Story = {
  args: {
    currentRun: {
      label: 'Run run-001',
      status: 'failed',
    },
    errorMessage: 'Activity stream unavailable.',
  },
}

function StoryFrame({ children }: { children: ReactNode }) {
  return (
    <main className="flex min-h-screen items-end bg-background text-foreground">
      <div className="h-28 w-full">{children}</div>
    </main>
  )
}
