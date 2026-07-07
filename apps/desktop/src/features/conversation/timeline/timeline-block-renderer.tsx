import type { ConversationEventRef, ResolvePermissionRequest } from '@/shared/tauri/commands'
import { AgentActivitySegmentView } from '../AgentActivitySegment'
import { ActivityRenderBlock } from './activity-render-block'
import { ArtifactSegmentView } from './artifact-segment-view'
import { AssistantTextSegmentView } from './assistant-text-segment-view'
import { ClarificationRequestSegmentView } from './clarification-request-segment-view'
import { CommandRenderBlock } from './command-render-block'
import { ErrorSegmentView } from './error-segment-view'
import { FileEditRenderBlock } from './file-edit-render-block'
import { NoticeSegmentView } from './notice-segment-view'
import { ReviewRequestSegmentView } from './review-request-segment-view'
import type { TimelineRenderBlock } from './timeline-render-blocks'
import { ToolGroupSegmentView } from './tool-group-segment-view'

export function TimelineBlockRenderer({
  block,
  conversationId,
  onOpenDetails,
  onPermissionResolve,
  onReviewContinue,
  runId,
  turnId,
}: {
  block: TimelineRenderBlock
  conversationId: string
  runId: string
  turnId: string
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: ResolvePermissionRequest) => void
  onReviewContinue?: (prompt: string) => void
}) {
  switch (block.kind) {
    case 'assistantText':
      return <AssistantTextSegmentView segment={block.segment} />
    case 'fileEdit':
      return <FileEditRenderBlock block={block} conversationId={conversationId} runId={runId} />
    case 'activity':
      return <ActivityRenderBlock block={block} conversationId={conversationId} runId={runId} />
    case 'commandGroup':
      return <CommandRenderBlock block={block} conversationId={conversationId} runId={runId} />
    case 'toolGroup':
      return (
        <ToolGroupSegmentView
          conversationId={conversationId}
          onOpenDetails={onOpenDetails}
          onPermissionResolve={onPermissionResolve}
          runId={runId}
          segment={block.segment}
          turnId={turnId}
        />
      )
    case 'artifact':
      return (
        <ArtifactSegmentView
          conversationId={conversationId}
          revisionIdOverride={block.revisionIdOverride}
          segment={block.segment}
        />
      )
    case 'reviewRequest':
      return <ReviewRequestSegmentView onContinue={onReviewContinue} segment={block.segment} />
    case 'clarificationRequest':
      return <ClarificationRequestSegmentView segment={block.segment} />
    case 'notice':
      return <NoticeSegmentView segment={block.segment} />
    case 'error':
      return <ErrorSegmentView segment={block.segment} />
    case 'agentActivity':
      return (
        <AgentActivitySegmentView
          conversationId={conversationId}
          onPermissionResolve={onPermissionResolve}
          parentRunId={runId}
          segment={block.segment}
          turnId={turnId}
        />
      )
  }
}
