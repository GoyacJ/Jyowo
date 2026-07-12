/* eslint-disable */
// Generated from jyowo-harness-contracts. Do not edit by hand.

/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ClientRequest".
 */
export type ClientRequest =
  | {
      clientId: TypedUlid
      clientVersion: string
      connectionToken: string
      lastAcknowledgedOffset: number
      type: 'handshake'
      userInstanceId: string
    }
  | {
      metadata: CommandMetadata
      title: string
      type: 'create_task'
      workspace: WorkspaceSelection
    }
  | {
      metadata: CommandMetadata
      taskId: TypedUlid
      title: string
      type: 'rename_task'
    }
  | {
      metadata: CommandMetadata
      pinned: boolean
      taskId: TypedUlid
      type: 'set_task_pinned'
    }
  | {
      archived: boolean
      metadata: CommandMetadata
      taskId: TypedUlid
      type: 'set_task_archived'
    }
  | {
      metadata: CommandMetadata
      taskId: TypedUlid
      type: 'remove_task'
    }
  | {
      attachments: TypedUlid[]
      content: string
      contextReferences: string[]
      metadata: CommandMetadata
      modelConfigId?: string | null
      permissionMode?:
        | 'default'
        | 'plan'
        | 'accept_edits'
        | 'bypass_permissions'
        | 'dont_ask'
        | 'auto'
      taskId: TypedUlid
      type: 'submit_message'
    }
  | {
      attachments: TypedUlid[]
      content: string
      contextReferences: string[]
      expectedRevision: number
      metadata: CommandMetadata
      queueItemId: TypedUlid
      taskId: TypedUlid
      type: 'edit_queued_message'
    }
  | {
      expectedRevision: number
      metadata: CommandMetadata
      queueItemId: TypedUlid
      taskId: TypedUlid
      type: 'delete_queued_message'
    }
  | {
      expectedRevision: number
      metadata: CommandMetadata
      mode: PromotionMode
      queueItemId: TypedUlid
      taskId: TypedUlid
      type: 'promote_queued_message'
    }
  | {
      metadata: CommandMetadata
      mode: StopMode
      taskId: TypedUlid
      type: 'stop_run'
    }
  | {
      indeterminateTools: IndeterminateToolDecision[]
      metadata: CommandMetadata
      taskId: TypedUlid
      type: 'continue_task'
    }
  | {
      metadata: CommandMetadata
      optionId: string
      permissionRequestId: TypedUlid
      requestRevision: number
      taskId: TypedUlid
      type: 'resolve_permission'
    }
  | {
      afterOffset: number
      type: 'subscribe_events'
    }
  | {
      taskId: TypedUlid
      type: 'load_task'
    }
  | {
      beforeGlobalOffset?: number | null
      limit: number
      taskId: TypedUlid
      type: 'load_task_events'
    }
  | {
      type: 'list_tasks'
    }
  | {
      type: 'list_runtime_tools'
      workspaceRoot?: string | null
    }
  | {
      type: 'list_memory_items'
      workspaceRoot?: string | null
    }
  | {
      memoryId: TypedUlid
      type: 'get_memory_item'
      workspaceRoot?: string | null
    }
  | {
      memoryId: TypedUlid
      type: 'delete_memory_item'
      workspaceRoot?: string | null
    }
  | {
      type: 'list_automations'
      workspaceRoot?: string | null
    }
  | {
      automation: AutomationSpec
      type: 'save_automation'
      workspaceRoot?: string | null
    }
  | {
      automationId: string
      enabled: boolean
      type: 'set_automation_enabled'
      workspaceRoot?: string | null
    }
  | {
      automationId: string
      type: 'delete_automation'
      workspaceRoot?: string | null
    }
  | {
      automationId: string
      type: 'run_automation_now'
      workspaceRoot?: string | null
    }
  | {
      automationId?: string | null
      type: 'list_automation_runs'
      workspaceRoot?: string | null
    }
  | {
      base64Data: string
      mediaType: string
      taskId: TypedUlid
      type: 'stage_blob'
    }
  | {
      blobId: TypedUlid
      type: 'read_blob'
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "TypedUlid".
 */
export type TypedUlid = string
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "WorkspaceMode".
 */
export type WorkspaceMode = 'current' | 'managed_worktree'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "PromotionMode".
 */
export type PromotionMode = 'safe_point' | 'force_stop'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "StopMode".
 */
export type StopMode = 'safe_point' | 'force'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "IndeterminateToolResolution".
 */
export type IndeterminateToolResolution = 'treat_as_failed' | 'execute_again'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "PermissionMode".
 */
export type PermissionMode =
  | 'default'
  | 'plan'
  | 'accept_edits'
  | 'bypass_permissions'
  | 'dont_ask'
  | 'auto'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "SandboxMode".
 */
export type SandboxMode =
  | ('none' | 'container' | 'remote')
  | {
      os_level: LocalIsolationTag
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "LocalIsolationTag".
 */
export type LocalIsolationTag = 'none' | 'bubblewrap' | 'seatbelt' | 'job_object'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ToolProfile".
 */
export type ToolProfile =
  | ('minimal' | 'coding' | 'full')
  | {
      custom: {
        allowlist?: string[]
        denylist?: string[]
        group_allowlist?: ToolGroup[]
        group_denylist?: ToolGroup[]
        mcp_included?: boolean
        plugin_included?: boolean
      }
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ToolGroup".
 */
export type ToolGroup =
  | (
      | 'file_system'
      | 'search'
      | 'network'
      | 'shell'
      | 'git'
      | 'worktree'
      | 'session'
      | 'artifact'
      | 'browser'
      | 'computer'
      | 'image'
      | 'notebook'
      | 'lsp'
      | 'automation'
      | 'workflow'
      | 'agent'
      | 'coordinator'
      | 'memory'
      | 'clarification'
      | 'meta'
    )
  | {
      custom: string
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "WorkspaceAccess".
 */
export type WorkspaceAccess =
  | ('none' | 'read_only')
  | {
      read_write: {
        allowed_writable_subpaths: string[]
      }
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "AutomationWorkspaceScope".
 */
export type AutomationWorkspaceScope = 'current_workspace'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ServerMessage".
 */
export type ServerMessage =
  | {
      agentCapabilities: AgentCapabilities
      daemonVersion: string
      latestGlobalOffset: number
      type: 'handshake'
      userInstanceId: string
    }
  | {
      commandId: TypedUlid
      committedOffset: number
      streamVersion: number
      taskId: TypedUlid
      type: 'command_accepted'
    }
  | {
      commandId?: TypedUlid | null
      currentStreamVersion?: number | null
      latestQueueItem?: QueueItemProjection | null
      reason: CommandRejectionReason
      taskId?: TypedUlid | null
      type: 'command_rejected'
    }
  | {
      projection: TaskProjection
      snapshotOffset: number
      timeline: TimelineItemProjection[]
      type: 'task_snapshot'
    }
  | {
      events: TaskEventEnvelope[]
      nextBeforeOffset?: number | null
      taskId: TypedUlid
      type: 'task_event_page'
    }
  | {
      tasks: TaskProjection[]
      type: 'task_list'
    }
  | {
      generation: number
      tools: RuntimeToolSummary[]
      type: 'runtime_tools'
    }
  | {
      items: MemoryRecord[]
      type: 'memory_items'
    }
  | {
      item?: MemoryRecord | null
      type: 'memory_item'
    }
  | {
      memoryId: TypedUlid
      type: 'memory_deleted'
    }
  | {
      automations: AutomationSpec[]
      type: 'automations'
    }
  | {
      automation: AutomationSpec
      type: 'automation_saved'
    }
  | {
      automation: AutomationSpec
      type: 'automation_enabled'
    }
  | {
      automationId: string
      type: 'automation_deleted'
    }
  | {
      run: AutomationRunRecord
      type: 'automation_run'
    }
  | {
      runs: AutomationRunRecord[]
      type: 'automation_runs'
    }
  | {
      afterOffset: number
      events: TaskEventEnvelope[]
      gap: boolean
      latestOffset: number
      type: 'event_batch'
    }
  | {
      base64Data?: string | null
      blobId: TypedUlid
      /**
       * @minItems 32
       * @maxItems 32
       */
      contentHash: [
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
        number,
      ]
      mediaType: string
      missing: boolean
      size: number
      type: 'blob'
    }
  | {
      code: ProtocolErrorCode
      message: string
      type: 'error'
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "QueueItemState".
 */
export type QueueItemState = 'queued' | 'promoting' | 'consumed' | 'deleted'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "CommandRejectionReason".
 */
export type CommandRejectionReason =
  | 'invalid_command'
  | 'wrong_expected_version'
  | 'stale_queue_revision'
  | 'invalid_transition'
  | 'permission_expired'
  | 'workspace_conflict'
  | 'not_found'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "RunState".
 */
export type RunState =
  | 'running'
  | 'waiting_permission'
  | 'yielding'
  | 'interrupted'
  | 'failed'
  | 'completed'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "RunTerminalReason".
 */
export type RunTerminalReason =
  | 'completed'
  | 'superseded'
  | 'forced_interruption'
  | 'interrupted_by_restart'
  | 'cancelled'
  | 'failed'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "DaemonPermissionKind".
 */
export type DaemonPermissionKind = 'command' | 'filesystem' | 'network' | 'mcp' | 'automation'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "PermissionRoute".
 */
export type PermissionRoute = 'foreground_task' | 'saved_policy'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "TaskState".
 */
export type TaskState =
  | 'idle'
  | 'running'
  | 'waiting_permission'
  | 'yielding'
  | 'interrupted'
  | 'failed'
  | 'completed'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "SubagentActorState".
 */
export type SubagentActorState =
  | 'starting'
  | 'running'
  | 'yielding'
  | 'background'
  | 'completed'
  | 'cancelled'
  | 'failed'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "TimelineEventKind".
 */
export type TimelineEventKind =
  | 'user_message'
  | 'assistant_text'
  | 'tool_activity'
  | 'command'
  | 'diff'
  | 'image'
  | 'permission'
  | 'compaction'
  | 'subagent'
  | 'notice'
  | 'error'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "EventSourceKind".
 */
export type EventSourceKind =
  | 'user'
  | 'assistant'
  | 'engine'
  | 'tool'
  | 'permission_broker'
  | 'supervisor'
  | 'subagent'
  | 'recovery'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryKind".
 */
export type MemoryKind =
  | ('user_preference' | 'feedback' | 'project_fact' | 'reference' | 'agent_self_note')
  | {
      custom: string
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryVisibility".
 */
export type MemoryVisibility =
  | 'tenant'
  | {
      private: {
        session_id: TypedUlid
      }
    }
  | {
      user: {
        user_id: string
      }
    }
  | {
      team: {
        team_id: TypedUlid
      }
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "AutomationRunStatus".
 */
export type AutomationRunStatus = 'started' | 'rejected' | 'failed'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ProtocolErrorCode".
 */
export type ProtocolErrorCode =
  | 'invalid_frame'
  | 'frame_too_large'
  | 'protocol_mismatch'
  | 'authentication_failed'
  | 'not_found'
  | 'internal'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ChildAttachment".
 */
export type ChildAttachment = 'attached' | 'detached'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MissedRunPolicy".
 */
export type MissedRunPolicy = 'skip' | 'run_once'

export interface DaemonProtocol {
  client: ClientFrame
  server: ServerFrame
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ClientFrame".
 */
export interface ClientFrame {
  protocolVersion: number
  request: ClientRequest
  requestId: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "CommandMetadata".
 */
export interface CommandMetadata {
  commandId: TypedUlid
  expectedStreamVersion: number
  idempotencyKey: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "WorkspaceSelection".
 */
export interface WorkspaceSelection {
  mode: WorkspaceMode
  root: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "IndeterminateToolDecision".
 */
export interface IndeterminateToolDecision {
  resolution: IndeterminateToolResolution
  toolUseId: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "AutomationSpec".
 */
export interface AutomationSpec {
  createdAt: string
  enabled?: boolean
  id: string
  missedRunPolicy?: 'skip' | 'run_once'
  permissionMode: PermissionMode
  prompt: string
  sandboxMode: SandboxMode
  schedule: AutomationSchedule
  toolProfile: ToolProfile
  updatedAt: string
  workspaceAccess: WorkspaceAccess
  workspaceScope: AutomationWorkspaceScope
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "AutomationSchedule".
 */
export interface AutomationSchedule {
  intervalMinutes: number
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ServerFrame".
 */
export interface ServerFrame {
  message: ServerMessage
  protocolVersion: number
  requestId?: string | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "AgentCapabilities".
 */
export interface AgentCapabilities {
  agentTeams: boolean
  backgroundAgents: boolean
  subagents: boolean
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "QueueItemProjection".
 */
export interface QueueItemProjection {
  attachments: TypedUlid[]
  consumedBy?: TypedUlid | null
  content: string
  contextReferences: string[]
  createdAt: string
  createdGlobalOffset: number
  queueItemId: TypedUlid
  revision: number
  state: QueueItemState
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "TaskProjection".
 */
export interface TaskProjection {
  actorId?: TypedUlid | null
  archived: boolean
  contextCursor?: number
  currentRun?: RunProjection | null
  lastGlobalOffset: number
  parent?: SubagentParentProjection | null
  pendingPermission?: PermissionProjection | null
  pinned?: boolean
  queue: QueueItemProjection[]
  removed?: boolean
  state: TaskState
  streamVersion: number
  subagents?: SubagentProjection[]
  taskId: TypedUlid
  title: string
  workspace?: WorkspaceSelection | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "RunProjection".
 */
export interface RunProjection {
  endedAt?: string | null
  incompleteOutput: boolean
  promotionMode?: PromotionMode | null
  segmentId: TypedUlid
  startedAt: string
  state: RunState
  terminalReason?: RunTerminalReason | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "SubagentParentProjection".
 */
export interface SubagentParentProjection {
  attachment?: 'attached' | 'detached'
  delegationId: TypedUlid
  parentSegmentId: TypedUlid
  parentTaskId: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "PermissionProjection".
 */
export interface PermissionProjection {
  details?: PermissionRequestDetails | null
  requestId: TypedUlid
  revision: number
  route: PermissionRoute
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "PermissionRequestDetails".
 */
export interface PermissionRequestDetails {
  actionPlanHash: string
  actorSource: unknown
  expiresAt: string
  kind: DaemonPermissionKind
  options: PermissionOption[]
  preview: string
  sandboxPolicyHash: string
  segmentId: TypedUlid
  subject: unknown
  workspace: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "PermissionOption".
 */
export interface PermissionOption {
  label: string
  optionId: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "SubagentProjection".
 */
export interface SubagentProjection {
  actorId: TypedUlid
  childTaskId: TypedUlid
  contextCursor: number
  delegationId: TypedUlid
  detached: boolean
  endedAt?: string | null
  parentSegmentId: TypedUlid
  parentTaskId: TypedUlid
  segmentId: TypedUlid
  startedAt: string
  state: SubagentActorState
  summary?: string | null
  workspaceLeaseId?: TypedUlid | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "TimelineItemProjection".
 */
export interface TimelineItemProjection {
  blobId?: TypedUlid | null
  globalOffset: number
  id: string
  incomplete: boolean
  kind: TimelineEventKind
  runSegmentId?: TypedUlid | null
  semanticGroupId?: string | null
  summary: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "TaskEventEnvelope".
 */
export interface TaskEventEnvelope {
  eventId: TypedUlid
  eventType: string
  globalOffset: number
  payload: unknown
  recordedAt: string
  schemaVersion: number
  source: EventSource
  streamSequence: number
  taskId: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "EventSource".
 */
export interface EventSource {
  actorId?: TypedUlid | null
  clientId?: TypedUlid | null
  kind: EventSourceKind
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "RuntimeToolSummary".
 */
export interface RuntimeToolSummary {
  access: string
  category: string
  deferPolicy: string
  description: string
  displayName: string
  executionChannel: string
  group: string
  groupLabel: string
  longRunning: boolean
  name: string
  originId?: string | null
  originKind: string
  requiredCapabilities: string[]
  serviceBinding?: RuntimeToolServiceBindingSummary | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "RuntimeToolServiceBindingSummary".
 */
export interface RuntimeToolServiceBindingSummary {
  operationId: string
  providerId: string
  routeKind: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryRecord".
 */
export interface MemoryRecord {
  content: string
  created_at: string
  deleted_at?: string | null
  expires_at?: string | null
  id: TypedUlid
  kind: MemoryKind
  metadata: MemoryMetadata
  tenant_id: TypedUlid
  updated_at: string
  visibility: MemoryVisibility
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryMetadata".
 */
export interface MemoryMetadata {
  source_trust?: number
  tags?: string[]
  ttl?: Duration | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "Duration".
 */
export interface Duration {
  nanos: number
  secs: number
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "AutomationRunRecord".
 */
export interface AutomationRunRecord {
  automationId: string
  completedAt?: string | null
  id: string
  message?: string | null
  runId?: string | null
  startedAt: string
  status: AutomationRunStatus
}
