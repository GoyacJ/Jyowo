import type { ToolAttempt, ToolPermissionState } from '@/shared/tauri/commands'

export type PendingToolPermission = ToolPermissionState & {
  conversationId: string
  toolAttempt: ToolAttempt
  turnId: string
}
