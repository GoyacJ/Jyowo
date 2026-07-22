import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it } from 'vitest'

import { createAppI18n } from '@/shared/i18n/i18n'

import { DiffPanel, parseUnifiedDiff } from './DiffPanel'

describe('DiffPanel', () => {
  it('parses unified diff files, hunks, and old/new line numbers', () => {
    const files = parseUnifiedDiff(diff)

    expect(files).toHaveLength(1)
    expect(files[0]?.label).toBe('src/scheduler.ts')
    expect(files[0]?.lines.find((line) => line.kind === 'hunk')?.text).toBe('@@ -10,2 +10,3 @@')
    expect(files[0]?.lines.filter((line) => line.kind === 'deletion')).toEqual([
      expect.objectContaining({ oldLine: 11, text: '-oldValue' }),
    ])
    expect(files[0]?.lines.filter((line) => line.kind === 'addition')).toEqual([
      expect.objectContaining({ newLine: 11, text: '+newValue' }),
      expect.objectContaining({ newLine: 12, text: '+extraValue' }),
    ])
  })

  it('renders file and line-level diff semantics', () => {
    render(
      <I18nextProvider i18n={createAppI18n('en-US')}>
        <DiffPanel loading={false} missing={false} text={diff} />
      </I18nextProvider>,
    )

    expect(screen.getByRole('region', { name: 'Changes in src/scheduler.ts' })).toBeInTheDocument()
    expect(screen.getByRole('cell', { name: 'Old line 11' })).toHaveTextContent('11')
    expect(screen.getByRole('cell', { name: 'New line 12' })).toHaveTextContent('12')
    expect(screen.getAllByText('Added:')).toHaveLength(2)
    expect(screen.getAllByText('Added:')[0]).toHaveClass('sr-only')
    expect(screen.getByText('Removed:')).toHaveClass('sr-only')
    expect(screen.getByText('+2')).toBeInTheDocument()
    expect(screen.getByText('2 added lines')).toHaveClass('sr-only')
    expect(screen.getByText('-1')).toBeInTheDocument()
    expect(screen.getByText('1 removed line')).toHaveClass('sr-only')
    expect(
      screen.getByTestId('unified-diff').querySelectorAll('[data-diff-line="addition"]'),
    ).toHaveLength(2)
  })
})

const diff = `diff --git a/src/scheduler.ts b/src/scheduler.ts
index 1234567..89abcde 100644
--- a/src/scheduler.ts
+++ b/src/scheduler.ts
@@ -10,2 +10,3 @@
 const value = 1
-oldValue
+newValue
+extraValue
`
