import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import { MemoryItemCard } from './MemoryItemCard'

const meta = {
  title: 'Memory/ItemCard',
  component: MemoryItemCard,
  parameters: {
    layout: 'centered',
  },
  args: {
    onDelete: () => {},
    onInspect: () => {},
  },
  decorators: [
    ((StoryComponent) => (
      <StoryFrame>
        <StoryComponent />
      </StoryFrame>
    )) satisfies Decorator,
  ],
} satisfies Meta<typeof MemoryItemCard>

export default meta

type Story = StoryObj<typeof meta>

export const UserPreference: Story = {
  args: {
    item: {
      contentPreview: 'Prefers concise Chinese responses focused on repository facts.',
      id: '01J00000000000000000000001',
      kind: 'user_preference',
      source: 'user_input',
      tags: ['tone', 'workflow'],
      updatedAt: '2026-06-17T10:21:00.000Z',
      visibility: 'tenant',
    },
  },
}

export const PrivateLongPreview: Story = {
  args: {
    item: {
      contentPreview:
        'Local workspace policy requires redaction before logs, traces, screenshots, support bundles, and frontend state. The preview clamps to keep the card height stable in dense lists.',
      id: '01J00000000000000000000002',
      kind: 'project_fact',
      source: 'agent_derived',
      tags: ['policy', 'redaction', 'workspace'],
      updatedAt: '2026-06-17T10:24:00.000Z',
      visibility: 'private',
    },
  },
}

export const ReferenceMemory: Story = {
  args: {
    item: {
      contentPreview: 'Conversation-first UI remains the primary product surface.',
      id: '01J00000000000000000000003',
      kind: 'reference',
      source: 'consolidated',
      tags: ['ux'],
      updatedAt: '2026-06-17T10:27:00.000Z',
      visibility: 'user',
    },
  },
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="w-[560px] bg-background p-6 text-foreground">{children}</main>
}
