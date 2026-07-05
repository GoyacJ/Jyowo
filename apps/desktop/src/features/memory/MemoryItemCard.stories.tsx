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
      contentHash: '0'.repeat(64),
      contentPreview: 'Prefers concise Chinese responses focused on repository facts.',
      deleted: false,
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
      contentHash: '1'.repeat(64),
      contentPreview:
        'Local workspace policy requires redaction before logs, traces, screenshots, support bundles, and frontend state. The preview clamps to keep the card height stable in dense lists.',
      deleted: false,
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
      contentHash: '2'.repeat(64),
      contentPreview: 'Conversation-first UI remains the primary product surface.',
      deleted: false,
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
