import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { UiState } from '@/shared/state/ui-store'
import { uiStore } from '@/shared/state/ui-store'
import type {
  CommandClient,
  ConversationTurn,
  PageConversationWorktreeResponse,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import {
  artifactRevision,
  assistantWork,
  changeSetFile,
  commandDetail,
  diffDetail,
  permissionState,
} from '@/testing/conversation-worktree-builders'
import { WorkbenchInspector } from './WorkbenchInspector'

export const validEvidenceContentHash = 'd'.repeat(64)

export function setupStore(overrides?: Partial<UiState>) {
  uiStore.setState({
    inspectorOpen: true,
    workbenchSelection: null,
    ...overrides,
  } as Partial<UiState>)
}

export function createInspectorQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  })
}

export function renderInspector(
  commandClient: CommandClient = createTestCommandClient(),
  queryClient = createInspectorQueryClient(),
  contextPane?: ReactNode,
) {
  return render(
    <CommandClientProvider client={commandClient}>
      <QueryClientProvider client={queryClient}>
        <WorkbenchInspector contextPane={contextPane} />
      </QueryClientProvider>
    </CommandClientProvider>,
  )
}

export function worktreePage(turns: ConversationTurn[]): PageConversationWorktreeResponse {
  return {
    turns,
    hasMoreBefore: false,
    hasMoreAfter: false,
    gap: false,
  }
}

export function inspectorTurn(): ConversationTurn {
  return {
    id: 'turn-inspector',
    conversationId: 'conversation-inspector',
    position: 0,
    user: {
      id: 'user-inspector',
      messageId: 'message-user-inspector',
      body: 'Inspect this run',
      timestamp: '2026-06-17T00:00:00.000Z',
    },
    assistant: assistantWork({
      id: 'assistant-inspector',
      runId: 'run-inspector',
      status: 'complete',
      segments: [
        {
          kind: 'process',
          id: 'segment-process-inspector',
          order: 0,
          status: 'complete',
          summary: 'Collected execution evidence',
          steps: [
            {
              id: 'step-command-inspector',
              order: 0,
              kind: 'command',
              status: 'complete',
              title: 'Ran desktop checks',
              detail: commandDetail({
                command: 'pnpm check:desktop',
                stdoutPreview: 'desktop checks passed',
                fullOutputRef: 'evidence-command-inspector',
                exitCode: 0,
              }),
            },
            {
              id: 'step-diff-inspector',
              order: 1,
              kind: 'diff',
              status: 'complete',
              title: 'Updated inspector',
              detail: diffDetail({
                id: 'change-set-inspector',
                summary: 'Updated inspector UI',
                files: [
                  changeSetFile({
                    path: 'apps/desktop/src/features/workbench/WorkbenchInspector.tsx',
                    addedLines: 12,
                    removedLines: 2,
                    preview: '+ render real inspector pane',
                    fullPatchRef: 'evidence-diff-inspector',
                  }),
                ],
              }),
            },
          ],
        },
        {
          kind: 'toolGroup',
          id: 'segment-tools-inspector',
          order: 1,
          attempts: [
            {
              id: 'tool-attempt-inspector',
              order: 0,
              toolUseId: 'tool-use-inspector',
              toolName: 'read_file',
              status: 'completed',
              outputSummary: 'Read WorkbenchInspector.tsx',
              durationMs: 23,
              permission: permissionState({
                id: 'permission-inspector',
                requestId: 'request-inspector',
                status: 'approved',
                toolUseId: 'tool-use-inspector',
              }),
            },
          ],
        },
        {
          kind: 'artifact',
          id: 'segment-artifact-inspector',
          order: 2,
          artifactId: 'artifact-inspector',
          title: 'Inspector notes',
          revision: artifactRevision({
            artifactId: 'artifact-inspector',
            revisionId: 'revision-inspector',
            kind: 'document',
            sourceRunId: 'run-inspector',
            title: 'Inspector notes',
            summary: 'Implementation notes',
            contentRef: 'evidence-artifact-inspector',
          }),
        },
      ],
    }),
  }
}
