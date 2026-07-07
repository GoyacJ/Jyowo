import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { Copy, ExternalLink, FileText } from 'lucide-react'
import { describe, expect, it, vi } from 'vitest'
import { EvidenceDisclosure } from './evidence-disclosure'

describe('EvidenceDisclosure', () => {
  it('renders icon, title, metadata, chevron, and body', () => {
    render(
      <EvidenceDisclosure
        icon={FileText}
        id="block-1"
        meta={<span>2 files</span>}
        onOpenChange={vi.fn()}
        open
        title="Edited files"
      >
        <p>src/app.ts</p>
      </EvidenceDisclosure>,
    )

    expect(screen.getByRole('button', { name: /Edited files/ })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
    expect(screen.getByText('2 files')).toBeInTheDocument()
    expect(screen.getByText('src/app.ts')).toBeInTheDocument()
    expect(screen.getByTestId('evidence-disclosure-icon')).toBeInTheDocument()
    expect(screen.getByTestId('evidence-disclosure-chevron')).toBeInTheDocument()
  })

  it('toggles with aria-expanded', () => {
    const onOpenChange = vi.fn()

    render(
      <EvidenceDisclosure
        icon={FileText}
        id="block-1"
        onOpenChange={onOpenChange}
        open={false}
        title="Commands"
      >
        <p>pnpm test</p>
      </EvidenceDisclosure>,
    )

    const button = screen.getByRole('button', { name: 'Commands' })
    expect(button).toHaveAttribute('aria-expanded', 'false')

    fireEvent.click(button)

    expect(onOpenChange).toHaveBeenCalledWith(true)
    expect(screen.queryByText('pnpm test')).not.toBeInTheDocument()
  })

  it('does not collapse forced-open blocks', () => {
    const onOpenChange = vi.fn()

    render(
      <EvidenceDisclosure
        forcedOpen
        icon={FileText}
        id="block-1"
        onOpenChange={onOpenChange}
        open={false}
        title="Failed command"
      >
        <p>exit 101</p>
      </EvidenceDisclosure>,
    )

    const button = screen.getByRole('button', { name: 'Failed command' })
    expect(button).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('exit 101')).toBeInTheDocument()

    fireEvent.click(button)

    expect(onOpenChange).not.toHaveBeenCalled()
    expect(button).toHaveAttribute('aria-expanded', 'true')
  })

  it('renders action buttons outside the toggle button', () => {
    render(
      <EvidenceDisclosure
        actions={
          <>
            <button aria-label="Copy summary" type="button">
              <Copy className="size-3.5" />
            </button>
            <button aria-label="Open in inspector" type="button">
              <ExternalLink className="size-3.5" />
            </button>
          </>
        }
        icon={FileText}
        id="block-1"
        open
        title="Activity"
      >
        <p>Read files</p>
      </EvidenceDisclosure>,
    )

    expect(screen.getByRole('button', { name: 'Copy summary' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open in inspector' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Activity' })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
  })

  it('keeps long title and metadata truncatable without shifting the body width', () => {
    render(
      <EvidenceDisclosure
        icon={FileText}
        id="block-1"
        meta={<span>very-long-metadata-value-that-should-truncate</span>}
        open
        title="very-long-title-value-that-should-truncate-without-overlapping"
      >
        <p>body</p>
      </EvidenceDisclosure>,
    )

    expect(screen.getByTestId('evidence-disclosure-title')).toHaveClass('truncate')
    expect(screen.getByTestId('evidence-disclosure-meta')).toHaveClass('truncate')
    expect(screen.getByTestId('evidence-disclosure-body')).toHaveClass('min-w-0')
  })

  it('supports keyboard activation through native button behavior', () => {
    const onOpenChange = vi.fn()

    render(
      <EvidenceDisclosure
        icon={FileText}
        id="block-1"
        onOpenChange={onOpenChange}
        open={false}
        title="Keyboard block"
      >
        <p>body</p>
      </EvidenceDisclosure>,
    )

    const button = screen.getByRole('button', { name: 'Keyboard block' })
    expect(button.tagName).toBe('BUTTON')
    button.focus()
    expect(button).toHaveFocus()

    fireEvent.click(button)
    expect(onOpenChange).toHaveBeenCalledWith(true)
  })
})
