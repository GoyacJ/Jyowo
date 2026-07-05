import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { commandDetail } from '@/testing/conversation-worktree-builders'
import { CommandEvidenceBlock } from './command-evidence-block'

const originalClipboard = navigator.clipboard

describe('CommandEvidenceBlock', () => {
  afterEach(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: originalClipboard,
    })
  })

  it('shows an error when copying visible output fails', async () => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error('denied')) },
    })

    render(
      <CommandEvidenceBlock
        execution={commandDetail({
          command: 'pnpm test',
          stdoutPreview: 'test output',
        })}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Copy output' }))

    expect(await screen.findByText('Copy failed')).toBeInTheDocument()
  })
})
