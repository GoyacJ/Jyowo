import { ChevronDown, ChevronRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import { useUiStore } from '@/shared/state/ui-store'
import type { ConversationEventRef, ToolAttempt } from '@/shared/tauri/commands'
import { PermissionInlinePanel } from './permission-inline-panel'

export function ToolAttemptRow({
  attempt,
  attemptCount,
  conversationId,
  defaultDetailOpen,
  onOpenDetails,
  onPermissionResolve,
  runId,
  segmentId,
  turnId,
}: {
  attempt: ToolAttempt
  attemptCount: number
  conversationId: string
  defaultDetailOpen?: boolean
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
  runId: string
  segmentId: string
  turnId: string
}) {
  const { t } = useTranslation('conversation')
  const eventRef = attempt.eventRefs?.[0]
  const executionStatus = t(`timeline.toolStatus.${attempt.status}`)
  const disclosureId = `tool-attempt:${conversationId}:${runId}:${turnId}:${segmentId}:${attempt.id}`
  const storedOpen = useUiStore((state) => state.evidenceDisclosureOpen[disclosureId])
  const setDisclosureOpen = useUiStore((state) => state.setEvidenceDisclosureOpen)
  const hasDetails = Boolean(attempt.permission || attempt.failureSummary)
  const forcedOpen = isForcedOpenAttempt(attempt)
  const defaultOpen = defaultDetailOpen ?? getDefaultOpen(attempt, attemptCount)
  const open = forcedOpen || (storedOpen ?? defaultOpen)
  const canToggle = hasDetails && !forcedOpen

  return (
    <div
      className="grid gap-2 rounded-md px-2 py-2"
      data-tool-attempt-id={attempt.id}
      data-tool-attempt-status={attempt.status}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        {canToggle ? (
          <button
            aria-expanded={open}
            className="flex min-w-0 items-center gap-1.5 text-left"
            onClick={() => setDisclosureOpen(disclosureId, !open)}
            type="button"
          >
            {open ? (
              <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
            ) : (
              <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />
            )}
            <ToolAttemptTitle executionStatus={executionStatus} toolName={attempt.toolName} />
          </button>
        ) : (
          <div className={cn('flex min-w-0 items-center gap-1.5', hasDetails ? 'pl-5' : null)}>
            <ToolAttemptTitle executionStatus={executionStatus} toolName={attempt.toolName} />
          </div>
        )}
        {eventRef ? (
          <button
            className="rounded-md border border-border px-2 py-1 text-xs"
            onClick={() => onOpenDetails?.(eventRef)}
            type="button"
          >
            {t('timeline.details')}
          </button>
        ) : null}
      </div>
      {open && hasDetails ? (
        <div className="grid gap-2 pl-5">
          {attempt.permission ? (
            <PermissionInlinePanel
              conversationId={conversationId}
              onResolve={onPermissionResolve}
              permission={attempt.permission}
              turnId={turnId}
            />
          ) : null}
          {attempt.failureSummary ? (
            <p className="border-destructive/40 border-l pl-3 text-destructive text-sm">
              {attempt.failureSummary}
            </p>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

function ToolAttemptTitle({
  executionStatus,
  toolName,
}: {
  executionStatus: string
  toolName: string
}) {
  const { t } = useTranslation('conversation')

  return (
    <span className="grid min-w-0 gap-0.5">
      <span className="truncate font-medium text-sm">{toolName}</span>
      <span className="text-muted-foreground text-xs">
        {t('timeline.executionStatus', { status: executionStatus })}
      </span>
    </span>
  )
}

function isForcedOpenAttempt(attempt: ToolAttempt) {
  return (
    attempt.status === 'failed' ||
    attempt.status === 'denied' ||
    attempt.status === 'running' ||
    attempt.status === 'waitingPermission' ||
    attempt.permission?.status === 'denied' ||
    attempt.permission?.status === 'failed' ||
    attempt.permission?.status === 'pending'
  )
}

function getDefaultOpen(attempt: ToolAttempt, attemptCount: number) {
  if (isForcedOpenAttempt(attempt)) {
    return true
  }

  if (attempt.status === 'completed' && attempt.permission) {
    return false
  }

  if (attempt.status === 'completed' && attemptCount > 2) {
    return false
  }

  return true
}
