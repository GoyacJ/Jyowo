import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import type { ConversationTurn } from '@/shared/tauri/commands'
import { ConversationTimeline } from './conversation-timeline'

const meta = {
  title: 'Conversation/Timeline',
  component: ConversationTimeline,
  parameters: {
    layout: 'fullscreen',
  },
  args: {
    title: 'Build the desktop foundation',
    turns: [],
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

const baseTurn: ConversationTurn = {
  id: 'turn:user-message-001',
  conversationId: 'conversation-001',
  position: 0,
  user: {
    id: 'user:user-message-001',
    messageId: 'user-message-001',
    body: 'Build the app shell and show the verification result.',
    timestamp: '2026-06-17T00:00:00.000Z',
  },
  assistant: {
    id: 'assistant:run-001',
    runId: 'run-001',
    status: 'running',
    segments: [
      {
        kind: 'thinking',
        id: 'segment:thinking:run-001',
        order: 0,
        status: 'withheld',
        summary: { text: '思考内容已折叠' },
      },
      {
        kind: 'text',
        id: 'segment:text:assistant-message-001',
        order: 1,
        messageId: 'assistant-message-001',
        body: 'Reading the workspace and preparing changes...',
      },
      {
        kind: 'toolGroup',
        id: 'segment:tools:tool-001',
        order: 2,
        attempts: [
          {
            id: 'tool:tool-001',
            order: 0,
            toolUseId: 'tool-001',
            toolName: 'read_file',
            status: 'completed',
          },
          {
            id: 'tool:tool-002',
            order: 1,
            toolUseId: 'tool-002',
            toolName: 'exec_command',
            status: 'failed',
            failureSummary: '工具执行失败。详情可在 Activity 中查看。',
          },
          {
            id: 'tool:tool-003',
            order: 2,
            toolUseId: 'tool-003',
            toolName: 'install_dependencies',
            status: 'waitingPermission',
            permission: {
              id: 'permission:01HZ0000000000000000000001',
              requestId: '01HZ0000000000000000000001',
              toolUseId: 'tool-003',
              status: 'pending',
              summary: 'Install dependencies',
            },
          },
        ],
      },
      {
        kind: 'artifact',
        id: 'segment:artifact:artifact-001',
        order: 3,
        artifactId: 'artifact-001',
        title: 'Verification notes',
        summary: 'Generated implementation notes.',
      },
      {
        kind: 'reviewRequest',
        id: 'segment:review:review-001',
        order: 4,
        requestId: 'review-001',
        title: 'Review generated foundation',
        body: 'Confirm before applying.',
      },
    ],
  },
}

export const Empty: Story = {}

export const Streaming: Story = {
  args: {
    turns: [
      {
        ...baseTurn,
        assistant: baseTurn.assistant
          ? {
              ...baseTurn.assistant,
              segments: baseTurn.assistant.segments.slice(0, 2),
            }
          : undefined,
      },
    ],
  },
}

export const PermissionPending: Story = {
  args: {
    turns: [baseTurn],
  },
}

export const ToolFailed: Story = {
  args: {
    turns: [baseTurn],
  },
}

export const ArtifactReady: Story = {
  args: {
    turns: [baseTurn],
  },
}

export const ReviewAndClarification: Story = {
  args: {
    turns: [
      {
        ...baseTurn,
        assistant: baseTurn.assistant
          ? {
              ...baseTurn.assistant,
              status: 'complete',
              segments: [
                ...baseTurn.assistant.segments,
                {
                  kind: 'clarificationRequest',
                  id: 'segment:clarification:request-002',
                  order: 5,
                  requestId: 'request-002',
                  prompt: 'Which release target should I use?',
                },
              ],
            }
          : undefined,
      },
    ],
  },
}

export const RunFailed: Story = {
  args: {
    turns: [
      {
        ...baseTurn,
        assistant: {
          id: 'assistant:run-001',
          runId: 'run-001',
          status: 'failed',
          segments: [
            {
              kind: 'error',
              id: 'segment:error:event-001',
              order: 0,
              body: 'Runtime unavailable',
            },
          ],
        },
      },
    ],
  },
}

export const LongConversation: Story = {
  args: {
    turns: [
      baseTurn,
      ...Array.from({ length: 8 }, (_, index) => ({
        ...baseTurn,
        id: `turn:user-message-extra-${index}`,
        position: index + 1,
        user: {
          ...baseTurn.user,
          id: `user:user-message-extra-${index}`,
          messageId: `user-message-extra-${index}`,
          body: `Continue with follow-up step ${index + 1}.`,
        },
        assistant: {
          id: `assistant:run-extra-${index}`,
          runId: `run-extra-${index}`,
          status: 'complete' as const,
          segments: [
            {
              kind: 'text' as const,
              id: `segment:text:assistant-extra-${index}`,
              order: 0,
              messageId: `assistant-extra-${index}`,
              body: `Completed follow-up step ${index + 1}.`,
            },
          ],
        },
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
