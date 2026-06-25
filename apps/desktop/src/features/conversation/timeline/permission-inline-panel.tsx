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
  }) => void
  permission: ToolPermissionState
  turnId: string
}) {
  const { t } = useTranslation('conversation')
  const canResolve = permission.status === 'pending' || permission.status === 'failed'
  const permissionStatus = t(`timeline.permissionStatusLabel.${permission.status}`)

  return (
    <div className="rounded-md bg-muted px-3 py-2 text-sm">
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
              className="rounded-md bg-primary px-2 py-1 text-primary-foreground text-xs"
              onClick={() =>
                onResolve?.({
                  conversationId,
                  requestId: permission.requestId,
                  decision: 'approve',
                })
              }
              type="button"
            >
              {t('timeline.approve')}
            </button>
          </span>
        ) : null}
      </div>
      {permission.summary ? (
        <p className="mt-1 text-muted-foreground text-xs">{permission.summary}</p>
      ) : null}
    </div>
  )
}
