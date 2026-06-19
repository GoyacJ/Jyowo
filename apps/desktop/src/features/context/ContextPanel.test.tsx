import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ContextPanel, type WorkspaceContext } from './ContextPanel'

const readyContext = {
  project: 'Desktop App',
  path: '~/projects/desktop-app',
  files: [
    { label: 'src/App.tsx', state: 'ready' },
    { label: 'src-tauri/src/main.rs', state: 'ready' },
  ],
  activeArtifact: 'App shell (WIP)',
  decisions: [
    {
      title: 'Choose IPC pattern',
      detail: 'When: Before adding AI features',
    },
  ],
  nextActions: ['Run app', 'Review generated shell'],
} satisfies WorkspaceContext

describe('ContextPanel', () => {
  it('renders project context, files, artifact, decisions, and next actions', () => {
    render(<ContextPanel context={readyContext} />)

    const panel = screen.getByRole('complementary', { name: 'Context' })

    expect(within(panel).getByText('Project')).toBeInTheDocument()
    expect(within(panel).getByText('Desktop App')).toBeInTheDocument()
    expect(within(panel).getByText('Path')).toBeInTheDocument()
    expect(within(panel).getByText('~/projects/desktop-app')).toBeInTheDocument()
    expect(within(panel).getByRole('list', { name: 'Files' })).toBeInTheDocument()
    expect(within(panel).getByText('src/App.tsx')).toBeInTheDocument()
    expect(within(panel).getByText('App shell (WIP)')).toBeInTheDocument()
    expect(within(panel).getByText('Choose IPC pattern')).toBeInTheDocument()
    expect(within(panel).getByRole('list', { name: 'Next actions' })).toBeInTheDocument()
    expect(within(panel).getByText('Run app')).toBeInTheDocument()
    expect(within(panel).getByRole('region', { name: 'Files' })).toBeInTheDocument()
    expect(within(panel).getByRole('region', { name: 'Next actions' })).toBeInTheDocument()
    expect(within(panel).queryByRole('button', { name: 'Close context' })).not.toBeInTheDocument()
    expect(within(panel).queryByRole('button', { name: 'Show all files' })).not.toBeInTheDocument()
  })

  it('renders an empty context state without turning it into an error', () => {
    render(<ContextPanel context={null} />)

    const panel = screen.getByRole('complementary', { name: 'Context' })

    expect(within(panel).getByText('No context selected')).toBeInTheDocument()
    expect(
      within(panel).getByText('Start a conversation to attach project context.'),
    ).toBeInTheDocument()
    expect(within(panel).queryByText('Error')).not.toBeInTheDocument()
  })

  it('renders loading and error states before empty context', () => {
    const { rerender } = render(<ContextPanel context={null} loading />)

    expect(screen.getByText('Loading context')).toBeInTheDocument()
    expect(screen.queryByText('No context selected')).not.toBeInTheDocument()

    rerender(<ContextPanel context={null} errorMessage="IPC unavailable" />)

    expect(screen.getByText('Context unavailable')).toBeInTheDocument()
    expect(screen.getByText('IPC unavailable')).toBeInTheDocument()
    expect(screen.queryByText('No context selected')).not.toBeInTheDocument()
  })

  it('keeps long file labels accessible', () => {
    const longFile =
      'apps/desktop/src/features/conversation/components/very-long-generated-artifact-preview-name.tsx'

    render(
      <ContextPanel
        context={{
          ...readyContext,
          files: [{ label: longFile, state: 'stale' }],
        }}
      />,
    )

    expect(screen.getByRole('listitem', { name: `${longFile} Stale` })).toBeInTheDocument()
  })

  it('renders explicit actions only when callbacks are provided', () => {
    const onAddFile = vi.fn()
    const onClose = vi.fn()
    const onDecisionSelect = vi.fn()
    const onNextAction = vi.fn()
    const onShowAllFiles = vi.fn()

    render(
      <ContextPanel
        context={{ ...readyContext, totalFileCount: 18 }}
        onAddFile={onAddFile}
        onClose={onClose}
        onDecisionSelect={onDecisionSelect}
        onNextAction={onNextAction}
        onShowAllFiles={onShowAllFiles}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Close context' }))
    fireEvent.click(screen.getByRole('button', { name: 'Add file' }))
    fireEvent.click(screen.getByRole('button', { name: 'Show all files' }))
    fireEvent.click(screen.getByRole('button', { name: /Choose IPC pattern/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Run app' }))

    expect(onClose).toHaveBeenCalledTimes(1)
    expect(onAddFile).toHaveBeenCalledTimes(1)
    expect(onShowAllFiles).toHaveBeenCalledTimes(1)
    expect(onDecisionSelect).toHaveBeenCalledWith(readyContext.decisions[0])
    expect(onNextAction).toHaveBeenCalledWith('Run app')
  })

  it('does not show a hardcoded file count for empty context files', () => {
    render(<ContextPanel context={{ ...readyContext, files: [], totalFileCount: 0 }} />)

    expect(screen.getByText('No files attached.')).toBeInTheDocument()
    expect(screen.queryByText('Show all (18)')).not.toBeInTheDocument()
  })
})
