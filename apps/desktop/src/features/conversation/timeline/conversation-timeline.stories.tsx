import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import type { ConversationBlock } from './conversation-blocks'
import { ConversationTimeline } from './conversation-timeline'

const meta = {
  title: 'Conversation/Timeline',
  component: ConversationTimeline,
  parameters: {
    layout: 'fullscreen',
  },
  args: {
    title: 'Build the desktop foundation',
    blocks: [],
  },
  decorators: [
    ((StoryComponent) => (
      <StoryFrame>
        <StoryComponent />
      </StoryFrame>
    )) satisfies Decorator,
  ],
} satisfies Meta<typeof ConversationTimeline>

export default meta

type Story = StoryObj<typeof meta>

const base = {
  conversationId: 'conversation-001',
  createdAt: '2026-06-17T00:00:00.000Z',
}

const longConversationBlocks: ConversationBlock[] = [
  {
    ...base,
    id: 'message-001',
    kind: 'userMessage',
    body: 'Build the app shell and show the verification result.',
    status: 'sent',
    conversationSequence: 1,
  },
  {
    ...base,
    id: 'assistant-stream',
    kind: 'assistantStreaming',
    body: 'Reading the workspace and preparing changes...',
    status: 'streaming',
    conversationSequence: 2,
  },
  {
    ...base,
    id: 'tools',
    kind: 'toolGroup',
    expanded: true,
    conversationSequence: 3,
    items: [
      { id: 'tool-001', name: 'read_file', status: 'completed', argumentsSummary: 'apps/desktop' },
      { id: 'tool-002', name: 'exec_command', status: 'failed', errorMessage: 'exit 1' },
    ],
  },
  {
    ...base,
    id: 'permission',
    kind: 'permissionRequest',
    requestId: '01HZ0000000000000000000001',
    operation: 'Install dependencies',
    reason: 'The run requested package installation.',
    target: 'workspace package manager',
    severity: 'high',
    decisionScope: 'current run',
    exposure: 'Can modify package metadata and lockfile.',
    workspaceBoundary: 'workspace://local',
    status: 'pending',
    conversationSequence: 4,
  },
  {
    ...base,
    id: 'artifact',
    kind: 'artifact',
    artifactId: 'artifact-001',
    artifactKind: 'markdown',
    title: 'Verification notes',
    description: 'Generated implementation notes.',
    actionLabel: 'Open',
    status: 'ready',
    preview: '# Verification\npnpm check:desktop passed.',
    conversationSequence: 5,
  },
  {
    ...base,
    id: 'diff',
    kind: 'diffReview',
    title: 'src/main.ts',
    status: 'pending',
    preview: '+ add timeline source',
    conversationSequence: 6,
  },
  {
    ...base,
    id: 'review',
    kind: 'reviewRequest',
    title: 'Review generated foundation',
    continuePrompt: 'Continue',
    status: 'pending',
    conversationSequence: 7,
  },
]

export const Empty: Story = {}

export const Streaming: Story = {
  args: {
    blocks: longConversationBlocks.slice(0, 2),
  },
}

export const PermissionPending: Story = {
  args: {
    blocks: [longConversationBlocks[0], longConversationBlocks[3]],
  },
}

export const ToolFailed: Story = {
  args: {
    blocks: [longConversationBlocks[0], longConversationBlocks[2]],
  },
}

export const ArtifactReady: Story = {
  args: {
    blocks: [longConversationBlocks[0], longConversationBlocks[4]],
  },
}

export const DiffReview: Story = {
  args: {
    blocks: [longConversationBlocks[0], longConversationBlocks[5], longConversationBlocks[6]],
  },
}

export const RunFailed: Story = {
  args: {
    blocks: [
      longConversationBlocks[0],
      {
        ...base,
        id: 'error',
        kind: 'error',
        message: 'Runtime unavailable',
        conversationSequence: 2,
      },
    ],
  },
}

export const LongConversation: Story = {
  args: {
    blocks: [
      ...longConversationBlocks,
      ...Array.from({ length: 8 }, (_, index) => ({
        ...base,
        id: `message-extra-${index}`,
        kind: 'assistantMessage' as const,
        body: `Completed follow-up step ${index + 1}.`,
        status: 'complete' as const,
        conversationSequence: 8 + index,
      })),
    ],
  },
}

function StoryFrame({ children }: { children: ReactNode }) {
  return (
    <main className="h-screen bg-background p-6 text-foreground">
      <div className="mx-auto h-full max-w-[980px]">{children}</div>
    </main>
  )
}
