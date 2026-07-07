import type { ConversationTurn } from '@/shared/tauri/commands'
import {
  assistantWork,
  changeSetFile,
  commandDetail,
  diffDetail,
} from '@/testing/conversation-worktree-builders'

type AttachmentReference = NonNullable<ConversationTurn['user']['attachments']>[number]

function attachment(
  hex: string,
  name: string,
  mimeType: string,
  sizeBytes: number,
): AttachmentReference {
  const byte = Number.parseInt(hex.slice(0, 2), 16)

  return {
    blobRef: {
      contentHash: Array.from({ length: 32 }, () => byte),
      contentType: mimeType,
      id: `blob-${name.replace(/[^a-z0-9]/gi, '-').toLowerCase()}`,
      size: sizeBytes,
    },
    id: `attachment-${hex.repeat(64).slice(0, 64)}`,
    mimeType,
    name,
    sizeBytes,
  }
}

export const codexStyleEvidenceTurns: ConversationTurn[] = [
  {
    id: 'turn:codex-evidence',
    conversationId: 'conversation-codex-evidence',
    position: 0,
    user: {
      id: 'user:codex-evidence',
      messageId: 'user-message-codex-evidence',
      body: '请按 Codex 风格把这次红测、文件修改和失败命令展示在同一条对话里。',
      attachments: [
        attachment('0123456789abcdef', 'reference.png', 'image/png', 2048),
        attachment('fedcba9876543210', 'notes.txt', 'text/plain', 128),
      ],
      timestamp: '2026-06-28T00:00:00.000Z',
    },
    assistant: assistantWork({
      id: 'assistant:run-codex-evidence',
      runId: 'run-codex-evidence',
      status: 'complete',
      segments: [
        {
          kind: 'text',
          id: 'segment:text:codex-progress',
          order: 0,
          messageId: 'assistant-message-codex-progress',
          body: 'RED 测试已就位。现在保留失败命令、diff 和工具结果，方便继续修复。',
        },
        {
          kind: 'process',
          id: 'segment:process:codex-evidence',
          order: 1,
          status: 'failed',
          summary: '正在整理证据链',
          steps: [
            {
              id: 'process-step:file-edit',
              order: 0,
              kind: 'fileEdit',
              status: 'complete',
              title: '已编辑 1 个文件',
              detail: {
                type: 'activity',
                summary: '已编辑的文件',
                itemCount: 1,
              },
            },
            {
              id: 'process-step:diff',
              order: 1,
              kind: 'diff',
              status: 'complete',
              title: 'SkillsPage.test.tsx +61 -2',
              detail: diffDetail({
                id: 'change-set-skills-page',
                summary: 'SkillsPage.test.tsx +61 -2',
                files: [
                  changeSetFile({
                    path: 'SkillsPage.test.tsx',
                    addedLines: 61,
                    removedLines: 2,
                    preview: [
                      '--- a/SkillsPage.test.tsx',
                      '+++ b/SkillsPage.test.tsx',
                      '@@ -12,7 +12,9 @@',
                      ' describe("SkillsPage", () => {',
                      '-  it("renders skills", () => {',
                      '-    expect(screen.getByText("old")).toBeInTheDocument()',
                      '+  it("renders enabled and disabled skills", () => {',
                      '+    expect(screen.getByText("Enabled")).toBeInTheDocument()',
                      '+    expect(screen.getByText("Disabled")).toBeInTheDocument()',
                      '   })',
                      ' })',
                    ].join('\n'),
                  }),
                ],
              }),
            },
            {
              id: 'process-step:command-failed',
              order: 2,
              kind: 'command',
              status: 'failed',
              title: '已运行命令，已持续 12s',
              detail: commandDetail({
                command: 'pnpm -C apps/desktop test -- SkillsPage',
                stdoutPreview:
                  'FAIL src/features/skills/SkillsPage.test.tsx\nExpected element not found.',
                exitCode: 1,
                durationMs: 12000,
              }),
            },
            {
              id: 'process-step:command-complete',
              order: 3,
              kind: 'command',
              status: 'complete',
              title: '已运行 1 条历史命令',
              detail: commandDetail({
                command: 'rg "SkillsPage" apps/desktop/src',
                stdoutPreview: 'apps/desktop/src/features/skills/SkillsPage.tsx',
                exitCode: 0,
                durationMs: 320,
              }),
            },
          ],
        },
        {
          kind: 'toolGroup',
          id: 'segment:tools:codex-evidence',
          order: 2,
          attempts: [
            {
              id: 'tool:read-skills-page',
              order: 0,
              toolUseId: 'tool-read-skills-page',
              toolName: 'read_file',
              status: 'completed',
            },
            {
              id: 'tool:test-skills-page',
              order: 1,
              toolUseId: 'tool-test-skills-page',
              toolName: 'exec_command',
              status: 'failed',
              failureSummary: '工具执行失败。可在详情中查看。',
            },
          ],
        },
        {
          kind: 'notice',
          id: 'segment:notice:context-compacted',
          order: 3,
          code: 'contextCompacted',
          body: '上下文已自动压缩',
        },
        {
          kind: 'text',
          id: 'segment:text:codex-final',
          order: 4,
          messageId: 'assistant-message-codex-final',
          body: '红测和失败证据已经保留，下一步修复实现。',
        },
      ],
    }),
  },
]

export const codexAttachmentStressTurns: ConversationTurn[] = [
  {
    ...codexStyleEvidenceTurns[0],
    id: 'turn:codex-attachment-stress',
    user: {
      ...codexStyleEvidenceTurns[0].user,
      id: 'user:codex-attachment-stress',
      messageId: 'user-message-codex-attachment-stress',
      attachments: [
        attachment('11', 'reference.png', 'image/png', 2048),
        attachment('22', 'wireframe.png', 'image/png', 4096),
        attachment('33', 'notes.txt', 'text/plain', 128),
        attachment('44', 'trace.log', 'text/plain', 1536),
        attachment('55', 'report.pdf', 'application/pdf', 32768),
      ],
    },
  },
]

export const codexLargeDiffTurns: ConversationTurn[] = [
  {
    ...codexStyleEvidenceTurns[0],
    id: 'turn:codex-large-diff',
    assistant: codexStyleEvidenceTurns[0].assistant
      ? {
          ...codexStyleEvidenceTurns[0].assistant,
          id: 'assistant:run-codex-large-diff',
          runId: 'run-codex-large-diff',
          segments: codexStyleEvidenceTurns[0].assistant.segments.map((segment) => {
            if (segment.kind !== 'process') {
              return segment
            }

            return {
              ...segment,
              id: 'segment:process:codex-large-diff',
              steps: (segment.steps ?? []).map((step) => {
                if (step.detail?.type !== 'diff') {
                  return step
                }

                return {
                  ...step,
                  id: 'process-step:large-diff',
                  title: 'ConversationTimeline.test.tsx +140 -12',
                  detail: diffDetail({
                    id: 'change-set-large-diff',
                    summary: 'ConversationTimeline.test.tsx +140 -12',
                    files: [
                      changeSetFile({
                        path: 'apps/desktop/src/features/conversation/timeline/ConversationTimeline.test.tsx',
                        addedLines: 140,
                        removedLines: 12,
                        preview: largeDiffPreview(),
                      }),
                    ],
                  }),
                }
              }),
            }
          }),
        }
      : undefined,
  },
]

export const codexRenderBlockMatrixTurns: ConversationTurn[] = [
  {
    id: 'turn:codex-render-block-matrix',
    conversationId: 'conversation-render-block-matrix',
    position: 0,
    user: {
      id: 'user:codex-render-block-matrix',
      messageId: 'user-message-codex-render-block-matrix',
      body: 'Show every timeline render block regression state in one projected turn.',
      timestamp: '2026-06-28T00:00:00.000Z',
    },
    assistant: assistantWork({
      id: 'assistant:run-render-block-matrix',
      runId: 'run-render-block-matrix',
      status: 'running',
      durationMs: 2345,
      segments: [
        {
          kind: 'text',
          id: 'segment:text:render-block-matrix',
          order: 0,
          messageId: 'assistant-message-render-block-matrix',
          body: 'Rendering file edits, activity, commands, and withheld output from safe projection data.',
        },
        {
          kind: 'process',
          id: 'segment:process:render-block-matrix',
          order: 1,
          status: 'running',
          summary: 'Rendering regression matrix',
          steps: [
            {
              id: 'process-step:matrix-file-edit',
              order: 0,
              kind: 'fileEdit',
              status: 'complete',
              title: 'Edited 2 files',
              detail: {
                type: 'activity',
                summary: 'Edited files',
                itemCount: 2,
              },
            },
            {
              id: 'process-step:matrix-diff',
              order: 1,
              kind: 'diff',
              status: 'complete',
              title: 'timeline render blocks +68 -3',
              detail: diffDetail({
                id: 'change-set-render-block-matrix',
                summary: 'timeline render blocks +68 -3',
                files: [
                  changeSetFile({
                    path: 'apps/desktop/src/features/conversation/timeline/timeline-render-blocks.ts',
                    addedLines: 41,
                    removedLines: 3,
                    preview:
                      '@@ -1,1 +1,2 @@\n export const oldValue = 1\n+export const newValue = 2',
                  }),
                  changeSetFile({
                    path: 'apps/desktop/src/features/conversation/timeline/timeline-render-blocks.test.ts',
                    status: 'added',
                    addedLines: 27,
                    removedLines: 0,
                    preview: '@@ -0,0 +1,2 @@\n+describe("timeline blocks", () => {})',
                  }),
                ],
              }),
            },
            {
              id: 'process-step:matrix-read',
              order: 2,
              kind: 'fileRead',
              status: 'complete',
              title: 'Read timeline files',
              detail: {
                type: 'activity',
                summary: 'Read timeline files',
                itemCount: 1,
                items: [
                  {
                    kind: 'file',
                    label:
                      'apps/desktop/src/features/conversation/timeline/timeline-render-blocks.ts',
                    detail: 'read',
                  },
                ],
              },
            },
            {
              id: 'process-step:matrix-search',
              order: 3,
              kind: 'fileSearch',
              status: 'complete',
              title: 'Searched render blocks',
              detail: {
                type: 'activity',
                summary: 'Searched render blocks',
                itemCount: 3,
              },
            },
            {
              id: 'process-step:matrix-command-running',
              order: 4,
              kind: 'command',
              status: 'running',
              title: 'Running desktop tests',
              detail: commandDetail({
                command: 'pnpm -C apps/desktop test --watch',
                stdoutPreview: largeCommandPreview(),
                durationMs: 5100,
                fullOutputRef: 'render-matrix-large-output-ref',
              }),
            },
            {
              id: 'process-step:matrix-command-withheld',
              order: 5,
              kind: 'command',
              status: 'failed',
              title: 'Read sensitive env',
              detail: commandDetail({
                command: 'cat .env',
                stdoutPreview: 'Output withheld from conversation timeline.',
                redactionState: 'withheld',
                fullOutputRef: 'render-matrix-withheld-output-ref',
              }),
            },
          ],
        },
      ],
    }),
  },
]

function largeDiffPreview() {
  const lines = ['@@ -10,6 +10,120 @@', ' describe("evidence timeline", () => {']

  for (let index = 0; index < 120; index += 1) {
    if (index % 15 === 0) {
      lines.push(`-  expect(oldState${index}).toBeVisible()`)
    }
    lines.push(`+  expect(evidenceRow${index}).toHaveTextContent("row ${index}")`)
  }

  lines.push(' })')

  return lines.join('\n')
}

function largeCommandPreview() {
  return Array.from({ length: 120 }, (_, index) => `large output line ${index}`).join('\n')
}
