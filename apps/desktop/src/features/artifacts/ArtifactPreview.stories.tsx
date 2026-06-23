import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import { ArtifactHistory } from './ArtifactHistory'
import { ArtifactPreview } from './ArtifactPreview'

const meta = {
  title: 'Artifacts/ArtifactPreview',
  component: ArtifactPreview,
  parameters: {
    layout: 'centered',
  },
} satisfies Meta<typeof ArtifactPreview>

export default meta

type Story = StoryObj<typeof meta>

const withStoryFrame: Decorator = (StoryComponent) => {
  return (
    <StoryFrame>
      <StoryComponent />
    </StoryFrame>
  )
}

export const Ready: Story = {
  args: {
    content: 'apps/desktop/src/features/conversation/ConversationWorkspace.tsx\npnpm check:desktop',
    kind: 'code',
    state: 'ready',
    title: 'Desktop foundation created',
  },
  decorators: [withStoryFrame],
}

export const Loading: Story = {
  args: {
    state: 'loading',
    title: 'Desktop foundation created',
  },
  decorators: [withStoryFrame],
}

export const PreviewError: Story = {
  args: {
    errorMessage: 'Preview unavailable.',
    state: 'error',
    title: 'Desktop foundation created',
  },
  decorators: [withStoryFrame],
}

export const LargePreview: Story = {
  args: {
    content: `${'Generated artifact line\n'.repeat(120)}`,
    kind: 'markdown',
    maxPreviewCharacters: 320,
    state: 'ready',
    title: 'Large artifact',
  },
  decorators: [withStoryFrame],
}

export const History: Story = {
  args: {
    state: 'ready',
    title: 'Artifact history',
  },
  render: () => (
    <StoryFrame>
      <ArtifactHistory
        artifacts={[
          {
            actionLabel: 'Open preview',
            description: 'Tauri + React + TypeScript with Vite',
            id: 'artifact-desktop-foundation',
            kind: 'code',
            status: 'ready',
            title: 'Desktop foundation created',
          },
          {
            actionLabel: 'Inspect',
            description: 'Verification checklist',
            id: 'artifact-verification',
            kind: 'markdown',
            status: 'pending',
            title: 'Verification notes',
          },
        ]}
      />
    </StoryFrame>
  ),
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="w-[720px] bg-background p-6 text-foreground">{children}</main>
}
