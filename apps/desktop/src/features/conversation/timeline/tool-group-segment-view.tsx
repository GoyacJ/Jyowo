import { useUiStore } from '@/shared/state/ui-store'
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
  const attempts = [...segment.attempts].sort((left, right) => left.order - right.order)
  const completedAttempts = attempts.filter(isCompletedAttempt)
  const visibleAttempts = attempts.filter((attempt) => !isCompletedAttempt(attempt))
  const completedGroupDisclosureId = `tool-attempt-group:${conversationId}:${runId}:${turnId}:${segment.id}:completed`
  const storedCompletedGroupOpen = useUiStore(
    (state) => state.evidenceDisclosureOpen[completedGroupDisclosureId],
  )
  const setDisclosureOpen = useUiStore((state) => state.setEvidenceDisclosureOpen)
  const completedGroupOpen = storedCompletedGroupOpen ?? false

  return (
    <section className="grid gap-1.5">
      <ToolEvidenceSummary
        attempts={attempts}
        completedGroupOpen={completedGroupOpen}
        onCompletedGroupToggle={
          completedAttempts.length > 0
            ? () => setDisclosureOpen(completedGroupDisclosureId, !completedGroupOpen)
            : undefined
        }
      />
      <div className="grid gap-1">
        {completedGroupOpen
          ? completedAttempts.map((attempt) => (
              <ToolAttemptRow
                attempt={attempt}
                attemptCount={attempts.length}
                conversationId={conversationId}
                defaultDetailOpen
                key={attempt.id}
                onOpenDetails={onOpenDetails}
                onPermissionResolve={onPermissionResolve}
                runId={runId}
                segmentId={segment.id}
                turnId={turnId}
              />
            ))
          : null}
        {visibleAttempts.map((attempt) => (
          <ToolAttemptRow
            attempt={attempt}
            attemptCount={attempts.length}
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

function isCompletedAttempt(attempt: ToolGroupSegment['attempts'][number]) {
  return attempt.status === 'completed'
}
