import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'

import type { ConversationTurn } from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'
import {
  codexAttachmentStressTurns,
  codexLargeDiffTurns,
  codexStyleEvidenceTurns,
} from './conversation-evidence-fixtures'
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
    ((StoryComponent, context) => {
      const queryClient = new QueryClient({
        defaultOptions: {
          queries: { retry: false },
        },
      })
      const theme = context.parameters.themeMode === 'dark' ? 'dark' : 'light'

      return (
        <CommandClientProvider client={createMockCommandClient()}>
          <QueryClientProvider client={queryClient}>
            <StoryFrame theme={theme}>
              <StoryComponent />
            </StoryFrame>
          </QueryClientProvider>
        </CommandClientProvider>
      )
    }) satisfies Decorator,
  ],
} satisfies Meta<typeof ConversationTimeline>

export default meta

type Story = StoryObj<typeof meta>

function storyTurn(
  id: string,
  body: string,
  assistant: NonNullable<ConversationTurn['assistant']>,
): ConversationTurn {
  return {
    id: `turn:${id}`,
    conversationId: 'conversation-001',
    position: 0,
    user: {
      id: `user:${id}`,
      messageId: id,
      body,
      timestamp: '2026-06-17T00:00:00.000Z',
    },
    assistant,
  }
}

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
        kind: 'process',
        id: 'segment:process:run-001',
        order: 0,
        status: 'running',
        summary: '正在处理请求',
        steps: [
          {
            id: 'process-step:story-reasoning',
            order: 0,
            kind: 'reasoning',
            status: 'running',
            title: '分析请求',
            body: '整理应用壳和验证结果的展示方式。',
          },
          {
            id: 'process-step:story-command',
            order: 1,
            kind: 'command',
            status: 'failed',
            title: '运行验证',
            detail: {
              type: 'command',
              command: 'pnpm check:desktop',
              output: 'lint failed',
              exitCode: 1,
              durationMs: 1300,
            },
          },
        ],
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

export const CodexEvidenceFlow: Story = {
  args: {
    title: 'Codex-style evidence flow',
    turns: codexStyleEvidenceTurns,
  },
  parameters: {
    backgrounds: { default: 'dark' },
    themeMode: 'dark',
  },
}

export const CodexEvidenceRunning: Story = {
  args: {
    title: 'Codex-style running evidence',
    turns: codexEvidenceTurnsWithStatus('running'),
  },
  parameters: {
    backgrounds: { default: 'dark' },
    themeMode: 'dark',
  },
}

export const CodexEvidenceFailedCommand: Story = {
  args: {
    title: 'Codex-style failed command',
    turns: codexStyleEvidenceTurns,
  },
  parameters: {
    backgrounds: { default: 'dark' },
    themeMode: 'dark',
  },
}

export const CodexEvidenceLargeDiff: Story = {
  args: {
    title: 'Codex-style large diff',
    turns: codexLargeDiffTurns,
  },
}

export const CodexEvidencePermissionPending: Story = {
  args: {
    title: 'Codex-style permission pending',
    turns: [baseTurn],
  },
  parameters: {
    backgrounds: { default: 'dark' },
    themeMode: 'dark',
  },
}

export const CodexEvidenceContextCompacted: Story = {
  args: {
    title: 'Codex-style context compaction',
    turns: codexStyleEvidenceTurns,
  },
}

export const CodexEvidenceAttachmentsMetadataOnly: Story = {
  args: {
    title: 'Codex-style attachment metadata',
    turns: codexAttachmentStressTurns,
  },
}

export const CodexEvidenceAttachmentsWithSafePreview: Story = {
  args: {
    title: 'Codex-style attachment preview fallback',
    turns: codexAttachmentStressTurns,
  },
  parameters: {
    docs: {
      description: {
        story:
          'Attachment safe preview is not exposed by the current worktree contract, so image attachments render as metadata chips instead of blob-backed thumbnails.',
      },
    },
  },
}

export const SimpleCompletedTurn: Story = {
  args: {
    turns: [
      storyTurn('simple-completed', 'Summarize the current project.', {
        id: 'assistant:run-simple',
        runId: 'run-simple',
        status: 'complete',
        segments: [
          {
            kind: 'text',
            id: 'segment:text:simple-completed',
            order: 0,
            messageId: 'assistant-message-simple',
            body: 'The project is a local AI workspace with a Rust runtime and React shell.',
          },
        ],
      }),
    ],
  },
}

export const ToolApprovedCompleted: Story = {
  args: {
    turns: [
      storyTurn('tool-approved', 'Read the package metadata.', {
        id: 'assistant:run-tool-approved',
        runId: 'run-tool-approved',
        status: 'complete',
        segments: [
          {
            kind: 'toolGroup',
            id: 'segment:tools:approved',
            order: 0,
            attempts: [
              {
                id: 'tool:approved',
                order: 0,
                toolUseId: 'tool-approved',
                toolName: 'read_file',
                status: 'completed',
                permission: {
                  id: 'permission:approved',
                  requestId: 'permission-approved',
                  toolUseId: 'tool-approved',
                  status: 'approved',
                },
              },
            ],
          },
          {
            kind: 'text',
            id: 'segment:text:tool-approved',
            order: 1,
            messageId: 'assistant-message-tool-approved',
            body: 'The metadata was read and no package changes are needed.',
          },
        ],
      }),
    ],
  },
}

export const MultipleToolAttempts: Story = {
  args: {
    turns: [
      storyTurn('multiple-tools', 'Run verification and recover from one failure.', {
        id: 'assistant:run-multiple-tools',
        runId: 'run-multiple-tools',
        status: 'running',
        segments: [
          {
            kind: 'toolGroup',
            id: 'segment:tools:multiple',
            order: 0,
            attempts: [
              {
                id: 'tool:multiple-1',
                order: 0,
                toolUseId: 'tool-multiple-1',
                toolName: 'pnpm test',
                status: 'failed',
                failureSummary: '工具执行失败。详情可在 Activity 中查看。',
              },
              {
                id: 'tool:multiple-2',
                order: 1,
                toolUseId: 'tool-multiple-2',
                toolName: 'pnpm test --runInBand',
                status: 'completed',
              },
              {
                id: 'tool:multiple-3',
                order: 2,
                toolUseId: 'tool-multiple-3',
                toolName: 'cargo test',
                status: 'waitingPermission',
                permission: {
                  id: 'permission:multiple-3',
                  requestId: 'permission-multiple-3',
                  toolUseId: 'tool-multiple-3',
                  status: 'pending',
                },
              },
            ],
          },
        ],
      }),
    ],
  },
}

export const ToolCallOnlyNoEmptyText: Story = {
  args: {
    turns: [
      storyTurn('tool-only', 'Inspect the workspace files.', {
        id: 'assistant:run-tool-only',
        runId: 'run-tool-only',
        status: 'running',
        segments: [
          {
            kind: 'toolGroup',
            id: 'segment:tools:tool-only',
            order: 0,
            attempts: [
              {
                id: 'tool:tool-only',
                order: 0,
                toolUseId: 'tool-only',
                toolName: 'list_files',
                status: 'running',
              },
            ],
          },
        ],
      }),
    ],
  },
}

export const WithheldThinking: Story = {
  args: {
    turns: [
      storyTurn('withheld-thinking', 'Plan the migration steps.', {
        id: 'assistant:run-withheld-thinking',
        runId: 'run-withheld-thinking',
        status: 'running',
        segments: [
          {
            kind: 'thinking',
            id: 'segment:thinking:withheld',
            order: 0,
            status: 'withheld',
            summary: { text: '思考内容已折叠' },
          },
        ],
      }),
    ],
  },
}

export const ImageArtifact: Story = {
  args: {
    turns: [
      storyTurn('image-artifact', '生成一张草鱼图片。', {
        id: 'assistant:run-image-artifact',
        runId: 'run-image-artifact',
        status: 'complete',
        segments: [
          {
            kind: 'process',
            id: 'segment:process:image-artifact',
            order: 0,
            status: 'complete',
            summary: '已完成工作过程',
            steps: [
              {
                id: 'process-step:image-reasoning',
                order: 0,
                kind: 'reasoning',
                status: 'complete',
                title: '分析图片需求',
                body: '确认需要生成图片并在对话中展示预览。',
              },
              {
                id: 'process-step:image-artifact',
                order: 1,
                kind: 'artifact',
                status: 'complete',
                title: 'Generated image',
                detail: {
                  type: 'artifact',
                  artifactId: 'artifact-image-001',
                  media: {
                    kind: 'image',
                    mimeType: 'image/png',
                    sizeBytes: 68,
                  },
                },
              },
            ],
          },
          {
            kind: 'text',
            id: 'segment:text:image-artifact',
            order: 1,
            messageId: 'assistant-message-image-artifact',
            body: '图片已生成。',
          },
        ],
      }),
    ],
  },
}

export const FinalAnswerAfterFailedTool: Story = {
  args: {
    turns: [
      storyTurn('failed-tool-final', 'Run the checks and summarize the result.', {
        id: 'assistant:run-failed-tool-final',
        runId: 'run-failed-tool-final',
        status: 'complete',
        segments: [
          {
            kind: 'toolGroup',
            id: 'segment:tools:failed-final',
            order: 0,
            attempts: [
              {
                id: 'tool:failed-final',
                order: 0,
                toolUseId: 'tool-failed-final',
                toolName: 'pnpm check',
                status: 'failed',
                failureSummary: '工具执行失败。详情可在 Activity 中查看。',
              },
            ],
          },
          {
            kind: 'text',
            id: 'segment:text:failed-tool-final',
            order: 1,
            messageId: 'assistant-message-failed-tool-final',
            body: 'The check failed before completion. The next step is to inspect the failing gate.',
          },
        ],
      }),
    ],
  },
}

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

function StoryFrame({ children, theme }: { children: ReactNode; theme: 'dark' | 'light' }) {
  return (
    <main
      className={`${theme === 'dark' ? 'dark ' : ''}h-screen bg-background p-6 text-foreground`}
    >
      <div className="mx-auto h-full max-w-[980px]">{children}</div>
    </main>
  )
}

function codexEvidenceTurnsWithStatus(
  status: NonNullable<ConversationTurn['assistant']>['status'],
): ConversationTurn[] {
  return codexStyleEvidenceTurns.map((turn) => ({
    ...turn,
    id: `${turn.id}:${status}`,
    assistant: turn.assistant
      ? {
          ...turn.assistant,
          id: `${turn.assistant.id}:${status}`,
          runId: `${turn.assistant.runId}:${status}`,
          status,
          segments: turn.assistant.segments.map((segment) => {
            if (segment.kind !== 'process') {
              return segment
            }

            return {
              ...segment,
              status: status === 'failed' ? 'failed' : 'running',
            }
          }),
        }
      : undefined,
  }))
}
