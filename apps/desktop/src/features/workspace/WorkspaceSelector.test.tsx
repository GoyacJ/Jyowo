import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { WorkspaceSelector } from './WorkspaceSelector'

describe('WorkspaceSelector', () => {
  it('shows the selected local workspace and reveals available workspaces on demand', () => {
    render(
      <WorkspaceSelector
        onSelect={vi.fn()}
        selectedWorkspaceRef="local:current"
        workspaces={[
          {
            name: 'Jyowo',
            path: 'Current project',
            ref: 'local:current',
          },
          {
            name: 'Design sandbox',
            path: 'Local prototype workspace',
            ref: 'local:design-sandbox',
          },
        ]}
      />,
    )

    const currentWorkspace = screen.getByRole('group', { name: 'Current workspace: Jyowo' })
    expect(currentWorkspace).toBeInTheDocument()
    expect(within(currentWorkspace).getByText('Current project')).toBeInTheDocument()
    expect(screen.queryByRole('list', { name: 'Available workspaces' })).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Choose workspace' }))

    const list = screen.getByRole('list', { name: 'Available workspaces' })
    expect(within(list).getByRole('button', { name: 'Jyowo Current project' })).toHaveAttribute(
      'aria-current',
      'true',
    )
    expect(
      within(list).getByRole('button', { name: 'Design sandbox Local prototype workspace' }),
    ).toBeInTheDocument()
  })
})
