import { type CommandDetails, CommandPreview } from './CommandPreview'
import { PermissionDialog, type PermissionRequestDetails } from './PermissionDialog'
import { type RawJsonDetails, RawJsonView } from './RawJsonView'
import { ToolCallCard, type ToolCallDetails } from './ToolCallCard'

export type RunEventDetailsModel = {
  command?: CommandDetails
  permissions?: PermissionRequestDetails[]
  rawJson?: RawJsonDetails
  toolCall?: ToolCallDetails
}

type RunEventDetailsProps = {
  event: RunEventDetailsModel
  onApprovePermission?: (permissionId: string) => void
  onDenyPermission?: (permissionId: string) => void
  resolvingPermissionId?: string
}

export function RunEventDetails({
  event,
  onApprovePermission,
  onDenyPermission,
  resolvingPermissionId,
}: RunEventDetailsProps) {
  return (
    <section aria-label="Run event details" className="space-y-4">
      {event.toolCall ? <ToolCallCard toolCall={event.toolCall} /> : null}
      {event.command ? <CommandPreview command={event.command} /> : null}
      {event.permissions?.map((permission) => (
        <PermissionDialog
          key={permission.id}
          onApprove={onApprovePermission}
          onDeny={onDenyPermission}
          permission={permission}
          resolving={resolvingPermissionId === permission.id}
        />
      ))}
      {event.rawJson ? <RawJsonView rawJson={event.rawJson} /> : null}
    </section>
  )
}
