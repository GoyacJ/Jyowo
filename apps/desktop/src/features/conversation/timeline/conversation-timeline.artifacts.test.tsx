import '@testing-library/jest-dom/vitest'

import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { CommandClient } from '@/shared/tauri/commands'
import { createTestCommandClient } from '@/testing/command-client'
import {
  codexAttachmentStressTurns,
  codexStyleEvidenceTurns,
} from '@/testing/conversation-evidence-fixtures'
import { ConversationTimeline } from './conversation-timeline'
import {
  imageProcessTurn,
  renderTimelineWithClient,
  resetTimelineTestState,
} from './conversation-timeline-test-utils'

describe('ConversationTimeline', () => {
  afterEach(() => {
    resetTimelineTestState()
  })

  it('renders process steps and image artifact previews from the safe projection', async () => {
    const previewRequests: Array<Parameters<CommandClient['getArtifactMediaPreview']>[0]> = []
    const dataUrl =
      'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII='
    const commandClient = {
      ...createTestCommandClient(),
      getArtifactMediaPreview: async (
        request: Parameters<CommandClient['getArtifactMediaPreview']>[0],
      ) => {
        previewRequests.push(request)
        return {
          dataUrl,
          mimeType: 'image/png',
          sizeBytes: 68,
        }
      },
    } satisfies CommandClient

    renderTimelineWithClient(
      <ConversationTimeline title="Image flow" turns={[imageProcessTurn()]} />,
      commandClient,
    )

    expect(screen.getByText('确认需要生成图片并展示结果。')).toBeInTheDocument()
    expect(screen.queryByText('已搜索图片工具')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Collapsed 1 history steps/ }))
    expect(screen.getByText('已搜索图片工具')).toBeInTheDocument()
    expect(screen.getByText('$ pnpm check:desktop')).toBeInTheDocument()
    expect(screen.getByText(/render process preview/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Copy diff' })).not.toBeInTheDocument()
    expect(screen.getByText('Generated image')).toBeInTheDocument()
    expect(screen.queryByText('Image artifact ready')).not.toBeInTheDocument()
    expect(screen.getByText('图片已生成。')).toBeInTheDocument()

    const image = await screen.findByRole('img', { name: 'Generated image' })
    expect(image).toHaveAttribute('src', dataUrl)
    expect(previewRequests).toEqual([
      {
        conversationId: 'conversation-image',
        artifactId: 'artifact-image-001',
        revisionId: 'revision-image-001',
      },
    ])
    expect(document.body.textContent ?? '').not.toContain('[REDACTED]')
  })

  it('uses the process artifact revision id before artifact segment fallbacks', async () => {
    const turn = imageProcessTurn()
    const process = turn.assistant?.segments.find((segment) => segment.kind === 'process')
    const artifactStep = process?.steps?.find((step) => step.kind === 'artifact')
    if (artifactStep?.detail?.type === 'artifact') {
      artifactStep.detail.revisionId = 'revision-image-process-old'
    }
    const artifact = turn.assistant?.segments.find((segment) => segment.kind === 'artifact')
    if (artifact?.kind === 'artifact') {
      artifact.revision.revisionId = 'revision-image-latest'
    }
    const getArtifactMediaPreview = vi.fn<CommandClient['getArtifactMediaPreview']>(async () => ({
      dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
      mimeType: 'image/png',
      sizeBytes: 68,
    }))

    renderTimelineWithClient(<ConversationTimeline title="Image flow" turns={[turn]} />, {
      ...createTestCommandClient(),
      getArtifactMediaPreview,
    })

    await screen.findByRole('img', { name: 'Generated image' })
    expect(getArtifactMediaPreview).toHaveBeenCalledWith({
      conversationId: 'conversation-image',
      artifactId: 'artifact-image-001',
      revisionId: 'revision-image-process-old',
    })
  })

  it('renders historical attachments before the user message without fetching blob content', () => {
    const getArtifactMediaPreview = vi.fn()
    renderTimelineWithClient(
      <ConversationTimeline title="Evidence conversation" turns={codexStyleEvidenceTurns} />,
      {
        ...createTestCommandClient(),
        getArtifactMediaPreview,
      },
    )

    const article = screen.getByLabelText('Conversation turn')
    const reference = screen.getByText('reference.png')
    const prompt = screen.getByText(
      '请按 Codex 风格把这次红测、文件修改和失败命令展示在同一条对话里。',
    )

    expect(article.compareDocumentPosition(reference) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    )
    expect(reference.compareDocumentPosition(prompt) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    )
    expect(screen.getByText('image/png')).toBeInTheDocument()
    expect(getArtifactMediaPreview).not.toHaveBeenCalled()
  })

  it('keeps image attachments as metadata chips when safe preview fails', async () => {
    const getAttachmentMediaPreview = vi.fn().mockRejectedValue(new Error('preview unavailable'))
    const getArtifactMediaPreview = vi.fn()
    const imageAttachment = codexStyleEvidenceTurns[0].user.attachments?.[0]
    renderTimelineWithClient(
      <ConversationTimeline title="Evidence conversation" turns={codexStyleEvidenceTurns} />,
      {
        ...createTestCommandClient(),
        getArtifactMediaPreview,
        getAttachmentMediaPreview,
      },
    )

    await waitFor(() => {
      expect(getAttachmentMediaPreview).toHaveBeenCalledWith({
        conversationId: codexStyleEvidenceTurns[0].conversationId,
        attachmentId: imageAttachment?.id,
      })
    })
    expect(screen.getByText('reference.png')).toBeInTheDocument()
    expect(screen.getByText('image/png')).toBeInTheDocument()
    expect(screen.queryByRole('img', { name: 'reference.png' })).not.toBeInTheDocument()
    expect(getArtifactMediaPreview).not.toHaveBeenCalled()
  })

  it('renders image attachment thumbnails through safe preview without fetch or artifact preview', async () => {
    const getAttachmentMediaPreview = vi.fn().mockResolvedValue({
      dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
      mimeType: 'image/png',
      sizeBytes: 67,
    })
    const getArtifactMediaPreview = vi.fn()
    const fetchSpy = vi.fn()
    const imageAttachment = codexStyleEvidenceTurns[0].user.attachments?.[0]
    vi.stubGlobal('fetch', fetchSpy)

    try {
      renderTimelineWithClient(
        <ConversationTimeline title="Evidence conversation" turns={codexStyleEvidenceTurns} />,
        {
          ...createTestCommandClient(),
          getArtifactMediaPreview,
          getAttachmentMediaPreview,
        },
      )

      expect(await screen.findByRole('img', { name: 'reference.png' })).toHaveAttribute(
        'src',
        'data:image/png;base64,iVBORw0KGgo=',
      )
      expect(getAttachmentMediaPreview).toHaveBeenCalledWith({
        conversationId: codexStyleEvidenceTurns[0].conversationId,
        attachmentId: imageAttachment?.id,
      })
      expect(getArtifactMediaPreview).not.toHaveBeenCalled()
      expect(fetchSpy).not.toHaveBeenCalled()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('renders attachment chips as a right-aligned metadata strip with internal overflow', () => {
    const getArtifactMediaPreview = vi.fn()
    const fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)

    try {
      renderTimelineWithClient(
        <ConversationTimeline title="Attachment evidence" turns={codexAttachmentStressTurns} />,
        {
          ...createTestCommandClient(),
          getArtifactMediaPreview,
        },
      )

      const attachmentStrip = screen.getByLabelText('User attachments')
      expect(attachmentStrip).toHaveClass('ml-auto')
      expect(attachmentStrip.parentElement).toHaveClass('overflow-x-auto')
      expect(within(attachmentStrip).getAllByRole('listitem')).toHaveLength(5)

      const reference = screen.getByText('reference.png')
      const prompt = screen.getByText(
        '请按 Codex 风格把这次红测、文件修改和失败命令展示在同一条对话里。',
      )
      expect(reference.compareDocumentPosition(prompt) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING,
      )

      expect(screen.getAllByText('image/png')).not.toHaveLength(0)
      expect(screen.getByText('report.pdf')).toBeInTheDocument()
      expect(screen.getByText('32 KB')).toBeInTheDocument()
      expect(screen.queryByRole('img', { name: /reference\.png/ })).not.toBeInTheDocument()
      expect(getArtifactMediaPreview).not.toHaveBeenCalled()
      expect(fetchSpy).not.toHaveBeenCalled()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('shows a safe placeholder when a process image artifact preview is loading or unavailable', async () => {
    const commandClient = {
      ...createTestCommandClient(),
      getArtifactMediaPreview: () => Promise.reject(new Error('preview unavailable')),
    } satisfies CommandClient

    renderTimelineWithClient(
      <ConversationTimeline title="Image flow" turns={[imageProcessTurn()]} />,
      commandClient,
    )

    expect(screen.getByText('Generated artifact')).toBeInTheDocument()
    expect(await screen.findByText('Image preview unavailable')).toBeInTheDocument()
    expect(document.body.textContent ?? '').not.toContain('/Users/')
    expect(document.body.textContent ?? '').not.toContain('.jyowo')
  })
})
