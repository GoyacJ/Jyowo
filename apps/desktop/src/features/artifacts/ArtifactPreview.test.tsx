import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { ArtifactPreview } from './ArtifactPreview'

describe('ArtifactPreview', () => {
  it('renders loading, error, and ready states', () => {
    const { rerender } = render(<ArtifactPreview state="loading" title="Desktop foundation" />)

    expect(screen.getByText('Loading artifact preview.')).toBeInTheDocument()

    rerender(
      <ArtifactPreview
        errorMessage="Preview unavailable"
        state="error"
        title="Desktop foundation"
      />,
    )

    expect(screen.getByText('Preview unavailable')).toBeInTheDocument()

    rerender(
      <ArtifactPreview
        content="src-tauri command boundary and renderer shell"
        kind="code"
        state="ready"
        title="Desktop foundation"
      />,
    )

    expect(screen.getByRole('region', { name: 'Desktop foundation preview' })).toBeInTheDocument()
    expect(screen.getByText('src-tauri command boundary and renderer shell')).toBeInTheDocument()
  })

  it('uses a large preview fallback instead of rendering all content', () => {
    const largeContent = `${'x'.repeat(1200)}\n${'y'.repeat(1200)}`

    render(
      <ArtifactPreview
        content={largeContent}
        kind="markdown"
        maxPreviewCharacters={128}
        state="ready"
        title="Large artifact"
      />,
    )

    expect(screen.getByText('Large preview truncated.')).toBeInTheDocument()
    expect(screen.getByText('Open artifact to inspect the full output.')).toBeInTheDocument()
    expect(screen.queryByText(largeContent)).not.toBeInTheDocument()
  })
})
