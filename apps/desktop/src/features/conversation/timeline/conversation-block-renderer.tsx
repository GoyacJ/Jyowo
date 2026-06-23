import { assertNever } from '@/shared/events/assert-never'
import { ArtifactBlockView } from './artifact-block-view'
import { AssistantMessageBlockView } from './assistant-message-block-view'
import { AssistantStreamingBlockView } from './assistant-streaming-block-view'
import { ClarificationRequestBlockView } from './clarification-request-block-view'
import type { ConversationBlock } from './conversation-blocks'
import { DiffReviewBlockView } from './diff-review-block-view'
import { ErrorBlockView } from './error-block-view'
import { PermissionRequestBlockView } from './permission-request-block-view'
import { ReviewRequestBlockView } from './review-request-block-view'
import { SystemNoticeBlockView } from './system-notice-block-view'
import { ThinkingBlockView } from './thinking-block-view'
import { ToolGroupBlockView } from './tool-group-block-view'
import { UserMessageBlockView } from './user-message-block-view'

export function ConversationBlockRenderer({
  block,
  onPermissionResolve,
  onReviewContinue,
}: {
  block: ConversationBlock
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
  onReviewContinue?: (prompt: string) => void
}) {
  switch (block.kind) {
    case 'userMessage':
      return <UserMessageBlockView block={block} />
    case 'assistantMessage':
      return <AssistantMessageBlockView block={block} />
    case 'assistantStreaming':
      return <AssistantStreamingBlockView block={block} />
    case 'thinking':
      return <ThinkingBlockView block={block} />
    case 'toolGroup':
      return <ToolGroupBlockView block={block} />
    case 'permissionRequest':
      return <PermissionRequestBlockView block={block} onResolve={onPermissionResolve} />
    case 'clarificationRequest':
      return <ClarificationRequestBlockView block={block} />
    case 'artifact':
      return <ArtifactBlockView block={block} />
    case 'diffReview':
      return <DiffReviewBlockView block={block} />
    case 'reviewRequest':
      return <ReviewRequestBlockView block={block} onContinue={onReviewContinue} />
    case 'error':
      return <ErrorBlockView block={block} />
    case 'systemNotice':
      return <SystemNoticeBlockView block={block} />
    case 'planTimeline':
      return (
        <section className="ml-12 border-border border-l pl-4">
          <h2 className="font-medium text-sm">Plan</h2>
          <ol className="mt-2 grid gap-1 text-sm">
            {block.items.map((item) => (
              <li className="flex justify-between gap-3" key={item.label}>
                <span>{item.label}</span>
                <span className="text-muted-foreground text-xs">{item.status}</span>
              </li>
            ))}
          </ol>
        </section>
      )
    case 'checkpoint':
      return (
        <section className="ml-12 border-border border-l pl-4 text-muted-foreground text-sm">
          {block.label}
        </section>
      )
    default:
      return assertNever(block)
  }
}
