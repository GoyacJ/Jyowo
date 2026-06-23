import type { RunEvent } from '@/shared/events/run-event-schema'
import type { GetConversationResponse, ListArtifactsResponse } from '@/shared/tauri/commands'

type ConversationBlockKind =
  | 'userMessage'
  | 'assistantMessage'
  | 'assistantStreaming'
  | 'thinking'
  | 'toolGroup'
  | 'permissionRequest'
  | 'clarificationRequest'
  | 'planTimeline'
  | 'artifact'
  | 'diffReview'
  | 'reviewRequest'
  | 'error'
  | 'checkpoint'
  | 'systemNotice'

type ConversationBlockBase = {
  id: string
  kind: ConversationBlockKind
  conversationId: string
  runId?: string
  turnId?: string
  conversationSequence: number
  runSequence?: number
  createdAt: string
  updatedAt?: string
  status?: string
}

export type UserMessageBlock = ConversationBlockBase & {
  kind: 'userMessage'
  messageId?: string
  clientMessageId?: string
  body: string
  status: 'sending' | 'sent' | 'failed'
  errorMessage?: string
}

export type AssistantMessageBlock = ConversationBlockBase & {
  kind: 'assistantMessage'
  messageId?: string
  body: string
  status: 'complete' | 'partial'
}

export type AssistantStreamingBlock = ConversationBlockBase & {
  kind: 'assistantStreaming'
  messageId?: string
  body: string
  status: 'streaming' | 'interrupted'
}

export type ThinkingBlock = ConversationBlockBase & {
  kind: 'thinking'
  body: string
  status: 'streaming' | 'complete'
  collapsed: boolean
}

export type ToolItem = {
  id: string
  name: string
  status: 'queued' | 'running' | 'completed' | 'failed' | 'denied'
  argumentsSummary?: string
  outputSummary?: string
  durationMs?: number
  errorMessage?: string
}

export type ToolGroupBlock = ConversationBlockBase & {
  kind: 'toolGroup'
  items: ToolItem[]
  expanded: boolean
}

export type PermissionRequestBlock = ConversationBlockBase & {
  kind: 'permissionRequest'
  requestId: string
  operation: string
  reason: string
  target: string
  severity: 'low' | 'medium' | 'high' | 'critical'
  decisionScope: string
  exposure: string
  workspaceBoundary: string
  decision?: 'approve' | 'deny'
  submitDecision?: 'approve' | 'deny'
  errorMessage?: string
  status: 'pending' | 'submitting' | 'resolved' | 'failed'
}

export type ClarificationRequestBlock = ConversationBlockBase & {
  kind: 'clarificationRequest'
  prompt: string
  status: 'pending' | 'answered'
}

type PlanTimelineBlock = ConversationBlockBase & {
  kind: 'planTimeline'
  items: Array<{ label: string; status: 'pending' | 'running' | 'completed' }>
}

export type ArtifactBlock = ConversationBlockBase & {
  kind: 'artifact'
  artifactId: string
  title: string
  description: string
  actionLabel: string
  artifactKind: string
  preview?: string
  status: 'failed' | 'pending' | 'ready' | 'running'
}

export type DiffReviewBlock = ConversationBlockBase & {
  kind: 'diffReview'
  artifactId?: string
  title: string
  preview?: string
  status: 'pending' | 'ready' | 'failed'
}

export type ReviewRequestBlock = ConversationBlockBase & {
  kind: 'reviewRequest'
  title: string
  continuePrompt: string
  status: 'pending' | 'submitted' | 'accepted'
}

export type ErrorBlock = ConversationBlockBase & {
  kind: 'error'
  message: string
}

type CheckpointBlock = ConversationBlockBase & {
  kind: 'checkpoint'
  label: string
}

export type SystemNoticeBlock = ConversationBlockBase & {
  kind: 'systemNotice'
  message: string
  tone: 'info' | 'warning'
}

export type ConversationBlock =
  | UserMessageBlock
  | AssistantMessageBlock
  | AssistantStreamingBlock
  | ThinkingBlock
  | ToolGroupBlock
  | PermissionRequestBlock
  | ClarificationRequestBlock
  | PlanTimelineBlock
  | ArtifactBlock
  | DiffReviewBlock
  | ReviewRequestBlock
  | ErrorBlock
  | CheckpointBlock
  | SystemNoticeBlock

export type ConversationSnapshot = GetConversationResponse['conversation']
export type ArtifactView = ListArtifactsResponse['artifacts'][number]
export type TimelineRunEvent = RunEvent
