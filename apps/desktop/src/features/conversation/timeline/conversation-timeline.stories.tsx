import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'

import { uiStore } from '@/shared/state/ui-store'
import type { ConversationTurn } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import {
  codexAttachmentStressTurns,
  codexLargeDiffTurns,
  codexStyleEvidenceTurns,
} from '@/testing/conversation-evidence-fixtures'
import {
  artifactRevision,
  assistantWork,
  commandDetail,
  permissionState,
} from '@/testing/conversation-worktree-builders'
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
      const evidenceDisclosureOpen = context.parameters.evidenceDisclosureOpen as
        | Record<string, boolean>
        | undefined

      uiStore.getState().resetEvidenceDisclosure()
      for (const [id, open] of Object.entries(evidenceDisclosureOpen ?? {})) {
        uiStore.getState().setEvidenceDisclosureOpen(id, open)
      }

      return (
        <CommandClientProvider client={createTestCommandClient()}>
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
  assistant: Parameters<typeof assistantWork>[0],
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
    assistant: assistantWork(assistant),
  }
}

function timelineBlockDisclosureId({
  blockId,
  conversationId = 'conversation-001',
  kind,
  runId,
}: {
  blockId: string
  conversationId?: string
  kind: 'activity' | 'commandGroup' | 'fileEdit'
  runId: string
}) {
  return `conversation:${conversationId}:run:${runId}:block:${kind}:${blockId}`
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
  assistant: assistantWork({
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
            detail: commandDetail({
              command: 'pnpm check:desktop',
              stdoutPreview: 'lint failed',
              exitCode: 1,
              durationMs: 1300,
            }),
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
            failureSummary: '工具执行失败。可在详情中查看。',
          },
          {
            id: 'tool:tool-003',
            order: 2,
            toolUseId: 'tool-003',
            toolName: 'install_dependencies',
            status: 'waitingPermission',
            permission: permissionState({
              id: 'permission:01HZ0000000000000000000001',
              requestId: '01HZ0000000000000000000001',
              toolUseId: 'tool-003',
              status: 'pending',
              reason: 'Install dependencies',
            }),
          },
        ],
      },
      {
        kind: 'artifact',
        id: 'segment:artifact:artifact-001',
        order: 3,
        artifactId: 'artifact-001',
        title: 'Verification notes',
        revision: artifactRevision({
          artifactId: 'artifact-001',
          revisionId: 'revision-artifact-001',
          kind: 'document',
          sourceRunId: 'run-001',
          title: 'Verification notes',
          summary: 'Generated implementation notes.',
          contentRef: 'evidence-artifact-001',
        }),
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
  }),
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

export const CodexEvidenceFileEditExpanded: Story = {
  args: {
    title: 'Codex-style expanded file edit',
    turns: codexStyleEvidenceTurns,
  },
  parameters: {
    evidenceDisclosureOpen: {
      [timelineBlockDisclosureId({
        conversationId: 'conversation-codex-evidence',
        runId: 'run-codex-evidence',
        kind: 'fileEdit',
        blockId: 'process:segment:process:codex-evidence:file-edit:process-step:file-edit',
      })]: true,
    },
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

export const CodexEvidenceCollapsedHistory: Story = {
  args: {
    title: 'Codex-style collapsed history',
    turns: [collapsedHistoryTurn()],
  },
  parameters: {
    backgrounds: { default: 'dark' },
    themeMode: 'dark',
  },
}

export const CodexEvidenceActivityExpanded: Story = {
  args: {
    title: 'Codex-style expanded read/search',
    turns: [collapsedHistoryTurn()],
  },
  parameters: {
    backgrounds: { default: 'dark' },
    evidenceDisclosureOpen: {
      [timelineBlockDisclosureId({
        runId: 'run-codex-collapsed-history',
        kind: 'activity',
        blockId:
          'process:segment:process:codex-collapsed-history:activity:process-step:collapsed-read',
      })]: true,
    },
    themeMode: 'dark',
  },
}

export const CodexEvidenceSuccessfulCommandCollapsed: Story = {
  args: {
    title: 'Codex-style collapsed successful command',
    turns: [successfulCommandHistoryTurn()],
  },
}

export const CodexEvidenceSuccessfulCommandExpanded: Story = {
  args: {
    title: 'Codex-style expanded successful command',
    turns: [successfulCommandHistoryTurn()],
  },
  parameters: {
    evidenceDisclosureOpen: {
      [timelineBlockDisclosureId({
        runId: 'run-codex-successful-command',
        kind: 'commandGroup',
        blockId:
          'process:segment:process:successful-command:commands:process-step:successful-command',
      })]: true,
    },
  },
}

export const CodexEvidenceCompletedRunWithFailedStep: Story = {
  args: {
    title: 'Codex-style completed run with failed step',
    turns: [completedRunWithFailedStepTurn()],
  },
  parameters: {
    backgrounds: { default: 'dark' },
    themeMode: 'dark',
  },
}

export const CodexEvidenceRepeatedSearchFailures: Story = {
  args: {
    title: 'Codex-style repeated search failures',
    turns: [repeatedSearchFailuresTurn()],
  },
  parameters: {
    backgrounds: { default: 'dark' },
    themeMode: 'dark',
  },
}

export const CodexEvidenceBottomComposerOverlap: Story = {
  args: {
    title: 'Codex-style bottom composer reserve',
    turns: bottomComposerOverlapTurns(),
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
                permission: permissionState({
                  id: 'permission:approved',
                  requestId: 'permission-approved',
                  toolUseId: 'tool-approved',
                  status: 'approved',
                  reason: 'Read package metadata',
                }),
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
                failureSummary: '工具执行失败。可在详情中查看。',
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
                permission: permissionState({
                  id: 'permission:multiple-3',
                  requestId: 'permission-multiple-3',
                  toolUseId: 'tool-multiple-3',
                  status: 'pending',
                  reason: 'Run cargo tests',
                }),
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
            kind: 'process',
            id: 'segment:process:withheld',
            order: 0,
            status: 'withheld',
            summary: '思考内容已折叠',
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
                failureSummary: '工具执行失败。可在详情中查看。',
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
        assistant: assistantWork({
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
        }),
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
        assistant: assistantWork({
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
        }),
      })),
    ],
  },
}

function collapsedHistoryTurn(): ConversationTurn {
  return storyTurn('codex-collapsed-history', '整理历史执行证据。', {
    id: 'assistant:run-codex-collapsed-history',
    runId: 'run-codex-collapsed-history',
    status: 'complete',
    segments: [
      {
        kind: 'process',
        id: 'segment:process:codex-collapsed-history',
        order: 0,
        status: 'failed',
        summary: '已结束但存在失败步骤',
        steps: [
          {
            id: 'process-step:collapsed-read',
            order: 0,
            kind: 'fileRead',
            status: 'complete',
            title: '已读取 package.json',
            detail: {
              type: 'activity',
              summary: '读取 package.json',
              itemCount: 1,
            },
          },
          {
            id: 'process-step:collapsed-search',
            order: 1,
            kind: 'fileSearch',
            status: 'complete',
            title: '已搜索 timeline',
            detail: {
              type: 'activity',
              summary: '搜索 timeline',
              itemCount: 2,
            },
          },
          {
            id: 'process-step:collapsed-rg',
            order: 2,
            kind: 'command',
            status: 'complete',
            title: '已运行历史命令',
            detail: commandDetail({
              command: 'rg "timeline" apps/desktop/src',
              stdoutPreview:
                'apps/desktop/src/features/conversation/timeline/conversation-timeline.tsx',
              exitCode: 0,
              durationMs: 180,
            }),
          },
          {
            id: 'process-step:collapsed-test-failed',
            order: 3,
            kind: 'command',
            status: 'failed',
            title: '测试失败',
            detail: commandDetail({
              command: 'pnpm -C apps/desktop test',
              stdoutPreview: '1 failed',
              exitCode: 1,
              durationMs: 2100,
            }),
          },
        ],
      },
    ],
  })
}

function successfulCommandHistoryTurn(): ConversationTurn {
  return storyTurn('codex-successful-command', '运行一次成功检查。', {
    id: 'assistant:run-codex-successful-command',
    runId: 'run-codex-successful-command',
    status: 'complete',
    segments: [
      {
        kind: 'process',
        id: 'segment:process:successful-command',
        order: 0,
        status: 'complete',
        summary: '检查已完成',
        steps: [
          {
            id: 'process-step:successful-command',
            order: 0,
            kind: 'command',
            status: 'complete',
            title: '检查通过',
            detail: commandDetail({
              command: 'pnpm -C apps/desktop typecheck',
              stdoutPreview: 'typecheck passed',
              exitCode: 0,
              durationMs: 1400,
            }),
          },
        ],
      },
    ],
  })
}

function completedRunWithFailedStepTurn(): ConversationTurn {
  return storyTurn('codex-complete-with-failed-step', '运行检查并总结结果。', {
    id: 'assistant:run-complete-with-failed-step',
    runId: 'run-complete-with-failed-step',
    status: 'complete',
    segments: [
      {
        kind: 'process',
        id: 'segment:process:complete-with-failed-step',
        order: 0,
        status: 'failed',
        summary: '已结束但存在失败步骤',
        steps: [
          {
            id: 'process-step:completed-typecheck',
            order: 0,
            kind: 'command',
            status: 'complete',
            title: '类型检查通过',
            detail: commandDetail({
              command: 'pnpm -C apps/desktop typecheck',
              stdoutPreview: 'passed',
              exitCode: 0,
              durationMs: 1500,
            }),
          },
          {
            id: 'process-step:failed-storybook',
            order: 1,
            kind: 'command',
            status: 'failed',
            title: 'Storybook 构建失败',
            detail: commandDetail({
              command: 'pnpm -C apps/desktop build-storybook',
              stdoutPreview: 'Missing i18n key: timeline.processGroup.history',
              exitCode: 1,
              durationMs: 6400,
            }),
          },
          {
            id: 'process-step:completed-summary',
            order: 2,
            kind: 'synthesis',
            status: 'complete',
            title: '整理失败摘要',
            body: 'run 已结束，但仍需要显示失败步骤。',
          },
        ],
      },
      {
        kind: 'text',
        id: 'segment:text:complete-with-failed-step',
        order: 1,
        messageId: 'assistant-message-complete-with-failed-step',
        body: '已保留失败步骤和后续处理线索。',
      },
    ],
  })
}

function repeatedSearchFailuresTurn(): ConversationTurn {
  return storyTurn('codex-repeated-search-failures', '查找相关文件。', {
    id: 'assistant:run-repeated-search-failures',
    runId: 'run-repeated-search-failures',
    status: 'running',
    segments: [
      {
        kind: 'toolGroup',
        id: 'segment:tools:repeated-search-failures',
        order: 0,
        attempts: [
          {
            id: 'tool:search-failure-1',
            order: 0,
            toolUseId: 'tool-search-failure-1',
            toolName: 'search_code',
            status: 'failed',
            failureSummary: '工具执行失败。可在详情中查看。',
          },
          {
            id: 'tool:search-failure-2',
            order: 1,
            toolUseId: 'tool-search-failure-2',
            toolName: 'search_code',
            status: 'failed',
            failureSummary: '工具执行失败。可在详情中查看。',
          },
          {
            id: 'tool:search-failure-3',
            order: 2,
            toolUseId: 'tool-search-failure-3',
            toolName: 'search_code',
            status: 'failed',
            failureSummary: '工具执行失败。可在详情中查看。',
          },
        ],
      },
    ],
  })
}

function bottomComposerOverlapTurns(): ConversationTurn[] {
  return Array.from({ length: 28 }, (_, index) => {
    const isLast = index === 27
    const turn = storyTurn(
      `bottom-reserve-${index}`,
      isLast ? '确认最后一条消息不会被 composer 遮挡。' : `继续第 ${index + 1} 轮。`,
      {
        id: `assistant:run-bottom-reserve-${index}`,
        runId: `run-bottom-reserve-${index}`,
        status: 'complete',
        segments: [
          {
            kind: 'text',
            id: `segment:text:bottom-reserve-${index}`,
            order: 0,
            messageId: `assistant-message-bottom-reserve-${index}`,
            body: isLast
              ? '最后一条 assistant 内容保留在底部 padding 之上。'
              : `第 ${index + 1} 轮已完成。`,
          },
        ],
      },
    )

    return {
      ...turn,
      conversationId: 'conversation-bottom-reserve',
      position: index,
    }
  })
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
