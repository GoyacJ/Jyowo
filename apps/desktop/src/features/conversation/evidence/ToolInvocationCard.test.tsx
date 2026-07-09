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

  it('shows offloaded result state without exposing blob details', () => {
    render(
      <ToolInvocationCard
        attempt={toolAttempt({
          outputSummary: undefined,
          resultKind: 'offloaded',
          truncated: true,
        })}
      />,
    )

    expect(screen.getByText('offloaded')).toBeInTheDocument()
    expect(screen.queryByText(/blob/i)).not.toBeInTheDocument()
  })

  it('renders only schema-declared display fields for tool output', () => {
    render(
      <ToolInvocationCard
        attempt={toolAttempt({
          argumentsPreview: 'Authorization: Bearer secret-token',
          outputSummary: 'Safe summary',
          resultKind: 'structured',
        })}
      />,
    )

    expect(screen.getByText('Safe summary')).toBeInTheDocument()
    expect(screen.getByText('structured')).toBeInTheDocument()
    expect(screen.queryByText(/secret-token/)).not.toBeInTheDocument()
  })

  it.each(['text', 'structured', 'blob', 'mixed', 'offloaded'] as const)(
    'shows %s result kind',
    (resultKind) => {
      render(
        <ToolInvocationCard
          attempt={toolAttempt({
            outputSummary: undefined,
            resultKind,
          })}
        />,
      )

      expect(screen.getByText(resultKind)).toBeInTheDocument()
    },
  )

  it('shows capability missing failures as a distinct tool state', () => {
    render(
      <ToolInvocationCard
        attempt={toolAttempt({
          failureKind: 'capabilityMissing',
          status: 'failed',
        })}
      />,
    )

    expect(screen.getByText('capabilityMissing')).toBeInTheDocument()
  })
})
