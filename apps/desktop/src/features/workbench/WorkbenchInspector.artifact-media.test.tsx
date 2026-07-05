import '@testing-library/jest-dom/vitest'

import { fireEvent, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { CommandClient } from '@/shared/tauri/commands'
import { createTestCommandClient } from '@/testing/command-client'
import { artifactRevision } from '@/testing/conversation-worktree-builders'
import {
  inspectorTurn,
  renderInspector,
  setupStore,
  validEvidenceContentHash,
  worktreePage,
} from './WorkbenchInspector.test-support'

describe('WorkbenchInspector artifact media pane', () => {
  it('keeps non-image media revisions metadata-only without content or image preview fetches', async () => {
    const getArtifactRevisionContent = vi.fn<CommandClient['getArtifactRevisionContent']>()
    const getArtifactMediaPreview = vi.fn<CommandClient['getArtifactMediaPreview']>()
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-video',
        revisionId: 'revision-video',
      },
    })
    renderInspector({
      ...createTestCommandClient({
        artifacts: {
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'Generated video.',
              id: 'artifact-video',
              kind: 'video',
              revisions: [
                {
                  kind: 'video',
                  media: {
                    kind: 'video',
                    mimeType: 'video/mp4',
                    sizeBytes: 2048,
                  },
                  revisionId: 'revision-video',
                  status: 'ready',
                  title: 'Generated video',
                  updatedAt: '2026-06-17T00:00:06.000Z',
                },
              ],
              status: 'ready',
              title: 'Generated video',
              updatedAt: '2026-06-17T00:00:06.000Z',
            },
          ],
        },
        artifactRevisionContent: getArtifactRevisionContent,
        conversationInspectorItem: {
          item: {
            kind: 'artifact',
            segment: {
              kind: 'artifact',
              id: 'segment-artifact-video',
              order: 2,
              artifactId: 'artifact-video',
              artifactKind: 'media',
              status: 'ready',
              source: 'assistant',
              title: 'Generated video',
              revision: artifactRevision({
                artifactId: 'artifact-video',
                revisionId: 'revision-video',
                kind: 'media',
                sourceRunId: 'run-inspector',
                title: 'Generated video',
                media: {
                  kind: 'video',
                  mimeType: 'video/mp4',
                  sizeBytes: 2048,
                },
              }),
            },
          },
        },
      }),
      getArtifactMediaPreview,
    })

    expect(await screen.findByText('Artifact content unavailable')).toBeInTheDocument()
    expect(getArtifactRevisionContent).not.toHaveBeenCalled()
    expect(getArtifactMediaPreview).not.toHaveBeenCalled()
  })

  it('exports artifact content by ref instead of copying loaded content', async () => {
    const originalClipboard = navigator.clipboard
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    const exportConversationEvidence = vi.fn().mockResolvedValue({
      byteLength: 21,
      contentType: 'text/plain; charset=utf-8',
      exportedAt: '2026-06-17T02:22:00.000Z',
      kind: 'artifact-content',
      path: '.jyowo/runtime/exports/evidence-artifact-content-fixture.txt',
      refId: 'evidence-artifact-inspector',
    })
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conversation-inspector',
        artifactId: 'artifact-inspector',
        revisionId: 'revision-inspector',
      },
    })
    try {
      renderInspector(
        createTestCommandClient({
          conversationWorktreePage: worktreePage([inspectorTurn()]),
          conversationEvidenceExport: exportConversationEvidence,
          artifactRevisionContent: {
            artifactId: 'artifact-inspector',
            byteLength: 21,
            content: 'real artifact content',
            contentBytes: 21,
            contentType: 'text/plain; charset=utf-8',
            hasMore: false,
            contentHash: validEvidenceContentHash,
            hashAlgorithm: 'blake3',
            kind: 'artifact-content',
            limitBytes: 65_536,
            maxBytes: 65_536,
            offsetBytes: 0,
            redactionState: 'clean',
            refId: 'evidence-artifact-inspector',
            returnedBytes: 21,
            revisionId: 'revision-inspector',
            totalBytes: 21,
            truncated: false,
          },
        }),
      )

      fireEvent.click(await screen.findByRole('button', { name: 'Export content' }))

      await waitFor(() =>
        expect(exportConversationEvidence).toHaveBeenCalledWith({
          conversationId: 'conversation-inspector',
          kind: 'artifact-content',
          refId: 'evidence-artifact-inspector',
        }),
      )
      expect(writeText).not.toHaveBeenCalled()
      expect(
        await screen.findByText('.jyowo/runtime/exports/evidence-artifact-content-fixture.txt'),
      ).toBeInTheDocument()
    } finally {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      })
    }
  })
})
