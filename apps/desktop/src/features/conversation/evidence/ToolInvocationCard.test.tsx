import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { ToolAttempt } from '@/shared/tauri/commands'
import { ToolInvocationCard } from './ToolInvocationCard'

function toolAttempt(overrides: Partial<ToolAttempt> = {}): ToolAttempt {
  return {
    id: 'attempt-1',
    order: 0,
    toolUseId: 'tool-use-1',
    toolName: 'read_file',
    status: 'completed',
    origin: 'builtin',
    argumentsPreview: 'path: WorkbenchInspector.tsx',
    outputSummary: 'Read WorkbenchInspector.tsx',
    affectedTargets: ['apps/desktop/src/features/workbench/WorkbenchInspector.tsx'],
    durationMs: 23,
    ...overrides,
  }
}

describe('ToolInvocationCard', () => {
  it('renders a non-interactive card when no action is provided', () => {
    render(<ToolInvocationCard attempt={toolAttempt()} />)

    expect(screen.getByText('read_file')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /read_file/ })).not.toBeInTheDocument()
  })

  it('renders an interactive card when an action is provided', () => {
    const onClick = vi.fn()
    render(<ToolInvocationCard attempt={toolAttempt()} onClick={onClick} />)

    fireEvent.click(screen.getByRole('button', { name: /read_file/ }))

    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('uses semantic status token classes instead of hardcoded product colors', () => {
    render(<ToolInvocationCard attempt={toolAttempt({ status: 'completed' })} />)

    expect(screen.getByText('completed')).toHaveClass('bg-success/10', 'text-success')
  })
})
