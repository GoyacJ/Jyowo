type ConversationMessageViewModel = {
  author: 'Jyowo' | 'You'
  avatar: string
  body: string
  id: string
  time: string
  tone?: 'assistant' | 'user'
}

type ConversationPlanItemStatus = 'completed' | 'pending' | 'running'

type ConversationPlanItemViewModel = {
  id: string
  label: string
  status: ConversationPlanItemStatus
}

type ConversationDiffLineViewModel = {
  content: string
  lineNumber: number
  type: 'added' | 'context' | 'removed'
}

type ConversationDiffPreviewViewModel = {
  addedLineCount: number
  filename: string
  lines: ConversationDiffLineViewModel[]
}

type ConversationArtifactViewModel = {
  actionLabel: string
  description: string
  errorMessage?: string
  id: string
  kind: string
  preview: string
  previewState?: 'error' | 'loading' | 'ready'
  sourceMessageId?: string
  sourceRunId: string
  status: 'failed' | 'pending' | 'ready' | 'running'
  title: string
}

type ConversationDecisionViewModel = {
  detail: string
  title: string
}

type ConversationNextActionViewModel = {
  id: string
  label: string
}

type ConversationActivityStatus = 'blocked' | 'failed' | 'queued' | 'running' | 'success'

type ConversationActivityItemViewModel = {
  id: string
  label: string
  status: ConversationActivityStatus
  time: string
}

type ConversationReviewRequestViewModel = {
  continueActionLabel: string
  title: string
}

export type ConversationRuntimeState = {
  activityItems: ConversationActivityItemViewModel[]
  artifacts: ConversationArtifactViewModel[]
  decisions: ConversationDecisionViewModel[]
  diffPreview: ConversationDiffPreviewViewModel | null
  messages: ConversationMessageViewModel[]
  nextActions: ConversationNextActionViewModel[]
  planItems: ConversationPlanItemViewModel[]
  reviewRequest: ConversationReviewRequestViewModel | null
}
