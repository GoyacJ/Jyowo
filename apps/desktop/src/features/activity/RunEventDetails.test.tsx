import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { RunEventDetails } from './RunEventDetails'

describe('RunEventDetails', () => {
  it('renders tool, command, permission, and redacted raw payload details', () => {
    render(
      <RunEventDetails
        event={{
          command: {
            approvalState: 'pending',
            args: ['commit', '-m', 'hello world'],
            cwd: '~/projects/desktop-app',
            environment: [
              { name: 'PATH', value: '/usr/bin' },
              { name: 'JYOWO_TOKEN', redacted: true, value: 'should-not-render' },
            ],
            executable: 'git',
            risk: 'critical',
          },
          permissions: [
            {
              decisionScope: 'current run',
              exposure: 'Read-only file metadata.',
              id: 'perm-low',
              label: 'Read project files',
              operation: 'Read files',
              reason: 'The tool needs project context.',
              risk: 'low',
              state: 'approved',
              target: 'apps/desktop/src',
              workspaceBoundary: 'workspace://local',
            },
            {
              decisionScope: 'current run',
              exposure: 'Can modify dependencies.',
              id: 'perm-medium',
              label: 'Install packages',
              operation: 'Install dependencies',
              reason: 'The run requested package installation.',
              risk: 'medium',
              state: 'pending',
              target: 'package manager',
              workspaceBoundary: 'workspace://local',
            },
            {
              decisionScope: 'current run',
              exposure: 'Can write files inside the workspace.',
              id: 'perm-high',
              label: 'Write files',
              operation: 'Write files',
              reason: 'The tool needs to update implementation files.',
              risk: 'high',
              state: 'pending',
              target: 'apps/desktop/src',
              workspaceBoundary: 'workspace://local',
            },
            {
              command: {
                args: ['rm', '-rf', 'dist'],
                cwd: 'workspace://local',
                executable: 'bash',
                risk: 'critical',
              },
              decisionScope: 'one request',
              diffSummary: 'Deletes generated build output.',
              exposure: 'Can delete files under the workspace.',
              id: 'perm-critical',
              label: 'Run destructive command',
              operation: 'Delete generated files',
              reason: 'The run requested cleanup before rebuilding.',
              risk: 'critical',
              state: 'pending',
              target: 'dist',
              workspaceBoundary: 'workspace://local',
            },
          ],
          rawJson: {
            payload: {
              event: 'tool_call',
              redacted: true,
              token: '[REDACTED]',
            },
          },
          toolCall: {
            argumentsSummary: 'Write apps/desktop/src/main.tsx',
            durationMs: 1280,
            errorDetails: 'Permission required before writing.',
            endedAt: '10:22:08 AM',
            outputSummary: 'Pending approval.',
            permissionState: 'pending',
            startedAt: '10:22:07 AM',
            status: 'blocked',
            toolName: 'write_file',
          },
        }}
      />,
    )

    const region = screen.getByRole('region', { name: 'Run event details' })
    expect(within(region).getByText('write_file')).toBeInTheDocument()
    expect(within(region).getByText('Blocked')).toBeInTheDocument()
    expect(within(region).getByText('1.28s')).toBeInTheDocument()
    expect(within(region).getByText('10:22:07 AM')).toBeInTheDocument()
    expect(within(region).getByText('10:22:08 AM')).toBeInTheDocument()
    expect(within(region).getByText('Permission pending')).toBeInTheDocument()
    expect(within(region).getByText('Write apps/desktop/src/main.tsx')).toBeInTheDocument()
    expect(within(region).getByText('Pending approval.')).toBeInTheDocument()
    expect(within(region).getByText('Permission required before writing.')).toBeInTheDocument()

    expect(within(region).getByText('git')).toBeInTheDocument()
    expect(within(region).getByText('commit')).toBeInTheDocument()
    expect(within(region).getByText('-m')).toBeInTheDocument()
    expect(within(region).getByText('"hello world"')).toBeInTheDocument()
    expect(within(region).getByText('~/projects/desktop-app')).toBeInTheDocument()
    expect(within(region).getByText('PATH=/usr/bin')).toBeInTheDocument()
    expect(within(region).getByText('JYOWO_TOKEN=[REDACTED]')).toBeInTheDocument()
    expect(within(region).queryByText('should-not-render')).not.toBeInTheDocument()
    expect(within(region).getAllByText('Critical risk')).toHaveLength(2)
    expect(within(region).getByText('Approval pending')).toBeInTheDocument()

    expect(within(region).getByText('Low risk')).toBeInTheDocument()
    expect(within(region).getByText('Medium risk')).toBeInTheDocument()
    expect(within(region).getByText('High risk')).toBeInTheDocument()
    expect(within(region).getAllByText('Operation')).not.toHaveLength(0)
    expect(within(region).getByText('Delete generated files')).toBeInTheDocument()
    expect(within(region).getAllByText('Target')).not.toHaveLength(0)
    expect(within(region).getAllByText('dist')).not.toHaveLength(0)
    expect(within(region).getAllByText('Reason')).not.toHaveLength(0)
    expect(
      within(region).getByText('The run requested cleanup before rebuilding.'),
    ).toBeInTheDocument()
    expect(within(region).getAllByText('Workspace boundary')).not.toHaveLength(0)
    expect(within(region).getAllByText('workspace://local')).not.toHaveLength(0)
    expect(within(region).getAllByText('Exposure')).not.toHaveLength(0)
    expect(within(region).getByText('Can delete files under the workspace.')).toBeInTheDocument()
    expect(within(region).getAllByText('Decision scope')).not.toHaveLength(0)
    expect(within(region).getByText('one request')).toBeInTheDocument()
    expect(within(region).getByText('Deletes generated build output.')).toBeInTheDocument()
    expect(within(region).getByText('bash')).toBeInTheDocument()
    expect(within(region).getByText('rm')).toBeInTheDocument()

    expect(within(region).getByText('"token": "[REDACTED]"')).toBeInTheDocument()
    expect(within(region).queryByText('"withheld": true')).not.toBeInTheDocument()
  })

  it('withholds raw payloads by policy', () => {
    render(
      <RunEventDetails
        event={{
          rawJson: {
            payload: {
              token: 'should-not-render',
            },
            withheld: true,
          },
        }}
      />,
    )

    expect(screen.getByText('Raw JSON withheld by policy')).toBeInTheDocument()
    expect(screen.queryByText('should-not-render')).not.toBeInTheDocument()
  })

  it('truncates large raw payloads', () => {
    const stringifySpy = vi.spyOn(JSON, 'stringify')

    render(
      <RunEventDetails
        event={{
          rawJson: {
            payload: {
              output: 'x'.repeat(5000),
            },
          },
        }}
      />,
    )

    expect(screen.getByText('Payload truncated')).toBeInTheDocument()
    expect(screen.queryByText('x'.repeat(5000))).not.toBeInTheDocument()
    expect(stringifySpy).not.toHaveBeenCalled()

    stringifySpy.mockRestore()
  })

  it('truncates large raw payload keys before rendering', () => {
    const stringifySpy = vi.spyOn(JSON, 'stringify')
    const largeKey = 'x'.repeat(5000)
    const { container } = render(
      <RunEventDetails
        event={{
          rawJson: {
            payload: {
              [largeKey]: 'redacted',
            },
          },
        }}
      />,
    )

    expect(screen.getByText('Payload truncated')).toBeInTheDocument()
    expect(container.textContent).not.toContain(largeKey)
    expect(stringifySpy).not.toHaveBeenCalled()

    stringifySpy.mockRestore()
  })

  it('emits permission intents without defaulting to destructive approval focus', () => {
    const onApprovePermission = vi.fn()
    const onDenyPermission = vi.fn()

    render(
      <RunEventDetails
        event={{
          permissions: [
            {
              id: 'perm-critical',
              label: 'Delete generated files',
              risk: 'critical',
              state: 'pending',
            },
          ],
        }}
        onApprovePermission={onApprovePermission}
        onDenyPermission={onDenyPermission}
      />,
    )

    expect(screen.getByRole('group', { name: 'Delete generated files' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Approve permission' })).not.toHaveFocus()
    fireEvent.click(screen.getByRole('button', { name: 'Deny permission' }))
    fireEvent.click(screen.getByRole('button', { name: 'Approve permission' }))
    expect(onDenyPermission).toHaveBeenCalledWith('perm-critical')
    expect(onApprovePermission).toHaveBeenCalledWith('perm-critical')
  })

  it('does not render dead permission action buttons without callbacks', () => {
    render(
      <RunEventDetails
        event={{
          permissions: [
            {
              id: 'perm-critical',
              label: 'Delete generated files',
              risk: 'critical',
              state: 'pending',
            },
          ],
        }}
      />,
    )

    expect(screen.queryByRole('button', { name: 'Deny permission' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Approve permission' })).not.toBeInTheDocument()
  })
})
