import { useTranslation } from 'react-i18next'
import type {
  AssistantWork,
  ConversationEventRef,
  ResolvePermissionRequest,
} from '@/shared/tauri/commands'
import { TimelineBlockRenderer } from './timeline-block-renderer'
import { buildTimelineRenderBlocks } from './timeline-render-blocks'

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
  onPermissionResolve?: (request: ResolvePermissionRequest) => void
  onReviewContinue?: (prompt: string) => void
  turnId: string
}) {
  const { t } = useTranslation('conversation')
  const modelLabel = assistant.model?.displayName
  const blocks = buildTimelineRenderBlocks(assistant)

  return (
    <section className="min-w-0 max-w-[1040px]">
      <div className="mb-2 flex items-center gap-2 text-muted-foreground text-xs">
        <span>{t('timeline.assistantAuthor')}</span>
        {modelLabel ? (
          <span
            className="max-w-48 truncate rounded border border-border px-1.5 py-0.5 text-[11px] text-muted-foreground/80"
            title={modelLabel}
          >
            {modelLabel}
          </span>
        ) : null}
        {assistant.durationMs !== undefined ? (
          <span className="text-muted-foreground/80">
            {t('timeline.assistantDuration', { duration: assistant.durationMs })}
          </span>
        ) : null}
      </div>
      {assistant.status === 'running' || assistant.status === 'failed' ? (
        <div className="mb-3 border-border border-b pb-2 text-muted-foreground text-xs">
          {assistant.status === 'running'
            ? t('timeline.assistantStatus.running')
            : t('timeline.assistantStatus.failed')}
        </div>
      ) : null}
      <div className="grid gap-3">
        {blocks.map((block) => (
          <TimelineBlockRenderer
            block={block}
            conversationId={conversationId}
            key={block.id}
            onOpenDetails={onOpenDetails}
            onPermissionResolve={onPermissionResolve}
            onReviewContinue={onReviewContinue}
            runId={assistant.runId}
            turnId={turnId}
          />
        ))}
      </div>
    </section>
  )
}
