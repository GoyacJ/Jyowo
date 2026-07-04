import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import type { UiState } from '@/shared/state/ui-store'
import { uiStore } from '@/shared/state/ui-store'
import { WorkbenchInspector } from './WorkbenchInspector'

function setupStore(overrides?: Partial<UiState>) {
  uiStore.setState({
    inspectorOpen: true,
    workbenchSelection: null,
    ...overrides,
  } as Partial<UiState>)
}

describe('WorkbenchInspector', () => {
  it('renders empty state when no selection', () => {
    setupStore({ inspectorOpen: true, workbenchSelection: null })
    render(<WorkbenchInspector />)
    expect(screen.getByText('No Selection')).toBeDefined()
  })

  it('renders context pane when context is selected', () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: { kind: 'context' },
    })
    render(<WorkbenchInspector />)
    expect(screen.getByText('Context')).toBeDefined()
  })

  it('renders decision pane when decision is selected', () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'decision',
        conversationId: 'conv-1',
        requestId: 'req-1',
      },
    })
    render(<WorkbenchInspector />)
    expect(screen.getByText('Decision')).toBeDefined()
  })

  it('renders terminal pane when command is selected', () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'command',
        conversationId: 'conv-1',
      },
    })
    render(<WorkbenchInspector />)
    expect(screen.getByText('Terminal')).toBeDefined()
  })

  it('renders diff pane when diff is selected', () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'diff',
        conversationId: 'conv-1',
        changeSetId: 'cs-1',
      },
    })
    render(<WorkbenchInspector />)
    expect(screen.getByText('Diff')).toBeDefined()
  })

  it('renders artifact pane when artifact is selected', () => {
    setupStore({
      inspectorOpen: true,
      workbenchSelection: {
        kind: 'artifact',
        conversationId: 'conv-1',
        artifactId: 'artifact-1',
      },
    })
    render(<WorkbenchInspector />)
    expect(screen.getByText('Artifact')).toBeDefined()
  })

  it('hides when inspector is closed', () => {
    setupStore({ inspectorOpen: false, workbenchSelection: null })
    const { container } = render(<WorkbenchInspector />)
    expect(container.innerHTML).toBe('')
  })
})
