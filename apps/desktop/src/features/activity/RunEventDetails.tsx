import { useTranslation } from 'react-i18next'

import { type CommandDetails, CommandPreview } from './CommandPreview'
import { type ActivityPermissionDetails, PermissionDialog } from './PermissionDialog'
import { type RawJsonDetails, RawJsonView } from './RawJsonView'
import { ToolCallCard, type ToolCallDetails } from './ToolCallCard'

type RunEventDetailsModel = {
  command?: CommandDetails
  permissions?: ActivityPermissionDetails[]
  rawJson?: RawJsonDetails
  toolCall?: ToolCallDetails
}

type RunEventDetailsProps = {
  event: RunEventDetailsModel
  onApprovePermission?: (permissionId: string) => void
  onDenyPermission?: (permissionId: string) => void
  readOnly?: boolean
  resolvingPermissionId?: string
}

export function RunEventDetails({
  event,
  onApprovePermission,
  onDenyPermission,
  readOnly = false,
  resolvingPermissionId,
}: RunEventDetailsProps) {
  const { t } = useTranslation('activity')
  const canResolvePermission = !readOnly

  return (
    <section aria-label={t('runEventDetails')} className="space-y-4">
      {event.toolCall ? <ToolCallCard toolCall={event.toolCall} /> : null}
      {event.command ? <CommandPreview command={event.command} /> : null}
      {event.permissions?.map((permission) => (
        <PermissionDialog
          key={permission.id}
          onApprove={canResolvePermission ? onApprovePermission : undefined}
          onDeny={canResolvePermission ? onDenyPermission : undefined}
          permission={permission}
          resolving={resolvingPermissionId === permission.id}
        />
      ))}
      {event.rawJson ? <RawJsonView rawJson={event.rawJson} /> : null}
    </section>
  )
}
