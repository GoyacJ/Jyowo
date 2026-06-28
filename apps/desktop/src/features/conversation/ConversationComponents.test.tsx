import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ConversationCanvas } from './ConversationCanvas'
import { DecisionCard } from './DecisionCard'
import { DiffPreview } from './DiffPreview'
import { DiffViewer } from './DiffViewer'
import type { PlanItem } from './PlanBlock'
import { PlanBlock } from './PlanBlock'
import { ReviewRequest } from './ReviewRequest'

const storyPlanItems = [
  { label: 'Initialize project & dependencies', status: 'Done' },
  { label: 'Configure Tauri command boundary', status: 'Done' },
  { label: 'Set up React + TypeScript (Vite)', status: 'Done' },
  { label: 'Add base app shell & IPC bridge', status: 'Done' },
  { label: 'Add scripts, README, and .gitignore', status: 'In progress' },
] satisfies PlanItem[]

const storyDiffLines = [
  '+ use serde::Serialize;',
  '+',
  '+ #[derive(Serialize)]',
  '+ struct AppInfoPayload {',
  '+   name: String,',
  '+   version: String,',
  '+ }',
  '+',
  '+ #[tauri::command]',
  '+ fn get_app_info() -> AppInfoPayload {',
  '+   AppInfoPayload {',
  '+     name: "Jyowo".into(),',
  '+     version: env!("CARGO_PKG_VERSION").into(),',
  '+   }',
  '+ }',
]

describe('conversation components', () => {
  it('renders a conversation canvas title and messages', () => {
    render(
      <ConversationCanvas title="Build the desktop foundation">
        <p>Use Vite for the renderer.</p>
      </ConversationCanvas>,
    )

    expect(
      screen.getByRole('heading', { name: 'Build the desktop foundation' }),
    ).toBeInTheDocument()
    expect(screen.getByText('Use Vite for the renderer.')).toBeInTheDocument()
  })

  it('renders completed and in-progress plan items', () => {
    render(<PlanBlock completedCount={4} items={storyPlanItems} totalCount={5} />)

    expect(screen.getByText('Plan')).toBeInTheDocument()
    expect(screen.getByText('4 / 5 completed')).toBeInTheDocument()
    expect(screen.getByRole('progressbar', { name: 'Plan progress' })).toHaveAttribute(
      'aria-valuenow',
      '80',
    )
    expect(screen.getByText('Initialize project & dependencies')).toBeInTheDocument()
    expect(screen.getByText('In progress')).toBeInTheDocument()
  })

  it('renders a diff preview with filename and added line count', () => {
    render(
      <DiffPreview
        addedLineCount={46}
        filename="apps/desktop/src-tauri/src/lib.rs"
        lines={storyDiffLines}
      />,
    )

    expect(screen.getByText('apps/desktop/src-tauri/src/lib.rs')).toBeInTheDocument()
    expect(screen.getByText('+46')).toBeInTheDocument()
    expect(screen.getByText('-0')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy diff' })).toBeInTheDocument()
  })

  it('keeps diff metadata lines neutral in legacy diff previews', () => {
    render(
      <DiffPreview
        addedLineCount={1}
        filename="apps/desktop/src/demo.ts"
        lines={['--- a/demo.ts', '+++ b/demo.ts', '@@ -1 +1 @@', '- oldValue()', '+ newValue()']}
        removedLineCount={1}
      />,
    )

    expect(screen.getByText('+++ b/demo.ts').closest('div')).not.toHaveClass('bg-success/10')
    expect(screen.getByText('--- a/demo.ts').closest('div')).not.toHaveClass('bg-destructive/10')
  })

  it('renders decision, review, and large diff states', () => {
    const onContinue = vi.fn()
    const onCopy = vi.fn()

    render(
      <>
        <DecisionCard detail="Before connecting runtime events" title="Review shell structure" />
        <ReviewRequest
          continueActionLabel="Continue"
          title="Review generated foundation"
          onContinue={onContinue}
        />
        <DiffViewer
          addedLineCount={2}
          filename="apps/desktop/src/demo.ts"
          lines={[
            { content: 'const previous = true', lineNumber: 1, type: 'removed' },
            { content: 'const next = true', lineNumber: 2, type: 'added' },
            { content: 'export { next }', lineNumber: 3, type: 'context' },
          ]}
          maxVisibleLines={2}
          onCopy={onCopy}
        />
      </>,
    )

    expect(
      screen.getByRole('region', { name: 'Decision needed: Review shell structure' }),
    ).toBeInTheDocument()
    expect(screen.getByText('Review generated foundation')).toBeInTheDocument()
    expect(
      screen.getByText('1 more lines hidden. Open in editor to inspect the full diff.'),
    ).toBeInTheDocument()

    screen.getByRole('button', { name: 'Continue' }).click()
    screen.getByRole('button', { name: 'Copy diff' }).click()

    expect(onContinue).toHaveBeenCalledTimes(1)
    expect(onCopy).toHaveBeenCalledTimes(1)
  })
})
