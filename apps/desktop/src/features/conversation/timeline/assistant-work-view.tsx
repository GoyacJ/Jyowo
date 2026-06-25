import { useTranslation } from 'react-i18next'
import type { AssistantWork, ConversationEventRef } from '@/shared/tauri/commands'
import { ArtifactSegmentView } from './artifact-segment-view'
import { AssistantTextSegmentView } from './assistant-text-segment-view'
import { ClarificationRequestSegmentView } from './clarification-request-segment-view'
import { ErrorSegmentView } from './error-segment-view'
import { NoticeSegmentView } from './notice-segment-view'
import { ReviewRequestSegmentView } from './review-request-segment-view'
import { ThinkingPanel } from './thinking-panel'
import { ToolGroupSegmentView } from './tool-group-segment-view'

export function AssistantWorkView({
  assistant,
  conversationId,
  onOpenDetails,
  onPermissionResolve,
  onReviewContinue,
  turnId,
}: {
  assistant: AssistantWork
  conversationId: string
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
  onReviewContinue?: (prompt: string) => void
  turnId: string
}) {
  const { t } = useTranslation('conversation')

  return (
    <section className="max-w-[86%]">
      <div className="mb-2 font-semibold text-sm">{t('timeline.assistantAuthor')}</div>
      <div className="grid gap-3">
        {assistant.segments.map((segment) => {
          switch (segment.kind) {
            case 'thinking':
              return <ThinkingPanel key={segment.id} segment={segment} />
            case 'text':
              return <AssistantTextSegmentView key={segment.id} segment={segment} />
            case 'toolGroup':
              return (
                <ToolGroupSegmentView
                  conversationId={conversationId}
                  key={segment.id}
                  onOpenDetails={onOpenDetails}
                  onPermissionResolve={onPermissionResolve}
                  segment={segment}
                  turnId={turnId}
                />
              )
            case 'artifact':
              return <ArtifactSegmentView key={segment.id} segment={segment} />
            case 'reviewRequest':
              return (
                <ReviewRequestSegmentView
                  key={segment.id}
                  onContinue={onReviewContinue}
                  segment={segment}
                />
              )
            case 'clarificationRequest':
              return <ClarificationRequestSegmentView key={segment.id} segment={segment} />
            case 'notice':
              return <NoticeSegmentView key={segment.id} segment={segment} />
            case 'error':
              return <ErrorSegmentView key={segment.id} segment={segment} />
          }
          return null
        })}
      </div>
    </section>
  )
}
