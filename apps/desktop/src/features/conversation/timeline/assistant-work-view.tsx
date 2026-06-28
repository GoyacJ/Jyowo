import { useTranslation } from 'react-i18next'
import type { AssistantWork, ConversationEventRef } from '@/shared/tauri/commands'
import { ArtifactSegmentView } from './artifact-segment-view'
import { AssistantTextSegmentView } from './assistant-text-segment-view'
import { ClarificationRequestSegmentView } from './clarification-request-segment-view'
import { ErrorSegmentView } from './error-segment-view'
import { NoticeSegmentView } from './notice-segment-view'
import { ProcessPanel } from './process-panel'
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
  const processImageArtifactIds = getProcessImageArtifactIds(assistant)

  return (
    <section className="max-w-[86%]">
      <div className="mb-2 text-muted-foreground text-xs">{t('timeline.assistantAuthor')}</div>
      {assistant.status === 'running' || assistant.status === 'failed' ? (
        <div className="mb-3 border-border border-b pb-2 text-muted-foreground text-xs">
          {assistant.status === 'running'
            ? t('timeline.assistantStatus.running')
            : t('timeline.assistantStatus.failed')}
        </div>
      ) : null}
      <div className="grid gap-3">
        {assistant.segments.map((segment) => {
          switch (segment.kind) {
            case 'process':
              return (
                <ProcessPanel
                  conversationId={conversationId}
                  key={segment.id}
                  runId={assistant.runId}
                  segment={segment}
                />
              )
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
                  runId={assistant.runId}
                  segment={segment}
                  turnId={turnId}
                />
              )
            case 'artifact':
              if (
                segment.status === 'ready' &&
                segment.media?.kind === 'image' &&
                processImageArtifactIds.has(segment.artifactId)
              ) {
                return null
              }
              return (
                <ArtifactSegmentView
                  conversationId={conversationId}
                  key={segment.id}
                  segment={segment}
                />
              )
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

function getProcessImageArtifactIds(assistant: AssistantWork) {
  const artifactIds = new Set<string>()

  for (const segment of assistant.segments) {
    if (segment.kind !== 'process') {
      continue
    }

    for (const step of segment.steps ?? []) {
      if (
        step.detail?.type === 'artifact' &&
        step.detail.media.kind === 'image' &&
        step.detail.artifactId
      ) {
        artifactIds.add(step.detail.artifactId)
      }
    }
  }

  return artifactIds
}
