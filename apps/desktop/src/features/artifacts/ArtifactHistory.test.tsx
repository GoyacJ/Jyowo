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
    status: 'ready',
    title: 'Desktop foundation created',
  },
  {
    actionLabel: 'Inspect',
    description: 'Follow-up verification checklist',
    id: 'artifact-verification',
    kind: 'markdown',
    preview: 'pnpm check:desktop\ncargo fmt --all --check',
    status: 'pending',
    title: 'Verification notes',
  },
] as const

describe('ArtifactHistory', () => {
  it('renders artifact title, kind, status, and actions', () => {
    const onOpenArtifact = vi.fn()

    render(<ArtifactHistory artifacts={artifacts} onOpenArtifact={onOpenArtifact} />)

    const firstArtifact = screen.getByRole('article', { name: 'Desktop foundation created' })
    expect(within(firstArtifact).getByText('code')).toBeInTheDocument()
    expect(within(firstArtifact).getByText('Ready')).toBeInTheDocument()

    within(firstArtifact).getByRole('button', { name: 'Open preview' }).click()

    expect(onOpenArtifact).toHaveBeenCalledWith('artifact-desktop-foundation')
  })

  it('keeps missing artifacts tied to conversation work', () => {
    render(<ArtifactHistory artifacts={[]} />)

    expect(screen.getByText('No artifacts for this conversation.')).toBeInTheDocument()
    expect(
      screen.getByText('Artifacts appear after Jyowo produces reviewable work.'),
    ).toBeInTheDocument()
  })
})
