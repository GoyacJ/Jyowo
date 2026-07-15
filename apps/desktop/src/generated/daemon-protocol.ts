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
      contextReferences: ConversationContextReference[]
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
      contextReferences: ConversationContextReference[]
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
      afterGlobalOffset: number
      limit: number
      type: 'load_events'
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
      taskId: TypedUlid
      type: 'list_skill_reference_candidates'
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
      actionPlanId?: TypedUlid | null
      content: string
      memoryId: TypedUlid
      type: 'update_memory_item'
      workspaceRoot?: string | null
    }
  | {
      actionPlanId?: TypedUlid | null
      memoryId: TypedUlid
      type: 'delete_memory_item'
      workspaceRoot?: string | null
    }
  | {
      request: ExportMemoryItemsRequest
      type: 'export_memory_items'
      workspaceRoot?: string | null
    }
  | {
      request: ListMemoryCandidatesRequest
      type: 'list_memory_candidates'
      workspaceRoot?: string | null
    }
  | {
      request: ApproveMemoryCandidateRequest
      type: 'approve_memory_candidate'
      workspaceRoot?: string | null
    }
  | {
      request: RejectMemoryCandidateRequest
      type: 'reject_memory_candidate'
      workspaceRoot?: string | null
    }
  | {
      request: MergeMemoryCandidateRequest
      type: 'merge_memory_candidate'
      workspaceRoot?: string | null
    }
  | {
      request: ListMemoryRecallTracesRequest
      type: 'list_memory_recall_traces'
      workspaceRoot?: string | null
    }
  | {
      request: GetMemoryRecallTraceRequest
      type: 'get_memory_recall_trace'
      workspaceRoot?: string | null
    }
  | {
      request: GetModelRequestPreviewRequest
      type: 'get_model_request_preview'
      workspaceRoot?: string | null
    }
  | {
      request: GetMemorySettingsRequest
      type: 'get_memory_settings'
      workspaceRoot?: string | null
    }
  | {
      request: UpdateMemorySettingsRequest
      type: 'update_memory_settings'
      workspaceRoot?: string | null
    }
  | {
      request: GetThreadMemorySettingsRequest
      type: 'get_thread_memory_settings'
      workspaceRoot?: string | null
    }
  | {
      request: UpdateThreadMemorySettingsRequest
      type: 'update_thread_memory_settings'
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
 * via the `definition` "ConversationContextReference".
 */
export type ConversationContextReference =
  | {
      kind: 'workspace_file'
      label: string
      path: string
    }
  | {
      id: string
      kind: 'artifact'
      label: string
    }
  | {
      id: string
      kind: 'conversation'
      label: string
    }
  | {
      id: string
      kind: 'memory'
      label: string
      /**
       * Hydrated content, if resolved. Mutually exclusive with `label`-only rendering.
       */
      resolved_content?: string | null
    }
  | {
      kind: 'skill'
      label: string
      parameters?: {
        [k: string]: unknown
      }
      skillId: SkillId
      source?: SkillSourceKind | null
      version?: 1
    }
  | {
      id: string
      kind: 'tool'
      label: string
    }
  | {
      id: string
      kind: 'mcp_server'
      label: string
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "SkillId".
 */
export type SkillId = string
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "SkillSourceKind".
 */
export type SkillSourceKind =
  | ('bundled' | 'workspace' | 'user')
  | {
      plugin: PluginId
    }
  | {
      mcp: McpServerId
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "PluginId".
 */
export type PluginId = string
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "McpServerId".
 */
export type McpServerId = string
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
 * via the `definition` "MemoryCandidateState".
 */
export type MemoryCandidateState =
  | 'proposed'
  | 'approved'
  | 'rejected'
  | 'promoted'
  | 'merged'
  | 'expired'
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
 * via the `definition` "MemoryThreadMode".
 */
export type MemoryThreadMode = 'off' | 'read_only' | 'read_write' | 'candidate_only'
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
      message?: string | null
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
      afterGlobalOffset: number
      events: TaskEventEnvelope[]
      hasMore: boolean
      latestGlobalOffset: number
      nextAfterGlobalOffset: number
      type: 'event_history_page'
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
      skills: SkillReferenceCandidate[]
      type: 'skill_reference_candidates'
    }
  | {
      items: DaemonMemoryItemSummary[]
      type: 'memory_items'
    }
  | {
      item: DaemonMemoryItem
      type: 'memory_item'
    }
  | {
      item: DaemonMemoryItem
      type: 'memory_updated'
    }
  | {
      memoryId: TypedUlid
      type: 'memory_deleted'
    }
  | {
      auditHash: string
      exportedAt: string
      format: string
      includeHashes: boolean
      includeMetadata: boolean
      includeRawContent: boolean
      itemCount: number
      path: string
      scope: string
      type: 'memory_exported'
    }
  | {
      candidates: MemoryCandidateListItem[]
      next_cursor?: string | null
      type: 'memory_candidates'
    }
  | {
      candidate: MemoryCandidate
      memory_id: TypedUlid
      type: 'memory_candidate_approved'
    }
  | {
      candidate: MemoryCandidate
      type: 'memory_candidate_rejected'
    }
  | {
      candidate_ids: TypedUlid[]
      memory_id: TypedUlid
      type: 'memory_candidates_merged'
    }
  | {
      next_cursor?: string | null
      traces: MemoryRecallTraceSummary[]
      type: 'memory_recall_traces'
    }
  | {
      trace: MemoryRecallTrace
      type: 'memory_recall_trace'
    }
  | {
      preview: MemoryModelRequestPreview
      type: 'model_request_preview'
    }
  | {
      settings: MemoryGlobalSettings
      type: 'memory_settings'
    }
  | {
      settings: MemoryGlobalSettings
      type: 'memory_settings_updated'
    }
  | {
      settings: MemoryThreadSettings
      type: 'thread_memory_settings'
    }
  | {
      settings: MemoryThreadSettings
      type: 'thread_memory_settings_updated'
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
  | 'file'
  | 'artifact'
  | 'image'
  | 'permission'
  | 'compaction'
  | 'subagent'
  | 'notice'
  | 'error'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "TimelineToolOperation".
 */
export type TimelineToolOperation =
  | 'read'
  | 'edit'
  | 'search'
  | 'command'
  | 'browse'
  | 'generate'
  | 'delegate'
  | 'other'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "TimelineToolStatus".
 */
export type TimelineToolStatus = 'requested' | 'running' | 'completed' | 'denied' | 'failed'
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
 * @minItems 32
 * @maxItems 32
 *
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ContentHash".
 */
export type ContentHash = [
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
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryEvidenceOrigin".
 */
export type MemoryEvidenceOrigin =
  | {
      user_message: {
        message_id: TypedUlid
        run_id: TypedUlid
        session_id: TypedUlid
      }
    }
  | {
      assistant_message: {
        message_id: TypedUlid
        run_id: TypedUlid
        session_id: TypedUlid
      }
    }
  | {
      subagent_output: {
        agent_id?: TypedUlid | null
        child_session_id: TypedUlid
        parent_session_id: TypedUlid
        run_id: TypedUlid
      }
    }
  | {
      builtin_tool_output: {
        tool_name: string
        tool_use_id: TypedUlid
      }
    }
  | {
      mcp_tool_output: {
        server_id: string
        tool_name: string
        tool_use_id: TypedUlid
      }
    }
  | {
      plugin_output: {
        plugin_id: string
        tool_name?: string | null
        tool_use_id?: TypedUlid | null
      }
    }
  | {
      web_retrieval: {
        fetch_tool_use_id?: TypedUlid | null
        url_hash: ContentHash
      }
    }
  | {
      workspace_file: {
        path_hash: ContentHash
        snapshot_id?: TypedUlid | null
        workspace_id: TypedUlid
      }
    }
  | {
      imported: {
        import_id: string
        importer: string
      }
    }
  | {
      consolidated: {
        from: TypedUlid[]
      }
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemorySource".
 */
export type MemorySource =
  | (
      | 'user_input'
      | 'agent_derived'
      | 'tool_output'
      | 'mcp_tool_output'
      | 'plugin_output'
      | 'web_retrieval'
      | 'workspace_file'
      | 'external_retrieval'
      | 'imported'
    )
  | {
      subagent_derived: {
        child_session: TypedUlid
      }
    }
  | {
      consolidated: {
        from: TypedUlid[]
      }
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryPolicyDecision".
 */
export type MemoryPolicyDecision =
  | 'allow'
  | {
      deny: {
        reason: MemoryPolicyDenyReason
      }
    }
  | {
      candidate_only: {
        reason: MemoryPolicyDenyReason
      }
    }
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryPolicyDenyReason".
 */
export type MemoryPolicyDenyReason =
  | 'global_use_disabled'
  | 'thread_use_disabled'
  | 'global_generation_disabled'
  | 'thread_generation_disabled'
  | 'external_context_generation_disabled'
  | 'missing_policy'
  | 'visibility_escalation_denied'
  | 'provider_not_writable'
  | 'tenant_mismatch'
  | 'tombstone_matched'
  | 'permission_required'
  | 'threat_blocked'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryDropReason".
 */
export type MemoryDropReason =
  | 'expired'
  | 'deleted'
  | 'visibility_denied'
  | 'policy_denied'
  | 'threat_blocked'
  | 'budget_exceeded'
  | 'duplicate'
  | 'provider_timeout'
  | 'provider_error'
  | 'score_below_threshold'
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryProviderTrust".
 */
export type MemoryProviderTrust = 'built_in' | 'workspace' | 'team' | 'plugin' | 'external'
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
 * via the `definition` "MemoryCandidateOperation".
 */
export type MemoryCandidateOperation =
  | 'create'
  | {
      update: {
        memory_id: TypedUlid
      }
    }
  | {
      delete: {
        memory_id: TypedUlid
      }
    }
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
 * via the `definition` "ExportMemoryItemsRequest".
 */
export interface ExportMemoryItemsRequest {
  explicitUserAction: boolean
  format: string
  includeHashes: boolean
  includeMetadata: boolean
  includeRawContent: boolean
  scope: string
  sessionId?: TypedUlid | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ListMemoryCandidatesRequest".
 */
export interface ListMemoryCandidatesRequest {
  cursor?: string | null
  limit: number
  session_id?: TypedUlid | null
  state?: MemoryCandidateState | null
  tenant_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ApproveMemoryCandidateRequest".
 */
export interface ApproveMemoryCandidateRequest {
  action_plan_id?: TypedUlid | null
  candidate_id: TypedUlid
  tenant_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "RejectMemoryCandidateRequest".
 */
export interface RejectMemoryCandidateRequest {
  candidate_id: TypedUlid
  reason: string
  tenant_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MergeMemoryCandidateRequest".
 */
export interface MergeMemoryCandidateRequest {
  action_plan_id?: TypedUlid | null
  candidate_ids: TypedUlid[]
  merged_record: MemoryRecordDraft
  tenant_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryRecordDraft".
 */
export interface MemoryRecordDraft {
  content: string
  expires_at?: string | null
  kind: MemoryKind
  metadata: MemoryMetadata
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
 * via the `definition` "ListMemoryRecallTracesRequest".
 */
export interface ListMemoryRecallTracesRequest {
  cursor?: string | null
  limit: number
  run_id?: TypedUlid | null
  session_id?: TypedUlid | null
  tenant_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "GetMemoryRecallTraceRequest".
 */
export interface GetMemoryRecallTraceRequest {
  tenant_id: TypedUlid
  trace_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "GetModelRequestPreviewRequest".
 */
export interface GetModelRequestPreviewRequest {
  run_id: TypedUlid
  session_id: TypedUlid
  tenant_id: TypedUlid
  trace_id?: TypedUlid | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "GetMemorySettingsRequest".
 */
export interface GetMemorySettingsRequest {
  tenant_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "UpdateMemorySettingsRequest".
 */
export interface UpdateMemorySettingsRequest {
  settings: MemoryGlobalSettings
  tenant_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryGlobalSettings".
 */
export interface MemoryGlobalSettings {
  disable_generation_when_external_context_used: boolean
  generate_memories: boolean
  max_memory_bytes: number
  max_recall_chars_per_turn: number
  max_recall_records_per_turn: number
  retention_days?: number | null
  use_memories: boolean
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "GetThreadMemorySettingsRequest".
 */
export interface GetThreadMemorySettingsRequest {
  session_id: TypedUlid
  tenant_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "UpdateThreadMemorySettingsRequest".
 */
export interface UpdateThreadMemorySettingsRequest {
  settings: MemoryThreadSettings
  tenant_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryThreadSettings".
 */
export interface MemoryThreadSettings {
  generate_memories?: boolean | null
  memory_mode: MemoryThreadMode
  session_id: TypedUlid
  use_memories?: boolean | null
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
  contextReferences: ConversationContextReference[]
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
  tool?: TimelineToolProjection | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "TimelineToolProjection".
 */
export interface TimelineToolProjection {
  command?: string | null
  durationMs?: number | null
  operation: TimelineToolOperation
  output?: string | null
  resultSummary?: string | null
  status: TimelineToolStatus
  subject?: string | null
  toolName: string
  toolUseId: string
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
 * via the `definition` "SkillReferenceCandidate".
 */
export interface SkillReferenceCandidate {
  label: string
  skillId: SkillId
  source: SkillSourceKind
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "DaemonMemoryItemSummary".
 */
export interface DaemonMemoryItemSummary {
  contentHash: string
  contentPreview: string
  deleted: boolean
  expiresAt?: string | null
  id: TypedUlid
  kind: string
  lastAccessedAt?: string | null
  providerId?: string | null
  source: string
  tags: string[]
  updatedAt: string
  visibility: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "DaemonMemoryItem".
 */
export interface DaemonMemoryItem {
  accessCount: number
  confidence: number
  content: string
  contentHash: string
  createdAt: string
  deleted: boolean
  expiresAt?: string | null
  id: TypedUlid
  kind: string
  lastAccessedAt?: string | null
  providerId?: string | null
  source: string
  tags: string[]
  updatedAt: string
  visibility: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryCandidateListItem".
 */
export interface MemoryCandidateListItem {
  created_at: string
  evidence: MemoryEvidence
  expires_at?: string | null
  id: TypedUlid
  operation?:
    | 'create'
    | {
        update: {
          memory_id: TypedUlid
        }
      }
    | {
        delete: {
          memory_id: TypedUlid
        }
      }
  proposed_record: MemoryRecordDraft
  state: MemoryCandidateState
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryEvidence".
 */
export interface MemoryEvidence {
  content_hash: ContentHash
  message_id?: TypedUlid | null
  origin: MemoryEvidenceOrigin
  run_id?: TypedUlid | null
  session_id?: TypedUlid | null
  source: MemorySource
  tool_use_id?: TypedUlid | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryCandidate".
 */
export interface MemoryCandidate {
  created_at: string
  evidence: MemoryEvidence
  expires_at?: string | null
  id: TypedUlid
  operation?:
    | 'create'
    | {
        update: {
          memory_id: TypedUlid
        }
      }
    | {
        delete: {
          memory_id: TypedUlid
        }
      }
  proposed_record: MemoryRecordDraft
  state: MemoryCandidateState
  tenant_id: TypedUlid
  updated_at: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryRecallTraceSummary".
 */
export interface MemoryRecallTraceSummary {
  at: string
  dropped_count: number
  injected_count: number
  redacted_count: number
  run_id: TypedUlid
  session_id: TypedUlid
  tenant_id: TypedUlid
  trace_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryRecallTrace".
 */
export interface MemoryRecallTrace {
  at: string
  candidates: MemoryCandidateTrace[]
  deadline_used_ms: number
  dropped: MemoryDroppedTrace[]
  injected: MemoryInjectedTrace[]
  injected_chars: number
  provider_results: MemoryProviderTrace[]
  query_text_hash: ContentHash
  redacted_count: number
  run_id: TypedUlid
  session_id: TypedUlid
  tenant_id: TypedUlid
  trace_id: TypedUlid
  turn: number
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryCandidateTrace".
 */
export interface MemoryCandidateTrace {
  content_hash: ContentHash
  memory_id: TypedUlid
  policy_decision: MemoryPolicyDecision
  provider_id: string
  score: MemoryScoreBreakdown
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryScoreBreakdown".
 */
export interface MemoryScoreBreakdown {
  access_score: number
  confidence_score: number
  explicit_selection_boost: number
  final_score: number
  lexical_score: number
  recency_score: number
  source_trust_score: number
  vector_score?: number | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryDroppedTrace".
 */
export interface MemoryDroppedTrace {
  content_hash?: ContentHash | null
  memory_id?: TypedUlid | null
  provider_id?: string | null
  reason: MemoryDropReason
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryInjectedTrace".
 */
export interface MemoryInjectedTrace {
  content_hash: ContentHash
  fence_id: string
  injected_chars: number
  memory_id: TypedUlid
  provider_id: string
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryProviderTrace".
 */
export interface MemoryProviderTrace {
  error_kind?: string | null
  latency_ms: number
  provider_id: string
  readable: boolean
  requested_count: number
  returned_count: number
  timed_out: boolean
  trust_level: MemoryProviderTrust
  writable: boolean
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryModelRequestPreview".
 */
export interface MemoryModelRequestPreview {
  content_hash: ContentHash
  policy_decisions?: string[]
  redacted_count: number
  run_id: TypedUlid
  sections: MemoryModelRequestPreviewSection[]
  session_id: TypedUlid
  token_estimate: number
  tool_names?: string[]
  trace_id?: TypedUlid | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MemoryModelRequestPreviewSection".
 */
export interface MemoryModelRequestPreviewSection {
  memory_ids: TypedUlid[]
  provider_id?: string | null
  redacted_content: string
  source: MemorySource
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
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ApproveMemoryCandidateResponse".
 */
export interface ApproveMemoryCandidateResponse {
  candidate: MemoryCandidate
  memory_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "GetMemoryRecallTraceResponse".
 */
export interface GetMemoryRecallTraceResponse {
  trace: MemoryRecallTrace
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "GetMemorySettingsResponse".
 */
export interface GetMemorySettingsResponse {
  settings: MemoryGlobalSettings
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "GetModelRequestPreviewResponse".
 */
export interface GetModelRequestPreviewResponse {
  preview: MemoryModelRequestPreview
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "GetThreadMemorySettingsResponse".
 */
export interface GetThreadMemorySettingsResponse {
  settings: MemoryThreadSettings
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ListMemoryCandidatesResponse".
 */
export interface ListMemoryCandidatesResponse {
  candidates: MemoryCandidateListItem[]
  next_cursor?: string | null
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "ListMemoryRecallTracesResponse".
 */
export interface ListMemoryRecallTracesResponse {
  next_cursor?: string | null
  traces: MemoryRecallTraceSummary[]
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "MergeMemoryCandidateResponse".
 */
export interface MergeMemoryCandidateResponse {
  candidate_ids: TypedUlid[]
  memory_id: TypedUlid
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "RejectMemoryCandidateResponse".
 */
export interface RejectMemoryCandidateResponse {
  candidate: MemoryCandidate
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "UpdateMemorySettingsResponse".
 */
export interface UpdateMemorySettingsResponse {
  settings: MemoryGlobalSettings
}
/**
 * This interface was referenced by `DaemonProtocol`'s JSON-Schema
 * via the `definition` "UpdateThreadMemorySettingsResponse".
 */
export interface UpdateThreadMemorySettingsResponse {
  settings: MemoryThreadSettings
}
