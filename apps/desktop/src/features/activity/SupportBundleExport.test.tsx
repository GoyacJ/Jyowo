import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SupportBundleExport } from './SupportBundleExport'

describe('SupportBundleExport', () => {
  it('exports a redacted support bundle and shows generated file paths', async () => {
    const onExport = vi.fn().mockResolvedValue({
      bundlePath: '.jyowo/runtime/exports/support-bundle.json',
      eventCount: 3,
      exportedAt: '2026-06-17T00:00:00.000Z',
      jsonlPath: '.jyowo/runtime/exports/events.jsonl',
      markdownPath: '.jyowo/runtime/exports/report.md',
      redacted: true,
    })

    render(<SupportBundleExport onExport={onExport} />)

    fireEvent.click(screen.getByRole('button', { name: 'Export support bundle' }))

    expect(
      await screen.findByText('.jyowo/runtime/exports/support-bundle.json'),
    ).toBeInTheDocument()
    expect(screen.getByText('.jyowo/runtime/exports/events.jsonl')).toBeInTheDocument()
    expect(screen.getByText('.jyowo/runtime/exports/report.md')).toBeInTheDocument()
    expect(screen.getByText('3 events')).toBeInTheDocument()
    expect(screen.getByText('Redacted')).toBeInTheDocument()
  })

  it('fails closed when export result is not redacted', async () => {
    const onExport = vi.fn().mockResolvedValue({
      bundlePath: '.jyowo/runtime/exports/support-bundle.json',
      eventCount: 1,
      exportedAt: '2026-06-17T00:00:00.000Z',
      jsonlPath: '.jyowo/runtime/exports/events.jsonl',
      markdownPath: '.jyowo/runtime/exports/report.md',
      redacted: false,
    })

    render(<SupportBundleExport onExport={onExport} />)

    fireEvent.click(screen.getByRole('button', { name: 'Export support bundle' }))

    expect(await screen.findByText('Support bundle export was not redacted.')).toBeInTheDocument()
    expect(screen.queryByText('.jyowo/runtime/exports/support-bundle.json')).not.toBeInTheDocument()
  })

  it('does not render raw export errors', async () => {
    const onExport = vi
      .fn()
      .mockRejectedValue(
        new Error('raw failure at /workspace with ghp_abcdefghijklmnopqrstuvwxyz0123456789'),
      )

    render(<SupportBundleExport onExport={onExport} />)

    fireEvent.click(screen.getByRole('button', { name: 'Export support bundle' }))

    expect(await screen.findByText('Support bundle export failed.')).toBeInTheDocument()
    expect(screen.queryByText(/ghp_/)).not.toBeInTheDocument()
    expect(screen.queryByText(/workspace/)).not.toBeInTheDocument()
  })
})
