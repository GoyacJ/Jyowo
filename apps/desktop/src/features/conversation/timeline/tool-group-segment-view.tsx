import type { ConversationEventRef, ToolGroupSegment } from '@/shared/tauri/commands'
import { ToolAttemptRow } from './tool-attempt-row'
import { ToolEvidenceSummary } from './tool-evidence-summary'

export function ToolGroupSegmentView({
  conversationId,
  onOpenDetails,
  onPermissionResolve,
  runId,
  segment,
  turnId,
}: {
  conversationId: string
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
  runId: string
  segment: ToolGroupSegment
  turnId: string
}) {
  return (
    <section className="grid gap-1.5">
      <ToolEvidenceSummary attempts={segment.attempts} />
      <div className="grid gap-1">
        {segment.attempts.map((attempt) => (
          <ToolAttemptRow
            attempt={attempt}
            attemptCount={segment.attempts.length}
            conversationId={conversationId}
            key={attempt.id}
            onOpenDetails={onOpenDetails}
            onPermissionResolve={onPermissionResolve}
            runId={runId}
            segmentId={segment.id}
            turnId={turnId}
          />
        ))}
      </div>
    </section>
  )
}
