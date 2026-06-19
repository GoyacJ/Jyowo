import '@testing-library/jest-dom/vitest'

import { render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ArtifactHistory } from './ArtifactHistory'

const artifacts = [
  {
    actionLabel: 'Open preview',
    description: 'Tauri + React + TypeScript with Vite',
    id: 'artifact-desktop-foundation',
    kind: 'code',
    preview: 'src-tauri command boundary and renderer shell',
    sourceMessageId: 'message-002',
    sourceRunId: 'run-001',
    status: 'ready',
    title: 'Desktop foundation created',
  },
  {
    actionLabel: 'Inspect',
    description: 'Follow-up verification checklist',
    id: 'artifact-verification',
    kind: 'markdown',
    preview: 'pnpm check:desktop\ncargo fmt --all --check',
    sourceMessageId: 'message-003',
    sourceRunId: 'run-002',
    status: 'pending',
    title: 'Verification notes',
  },
] as const

describe('ArtifactHistory', () => {
  it('renders artifact title, kind, status, source run, and actions', () => {
    const onOpenArtifact = vi.fn()
    const onOpenSource = vi.fn()

    render(
      <ArtifactHistory
        artifacts={artifacts}
        onOpenArtifact={onOpenArtifact}
        onOpenSource={onOpenSource}
      />,
    )

    const firstArtifact = screen.getByRole('article', { name: 'Desktop foundation created' })
    expect(within(firstArtifact).getByText('code')).toBeInTheDocument()
    expect(within(firstArtifact).getByText('Ready')).toBeInTheDocument()
    expect(within(firstArtifact).getByText('run-001')).toBeInTheDocument()

    within(firstArtifact).getByRole('button', { name: 'Open preview' }).click()
    within(firstArtifact).getByRole('button', { name: 'Show source message' }).click()

    expect(onOpenArtifact).toHaveBeenCalledWith('artifact-desktop-foundation')
    expect(onOpenSource).toHaveBeenCalledWith('message-002')
  })

  it('keeps missing artifacts tied to conversation work', () => {
    render(<ArtifactHistory artifacts={[]} />)

    expect(screen.getByText('No artifacts for this conversation.')).toBeInTheDocument()
    expect(
      screen.getByText('Artifacts appear after Jyowo produces reviewable work.'),
    ).toBeInTheDocument()
  })
})
