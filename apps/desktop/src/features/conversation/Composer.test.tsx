import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { ConversationModelCapability } from '@/shared/tauri/commands'

import { Composer } from './Composer'

const attachment = {
  blobRef: {
    contentHash: Array.from({ length: 32 }, () => 1),
    contentType: 'text/plain',
    id: '01J00000000000000000000000',
    size: 128,
  },
  id: 'attachment-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
  mimeType: 'text/plain',
  name: 'notes.txt',
  sizeBytes: 128,
}

const textOnlyCapability: ConversationModelCapability = {
  inputModalities: ['text'],
  outputModalities: ['text'],
  contextWindow: 128000,
  maxOutputTokens: 8192,
  streaming: true,
  toolCalling: true,
  reasoning: false,
  promptCache: false,
  structuredOutput: true,
}

const referenceCandidates = {
  artifacts: [{ id: 'artifact-001', label: 'Build notes' }],
  conversations: [],
  files: [
    {
      label: 'Composer.tsx',
      path: 'apps/desktop/src/features/conversation/Composer.tsx',
    },
  ],
  memories: [],
  mcpServers: [{ id: 'mcp-filesystem', label: 'Filesystem MCP' }],
  skills: [{ id: 'skill-review', label: 'Code review skill' }],
  tools: [{ id: 'builtin.grep', label: 'Search files' }],
}

describe('Composer', () => {
  it('submits typed text as structured draft', async () => {
    const onSubmit = vi.fn()

    render(<Composer onSubmit={onSubmit} />)

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Continue the setup' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({
        attachments: [],
        contextReferences: [],
        prompt: 'Continue the setup',
      }),
    )
  })

  it('submits with Enter and keeps Shift Enter as newline', async () => {
    const onSubmit = vi.fn()

    render(<Composer onSubmit={onSubmit} />)

    const input = screen.getByPlaceholderText('Ask Jyowo anything about this project...')
    fireEvent.change(input, {
      target: { value: 'First line' },
    })
    fireEvent.keyDown(input, { key: 'Enter', shiftKey: true })
    fireEvent.change(input, {
      target: { value: 'First line\nSecond line' },
    })
    fireEvent.keyDown(input, { key: 'Enter' })

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({
        attachments: [],
        contextReferences: [],
        prompt: 'First line\nSecond line',
      }),
    )
  })

  it('does not submit Enter while IME composition is active', () => {
    const onSubmit = vi.fn()

    render(<Composer onSubmit={onSubmit} />)

    const input = screen.getByPlaceholderText('Ask Jyowo anything about this project...')
    fireEvent.change(input, {
      target: { value: '输入中' },
    })
    fireEvent.keyDown(input, { isComposing: true, key: 'Enter' })

    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('blocks empty submit', () => {
    const onSubmit = vi.fn()

    render(<Composer onSubmit={onSubmit} />)

    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('gives all context buttons accessible names', () => {
    render(<Composer onSubmit={vi.fn()} />)

    expect(screen.getByRole('button', { name: 'Attach file' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Reference project object' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Command mode' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '>_' })).not.toBeInTheDocument()
  })

  it('renders model selection inside the composer toolbar', () => {
    const onModelConfigChange = vi.fn()

    render(
      <Composer
        modelConfigId=""
        modelConfigs={[
          {
            id: 'openai-work',
            label: 'OpenAI Work / gpt-5.4-mini',
          },
        ]}
        onModelConfigChange={onModelConfigChange}
        onSubmit={vi.fn()}
      />,
    )

    const modelSelector = screen.getByLabelText('Model') as HTMLSelectElement
    expect(modelSelector.closest('form')).not.toBeNull()

    fireEvent.change(modelSelector, { target: { value: 'openai-work' } })

    expect(onModelConfigChange).toHaveBeenCalledWith('openai-work')
  })

  it('disables attachments when the selected model only accepts text', () => {
    render(<Composer modelCapability={textOnlyCapability} onSubmit={vi.fn()} />)

    expect(screen.getByRole('button', { name: 'Attach file' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Reference project object' })).not.toBeDisabled()
  })

  it('disables attachments when the selected model capability is unknown', () => {
    render(<Composer modelCapability={null} onSubmit={vi.fn()} />)

    expect(screen.getByRole('button', { name: 'Attach file' })).toBeDisabled()
  })

  it('enables attachments when the selected model accepts media or files', () => {
    render(
      <Composer
        modelCapability={{
          ...textOnlyCapability,
          inputModalities: ['text', 'image'],
        }}
        onSubmit={vi.fn()}
      />,
    )

    expect(screen.getByRole('button', { name: 'Attach file' })).not.toBeDisabled()
  })

  it('passes accepted attachment modalities to the picker', async () => {
    const onPickAttachmentPath = vi.fn().mockResolvedValue(null)

    render(
      <Composer
        modelCapability={{
          ...textOnlyCapability,
          inputModalities: ['text', 'image', 'video', 'file'],
        }}
        onCreateAttachmentFromPath={vi.fn()}
        onPickAttachmentPath={onPickAttachmentPath}
        onSubmit={vi.fn()}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Attach file' }))

    await waitFor(() =>
      expect(onPickAttachmentPath).toHaveBeenCalledWith(['image', 'video', 'file']),
    )
  })

  it('adds an attachment chip from the picker and submits it', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        onCreateAttachmentFromPath={vi.fn().mockResolvedValue({ attachment })}
        onPickAttachmentPath={vi.fn().mockResolvedValue('/tmp/notes.txt')}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Attach file' }))
    expect(await screen.findByText('notes.txt')).toBeInTheDocument()

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Use this file' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          attachments: [attachment],
        }),
      ),
    )
  })

  it('adds and removes a reference chip before submit', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        onListReferenceCandidates={vi.fn().mockResolvedValue(referenceCandidates)}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Reference project object' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Composer.tsx' }))
    expect(screen.getByText('Composer.tsx')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Remove reference Composer.tsx' }))
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'No reference now' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          contextReferences: [],
        }),
      ),
    )
  })

  it('adds skill, tool, and MCP references from the picker', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        onListReferenceCandidates={vi.fn().mockResolvedValue(referenceCandidates)}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Reference project object' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Code review skill' }))
    fireEvent.click(screen.getByRole('button', { name: 'Reference project object' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Search files' }))
    fireEvent.click(screen.getByRole('button', { name: 'Reference project object' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Filesystem MCP' }))

    expect(screen.getByText('Code review skill')).toBeInTheDocument()
    expect(screen.getByText('Search files')).toBeInTheDocument()
    expect(screen.getByText('Filesystem MCP')).toBeInTheDocument()

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Use these capabilities' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          contextReferences: [
            { id: 'skill-review', kind: 'skill', label: 'Code review skill' },
            { id: 'builtin.grep', kind: 'tool', label: 'Search files' },
            {
              id: 'mcp-filesystem',
              kind: 'mcp_server',
              label: 'Filesystem MCP',
            },
          ],
        }),
      ),
    )
  })

  it('disables context controls and chip removal while pending', async () => {
    const { rerender } = render(
      <Composer
        onCreateAttachmentFromPath={vi.fn().mockResolvedValue({ attachment })}
        onPickAttachmentPath={vi.fn().mockResolvedValue('/tmp/notes.txt')}
        onSubmit={vi.fn()}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Attach file' }))
    expect(await screen.findByText('notes.txt')).toBeInTheDocument()

    rerender(<Composer onSubmit={vi.fn()} pending />)

    expect(screen.getByRole('button', { name: 'Attach file' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Reference project object' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Remove attachment notes.txt' })).toBeDisabled()
  })

  it('uses explicit composer modes for disabled and ready states', () => {
    const { rerender } = render(<Composer mode={{ kind: 'running-disabled' }} onSubmit={vi.fn()} />)

    expect(screen.getByPlaceholderText('Ask Jyowo anything about this project...')).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled()

    rerender(<Composer mode={{ kind: 'clarification-reply' }} onSubmit={vi.fn()} />)

    expect(screen.getByPlaceholderText('Ask Jyowo anything about this project...')).toBeEnabled()
  })

  it('shows a cancel action while a run is active', () => {
    const onCancelRun = vi.fn()

    render(
      <Composer
        mode={{ kind: 'running-disabled', canCancel: true }}
        onCancelRun={onCancelRun}
        onSubmit={vi.fn()}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Cancel run' }))

    expect(onCancelRun).toHaveBeenCalledTimes(1)
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled()
  })

  it('keeps text and chips when submit fails', async () => {
    render(
      <Composer
        onCreateAttachmentFromPath={vi.fn().mockResolvedValue({ attachment })}
        onPickAttachmentPath={vi.fn().mockResolvedValue('/tmp/notes.txt')}
        onSubmit={vi.fn().mockRejectedValue(new Error('Run failed'))}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Attach file' }))
    expect(await screen.findByText('notes.txt')).toBeInTheDocument()
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Keep this draft' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => expect(screen.getByDisplayValue('Keep this draft')).toBeInTheDocument())
    expect(screen.getByText('notes.txt')).toBeInTheDocument()
  })

  it('does not render a local runtime label inside the input surface', () => {
    render(<Composer onSubmit={vi.fn()} />)

    expect(screen.queryByText('Local')).not.toBeInTheDocument()
  })

  it('shows retry when an error is present', () => {
    const onRetry = vi.fn()

    render(<Composer errorMessage="Run failed" onRetry={onRetry} onSubmit={vi.fn()} />)

    expect(screen.getByText('Run failed')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(onRetry).toHaveBeenCalledTimes(1)
  })
})
