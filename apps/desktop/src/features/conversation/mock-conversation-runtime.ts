import type { ConversationRuntimeState } from './conversation-models'

const initialUserMessage =
  "Let's scaffold the desktop app with Tauri + React + TypeScript.\nUse Vite for the renderer. Keep it minimal and ready for local AI features."

const initialAssistantMessage =
  "I'll set up the foundation with a clean project structure, dev scripts, and base app shell."

const demoPlanItems = [
  { id: 'plan-project-structure', label: 'Create project structure', status: 'completed' },
  { id: 'plan-vite-renderer', label: 'Add Vite renderer', status: 'completed' },
  { id: 'plan-tauri-shell', label: 'Connect Tauri shell', status: 'completed' },
  { id: 'plan-dev-scripts', label: 'Add dev scripts', status: 'completed' },
  { id: 'plan-tests', label: 'Add verification tests', status: 'running' },
] satisfies ConversationRuntimeState['planItems']

const demoDiffPreview = {
  addedLineCount: 46,
  filename: 'apps/desktop/src-tauri/src/lib.rs',
  lines: [
    { content: 'pub fn run() {', lineNumber: 1, type: 'context' },
    { content: '    tauri::Builder::default()', lineNumber: 2, type: 'added' },
    { content: '        .run(tauri::generate_context!())', lineNumber: 3, type: 'added' },
    { content: '        .expect("error while running tauri app");', lineNumber: 4, type: 'added' },
    { content: '}', lineNumber: 5, type: 'context' },
  ],
} satisfies ConversationRuntimeState['diffPreview']

export function createMockConversationState(): ConversationRuntimeState {
  return {
    activityItems: [
      { id: 'activity-start-run', label: 'start_run', status: 'queued', time: 'Now' },
    ],
    artifacts: [
      {
        actionLabel: 'Run app',
        description: 'Tauri + React + TypeScript with Vite',
        id: 'artifact-desktop-foundation',
        kind: 'app',
        preview: 'Tauri command boundary, React renderer shell, and Vite development scripts.',
        previewState: 'ready',
        sourceMessageId: 'message-initial-assistant',
        sourceRunId: 'run-001',
        status: 'ready',
        title: 'Desktop foundation created',
      },
      {
        actionLabel: 'Inspect',
        description: 'Follow-up verification checklist',
        id: 'artifact-verification-notes',
        kind: 'markdown',
        preview: 'pnpm check:desktop\ncargo fmt --all --check',
        previewState: 'loading',
        sourceMessageId: 'message-initial-assistant',
        sourceRunId: 'run-001',
        status: 'pending',
        title: 'Verification notes',
      },
    ],
    decisions: [{ title: 'Review shell structure', detail: 'Before connecting runtime events' }],
    diffPreview: demoDiffPreview,
    messages: [
      {
        author: 'You',
        avatar: 'JD',
        body: initialUserMessage,
        id: 'message-initial-user',
        time: '10:21 AM',
        tone: 'user',
      },
      {
        author: 'Jyowo',
        avatar: 'J',
        body: initialAssistantMessage,
        id: 'message-initial-assistant',
        time: '10:22 AM',
        tone: 'assistant',
      },
    ],
    nextActions: [{ id: 'next-run-app', label: 'Run app' }],
    planItems: demoPlanItems.map((item) => ({ ...item })),
    reviewRequest: null,
  }
}

export const mockConversationRuntime = {
  completeWork(state: ConversationRuntimeState): ConversationRuntimeState {
    return {
      ...state,
      activityItems: state.activityItems.map((item) =>
        item.id === 'activity-start-run' ? { ...item, status: 'success' } : item,
      ),
      planItems: state.planItems.map((item) => ({ ...item, status: 'completed' })),
    }
  },

  markActivityRunning(state: ConversationRuntimeState): ConversationRuntimeState {
    return {
      ...state,
      activityItems: state.activityItems.map((item) =>
        item.id === 'activity-start-run' ? { ...item, status: 'running' } : item,
      ),
    }
  },

  produceArtifactSummary(state: ConversationRuntimeState): ConversationRuntimeState {
    if (state.artifacts.length > 0) {
      return state
    }

    return {
      ...state,
      artifacts: [
        {
          actionLabel: 'Run app',
          description: 'Tauri + React + TypeScript with Vite',
          id: 'artifact-desktop-foundation',
          kind: 'app',
          preview: 'Tauri command boundary, React renderer shell, and Vite development scripts.',
          previewState: 'ready',
          sourceMessageId: 'message-initial-assistant',
          sourceRunId: 'run-001',
          status: 'ready',
          title: 'Desktop foundation created',
        },
      ],
    }
  },

  producePlan(state: ConversationRuntimeState): ConversationRuntimeState {
    if (state.planItems.length > 0) {
      return state
    }

    return {
      ...state,
      planItems: demoPlanItems.map((item) => ({ ...item })),
    }
  },

  requestReview(state: ConversationRuntimeState): ConversationRuntimeState {
    if (state.reviewRequest) {
      return state
    }

    return {
      ...state,
      reviewRequest: {
        continueActionLabel: 'Continue',
        title: 'Review generated foundation',
      },
    }
  },

  submitMessage(state: ConversationRuntimeState, body: string): ConversationRuntimeState {
    const trimmedBody = body.trim()

    if (!trimmedBody) {
      return state
    }

    return {
      ...state,
      messages: [
        ...state.messages,
        {
          author: 'You',
          avatar: 'JD',
          body: trimmedBody,
          id: `message-user-${state.messages.length + 1}`,
          time: 'Now',
          tone: 'user',
        },
      ],
    }
  },
}
