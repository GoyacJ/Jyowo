import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { ToolPermissionState } from '@/shared/tauri/commands'

export function PermissionInlinePanel({
  conversationId,
  onResolve,
  permission,
}: {
  conversationId: string
  onResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
    confirmationText?: string
  }) => void
  permission: ToolPermissionState
  turnId: string
}) {
  const { t } = useTranslation('conversation')
  const [confirmationText, setConfirmationText] = useState('')
  const canResolve = permission.status === 'pending' || permission.status === 'failed'
  const permissionStatus = t(`timeline.permissionStatusLabel.${permission.status}`)
  const summary = displayPermissionSummary(permission.summary, permission.status)
  const expectedConfirmation = permission.confirmationExpected
  const canApprove = !expectedConfirmation || confirmationText === expectedConfirmation

  return (
    <div
      className="rounded-md bg-muted px-3 py-2 text-sm"
      data-permission-request-id={permission.requestId}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span>{t('timeline.permissionStatus', { status: permissionStatus })}</span>
        {canResolve ? (
          <span className="flex gap-2">
            <button
              className="rounded-md border border-border px-2 py-1 text-xs"
              onClick={() =>
                onResolve?.({
                  conversationId,
                  requestId: permission.requestId,
                  decision: 'deny',
                })
              }
              type="button"
            >
              {t('timeline.deny')}
            </button>
            <button
              className="rounded-md bg-primary px-2 py-1 text-primary-foreground text-xs disabled:cursor-not-allowed disabled:opacity-60"
              disabled={!canApprove}
              onClick={() =>
                onResolve?.({
                  conversationId,
                  requestId: permission.requestId,
                  decision: 'approve',
                  ...(expectedConfirmation ? { confirmationText } : {}),
                })
              }
              type="button"
            >
              {t('timeline.approve')}
            </button>
          </span>
        ) : null}
      </div>
      {canResolve && expectedConfirmation ? (
        <label className="mt-2 grid gap-1 text-xs">
          <span className="text-muted-foreground">{t('timeline.confirmationText')}</span>
          <input
            className="h-8 rounded-md border border-border bg-background px-2 text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onChange={(event) => setConfirmationText(event.currentTarget.value)}
            value={confirmationText}
          />
        </label>
      ) : null}
      {summary ? <p className="mt-1 text-muted-foreground text-xs">{summary}</p> : null}
    </div>
  )
}

function displayPermissionSummary(
  summary: string | undefined,
  status: ToolPermissionState['status'],
) {
  if (!summary) {
    return null
  }

  if (status === 'approved' && /^approved\b/i.test(summary)) {
    return null
  }

  if (status === 'pending' && /^(awaiting approval|pending approval|pending)\b/i.test(summary)) {
    return null
  }

  if (
    (summary === '需要批准后才能继续。' || summary === '需要批准后才可继续。') &&
    status !== 'pending'
  ) {
    return null
  }

  return summary
}
