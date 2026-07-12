import type {
  DaemonMemoryItemSummary,
  MemoryGlobalSettings,
  TypedUlid,
} from '@/generated/daemon-protocol'

export const DEFAULT_MEMORY_TENANT_ID: TypedUlid = '00000000000000000000000001'

export type MemoryItemSummary = DaemonMemoryItemSummary
export type UpdateMemoryItemRequest = {
  actionPlanId?: TypedUlid
  content: string
  id: TypedUlid
}
export type DeleteMemoryItemRequest = {
  actionPlanId?: TypedUlid
  id: TypedUlid
}
export type UpdateMemorySettingsRequest = {
  settings: MemoryGlobalSettings
  tenant_id: TypedUlid
}
