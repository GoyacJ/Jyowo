import type { Meta, StoryObj } from '@storybook/react-vite'

import { ContextPanel, type WorkspaceContext } from './ContextPanel'

const readyContext = {
  project: 'Desktop App',
  path: '~/projects/desktop-app',
  files: [
    { label: 'src/' },
    { label: 'src-tauri/' },
    { label: 'package.json' },
    { label: 'src-tauri/src/main.rs' },
    { label: 'src-tauri/tauri.conf.json' },
    { label: 'vite.config.ts' },
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

const meta = {
  title: 'Context/Panel',
  component: ContextPanel,
  parameters: {
    layout: 'fullscreen',
  },
  args: {
    context: readyContext,
  },
} satisfies Meta<typeof ContextPanel>

export default meta

type Story = StoryObj<typeof meta>

export const Ready: Story = {}

export const Empty: Story = {
  args: {
    context: null,
  },
}

export const Loading: Story = {
  args: {
    context: null,
    loading: true,
  },
}

export const ErrorState: Story = {
  name: 'Error',
  args: {
    context: null,
    errorMessage: 'Workspace context is unavailable.',
  },
}

export const MissingFile: Story = {
  args: {
    context: {
      ...readyContext,
      files: [{ label: 'src/App.tsx' }, { label: 'src-tauri/src/main.rs', state: 'missing' }],
    },
  },
}

export const StaleContext: Story = {
  args: {
    context: {
      ...readyContext,
      files: readyContext.files.map((file) => ({ ...file, state: 'stale' })),
      decisions: [
        {
          title: 'Refresh project context',
          detail: 'When: Before editing generated files',
        },
      ],
    },
  },
}

export const LongFileList: Story = {
  args: {
    context: {
      ...readyContext,
      files: [
        ...readyContext.files,
        { label: 'apps/desktop/src/features/conversation/ConversationWorkspace.tsx' },
        { label: 'apps/desktop/src/features/context/components/very-long-reference-name.tsx' },
        { label: 'crates/jyowo-harness-contracts/src/events/mod.rs' },
        { label: 'crates/jyowo-harness-sdk/src/lib.rs' },
      ],
    },
  },
}

export const NoDecisions: Story = {
  args: {
    context: {
      ...readyContext,
      activeArtifact: undefined,
      decisions: [],
      nextActions: [],
    },
  },
}
