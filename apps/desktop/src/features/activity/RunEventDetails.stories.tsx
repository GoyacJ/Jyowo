import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import { RunEventDetails } from './RunEventDetails'

const meta = {
  title: 'Activity/RunEventDetails',
  component: RunEventDetails,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    ((StoryComponent) => (
      <StoryFrame>
        <StoryComponent />
      </StoryFrame>
    )) satisfies Decorator,
  ],
} satisfies Meta<typeof RunEventDetails>

export default meta

type Story = StoryObj<typeof meta>

export const ToolSucceeded: Story = {
  args: {
    event: {
      toolCall: {
        argumentsSummary: 'Input withheld from conversation timeline.',
        durationMs: 438,
        endedAt: '2026-06-17T10:21:14.000Z',
        outputSummary: 'Output withheld from conversation timeline.',
        permissionState: 'not_required',
        startedAt: '2026-06-17T10:21:13.562Z',
        status: 'success',
        toolName: 'read_file',
      },
    },
  },
}

export const ToolFailed: Story = {
  args: {
    event: {
      toolCall: {
        argumentsSummary: 'Input withheld from conversation timeline.',
        errorDetails: 'Storybook build exited with code 1.',
        permissionState: 'not_required',
        status: 'failed',
        toolName: 'build_storybook',
      },
    },
  },
}

export const PermissionPending: Story = {
  args: {
    event: {
      command: {
        approvalState: 'pending',
        args: ['-C', 'apps/desktop', 'build-storybook'],
        cwd: 'workspace://local',
        environment: [
          { name: 'NODE_ENV', value: 'production' },
          { name: 'OPENAI_API_KEY', redacted: true, value: '[REDACTED]' },
        ],
        executable: 'pnpm',
        risk: 'high',
      },
      permissions: [
        {
          command: {
            args: ['-C', 'apps/desktop', 'build-storybook'],
            cwd: 'workspace://local',
            executable: 'pnpm',
            risk: 'high',
          },
          decisionScope: 'This run only',
          diffSummary: 'No file changes requested by this command.',
          exposure: 'Local process output is visible to the run.',
          id: 'permission-story-pending',
          label: 'Build Storybook',
          operation: 'Run local command',
          reason: 'Verify complex UI state stories.',
          risk: 'high',
          state: 'pending',
          target: 'apps/desktop',
          workspaceBoundary: 'workspace://local',
        },
      ],
    },
    onApprovePermission: () => {},
    onDenyPermission: () => {},
  },
}

export const RedactedPayload: Story = {
  args: {
    event: {
      rawJson: {
        payload: {},
        withheld: true,
      },
      toolCall: {
        argumentsSummary: 'Input withheld from conversation timeline.',
        outputSummary: 'Output withheld from conversation timeline.',
        permissionState: 'approved',
        status: 'redacted',
        toolName: 'provider.inspect',
      },
    },
  },
}

export const LargeOutput: Story = {
  args: {
    event: {
      rawJson: {
        payload: {
          event: 'run.output',
          lines: Array.from({ length: 220 }, (_, index) => ({
            index,
            text: `Generated output line ${index + 1}`,
          })),
        },
      },
      toolCall: {
        argumentsSummary: 'Input withheld from conversation timeline.',
        outputSummary: 'Output withheld from conversation timeline.',
        permissionState: 'not_required',
        status: 'running',
        toolName: 'output.preview',
      },
    },
  },
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="w-[760px] bg-background p-6 text-foreground">{children}</main>
}
