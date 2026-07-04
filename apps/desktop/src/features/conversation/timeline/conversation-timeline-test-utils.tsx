import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render } from '@testing-library/react'
import type { ReactNode } from 'react'

import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient, ConversationTurn, RunModelSnapshot } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import {
  artifactRevision,
  assistantWork,
  changeSetFile,
  commandDetail,
  diffDetail,
  permissionState,
} from '@/testing/conversation-worktree-builders'

export const timestamp = '2026-06-17T00:00:00.000Z'

const openAiRunModelSnapshot: RunModelSnapshot = {
  modelConfigId: 'provider-config-001',
  providerId: 'openai',
  modelId: 'gpt-4.1',
  displayName: 'GPT-4.1',
  protocol: 'responses',
}

const minimaxRunModelSnapshot: RunModelSnapshot = {
  ...openAiRunModelSnapshot,
  modelConfigId: 'minimax-config',
  providerId: 'minimax',
  modelId: 'MiniMax-M3',
  displayName: 'MiniMax M3',
  protocol: 'chat_completions',
}

export function resetTimelineTestState() {
  act(() => {
    uiStore.getState().clearTimelineScrollRequest()
    uiStore.getState().resetEvidenceDisclosure()
  })
}

export function renderTimelineWithClient(children: ReactNode, commandClient: CommandClient) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  })

  return render(
    <CommandClientProvider client={commandClient}>
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    </CommandClientProvider>,
  )
}

export function turn(
  finalBody: string,
  suffix = '001',
  permissionRequestId = 'request-001',
): ConversationTurn {
  return {
    id: `turn:user-message-${suffix}`,
    conversationId: 'conversation-001',
    position: 0,
    user: {
      id: `user:user-message-${suffix}`,
      messageId: `user-message-${suffix}`,
      body: 'Prompt',
      timestamp,
    },
    assistant: assistantWork({
      id: `assistant:run-${suffix}`,
      runId: `run-${suffix}`,
      model: openAiRunModelSnapshot,
      status: 'running',
      segments: [
        {
          kind: 'process',
          id: `segment:process:run-${suffix}`,
          order: 0,
          status: 'withheld',
          summary: '思考内容已折叠',
        },
        {
          kind: 'toolGroup',
          id: `segment:tools:tool-use-${suffix}`,
          order: 1,
          attempts: [
            {
              id: `tool:tool-use-${suffix}`,
              order: 0,
              toolUseId: `tool-use-${suffix}`,
              toolName: 'read_file',
              status: 'failed',
              permission: permissionState({
                id: `permission:${permissionRequestId}`,
                requestId: permissionRequestId,
                toolUseId: `tool-use-${suffix}`,
                status: 'approved',
              }),
              failureSummary: '工具执行失败。可在详情中查看。',
              eventRefs: [
                {
                  eventId: 'event-tool',
                  cursor: {
                    eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
                    conversationSequence: 3,
                  },
                },
              ],
            },
          ],
        },
        {
          kind: 'text',
          id: `segment:text:assistant-message-${suffix}`,
          order: 2,
          messageId: `assistant-message-${suffix}`,
          body: finalBody,
        },
      ],
    }),
  }
}

export function reasoningTurn(): ConversationTurn {
  return {
    ...turn('Final answer', 'reasoning'),
    assistant: assistantWork({
      id: 'assistant:run-reasoning',
      runId: 'run-reasoning',
      status: 'complete',
      segments: [
        {
          kind: 'process',
          id: 'segment:process:run-reasoning',
          order: 0,
          status: 'complete',
          summary: '已完成推理过程',
          steps: [
            {
              id: 'process-step:run-reasoning:summary',
              order: 0,
              kind: 'reasoning',
              status: 'complete',
              title: '推理过程',
              body: 'Checked project context.',
            },
            {
              id: 'process-step:run-reasoning:tool-plan',
              order: 1,
              kind: 'tool',
              status: 'complete',
              title: '准备使用 read_file',
            },
            {
              id: 'process-step:run-reasoning:tool-result',
              order: 2,
              kind: 'tool',
              status: 'complete',
              title: 'read_file 已完成',
            },
          ],
        },
        {
          kind: 'text',
          id: 'segment:text:assistant-message-reasoning',
          order: 1,
          messageId: 'assistant-message-reasoning',
          body: 'Final answer',
        },
      ],
    }),
  }
}

export function minimaxTurn(): ConversationTurn {
  return {
    id: 'turn:user-minimax',
    conversationId: 'conversation-minimax',
    position: 0,
    user: {
      id: 'user:user-minimax',
      messageId: 'user-minimax',
      body: '帮我生成一张海报图',
      timestamp,
    },
    assistant: assistantWork({
      id: 'assistant:run-minimax',
      runId: 'run-minimax',
      model: minimaxRunModelSnapshot,
      status: 'complete',
      segments: [
        {
          kind: 'process',
          id: 'segment:process:run-minimax',
          order: 0,
          status: 'running',
          summary: '正在检查可用的图像工具',
        },
        {
          kind: 'toolGroup',
          id: 'segment:tools:tool-minimax',
          order: 1,
          attempts: [
            {
              id: 'tool:tool-minimax',
              order: 0,
              toolUseId: 'tool-minimax',
              toolName: 'MiniMaxTextToImage',
              status: 'failed',
              permission: permissionState({
                id: 'permission:permission-minimax',
                requestId: 'permission-minimax',
                toolUseId: 'tool-minimax',
                status: 'approved',
              }),
              failureSummary: '工具执行失败。可在详情中查看。',
            },
          ],
        },
        {
          kind: 'artifact',
          id: 'segment:artifact:artifact-minimax',
          order: 2,
          artifactId: 'artifact-minimax',
          title: '海报生成提示词',
          summary: '可复用的图像生成提示词已准备好。',
          revision: artifactRevision({
            artifactId: 'artifact-minimax',
            revisionId: 'revision-minimax-001',
            kind: 'document',
            sourceRunId: 'run-minimax',
            title: '海报生成提示词',
            summary: '可复用的图像生成提示词已准备好。',
            contentRef: 'evidence-artifact-minimax',
          }),
        },
        {
          kind: 'text',
          id: 'segment:text:assistant-final',
          order: 3,
          messageId: 'assistant-final',
          body: '图像工具失败后，我保留了可复用的提示词和下一步建议。',
        },
      ],
    }),
  }
}

export function toolEvidenceTurn({
  conversationId = 'conversation-tool-evidence',
  runId = 'run-tool-evidence',
}: {
  conversationId?: string
  runId?: string
} = {}): ConversationTurn {
  return {
    id: 'turn:user-tool-evidence',
    conversationId,
    position: 0,
    user: {
      id: 'user:user-tool-evidence',
      messageId: 'user-tool-evidence',
      body: '检查工具执行过程',
      timestamp,
    },
    assistant: assistantWork({
      id: 'assistant:run-tool-evidence',
      runId,
      status: 'running',
      segments: [
        {
          kind: 'toolGroup',
          id: 'segment:tools:tool-evidence',
          order: 0,
          attempts: [
            {
              id: 'tool:read-file',
              order: 0,
              toolUseId: 'tool-read-file',
              toolName: 'read_file',
              status: 'completed',
              permission: permissionState({
                id: 'permission:read-file',
                requestId: 'permission-read-file',
                toolUseId: 'tool-read-file',
                status: 'approved',
              }),
            },
            {
              id: 'tool:list-files',
              order: 1,
              toolUseId: 'tool-list-files',
              toolName: 'list_files',
              status: 'completed',
            },
            {
              id: 'tool:exec-command',
              order: 2,
              toolUseId: 'tool-exec-command',
              toolName: 'exec_command',
              status: 'failed',
              failureSummary: '工具执行失败。可在详情中查看。',
            },
            {
              id: 'tool:search-code',
              order: 3,
              toolUseId: 'tool-search-code',
              toolName: 'search_code',
              status: 'running',
            },
            {
              id: 'tool:write-file',
              order: 4,
              toolUseId: 'tool-write-file',
              toolName: 'write_file',
              status: 'waitingPermission',
              permission: permissionState({
                id: 'permission:write-file',
                requestId: 'permission-write-file',
                toolUseId: 'tool-write-file',
                status: 'pending',
              }),
            },
          ],
        },
      ],
    }),
  }
}

export function processHistoryTurn(): ConversationTurn {
  return {
    id: 'turn:user-process-history',
    conversationId: 'conversation-process-history',
    position: 0,
    user: {
      id: 'user:user-process-history',
      messageId: 'user-process-history',
      body: '整理执行历史',
      timestamp,
    },
    assistant: assistantWork({
      id: 'assistant:run-process-history',
      runId: 'run-process-history',
      status: 'complete',
      segments: [
        {
          kind: 'process',
          id: 'segment:process:history',
          order: 0,
          status: 'failed',
          summary: '已结束但存在失败步骤',
          steps: [
            {
              id: 'process-step:read-package',
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
              id: 'process-step:search-timeline',
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
              id: 'process-step:rg-complete',
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
              id: 'process-step:test-failed',
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
            {
              id: 'process-step:lint-non-zero',
              order: 4,
              kind: 'command',
              status: 'complete',
              title: 'lint 退出码非零',
              detail: commandDetail({
                command: 'pnpm -C apps/desktop lint',
                stdoutPreview: 'lint errors',
                exitCode: 2,
                durationMs: 900,
              }),
            },
          ],
        },
      ],
    }),
  }
}

export function imageProcessTurn(): ConversationTurn {
  return {
    id: 'turn:user-image',
    conversationId: 'conversation-image',
    position: 0,
    user: {
      id: 'user:user-image',
      messageId: 'user-image',
      body: '生成一张草鱼图片',
      timestamp,
    },
    assistant: assistantWork({
      id: 'assistant:run-image',
      runId: 'run-image',
      status: 'complete',
      segments: [
        {
          kind: 'process',
          id: 'segment:process:run-image',
          order: 0,
          status: 'complete',
          summary: '已完成工作过程',
          steps: [
            {
              id: 'process-step:reasoning',
              order: 0,
              kind: 'reasoning',
              status: 'complete',
              title: '分析请求',
              body: '确认需要生成图片并展示结果。',
            },
            {
              id: 'process-step:activity',
              order: 1,
              kind: 'fileSearch',
              status: 'complete',
              title: '已搜索图片工具',
              detail: {
                type: 'activity',
                summary: '已搜索图片工具',
                itemCount: 1,
              },
            },
            {
              id: 'process-step:command',
              order: 2,
              kind: 'command',
              status: 'complete',
              title: '运行检查',
              detail: commandDetail({
                command: 'pnpm check:desktop',
                stdoutPreview: 'passed',
                exitCode: 0,
                durationMs: 1200,
              }),
            },
            {
              id: 'process-step:diff',
              order: 3,
              kind: 'diff',
              status: 'complete',
              title: '更新图片展示',
              detail: diffDetail({
                id: 'change-set-image',
                summary: '更新图片展示',
                files: [
                  changeSetFile({
                    path: 'apps/desktop/src/features/conversation/timeline/artifact-segment-view.tsx',
                    addedLines: 1,
                    removedLines: 0,
                    preview: '+ render process preview',
                  }),
                ],
              }),
            },
            {
              id: 'process-step:artifact',
              order: 4,
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
          kind: 'artifact',
          id: 'segment:artifact:artifact-image-001',
          order: 1,
          artifactId: 'artifact-image-001',
          artifactKind: 'image',
          status: 'ready',
          source: 'tool',
          title: 'Generated image',
          summary: 'Image artifact ready',
          revision: artifactRevision({
            artifactId: 'artifact-image-001',
            revisionId: 'revision-image-001',
            kind: 'image',
            sourceRunId: 'run-image',
            title: 'Generated image',
            summary: 'Image artifact ready',
            previewRef: 'evidence-preview-image-001',
            contentRef: 'evidence-content-image-001',
            media: {
              kind: 'image',
              mimeType: 'image/png',
              sizeBytes: 68,
            },
          }),
        },
        {
          kind: 'text',
          id: 'segment:text:assistant-final-image',
          order: 2,
          messageId: 'assistant-final-image',
          body: '图片已生成。',
        },
      ],
    }),
  }
}
