import type { Meta, StoryObj } from '@storybook/react-vite'

import type { TimelineItemProjection } from '@/generated/daemon-protocol'
import { TaskTimeline } from './TaskTimeline'

const meta = {
  component: TaskTimeline,
  decorators: [
    (Story) => (
      <main className="mx-auto min-h-screen max-w-[820px] bg-background px-6 py-10 text-foreground">
        <Story />
      </main>
    ),
  ],
  parameters: { layout: 'fullscreen' },
  title: 'Tasks/Task timeline',
} satisfies Meta<typeof TaskTimeline>

export default meta
type Story = StoryObj<typeof meta>

export const CodexFixture: Story = {
  args: {
    items: [
      fixture(1, 'user_message', 'Inspect the scheduler and verify recovery.'),
      fixture(2, 'notice', 'Run started', 'segment-1'),
      fixture(3, 'assistant_text', 'I’m tracing the journal replay path.', 'segment-1'),
      fixture(4, 'tool_activity', 'Read task_store.rs', 'segment-1'),
      fixture(5, 'command', 'cargo test -p jyowo-harness-journal', 'segment-1'),
      fixture(6, 'diff', '2 files changed, 18 insertions', 'segment-1'),
      fixture(7, 'image', 'Recovery state diagram', 'segment-1'),
      fixture(8, 'permission', 'Permission requested for integration test', 'segment-1'),
      fixture(9, 'compaction', 'Context compacted', 'segment-1'),
      fixture(10, 'subagent', 'Review agent found no remaining issue', 'segment-1'),
      fixture(11, 'assistant_text', 'The interrupted output remains visible', 'segment-1', true),
      fixture(12, 'notice', 'Run interrupted by restart', 'segment-1', true),
      fixture(13, 'user_message', 'Continue from the last committed offset.'),
      fixture(14, 'notice', 'Run started', 'segment-2'),
      fixture(15, 'error', 'Command failed; retry or inspect output', 'segment-2'),
    ],
  },
}

function fixture(
  globalOffset: number,
  kind: TimelineItemProjection['kind'],
  summary: string,
  runSegmentId?: string,
  incomplete = false,
): TimelineItemProjection {
  return { globalOffset, id: `fixture-${globalOffset}`, incomplete, kind, runSegmentId, summary }
}
