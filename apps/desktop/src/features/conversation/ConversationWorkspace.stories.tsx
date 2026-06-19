import type { Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import { ArtifactSummary } from './ArtifactSummary'
import { Composer } from './Composer'
import { ConversationCanvas } from './ConversationCanvas'
import { ConversationMessage } from './ConversationMessage'
import { ConversationWorkspace } from './ConversationWorkspace'
import { DecisionCard } from './DecisionCard'
import { DiffPreview } from './DiffPreview'
import type { PlanItem } from './PlanBlock'
import { PlanBlock } from './PlanBlock'
import { ProgressBlock } from './ProgressBlock'
import { prototypeDiffLines, prototypePlanItems } from './prototype-data'
import { ReviewRequest } from './ReviewRequest'

const meta = {
  title: 'Conversation/Workspace',
  component: ConversationWorkspace,
  parameters: {
    layout: 'fullscreen',
  },
} satisfies Meta<typeof ConversationWorkspace>

export default meta

type Story = StoryObj<typeof meta>

export const Ready: Story = {}

export const Loading: Story = {
  render: () => (
    <StoryFrame>
      <ConversationCanvas title="Build the desktop foundation">
        <div
          aria-label="Loading conversation"
          className="grid gap-4 rounded-md border border-border bg-surface p-5"
          role="status"
        >
          <div className="h-4 w-1/3 rounded-md bg-muted" />
          <div className="h-20 rounded-md bg-muted" />
          <div className="h-28 rounded-md bg-muted" />
        </div>
      </ConversationCanvas>
      <Composer disabled onSubmit={() => {}} />
    </StoryFrame>
  ),
}

export const Empty: Story = {
  render: () => (
    <StoryFrame>
      <ConversationCanvas title="New conversation">
        <div className="rounded-md border border-dashed border-border bg-surface px-6 py-12 text-center">
          <h2 className="font-medium text-lg">Ask Jyowo to work on this project.</h2>
          <p className="mx-auto mt-2 max-w-md text-muted-foreground text-sm">
            The first message should describe the change, question, or review target.
          </p>
        </div>
      </ConversationCanvas>
      <Composer onSubmit={() => {}} />
    </StoryFrame>
  ),
}

export const ErrorState: Story = {
  render: () => (
    <StoryFrame>
      <ConversationCanvas title="Build the desktop foundation">
        <ConversationMessage
          author="You"
          avatar="JD"
          body="Continue the desktop setup."
          time="10:21 AM"
        />
        <ConversationMessage
          author="Jyowo"
          avatar="J"
          body="The run failed before the next step could start."
          time="10:22 AM"
          tone="assistant"
        />
      </ConversationCanvas>
      <Composer errorMessage="Run failed" onRetry={() => {}} onSubmit={() => {}} />
    </StoryFrame>
  ),
}

export const Streaming: Story = {
  render: () => (
    <StoryFrame>
      <ConversationCanvas title="Build the desktop foundation">
        <ConversationMessage
          author="You"
          avatar="JD"
          body="Use Vite for the renderer and keep the shell conversation-first."
          time="10:21 AM"
        />
        <ConversationMessage
          author="Jyowo"
          avatar="J"
          body="I am checking the current app structure and preparing the next change..."
          time="10:22 AM"
          tone="assistant"
        >
          <PlanBlock completedCount={2} items={streamingPlanItems} totalCount={5} />
          <ProgressBlock label="start_run" status="running" time="Now" />
        </ConversationMessage>
      </ConversationCanvas>
      <Composer pending onSubmit={() => {}} />
    </StoryFrame>
  ),
}

export const PermissionPending: Story = {
  render: () => (
    <StoryFrame>
      <ConversationCanvas title="Build the desktop foundation">
        <ConversationMessage
          author="You"
          avatar="JD"
          body="Install the desktop dependencies."
          time="10:21 AM"
        />
        <ConversationMessage
          author="Jyowo"
          avatar="J"
          body="I need permission before changing dependencies."
          time="10:22 AM"
          tone="assistant"
        >
          <div className="mt-4 rounded-md border border-warning/30 bg-warning/5 p-4">
            <p className="font-medium text-sm">Permission needed</p>
            <p className="mt-1 text-muted-foreground text-sm">
              Install dependencies for the local desktop workspace.
            </p>
            <div className="mt-3 flex gap-2">
              <button
                className="rounded-md bg-primary px-3 py-1.5 text-primary-foreground text-sm"
                type="button"
              >
                Approve
              </button>
              <button className="rounded-md border border-border px-3 py-1.5 text-sm" type="button">
                Deny
              </button>
            </div>
          </div>
        </ConversationMessage>
      </ConversationCanvas>
      <Composer disabled onSubmit={() => {}} />
    </StoryFrame>
  ),
}

export const Completed: Story = {
  render: () => (
    <StoryFrame>
      <ConversationCanvas title="Build the desktop foundation">
        <ConversationMessage
          author="You"
          avatar="JD"
          body="Create the desktop foundation and make it runnable."
          time="10:21 AM"
        />
        <ConversationMessage
          author="Jyowo"
          avatar="J"
          body="The desktop foundation is complete and ready to review."
          time="10:27 AM"
          tone="assistant"
        >
          <PlanBlock completedCount={5} items={completedPlanItems} totalCount={5} />
          <DiffPreview
            addedLineCount={46}
            filename="apps/desktop/src-tauri/src/lib.rs"
            lines={prototypeDiffLines}
          />
          <ArtifactSummary
            artifacts={[
              {
                actionLabel: 'Run app',
                description: 'Tauri + React + TypeScript with Vite',
                id: 'artifact-desktop-foundation',
                kind: 'app',
                preview:
                  'Tauri command boundary, React renderer shell, and Vite development scripts.',
                previewState: 'ready',
                sourceMessageId: 'message-story-assistant',
                sourceRunId: 'run-001',
                status: 'ready',
                title: 'Desktop foundation created',
              },
            ]}
          />
          <DecisionCard detail="Before connecting runtime events" title="Review shell structure" />
          <ReviewRequest continueActionLabel="Continue" title="Review generated foundation" />
        </ConversationMessage>
      </ConversationCanvas>
      <Composer onSubmit={() => {}} />
    </StoryFrame>
  ),
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="min-h-screen bg-background p-6 text-foreground">{children}</main>
}

const streamingPlanItems = prototypePlanItems.map((item, index) => ({
  ...item,
  status: index < 2 ? 'Done' : 'In progress',
})) satisfies PlanItem[]

const completedPlanItems = prototypePlanItems.map((item) => ({
  ...item,
  status: 'Done',
})) satisfies PlanItem[]
