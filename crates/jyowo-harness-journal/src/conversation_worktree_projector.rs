//! Pure conversation worktree projection.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::Arc;

use crate::evidence::{
    EvidenceRefRecord, EvidenceRefSource, EvidenceRefStore, RedactionProvenance,
};

use harness_contracts::{
    AgentActivityKind, AgentActivityPermissionState, AgentActivitySegment, AgentActivityStatus,
    AgentTeamActivityDetails, AgentTeamMemberActivity, AgentTeamTaskActivity, ArtifactMediaKind,
    ArtifactMediaPreview, ArtifactRevisionKind, ArtifactRevisionStatus, ArtifactRevisionSummary,
    ArtifactSegment, ArtifactSource, ArtifactStatus, AssistantNoticeCode, AssistantSegment,
    AssistantWork, AssistantWorkModelSnapshot, AssistantWorkStatus, BlobId, BlobRef, BlobRetention,
    ChangeSet, ChangeSetFile, ChangeSetFileStatus, ClarificationRequestSegment, CommandExecution,
    ConversationAttachmentReference, ConversationCursor, ConversationEventRef,
    ConversationTimelineEvent, ConversationTurn, ConversationTurnUserMessage,
    ConversationWorktreePage, DataExposure, DataExposureSecretRisk, DecisionConfirmation,
    DecisionKind, DecisionLifetime, DecisionMatcherKind, DecisionMatcherSummary, DecisionOperation,
    DecisionOption, DecisionPolicy, DecisionRequestState, DecisionRequestStatus, DecisionTarget,
    DecisionTargetKind, ErrorSegment, EvidenceRedactionState, EvidenceRefId, EvidenceRefKind,
    JournalError, NoticeSegment, ProcessSegment, ProcessSegmentStatus, ProcessStep,
    ProcessStepDetail, ProcessStepKind, ProcessStepStatus, ReviewRequestSegment, RiskLevel,
    TenantId, TextSegment, ToolAttempt, ToolAttemptOrigin, ToolAttemptStatus, ToolGroupSegment,
    UiSafeText, UiVisibility,
};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ConversationTurnPageDirection {
    Before,
    After,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ConversationWorktreeProjection {
    pub turns: Vec<ConversationTurn>,
    pub event_cursor: Option<ConversationCursor>,
    pub event_refs: Vec<ConversationEventRef>,
}

#[must_use]
pub fn project_conversation_worktree_snapshot(
    conversation_id: &str,
    events: impl IntoIterator<Item = ConversationTimelineEvent>,
) -> ConversationWorktreeProjection {
    let mut state = ProjectionState {
        conversation_id,
        turns: Vec::new(),
        run_turns: HashMap::new(),
        request_tools: HashMap::new(),
        run_models: HashMap::new(),
        subagent_requests: HashMap::new(),
        agent_tool_tasks: HashMap::new(),
        seen_event_ids: HashSet::new(),
        event_cursor: None,
        event_refs: Vec::new(),
    };

    for event in events {
        if !state.seen_event_ids.insert(event.id.clone()) {
            continue;
        }
        state.event_cursor = Some(event.cursor);
        let event_ref = event_ref(&event);
        state.event_refs.push(event_ref.clone());
        match event.event_type.as_str() {
            "run.started" => state.project_run_started(&event, event_ref),
            "user.message.appended" => state.project_user_message(&event, event_ref),
            "assistant.completed" => state.project_assistant_completed(&event, event_ref),
            "assistant.delta" => state.project_assistant_delta(&event, event_ref),
            "assistant.thinking.delta" => state.project_thinking(&event, event_ref),
            "tool.requested" => state.project_tool_requested(&event, event_ref),
            "tool.completed" => {
                if let Some(tool_name) = state.update_tool_status(
                    &event,
                    event_ref.clone(),
                    ToolAttemptStatus::Completed,
                ) {
                    state.project_tool_completed_process_step(&event, event_ref, tool_name);
                }
            }
            "tool.failed" => state.project_tool_failed(&event, event_ref),
            "tool.denied" => {
                state.update_tool_status(&event, event_ref, ToolAttemptStatus::Denied);
            }
            "permission.requested" => state.project_permission_requested(&event, event_ref),
            "permission.resolved" => state.project_permission_resolved(&event, event_ref),
            "artifact.created" | "artifact.updated" => {
                state.project_artifact_lifecycle(&event, event_ref)
            }
            "assistant.review.requested" => state.project_review_requested(&event, event_ref),
            "assistant.clarification.requested" => {
                state.project_clarification_requested(&event, event_ref)
            }
            "assistant.notice" => state.project_notice(&event, event_ref),
            "run.ended" => state.project_run_ended(&event, event_ref),
            "engine.failed" => state.project_engine_failed(&event, event_ref),
            "subagent.spawned" => state.project_subagent_spawned(&event, event_ref),
            "subagent.announced" => state.project_subagent_announced(&event, event_ref),
            "subagent.terminated" => state.project_subagent_terminated(&event, event_ref),
            "subagent.stalled" => state.project_subagent_stalled(&event, event_ref),
            "subagent.permission.forwarded" => {
                state.project_subagent_permission_forwarded(&event, event_ref);
            }
            "subagent.permission.resolved" => {
                state.project_subagent_permission_resolved(&event, event_ref);
            }
            "team.created" => state.project_team_created(&event, event_ref),
            "team.member.joined" => state.project_team_member_joined(&event, event_ref),
            "team.member.left" => state.project_team_member_left(&event, event_ref),
            "team.member.stalled" => state.project_team_member_stalled(&event, event_ref),
            "agent.message.sent" => state.project_team_message_sent(&event, event_ref),
            "agent.message.routed" => state.project_team_message_routed(&event, event_ref),
            "team.turn.completed" => state.project_team_turn_completed(&event, event_ref),
            "team.task.updated" => state.project_team_task_updated(&event, event_ref),
            "team.terminated" => state.project_team_terminated(&event, event_ref),
            "background.started" => state.project_background_started(&event, event_ref),
            "background.state.changed" => {
                state.project_background_state_changed(&event, event_ref);
            }
            "background.input.requested" => {
                state.project_background_input_requested(&event, event_ref);
            }
            "background.input.submitted" => {
                state.project_background_input_submitted(&event, event_ref);
            }
            "background.permission.requested" => {
                state.project_background_permission_requested(&event, event_ref);
            }
            "background.permission.resolved" => {
                state.project_background_permission_resolved(&event, event_ref);
            }
            "background.cancelled" => state.project_background_cancelled(&event, event_ref),
            "background.completed" => state.project_background_completed(&event, event_ref),
            "background.failed" => state.project_background_failed(&event, event_ref),
            "background.interrupted" => state.project_background_interrupted(&event, event_ref),
            "background.archived" => state.project_background_archived(&event, event_ref),
            "background.deleted" => state.project_background_deleted(&event, event_ref),
            _ => {}
        }
    }

    ConversationWorktreeProjection {
        turns: state.turns,
        event_cursor: state.event_cursor,
        event_refs: state.event_refs,
    }
}

pub async fn project_conversation_worktree_snapshot_with_evidence(
    conversation_id: &str,
    events: impl IntoIterator<Item = ConversationTimelineEvent>,
    tenant_id: TenantId,
    evidence_store: Arc<EvidenceRefStore>,
) -> Result<ConversationWorktreeProjection, JournalError> {
    let mut enriched_events = Vec::new();
    for mut event in events {
        enrich_event_with_evidence(conversation_id, tenant_id, &evidence_store, &mut event).await?;
        enriched_events.push(event);
    }
    Ok(project_conversation_worktree_snapshot(
        conversation_id,
        enriched_events,
    ))
}

#[must_use]
pub fn worktree_projection_page(
    projection: ConversationWorktreeProjection,
    gap: bool,
) -> ConversationWorktreePage {
    ConversationWorktreePage {
        turns: projection.turns,
        page_cursor: None,
        event_cursor: projection.event_cursor,
        has_more_before: false,
        has_more_after: false,
        gap,
    }
}

async fn enrich_event_with_evidence(
    conversation_id: &str,
    tenant_id: TenantId,
    evidence_store: &Arc<EvidenceRefStore>,
    event: &mut ConversationTimelineEvent,
) -> Result<(), JournalError> {
    match event.event_type.as_str() {
        "tool.completed" => {
            if process_step_kind_for_tool_name(
                &string_field(&event.payload, "toolName").unwrap_or_default(),
            ) == ProcessStepKind::Command
            {
                if let Some(bytes) = command_output_bytes(&event.payload) {
                    let ref_id = store_blob_payload_evidence(
                        evidence_store,
                        tenant_id,
                        conversation_id,
                        event,
                        EvidenceRefKind::CommandOutput,
                        "text/plain; charset=utf-8",
                        None,
                        None,
                        bytes,
                    )
                    .await?;
                    set_object_field(
                        &mut event.payload,
                        "fullOutputRef",
                        Value::String(String::from(ref_id)),
                    );
                    set_object_field(&mut event.payload, "truncated", Value::Bool(true));
                }
            }
            enrich_diff_evidence(conversation_id, tenant_id, evidence_store, event).await?;
        }
        "artifact.created" | "artifact.updated" => {
            enrich_artifact_evidence(conversation_id, tenant_id, evidence_store, event).await?;
        }
        _ => {}
    }
    Ok(())
}

async fn enrich_diff_evidence(
    conversation_id: &str,
    tenant_id: TenantId,
    evidence_store: &Arc<EvidenceRefStore>,
    event: &mut ConversationTimelineEvent,
) -> Result<(), JournalError> {
    let Some(files) = event
        .payload
        .get("diff")
        .and_then(|diff| diff.get("files"))
        .and_then(Value::as_array)
        .cloned()
    else {
        return Ok(());
    };

    let mut refs = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let Some(patch) = string_field(file, "patch") else {
            continue;
        };
        let ref_id = store_blob_payload_evidence(
            evidence_store,
            tenant_id,
            conversation_id,
            event,
            EvidenceRefKind::DiffPatch,
            "text/x-diff; charset=utf-8",
            None,
            None,
            patch.into_bytes(),
        )
        .await?;
        refs.push((index, ref_id));
    }

    if refs.is_empty() {
        return Ok(());
    }
    let Some(files) = event
        .payload
        .get_mut("diff")
        .and_then(|diff| diff.get_mut("files"))
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    for (index, ref_id) in refs {
        if let Some(Value::Object(file)) = files.get_mut(index) {
            file.insert(
                "fullPatchRef".to_owned(),
                Value::String(String::from(ref_id)),
            );
            file.remove("patch");
        }
    }
    Ok(())
}

async fn enrich_artifact_evidence(
    conversation_id: &str,
    tenant_id: TenantId,
    evidence_store: &Arc<EvidenceRefStore>,
    event: &mut ConversationTimelineEvent,
) -> Result<(), JournalError> {
    let Some(revision_id) = string_field(&event.payload, "revisionId") else {
        return Ok(());
    };
    let Some(artifact_id) = string_field(&event.payload, "artifactId") else {
        return Ok(());
    };
    let Some(blob_ref) = event
        .payload
        .get("blobRef")
        .cloned()
        .and_then(|value| serde_json::from_value::<BlobRef>(value).ok())
    else {
        return Ok(());
    };
    let content_type = blob_ref
        .content_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let ref_id = register_existing_blob_evidence(
        evidence_store,
        tenant_id,
        conversation_id,
        event,
        EvidenceRefKind::ArtifactContent,
        content_type,
        Some(artifact_id),
        Some(revision_id),
        blob_ref,
    )
    .await?;
    set_object_field(
        &mut event.payload,
        "contentRef",
        Value::String(String::from(ref_id)),
    );
    Ok(())
}

async fn store_blob_payload_evidence(
    evidence_store: &EvidenceRefStore,
    tenant_id: TenantId,
    conversation_id: &str,
    event: &ConversationTimelineEvent,
    kind: EvidenceRefKind,
    content_type: &str,
    artifact_id: Option<String>,
    revision_id: Option<String>,
    bytes: Vec<u8>,
) -> Result<EvidenceRefId, JournalError> {
    let hash = blake3::hash(&bytes);
    let content_hash = hash.as_bytes().to_vec();
    let record = EvidenceRefRecord {
        id: evidence_ref_id(kind, event, hash.as_bytes()),
        kind,
        conversation_id: conversation_id.to_owned(),
        run_id: event.run_id.clone(),
        source_event_refs: vec![event_ref(event)],
        artifact_id,
        revision_id,
        content_type: content_type.to_owned(),
        byte_length: bytes.len() as u64,
        content_hash,
        redaction_state: redaction_state_from_event(event),
        redaction_provenance: RedactionProvenance {
            redactor_version: "event-redacted-v1".to_owned(),
        },
        retention: BlobRetention::TenantScoped,
        source: EvidenceRefSource::JournalPayload {
            event_id: event.id.clone(),
            json_pointer: String::new(),
        },
    };
    evidence_store
        .store_blob_evidence(tenant_id, record, bytes)
        .await
}

async fn register_existing_blob_evidence(
    evidence_store: &EvidenceRefStore,
    tenant_id: TenantId,
    conversation_id: &str,
    event: &ConversationTimelineEvent,
    kind: EvidenceRefKind,
    content_type: String,
    artifact_id: Option<String>,
    revision_id: Option<String>,
    blob_ref: BlobRef,
) -> Result<EvidenceRefId, JournalError> {
    let record = EvidenceRefRecord {
        id: evidence_ref_id(kind, event, &blob_ref.content_hash),
        kind,
        conversation_id: conversation_id.to_owned(),
        run_id: event.run_id.clone(),
        source_event_refs: vec![event_ref(event)],
        artifact_id,
        revision_id,
        content_type,
        byte_length: blob_ref.size,
        content_hash: blob_ref.content_hash.to_vec(),
        redaction_state: redaction_state_from_event(event),
        redaction_provenance: RedactionProvenance {
            redactor_version: "event-redacted-v1".to_owned(),
        },
        retention: BlobRetention::TenantScoped,
        source: EvidenceRefSource::Blob { blob_ref },
    };
    evidence_store
        .store_journal_evidence(tenant_id, record)
        .await
}

fn evidence_ref_id(
    kind: EvidenceRefKind,
    event: &ConversationTimelineEvent,
    hash: &[u8; 32],
) -> EvidenceRefId {
    let kind = match kind {
        EvidenceRefKind::CommandOutput => "command-output",
        EvidenceRefKind::DiffPatch => "diff-patch",
        EvidenceRefKind::ArtifactContent => "artifact-content",
    };
    let mut hash_hex = String::with_capacity(64);
    for byte in hash {
        write!(&mut hash_hex, "{byte:02x}").expect("writing to String should not fail");
    }
    EvidenceRefId::new(format!("evidence:{kind}:{}:{hash_hex}", event.id))
}

fn command_output_bytes(payload: &Value) -> Option<Vec<u8>> {
    let stdout = string_field(payload, "stdout");
    let stderr = string_field(payload, "stderr");
    match (stdout, stderr) {
        (Some(stdout), Some(stderr)) if !stdout.is_empty() && !stderr.is_empty() => {
            Some(format!("{stdout}\n{stderr}").into_bytes())
        }
        (Some(stdout), _) if !stdout.is_empty() => Some(stdout.into_bytes()),
        (_, Some(stderr)) if !stderr.is_empty() => Some(stderr.into_bytes()),
        _ => None,
    }
}

fn redaction_state_from_event(event: &ConversationTimelineEvent) -> EvidenceRedactionState {
    match string_field(&event.payload, "redactionState").as_deref() {
        Some("redacted") => EvidenceRedactionState::Redacted,
        Some("withheld") => EvidenceRedactionState::Withheld,
        Some("clean") => EvidenceRedactionState::Clean,
        _ if event.visibility == "withheld" => EvidenceRedactionState::Withheld,
        _ => EvidenceRedactionState::Clean,
    }
}

fn set_object_field(payload: &mut Value, key: &str, value: Value) {
    if let Value::Object(map) = payload {
        map.insert(key.to_owned(), value);
        return;
    }
    let mut map = Map::new();
    map.insert(key.to_owned(), value);
    *payload = Value::Object(map);
}

struct ProjectionState<'a> {
    conversation_id: &'a str,
    turns: Vec<ConversationTurn>,
    run_turns: HashMap<String, usize>,
    request_tools: HashMap<String, String>,
    run_models: HashMap<String, AssistantWorkModelSnapshot>,
    subagent_requests: HashMap<String, String>,
    agent_tool_tasks: HashMap<String, (String, String)>,
    seen_event_ids: HashSet<String>,
    event_cursor: Option<ConversationCursor>,
    event_refs: Vec<ConversationEventRef>,
}

impl ProjectionState<'_> {
    fn project_run_started(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Ok(model) =
            serde_json::from_value::<AssistantWorkModelSnapshot>(event.payload["model"].clone())
        else {
            return;
        };
        self.run_models.insert(event.run_id.clone(), model.clone());
        let assistant = self.assistant_work(event, event_ref);
        assistant.model = Some(model);
    }

    fn project_user_message(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let position = event.cursor.conversation_sequence;
        let user = user_message_from_event(event, event_ref);
        let turn_id = format!("turn:{}", user.message_id);

        if let Some(index) = self.run_turns.get(&event.run_id).copied() {
            if self
                .turns
                .get(index)
                .is_some_and(|turn| is_synthetic_user_message_for_run(&turn.user, &event.run_id))
            {
                let turn = &mut self.turns[index];
                turn.id = turn_id;
                turn.position = position;
                turn.user = user;
                self.sort_turns_by_position_and_rebuild_run_turns();
                return;
            }
        }

        let index = self.turns.len();
        self.run_turns.insert(event.run_id.clone(), index);
        let turn = ConversationTurn {
            id: turn_id,
            conversation_id: self.conversation_id.to_owned(),
            position,
            user,
            assistant: None,
        };
        self.turns.push(turn);
    }

    fn project_assistant_completed(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let body = string_field(&event.payload, "body").unwrap_or_default();
        let message_id =
            string_field(&event.payload, "messageId").unwrap_or_else(|| event.id.clone());
        if assistant_completed_has_tool_uses(&event.payload) {
            self.promote_assistant_message_to_process(event, event_ref, message_id, body);
            return;
        }
        if is_redacted_only(&body) && self.run_has_ready_image_artifact(&event.run_id) {
            self.remove_text_segment(&event.run_id, &message_id);
            return;
        }
        self.upsert_text_segment(event, event_ref, message_id, body, true);
    }

    fn project_assistant_delta(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(text) = string_field(&event.payload, "text") else {
            return;
        };
        if text.trim().is_empty() {
            return;
        }
        let message_id =
            string_field(&event.payload, "messageId").unwrap_or_else(|| event.id.clone());
        self.upsert_text_segment(event, event_ref, message_id, text, false);
    }

    fn promote_assistant_message_to_process(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        message_id: String,
        completed_body: String,
    ) {
        let existing_body = self
            .remove_text_segment(&event.run_id, &message_id)
            .map(|segment| segment.body.into_string());
        let body = if completed_body.trim().is_empty() {
            existing_body.unwrap_or_default()
        } else {
            completed_body
        };
        if body.trim().is_empty() {
            return;
        }
        self.append_process_step(
            event,
            event_ref,
            ProcessStepKind::Reasoning,
            ProcessStepStatus::Complete,
            "工作过程".to_owned(),
            Some(ui_text(body)),
            None,
        );
    }

    fn upsert_text_segment(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        message_id: String,
        body: String,
        replace_body: bool,
    ) {
        let body_is_empty = body.trim().is_empty();
        if is_redacted_only(&body) && self.run_has_ready_image_artifact(&event.run_id) {
            self.remove_text_segment(&event.run_id, &message_id);
            return;
        }
        let assistant = self.assistant_work(event, event_ref.clone());
        if let Some(existing) = assistant
            .segments
            .iter_mut()
            .find_map(|segment| match segment {
                AssistantSegment::Text(text) if text.message_id == message_id => Some(text),
                _ => None,
            })
        {
            if !body_is_empty {
                let next_body = if replace_body {
                    body
                } else {
                    format!("{}{}", existing.body.as_str(), body)
                };
                existing.body = ui_text(next_body);
            }
            existing.event_refs.push(event_ref);
            return;
        }
        if body_is_empty {
            return;
        }
        let order = assistant.segments.len() as u32;
        assistant.segments.push(AssistantSegment::Text(TextSegment {
            id: format!("segment:text:{message_id}"),
            order,
            message_id,
            body: ui_text(body),
            event_refs: vec![event_ref],
        }));
    }

    fn project_thinking(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let visibility = if event.visibility == "withheld" {
            UiVisibility::Withheld
        } else {
            UiVisibility::UserSafe
        };
        let summary = safe_summary_field(&event.payload);
        let safe_summary_delta = safe_summary_delta_field(&event.payload);
        let status = match (
            visibility,
            string_field(&event.payload, "status").as_deref(),
        ) {
            (UiVisibility::Withheld, _) | (_, Some("withheld")) => ProcessSegmentStatus::Withheld,
            (_, Some("complete" | "completed")) => ProcessSegmentStatus::Complete,
            _ => ProcessSegmentStatus::Running,
        };
        let process_summary = if let Some(ref text) = summary {
            text.clone()
        } else {
            ui_text(match status {
                ProcessSegmentStatus::Running => "正在处理请求",
                ProcessSegmentStatus::Complete => "已完成工作过程",
                ProcessSegmentStatus::Withheld => "过程内容已折叠",
                _ => "正在处理请求",
            })
        };
        self.ensure_process_segment(event, event_ref.clone(), status, process_summary);
        if let Some(delta) = safe_summary_delta {
            self.append_reasoning_summary_delta(event, event_ref, delta);
        }
    }

    fn ensure_process_segment(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        status: ProcessSegmentStatus,
        summary: UiSafeText,
    ) -> &mut ProcessSegment {
        self.assistant_work(event, event_ref.clone());
        let order = self.next_segment_order(&event.run_id);
        let assistant = self.assistant_work(event, event_ref.clone());
        if let Some(index) = assistant
            .segments
            .iter()
            .position(|segment| matches!(segment, AssistantSegment::Process(_)))
        {
            let AssistantSegment::Process(process) = &mut assistant.segments[index] else {
                unreachable!("process segment index changed");
            };
            process.status = status;
            process.summary = summary;
            process.event_refs.push(event_ref);
            return process;
        }
        assistant.segments.insert(
            0,
            AssistantSegment::Process(ProcessSegment {
                id: format!("segment:process:{}", event.run_id),
                order,
                status,
                summary,
                steps: Vec::new(),
                event_refs: vec![event_ref],
            }),
        );
        renumber_segments(assistant);
        let AssistantSegment::Process(process) = &mut assistant.segments[0] else {
            unreachable!("inserted process segment missing");
        };
        process
    }

    fn append_tool_process_step(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        status: ProcessStepStatus,
        title: String,
        tool_name: String,
    ) {
        let kind = process_step_kind_for_tool_name(&tool_name);
        let detail = process_step_detail_for_tool(event, &tool_name, kind, &title);
        let step_id = tool_process_step_id(event, kind);
        self.upsert_process_step(event, event_ref, step_id, kind, status, title, None, detail);
    }

    fn project_tool_completed_process_step(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        tool_name: String,
    ) {
        let kind = process_step_kind_for_tool_name(&tool_name);
        let title = tool_step_title(&tool_name, ToolProcessPhase::Completed);
        match kind {
            ProcessStepKind::FileRead | ProcessStepKind::FileSearch => {
                self.remove_tool_process_step(event, kind);
                self.upsert_aggregate_process_step(event, event_ref, kind, title);
            }
            _ => {
                let detail = process_step_detail_for_tool(event, &tool_name, kind, &title);
                let step_id = tool_process_step_id(event, kind);
                self.upsert_process_step(
                    event,
                    event_ref.clone(),
                    step_id,
                    kind,
                    ProcessStepStatus::Complete,
                    title,
                    None,
                    detail,
                );
                if let Some(diff_detail) = diff_process_detail_from_payload(&event.payload) {
                    self.upsert_process_step(
                        event,
                        event_ref,
                        format!("process-step:{}:diff:{}", event.run_id, event.id),
                        ProcessStepKind::Diff,
                        ProcessStepStatus::Complete,
                        "修改差异".to_owned(),
                        None,
                        Some(diff_detail),
                    );
                }
            }
        }
    }

    fn append_reasoning_summary_delta(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        delta: UiSafeText,
    ) {
        let process = self.ensure_process_segment(
            event,
            event_ref.clone(),
            ProcessSegmentStatus::Running,
            ui_text("正在处理请求"),
        );
        if let Some(step) = process
            .steps
            .iter_mut()
            .find(|step| matches!(step.kind, ProcessStepKind::Reasoning))
        {
            let merged = match step.body.as_ref() {
                Some(body) => format!("{}{}", body.as_str(), delta.as_str()),
                None => delta.as_str().to_owned(),
            };
            step.body = Some(ui_text(merged));
            step.event_refs.push(event_ref);
            return;
        }

        process.steps.push(ProcessStep {
            id: format!("process-step:{}:reasoning", event.run_id),
            order: process.steps.len() as u32,
            kind: ProcessStepKind::Reasoning,
            status: ProcessStepStatus::Running,
            title: ui_text("推理过程"),
            body: Some(delta),
            detail: None,
            visibility: UiVisibility::UserSafe,
            event_refs: vec![event_ref],
        });
    }

    fn append_process_step(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        kind: ProcessStepKind,
        status: ProcessStepStatus,
        title: String,
        body: Option<UiSafeText>,
        detail: Option<ProcessStepDetail>,
    ) {
        let process = self.ensure_process_segment(
            event,
            event_ref.clone(),
            ProcessSegmentStatus::Running,
            ui_text("正在处理请求"),
        );
        let order = process.steps.len() as u32;
        process.steps.push(ProcessStep {
            id: format!("process-step:{}:{}:{}", event.run_id, event.id, order),
            order,
            kind,
            status,
            title: ui_text(title),
            body,
            detail,
            visibility: UiVisibility::UserSafe,
            event_refs: vec![event_ref],
        });
    }

    fn upsert_process_step(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        step_id: String,
        kind: ProcessStepKind,
        status: ProcessStepStatus,
        title: String,
        body: Option<UiSafeText>,
        detail: Option<ProcessStepDetail>,
    ) {
        let process = self.ensure_process_segment(
            event,
            event_ref.clone(),
            ProcessSegmentStatus::Running,
            ui_text("正在处理请求"),
        );
        if let Some(step) = process.steps.iter_mut().find(|step| step.id == step_id) {
            step.kind = kind;
            step.status = status;
            step.title = ui_text(title);
            step.body = body;
            step.detail = merge_process_step_detail(step.detail.as_ref(), detail);
            step.event_refs.push(event_ref);
            return;
        }
        let order = process.steps.len() as u32;
        process.steps.push(ProcessStep {
            id: step_id,
            order,
            kind,
            status,
            title: ui_text(title),
            body,
            detail,
            visibility: UiVisibility::UserSafe,
            event_refs: vec![event_ref],
        });
    }

    fn upsert_aggregate_process_step(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        kind: ProcessStepKind,
        title: String,
    ) {
        let step_id = aggregate_process_step_id(&event.run_id, kind);
        let next_count = u32_field(&event.payload, "itemCount").unwrap_or(1);
        let process = self.ensure_process_segment(
            event,
            event_ref.clone(),
            ProcessSegmentStatus::Running,
            ui_text("正在处理请求"),
        );
        if let Some(step) = process.steps.iter_mut().find(|step| step.id == step_id) {
            step.status = ProcessStepStatus::Complete;
            step.title = ui_text(title.clone());
            step.detail = Some(merged_activity_detail(
                &title,
                step.detail.as_ref(),
                next_count,
            ));
            step.event_refs.push(event_ref);
            return;
        }
        let order = process.steps.len() as u32;
        process.steps.push(ProcessStep {
            id: step_id,
            order,
            kind,
            status: ProcessStepStatus::Complete,
            title: ui_text(title.clone()),
            body: None,
            detail: Some(ProcessStepDetail::Activity {
                summary: ui_text(title),
                item_count: Some(next_count),
            }),
            visibility: UiVisibility::UserSafe,
            event_refs: vec![event_ref],
        });
    }

    fn remove_tool_process_step(
        &mut self,
        event: &ConversationTimelineEvent,
        kind: ProcessStepKind,
    ) {
        let Some(index) = self.run_turns.get(&event.run_id).copied() else {
            return;
        };
        let Some(assistant) = self.turns[index].assistant.as_mut() else {
            return;
        };
        for segment in &mut assistant.segments {
            let AssistantSegment::Process(process) = segment else {
                continue;
            };
            let step_id = tool_process_step_id(event, kind);
            process.steps.retain(|step| step.id != step_id);
            renumber_process_steps(process);
            return;
        }
    }

    fn project_tool_requested(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        self.assistant_work(event, event_ref.clone());
        let Some(tool_use_id) = string_field(&event.payload, "toolUseId") else {
            return;
        };
        let tool_name =
            ui_text(string_field(&event.payload, "toolName").unwrap_or_else(|| "Tool".to_owned()))
                .into_string();
        let is_agent_tool = tool_name.eq_ignore_ascii_case("agent");
        let group = self.tool_group(&event.run_id, &tool_use_id, event_ref.clone());
        if group
            .attempts
            .iter()
            .any(|attempt| attempt.tool_use_id == tool_use_id)
        {
            return;
        }
        let order = group.attempts.len() as u32;
        group.attempts.push(ToolAttempt {
            id: format!("tool:{tool_use_id}"),
            order,
            tool_use_id: tool_use_id.clone(),
            tool_name: tool_name.clone(),
            origin: ToolAttemptOrigin::Unknown,
            status: ToolAttemptStatus::Running,
            arguments_preview: None,
            output_summary: None,
            affected_targets: vec![],
            started_at: None,
            ended_at: None,
            duration_ms: None,
            retry_of: None,
            failure_phase: None,
            failure_summary: None,
            permission: None,
            event_refs: vec![event_ref.clone()],
        });
        self.append_tool_process_step(
            event,
            event_ref,
            ProcessStepStatus::Running,
            tool_step_title(&tool_name, ToolProcessPhase::Requested),
            tool_name,
        );
        if is_agent_tool {
            if let (Some(role), Some(task)) = (
                string_field(&event.payload, "role"),
                string_field(&event.payload, "taskSummary"),
            ) {
                self.agent_tool_tasks.insert(tool_use_id, (role, task));
            }
        }
    }

    fn update_tool_status(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        status: ToolAttemptStatus,
    ) -> Option<String> {
        let Some(tool_use_id) = string_field(&event.payload, "toolUseId") else {
            return None;
        };
        if let Some(attempt) = self.tool_attempt_mut(&event.run_id, &tool_use_id) {
            attempt.status = status;
            attempt.event_refs.push(event_ref);
            return Some(attempt.tool_name.as_str().to_owned());
        }
        None
    }

    fn project_tool_failed(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(tool_use_id) = string_field(&event.payload, "toolUseId") else {
            return;
        };
        if let Some(attempt) = self.tool_attempt_mut(&event.run_id, &tool_use_id) {
            attempt.status = ToolAttemptStatus::Failed;
            attempt.failure_summary = Some(safe_tool_failure_summary(event));
            attempt.event_refs.push(event_ref.clone());
            let tool_name = attempt.tool_name.as_str().to_owned();
            self.append_tool_process_step(
                event,
                event_ref,
                ProcessStepStatus::Failed,
                tool_step_title(&tool_name, ToolProcessPhase::Failed),
                tool_name,
            );
        }
    }

    fn project_permission_requested(
        &mut self,
        event: &ConversationTimelineEvent,
        _event_ref: ConversationEventRef,
    ) {
        let Some(request_id) = string_field(&event.payload, "requestId") else {
            return;
        };
        let tool_use_id = string_field(&event.payload, "toolUseId")
            .or_else(|| self.unique_tool_attempt_id_for_run(&event.run_id));
        let Some(tool_use_id) = tool_use_id else {
            return;
        };
        self.request_tools
            .insert(request_id.clone(), tool_use_id.clone());
        let auto_resolved = bool_field(&event.payload, "autoResolved").unwrap_or(false);
        let permission = permission_request_state_from_payload(
            request_id.clone(),
            tool_use_id.clone(),
            &event.payload,
            auto_resolved,
        );
        let permission_risk_level = permission.risk_level;
        let permission_sandbox = permission.policy.sandbox.clone();
        if let Some(attempt) = self.tool_attempt_mut(&event.run_id, &tool_use_id) {
            attempt.status = if auto_resolved {
                ToolAttemptStatus::Running
            } else {
                ToolAttemptStatus::WaitingPermission
            };
            attempt.permission = Some(permission);
        }
        self.apply_permission_metadata_to_command_step(
            &event.run_id,
            &tool_use_id,
            &request_id,
            permission_risk_level,
            permission_sandbox,
        );
    }

    fn project_permission_resolved(
        &mut self,
        event: &ConversationTimelineEvent,
        _event_ref: ConversationEventRef,
    ) {
        let Some(request_id) = string_field(&event.payload, "requestId") else {
            return;
        };
        let Some(tool_use_id) = self.request_tools.get(&request_id).cloned() else {
            return;
        };
        let status = match string_field(&event.payload, "decision").as_deref() {
            Some("approve" | "approved" | "allow") => DecisionRequestStatus::Approved,
            Some("deny" | "denied") => DecisionRequestStatus::Denied,
            Some("failed") => DecisionRequestStatus::Failed,
            _ => DecisionRequestStatus::Denied,
        };
        if let Some(attempt) = self.tool_attempt_mut(&event.run_id, &tool_use_id) {
            if matches!(attempt.status, ToolAttemptStatus::WaitingPermission) {
                attempt.status = ToolAttemptStatus::Running;
            }
            if let Some(permission) = attempt.permission.as_mut() {
                permission.status = status;
            }
        }
    }

    fn project_artifact_lifecycle(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(artifact_id) = string_field(&event.payload, "artifactId") else {
            return;
        };
        let Some(revision_id) = string_field(&event.payload, "revisionId") else {
            return;
        };
        let content_ref = string_field(&event.payload, "contentRef").map(EvidenceRefId::new);
        let existing_snapshot = self.artifact_segment_snapshot(&event.run_id, &artifact_id);
        let existing_process_snapshot =
            self.artifact_process_step_snapshot(&event.run_id, &artifact_id);
        let title = string_field(&event.payload, "title")
            .or_else(|| {
                existing_snapshot
                    .as_ref()
                    .map(|artifact| artifact.title.as_str().to_owned())
            })
            .or_else(|| {
                existing_process_snapshot
                    .as_ref()
                    .map(|(title, _)| title.clone())
            });
        let summary = artifact_summary(&event.payload).or_else(|| {
            existing_snapshot
                .as_ref()
                .and_then(|artifact| artifact.summary.clone())
        });
        let kind = maybe_artifact_kind(&event.payload)
            .or_else(|| {
                existing_snapshot
                    .as_ref()
                    .map(|artifact| artifact.kind.clone())
            })
            .or_else(|| {
                existing_process_snapshot
                    .as_ref()
                    .map(|(_, media)| artifact_media_kind_label(media.kind).to_owned())
            })
            .unwrap_or_else(|| "file".to_owned());
        let status = maybe_artifact_status(&event.payload)
            .or_else(|| existing_snapshot.as_ref().map(|artifact| artifact.status))
            .or_else(|| {
                existing_process_snapshot
                    .as_ref()
                    .map(|_| ArtifactStatus::Ready)
            })
            .unwrap_or(ArtifactStatus::Ready);
        let source = maybe_artifact_source(&event.payload)
            .or_else(|| existing_snapshot.as_ref().map(|artifact| artifact.source))
            .unwrap_or(ArtifactSource::Assistant);
        let media = artifact_media_preview(&event.payload, &kind)
            .or_else(|| {
                existing_snapshot
                    .as_ref()
                    .and_then(|artifact| artifact.revision.media.clone())
            })
            .or_else(|| {
                existing_process_snapshot
                    .as_ref()
                    .map(|(_, media)| media.clone())
            });
        if is_ready_image_artifact(status, media.as_ref()) {
            self.remove_artifact_segment(&event.run_id, &artifact_id);
            self.remove_redacted_text_segments(&event.run_id);
            self.append_artifact_process_step(event, artifact_id, revision_id, title, media);
            return;
        }
        let assistant = self.assistant_work(event, event_ref.clone());
        if let Some(existing) = assistant
            .segments
            .iter_mut()
            .find_map(|segment| match segment {
                AssistantSegment::Artifact(artifact) if artifact.artifact_id == artifact_id => {
                    Some(artifact)
                }
                _ => None,
            })
        {
            if let Some(title) = title {
                existing.title = ui_text(title);
            }
            if let Some(summary) = summary {
                existing.summary = Some(summary);
            }
            existing.kind = kind.clone();
            existing.status = status;
            existing.source = source;
            existing.revision.revision_id = revision_id;
            existing.revision.content_ref = content_ref;
            existing.revision.media = media.clone();
            existing.event_refs.push(event_ref);
            return;
        }
        let order = assistant.segments.len() as u32;
        assistant
            .segments
            .push(AssistantSegment::Artifact(ArtifactSegment {
                id: format!("segment:artifact:{artifact_id}"),
                order,
                artifact_id: artifact_id.clone(),
                kind: kind.clone(),
                status,
                source,
                title: ui_text(title.clone().unwrap_or_else(|| "Artifact".to_owned())),
                summary: summary.clone(),
                revision: ArtifactRevisionSummary {
                    artifact_id: artifact_id.clone(),
                    revision_id,
                    kind: artifact_revision_kind_from_str(&kind),
                    status: match status {
                        ArtifactStatus::Pending => ArtifactRevisionStatus::Pending,
                        ArtifactStatus::Running => ArtifactRevisionStatus::Running,
                        ArtifactStatus::Failed => ArtifactRevisionStatus::Failed,
                        ArtifactStatus::Ready => ArtifactRevisionStatus::Ready,
                        _ => ArtifactRevisionStatus::Pending,
                    },
                    source_run_id: event.run_id.clone(),
                    title: title.clone().unwrap_or_else(|| "Artifact".to_owned()),
                    summary: summary.as_ref().map(|s| s.as_str().to_owned()),
                    preview_ref: None,
                    content_ref,
                    media: media.clone(),
                },
                event_refs: vec![event_ref],
            }));
    }

    fn append_artifact_process_step(
        &mut self,
        event: &ConversationTimelineEvent,
        artifact_id: String,
        revision_id: String,
        title: Option<String>,
        media: Option<ArtifactMediaPreview>,
    ) {
        let Some(media) = media else {
            return;
        };
        self.upsert_process_step(
            event,
            event_ref(event),
            format!("process-step:{}:artifact:{artifact_id}", event.run_id),
            ProcessStepKind::Artifact,
            ProcessStepStatus::Complete,
            title.unwrap_or_else(|| "生成的 Artifact".to_owned()),
            None,
            Some(ProcessStepDetail::Artifact {
                artifact_id,
                revision_id: Some(revision_id),
                media,
            }),
        );
    }

    fn project_review_requested(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(request_id) = string_field(&event.payload, "requestId") else {
            return;
        };
        let title = string_field(&event.payload, "title").unwrap_or_else(|| "Review".to_owned());
        let body = string_field(&event.payload, "body").map(ui_text);
        let assistant = self.assistant_work(event, event_ref.clone());
        if let Some(existing) = assistant
            .segments
            .iter_mut()
            .find_map(|segment| match segment {
                AssistantSegment::ReviewRequest(review) if review.request_id == request_id => {
                    Some(review)
                }
                _ => None,
            })
        {
            existing.title = ui_text(title);
            existing.body = body;
            existing.event_refs.push(event_ref);
            return;
        }
        let order = assistant.segments.len() as u32;
        assistant
            .segments
            .push(AssistantSegment::ReviewRequest(ReviewRequestSegment {
                id: format!("segment:review:{request_id}"),
                order,
                request_id,
                title: ui_text(title),
                body,
                event_refs: vec![event_ref],
            }));
    }

    fn project_clarification_requested(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(request_id) = string_field(&event.payload, "requestId") else {
            return;
        };
        let prompt =
            string_field(&event.payload, "prompt").unwrap_or_else(|| "Clarification".to_owned());
        let assistant = self.assistant_work(event, event_ref.clone());
        if let Some(existing) = assistant
            .segments
            .iter_mut()
            .find_map(|segment| match segment {
                AssistantSegment::ClarificationRequest(clarification)
                    if clarification.request_id == request_id =>
                {
                    Some(clarification)
                }
                _ => None,
            })
        {
            existing.prompt = ui_text(prompt);
            existing.event_refs.push(event_ref);
            return;
        }
        let order = assistant.segments.len() as u32;
        assistant
            .segments
            .push(AssistantSegment::ClarificationRequest(
                ClarificationRequestSegment {
                    id: format!("segment:clarification:{request_id}"),
                    order,
                    request_id,
                    prompt: ui_text(prompt),
                    event_refs: vec![event_ref],
                },
            ));
    }

    fn project_notice(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let notice_id =
            string_field(&event.payload, "noticeId").unwrap_or_else(|| event.id.clone());
        let Some(body) = string_field(&event.payload, "body") else {
            return;
        };
        let code = notice_code_field(&event.payload, "code");
        let assistant = self.assistant_work(event, event_ref.clone());
        if let Some(existing) = assistant
            .segments
            .iter_mut()
            .find_map(|segment| match segment {
                AssistantSegment::Notice(notice)
                    if notice.id == format!("segment:notice:{notice_id}") =>
                {
                    Some(notice)
                }
                _ => None,
            })
        {
            existing.body = ui_text(body);
            existing.code = code;
            existing.event_refs.push(event_ref);
            return;
        }
        let order = assistant.segments.len() as u32;
        assistant
            .segments
            .push(AssistantSegment::Notice(NoticeSegment {
                id: format!("segment:notice:{notice_id}"),
                order,
                body: ui_text(body),
                code,
                event_refs: vec![event_ref],
            }));
    }

    fn project_run_ended(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let assistant = self.assistant_work(event, event_ref);
        assistant.status = match string_field(&event.payload, "reason").as_deref() {
            Some("cancelled") => AssistantWorkStatus::Cancelled,
            Some("error" | "failed") => AssistantWorkStatus::Failed,
            _ => AssistantWorkStatus::Complete,
        };
        if matches!(assistant.status, AssistantWorkStatus::Complete) {
            for segment in &mut assistant.segments {
                let AssistantSegment::Process(process) = segment else {
                    continue;
                };
                if matches!(process.status, ProcessSegmentStatus::Running) {
                    let has_failed_step = process
                        .steps
                        .iter()
                        .any(|step| matches!(step.status, ProcessStepStatus::Failed));
                    process.status = if has_failed_step {
                        ProcessSegmentStatus::Failed
                    } else {
                        ProcessSegmentStatus::Complete
                    };
                    process.summary = ui_text(if has_failed_step {
                        "已结束但存在失败步骤"
                    } else {
                        "已完成工作过程"
                    });
                    for step in &mut process.steps {
                        if matches!(step.status, ProcessStepStatus::Running) {
                            step.status = ProcessStepStatus::Complete;
                        }
                    }
                }
            }
        } else {
            for segment in &mut assistant.segments {
                let AssistantSegment::Process(process) = segment else {
                    continue;
                };
                process.status = match assistant.status {
                    AssistantWorkStatus::Failed => ProcessSegmentStatus::Failed,
                    AssistantWorkStatus::Cancelled => ProcessSegmentStatus::Cancelled,
                    AssistantWorkStatus::Running | AssistantWorkStatus::Complete => process.status,
                };
            }
        }
    }

    fn project_engine_failed(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let order = self.next_segment_order(&event.run_id);
        let assistant = self.assistant_work(event, event_ref.clone());
        assistant.status = AssistantWorkStatus::Failed;
        assistant
            .segments
            .push(AssistantSegment::Error(ErrorSegment {
                id: format!("segment:error:{}", event.id),
                order,
                body: ui_text("执行失败。可在详情中查看。"),
                event_refs: vec![event_ref],
            }));
    }

    fn project_subagent_spawned(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(subagent_id) = string_field(&event.payload, "subagentId") else {
            return;
        };
        let role = string_field(&event.payload, "role").unwrap_or_else(|| "Subagent".to_owned());
        let task_summary = string_field(&event.payload, "taskSummary").or_else(|| {
            string_field(&event.payload, "triggerToolUseId").and_then(|tool_use_id| {
                self.agent_tool_tasks
                    .get(&tool_use_id)
                    .map(|(_, task)| task.clone())
            })
        });
        let task_summary = task_summary.map(ui_text).unwrap_or_else(|| {
            ui_text("Subagent task details withheld from conversation timeline.")
        });
        let order = self.next_segment_order(&event.run_id);
        let assistant = self.assistant_work(event, event_ref.clone());
        assistant
            .segments
            .push(AssistantSegment::AgentActivity(AgentActivitySegment {
                id: format!("segment:agent:{subagent_id}"),
                order,
                activity_kind: AgentActivityKind::Subagent,
                agent_id: subagent_id,
                role: ui_text(role),
                task_summary,
                status: AgentActivityStatus::Running,
                result_summary: None,
                permission: None,
                team: None,
                event_refs: vec![event_ref],
            }));
    }

    fn project_subagent_announced(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(subagent_id) = string_field(&event.payload, "subagentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &subagent_id) else {
            return;
        };
        segment.event_refs.push(event_ref.clone());
        if bool_field(&event.payload, "redacted").unwrap_or(false) {
            segment.status = AgentActivityStatus::Redacted;
            segment.result_summary = Some(ui_text(
                "Subagent result withheld from conversation timeline.",
            ));
            return;
        }
        if let Some(status) = string_field(&event.payload, "status") {
            segment.status = subagent_announced_status(&status);
        }
        if let Some(summary) = string_field(&event.payload, "resultSummary") {
            let safe_summary = ui_text(summary);
            if safe_summary.as_str().contains("[REDACTED]") {
                segment.status = AgentActivityStatus::Redacted;
                segment.result_summary = Some(ui_text(
                    "Subagent result withheld from conversation timeline.",
                ));
            } else {
                segment.result_summary = Some(safe_summary);
            }
        }
    }

    fn project_subagent_terminated(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(subagent_id) = string_field(&event.payload, "subagentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &subagent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        if matches!(
            segment.status,
            AgentActivityStatus::Completed
                | AgentActivityStatus::Failed
                | AgentActivityStatus::Cancelled
                | AgentActivityStatus::Redacted
        ) {
            return;
        }
        if let Some(reason) = string_field(&event.payload, "reason") {
            segment.status = subagent_termination_status(&reason);
        }
    }

    fn project_subagent_stalled(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(subagent_id) = string_field(&event.payload, "subagentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &subagent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        if !matches!(
            segment.status,
            AgentActivityStatus::Completed
                | AgentActivityStatus::Failed
                | AgentActivityStatus::Cancelled
                | AgentActivityStatus::Redacted
        ) {
            segment.status = AgentActivityStatus::Stalled;
        }
    }

    fn project_subagent_permission_forwarded(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(subagent_id) = string_field(&event.payload, "subagentId") else {
            return;
        };
        let Some(request_id) = string_field(&event.payload, "requestId") else {
            return;
        };
        self.subagent_requests
            .insert(request_id.clone(), subagent_id.clone());
        let Some(segment) = self.agent_activity_mut(&event.run_id, &subagent_id) else {
            return;
        };
        segment.status = AgentActivityStatus::WaitingPermission;
        segment.permission = Some(AgentActivityPermissionState {
            id: format!("permission:{request_id}"),
            request_id,
            status: DecisionRequestStatus::Pending,
            summary: string_field(&event.payload, "reason").map(ui_text),
            event_refs: vec![event_ref.clone()],
        });
        segment.event_refs.push(event_ref);
    }

    fn project_subagent_permission_resolved(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(request_id) = string_field(&event.payload, "requestId") else {
            return;
        };
        let Some(subagent_id) = self.subagent_requests.get(&request_id).cloned() else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &subagent_id) else {
            return;
        };
        let status = match string_field(&event.payload, "decision").as_deref() {
            Some("approve" | "approved" | "allow") => DecisionRequestStatus::Approved,
            Some("deny" | "denied") => DecisionRequestStatus::Denied,
            Some("failed") => DecisionRequestStatus::Failed,
            _ => DecisionRequestStatus::Denied,
        };
        if let Some(permission) = segment.permission.as_mut() {
            permission.status = status;
            permission.summary = None;
            permission.event_refs.push(event_ref.clone());
        }
        segment.event_refs.push(event_ref);
        if matches!(segment.status, AgentActivityStatus::WaitingPermission)
            && matches!(status, DecisionRequestStatus::Approved)
        {
            segment.status = AgentActivityStatus::Running;
        }
    }

    fn agent_activity_mut(
        &mut self,
        run_id: &str,
        agent_id: &str,
    ) -> Option<&mut AgentActivitySegment> {
        let index = self.run_turns.get(run_id).copied()?;
        let assistant = self.turns[index].assistant.as_mut()?;
        assistant
            .segments
            .iter_mut()
            .find_map(|segment| match segment {
                AssistantSegment::AgentActivity(activity) if activity.agent_id == agent_id => {
                    Some(activity)
                }
                _ => None,
            })
    }

    fn project_team_created(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(team_id) = string_field(&event.payload, "teamId") else {
            return;
        };
        let name = string_field(&event.payload, "name").unwrap_or_else(|| "Agent team".to_owned());
        let topology = string_field(&event.payload, "topologyKind")
            .unwrap_or_else(|| "coordinator_worker".to_owned());
        let order = self.next_segment_order(&event.run_id);
        let assistant = self.assistant_work(event, event_ref.clone());
        assistant
            .segments
            .push(AssistantSegment::AgentActivity(AgentActivitySegment {
                id: format!("segment:agent-team:{team_id}"),
                order,
                activity_kind: AgentActivityKind::AgentTeam,
                agent_id: team_id,
                role: ui_text(name),
                task_summary: ui_text("Coordinating agent team."),
                status: AgentActivityStatus::Running,
                result_summary: None,
                permission: None,
                team: Some(AgentTeamActivityDetails {
                    topology: ui_text(topology),
                    lead: None,
                    members: Vec::new(),
                    current_tasks: Vec::new(),
                    mailbox_count: 0,
                    mailbox_summaries: Vec::new(),
                }),
                event_refs: vec![event_ref],
            }));
    }

    fn project_team_member_joined(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(team_id) = string_field(&event.payload, "teamId") else {
            return;
        };
        let Some(agent_id) = string_field(&event.payload, "agentId") else {
            return;
        };
        let role = string_field(&event.payload, "role").unwrap_or_else(|| "Member".to_owned());
        let member = AgentTeamMemberActivity {
            agent_id,
            role: ui_text(role),
            status: AgentActivityStatus::Running,
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &team_id) else {
            return;
        };
        if let Some(team) = segment.team.as_mut() {
            if !team
                .members
                .iter()
                .any(|existing| existing.agent_id == member.agent_id)
            {
                team.members.push(member.clone());
            }
            if team.lead.is_none() {
                team.lead = Some(member);
            }
        }
        segment.event_refs.push(event_ref);
    }

    fn project_team_member_left(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(team_id) = string_field(&event.payload, "teamId") else {
            return;
        };
        let Some(agent_id) = string_field(&event.payload, "agentId") else {
            return;
        };
        let status = match string_field(&event.payload, "reason").as_deref() {
            Some("goal_achieved" | "goalAchieved") => AgentActivityStatus::Completed,
            Some("interrupted" | "removed") => AgentActivityStatus::Cancelled,
            Some("stalled_removed" | "stalledRemoved") => AgentActivityStatus::Stalled,
            Some("quota_exceeded" | "quotaExceeded") | Some("error") => AgentActivityStatus::Failed,
            _ => AgentActivityStatus::Cancelled,
        };
        self.update_team_member_status(&event.run_id, &team_id, &agent_id, status, event_ref);
    }

    fn project_team_member_stalled(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(team_id) = string_field(&event.payload, "teamId") else {
            return;
        };
        let Some(agent_id) = string_field(&event.payload, "agentId") else {
            return;
        };
        self.update_team_member_status(
            &event.run_id,
            &team_id,
            &agent_id,
            AgentActivityStatus::Stalled,
            event_ref,
        );
    }

    fn project_team_task_updated(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(team_id) = string_field(&event.payload, "teamId") else {
            return;
        };
        let Some(task_id) = string_field(&event.payload, "taskId") else {
            return;
        };
        let title = string_field(&event.payload, "title").unwrap_or_else(|| "Team task".to_owned());
        let status = string_field(&event.payload, "status").unwrap_or_else(|| "running".to_owned());
        let task = AgentTeamTaskActivity {
            id: task_id,
            title: ui_text(title),
            status: ui_text(status),
            assignee_profile_id: string_field(&event.payload, "assigneeProfileId"),
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &team_id) else {
            return;
        };
        if let Some(team) = segment.team.as_mut() {
            if let Some(existing) = team
                .current_tasks
                .iter_mut()
                .find(|existing| existing.id == task.id)
            {
                *existing = task;
            } else {
                team.current_tasks.push(task);
            }
        }
        segment.event_refs.push(event_ref);
    }

    fn project_team_message_sent(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(team_id) = string_field(&event.payload, "teamId") else {
            return;
        };
        let message_id =
            string_field(&event.payload, "messageId").unwrap_or_else(|| "message".to_owned());
        let Some(segment) = self.agent_activity_mut(&event.run_id, &team_id) else {
            return;
        };
        if let Some(team) = segment.team.as_mut() {
            team.mailbox_count = team.mailbox_count.saturating_add(1);
            team.mailbox_summaries
                .push(ui_text(format!("Queued message {message_id}.")));
        }
        segment.event_refs.push(event_ref);
    }

    fn project_team_message_routed(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(team_id) = string_field(&event.payload, "teamId") else {
            return;
        };
        let message_id =
            string_field(&event.payload, "messageId").unwrap_or_else(|| "message".to_owned());
        let recipient_count = event
            .payload
            .get("resolvedRecipients")
            .and_then(|value| value.as_array())
            .map_or(0, Vec::len);
        let Some(segment) = self.agent_activity_mut(&event.run_id, &team_id) else {
            return;
        };
        if let Some(team) = segment.team.as_mut() {
            team.mailbox_count = team.mailbox_count.saturating_add(1);
            let noun = if recipient_count == 1 {
                "member"
            } else {
                "members"
            };
            team.mailbox_summaries.push(ui_text(format!(
                "Routed message {message_id} to {recipient_count} {noun}."
            )));
        }
        segment.event_refs.push(event_ref);
    }

    fn project_team_turn_completed(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(team_id) = string_field(&event.payload, "teamId") else {
            return;
        };
        let participants: Vec<String> = event
            .payload
            .get("participatingAgents")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let Some(segment) = self.agent_activity_mut(&event.run_id, &team_id) else {
            return;
        };
        if let Some(team) = segment.team.as_mut() {
            for member in &mut team.members {
                if participants
                    .iter()
                    .any(|agent_id| agent_id == &member.agent_id)
                {
                    member.status = AgentActivityStatus::Completed;
                }
            }
            if let Some(lead) = team.lead.as_mut() {
                if participants
                    .iter()
                    .any(|agent_id| agent_id == &lead.agent_id)
                {
                    lead.status = AgentActivityStatus::Completed;
                }
            }
        }
        segment.event_refs.push(event_ref);
    }

    fn project_team_terminated(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(team_id) = string_field(&event.payload, "teamId") else {
            return;
        };
        let reason = string_field(&event.payload, "reason").unwrap_or_else(|| "error".to_owned());
        let Some(segment) = self.agent_activity_mut(&event.run_id, &team_id) else {
            return;
        };
        segment.status = team_termination_status(&reason);
        segment.result_summary = Some(ui_text(team_result_summary(&reason)));
        segment.event_refs.push(event_ref);
    }

    fn project_background_started(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let task_summary = string_field(&event.payload, "title")
            .map(ui_text)
            .unwrap_or_else(|| ui_text("Background agent started."));
        let order = self.next_segment_order(&event.run_id);
        let assistant = self.assistant_work(event, event_ref.clone());
        assistant
            .segments
            .push(AssistantSegment::AgentActivity(AgentActivitySegment {
                id: format!("segment:background-agent:{background_agent_id}"),
                order,
                activity_kind: AgentActivityKind::BackgroundAgent,
                agent_id: background_agent_id,
                role: ui_text("Background agent"),
                task_summary,
                status: AgentActivityStatus::Running,
                result_summary: None,
                permission: None,
                team: None,
                event_refs: vec![event_ref],
            }));
    }

    fn project_background_state_changed(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        if is_terminal_agent_activity_status(segment.status) {
            return;
        }
        if let Some(to) = string_field(&event.payload, "to") {
            segment.status = background_agent_state_status(&to);
        }
        if matches!(
            segment.status,
            AgentActivityStatus::Failed
                | AgentActivityStatus::Cancelled
                | AgentActivityStatus::Stalled
        ) {
            segment.result_summary = string_field(&event.payload, "reason").map(ui_text);
        }
    }

    fn project_background_input_requested(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        if !is_terminal_agent_activity_status(segment.status) {
            segment.status = AgentActivityStatus::WaitingInput;
        }
    }

    fn project_background_input_submitted(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        if matches!(segment.status, AgentActivityStatus::WaitingInput) {
            segment.status = AgentActivityStatus::Running;
        }
    }

    fn project_background_permission_requested(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(request_id) = string_field(&event.payload, "requestId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        segment.status = AgentActivityStatus::WaitingPermission;
        segment.permission = Some(AgentActivityPermissionState {
            id: format!("permission:{request_id}"),
            request_id,
            status: DecisionRequestStatus::Pending,
            summary: string_field(&event.payload, "reason").map(ui_text),
            event_refs: vec![event_ref.clone()],
        });
        segment.event_refs.push(event_ref);
    }

    fn project_background_permission_resolved(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        let status = permission_status_from_decision(&event.payload);
        if let Some(permission) = segment.permission.as_mut() {
            permission.status = status;
            permission.summary = None;
            permission.event_refs.push(event_ref.clone());
        }
        segment.event_refs.push(event_ref);
        if matches!(status, DecisionRequestStatus::Approved)
            && matches!(segment.status, AgentActivityStatus::WaitingPermission)
        {
            segment.status = AgentActivityStatus::Running;
        } else if matches!(
            status,
            DecisionRequestStatus::Denied | DecisionRequestStatus::Failed
        ) {
            segment.status = AgentActivityStatus::Failed;
        }
    }

    fn project_background_cancelled(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        if !is_terminal_agent_activity_status(segment.status) {
            segment.status = AgentActivityStatus::Cancelled;
            segment.result_summary = string_field(&event.payload, "reason").map(ui_text);
        }
    }

    fn project_background_completed(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        segment.status = AgentActivityStatus::Completed;
        segment.result_summary = string_field(&event.payload, "summary").map(ui_text);
    }

    fn project_background_failed(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        segment.status = AgentActivityStatus::Failed;
        segment.result_summary = string_field(&event.payload, "error").map(ui_text);
    }

    fn project_background_interrupted(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        if !is_terminal_agent_activity_status(segment.status) {
            segment.status = AgentActivityStatus::Stalled;
            segment.result_summary = string_field(&event.payload, "reason").map(ui_text);
        }
    }

    fn project_background_archived(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        if !is_terminal_agent_activity_status(segment.status) {
            segment.status = AgentActivityStatus::Cancelled;
            segment.result_summary = Some(ui_text("Background agent archived."));
        }
    }

    fn project_background_deleted(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(background_agent_id) = string_field(&event.payload, "backgroundAgentId") else {
            return;
        };
        let Some(segment) = self.agent_activity_mut(&event.run_id, &background_agent_id) else {
            return;
        };
        segment.event_refs.push(event_ref);
        segment.status = AgentActivityStatus::Redacted;
        segment.result_summary = Some(ui_text("Background agent record removed."));
    }

    fn update_team_member_status(
        &mut self,
        run_id: &str,
        team_id: &str,
        agent_id: &str,
        status: AgentActivityStatus,
        event_ref: ConversationEventRef,
    ) {
        let Some(segment) = self.agent_activity_mut(run_id, team_id) else {
            return;
        };
        if let Some(team) = segment.team.as_mut() {
            if let Some(member) = team
                .members
                .iter_mut()
                .find(|member| member.agent_id == agent_id)
            {
                member.status = status;
            }
            if let Some(lead) = team.lead.as_mut() {
                if lead.agent_id == agent_id {
                    lead.status = status;
                }
            }
        }
        if matches!(
            status,
            AgentActivityStatus::Failed | AgentActivityStatus::Stalled
        ) {
            segment.status = status;
        }
        segment.event_refs.push(event_ref);
    }

    fn assistant_work(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) -> &mut AssistantWork {
        let index = self.turn_index_for_run(event);
        let turn = &mut self.turns[index];
        turn.assistant
            .get_or_insert_with(|| AssistantWork {
                id: format!("assistant:{}", event.run_id),
                run_id: event.run_id.clone(),
                projection_version: 1,
                stream_version: 0,
                model: self.run_models.get(&event.run_id).cloned(),
                status: AssistantWorkStatus::Running,
                segments: Vec::new(),
                event_refs: Vec::new(),
            })
            .event_refs
            .push(event_ref);
        turn.assistant.as_mut().expect("assistant inserted")
    }

    fn turn_index_for_run(&mut self, event: &ConversationTimelineEvent) -> usize {
        if let Some(index) = self.run_turns.get(&event.run_id).copied() {
            return index;
        }
        let message_id = synthetic_message_id(&event.run_id);
        let index = self.turns.len();
        self.turns.push(ConversationTurn {
            id: format!("turn:{message_id}"),
            conversation_id: self.conversation_id.to_owned(),
            position: event.cursor.conversation_sequence,
            user: ConversationTurnUserMessage {
                id: format!("user:{message_id}"),
                message_id,
                body: ui_text(""),
                client_message_id: None,
                attachments: Vec::new(),
                timestamp: event.timestamp,
                event_refs: Vec::new(),
            },
            assistant: None,
        });
        self.run_turns.insert(event.run_id.clone(), index);
        index
    }

    fn sort_turns_by_position_and_rebuild_run_turns(&mut self) {
        let run_turn_ids = self
            .run_turns
            .iter()
            .filter_map(|(run_id, index)| {
                self.turns
                    .get(*index)
                    .map(|turn| (run_id.clone(), turn.id.clone()))
            })
            .collect::<Vec<_>>();

        self.turns.sort_by_key(|turn| turn.position);

        let turn_indexes = self
            .turns
            .iter()
            .enumerate()
            .map(|(index, turn)| (turn.id.clone(), index))
            .collect::<HashMap<_, _>>();
        self.run_turns.clear();
        for (run_id, turn_id) in run_turn_ids {
            if let Some(index) = turn_indexes.get(&turn_id).copied() {
                self.run_turns.insert(run_id, index);
            }
        }
    }

    fn next_segment_order(&self, run_id: &str) -> u32 {
        self.run_turns
            .get(run_id)
            .and_then(|index| self.turns.get(*index))
            .and_then(|turn| turn.assistant.as_ref())
            .map_or(0, |assistant| assistant.segments.len() as u32)
    }

    fn tool_group(
        &mut self,
        run_id: &str,
        first_tool_use_id: &str,
        event_ref: ConversationEventRef,
    ) -> &mut ToolGroupSegment {
        let index = self
            .run_turns
            .get(run_id)
            .copied()
            .expect("assistant work exists");
        let assistant = self.turns[index]
            .assistant
            .as_mut()
            .expect("assistant work exists");
        if let Some(position) = assistant
            .segments
            .iter()
            .position(|segment| matches!(segment, AssistantSegment::ToolGroup(_)))
        {
            let AssistantSegment::ToolGroup(group) = &mut assistant.segments[position] else {
                unreachable!("position matched tool group")
            };
            group.event_refs.push(event_ref);
            return group;
        }
        let order = assistant.segments.len() as u32;
        assistant
            .segments
            .push(AssistantSegment::ToolGroup(ToolGroupSegment {
                id: format!("segment:tools:{first_tool_use_id}"),
                order,
                attempts: Vec::new(),
                event_refs: vec![event_ref],
            }));
        let Some(AssistantSegment::ToolGroup(group)) = assistant.segments.last_mut() else {
            unreachable!("tool group was just pushed")
        };
        group
    }

    fn tool_attempt_mut(&mut self, run_id: &str, tool_use_id: &str) -> Option<&mut ToolAttempt> {
        let index = self.run_turns.get(run_id).copied()?;
        let assistant = self.turns[index].assistant.as_mut()?;
        assistant
            .segments
            .iter_mut()
            .find_map(|segment| match segment {
                AssistantSegment::ToolGroup(group) => group
                    .attempts
                    .iter_mut()
                    .find(|attempt| attempt.tool_use_id == tool_use_id),
                _ => None,
            })
    }

    fn remove_text_segment(&mut self, run_id: &str, message_id: &str) -> Option<TextSegment> {
        let index = self.run_turns.get(run_id).copied()?;
        let assistant = self.turns[index].assistant.as_mut()?;
        let position = assistant
            .segments
            .iter()
            .position(|segment| matches!(segment, AssistantSegment::Text(text) if text.message_id == message_id))?;
        let AssistantSegment::Text(text) = assistant.segments.remove(position) else {
            unreachable!("position matched text segment");
        };
        renumber_segments(assistant);
        Some(text)
    }

    fn remove_redacted_text_segments(&mut self, run_id: &str) {
        let Some(index) = self.run_turns.get(run_id).copied() else {
            return;
        };
        let Some(assistant) = self.turns[index].assistant.as_mut() else {
            return;
        };
        assistant.segments.retain(|segment| {
            !matches!(
                segment,
                AssistantSegment::Text(text) if is_redacted_only(text.body.as_str())
            )
        });
        renumber_segments(assistant);
    }

    fn run_has_ready_image_artifact(&self, run_id: &str) -> bool {
        let Some(index) = self.run_turns.get(run_id).copied() else {
            return false;
        };
        let Some(assistant) = self.turns[index].assistant.as_ref() else {
            return false;
        };
        assistant.segments.iter().any(|segment| {
            matches!(
                segment,
                AssistantSegment::Artifact(artifact)
                    if is_ready_image_artifact(artifact.status, artifact.revision.media.as_ref())
            ) || matches!(
                segment,
                AssistantSegment::Process(process)
                    if process.steps.iter().any(|step| matches!(
                        &step.detail,
                        Some(ProcessStepDetail::Artifact { media, .. })
                            if matches!(media.kind, ArtifactMediaKind::Image)
                    ))
            )
        })
    }

    fn artifact_segment_snapshot(
        &self,
        run_id: &str,
        artifact_id: &str,
    ) -> Option<ArtifactSegment> {
        let index = self.run_turns.get(run_id).copied()?;
        let assistant = self.turns[index].assistant.as_ref()?;
        assistant.segments.iter().find_map(|segment| match segment {
            AssistantSegment::Artifact(artifact) if artifact.artifact_id == artifact_id => {
                Some(artifact.clone())
            }
            _ => None,
        })
    }

    fn artifact_process_step_snapshot(
        &self,
        run_id: &str,
        artifact_id: &str,
    ) -> Option<(String, ArtifactMediaPreview)> {
        let index = self.run_turns.get(run_id).copied()?;
        let assistant = self.turns[index].assistant.as_ref()?;
        assistant.segments.iter().find_map(|segment| {
            let AssistantSegment::Process(process) = segment else {
                return None;
            };
            process.steps.iter().find_map(|step| match &step.detail {
                Some(ProcessStepDetail::Artifact {
                    artifact_id: step_artifact_id,
                    revision_id: _,
                    media,
                }) if step_artifact_id == artifact_id => {
                    Some((step.title.as_str().to_owned(), media.clone()))
                }
                _ => None,
            })
        })
    }

    fn remove_artifact_segment(&mut self, run_id: &str, artifact_id: &str) {
        let Some(index) = self.run_turns.get(run_id).copied() else {
            return;
        };
        let Some(assistant) = self.turns[index].assistant.as_mut() else {
            return;
        };
        assistant.segments.retain(|segment| {
            !matches!(
                segment,
                AssistantSegment::Artifact(artifact) if artifact.artifact_id == artifact_id
            )
        });
        renumber_segments(assistant);
    }

    fn unique_tool_attempt_id_for_run(&self, run_id: &str) -> Option<String> {
        let index = self.run_turns.get(run_id).copied()?;
        let assistant = self.turns[index].assistant.as_ref()?;
        let mut tool_use_ids = assistant
            .segments
            .iter()
            .filter_map(|segment| match segment {
                AssistantSegment::ToolGroup(group) => Some(&group.attempts),
                _ => None,
            })
            .flat_map(|attempts| attempts.iter().map(|attempt| &attempt.tool_use_id));
        let tool_use_id = tool_use_ids.next()?;
        if tool_use_ids.next().is_some() {
            return None;
        }
        Some(tool_use_id.clone())
    }

    fn apply_permission_metadata_to_command_step(
        &mut self,
        run_id: &str,
        tool_use_id: &str,
        request_id: &str,
        risk_level: RiskLevel,
        sandbox: Option<String>,
    ) {
        let Some(index) = self.run_turns.get(run_id).copied() else {
            return;
        };
        let Some(assistant) = self.turns[index].assistant.as_mut() else {
            return;
        };
        let step_id = format!(
            "process-step:{run_id}:{}:{tool_use_id}",
            process_step_kind_id(ProcessStepKind::Command)
        );
        for segment in &mut assistant.segments {
            let AssistantSegment::Process(process) = segment else {
                continue;
            };
            let Some(step) = process.steps.iter_mut().find(|step| step.id == step_id) else {
                continue;
            };
            let Some(ProcessStepDetail::Command(command)) = step.detail.as_mut() else {
                return;
            };
            command.approval_request_id = Some(request_id.to_owned());
            command.risk_level = max_risk_level(command.risk_level, risk_level);
            if command.sandbox.is_none() {
                command.sandbox = sandbox;
            }
            return;
        }
    }
}

#[must_use]
pub fn safe_tool_failure_summary(_event: &ConversationTimelineEvent) -> String {
    "工具执行失败。可在详情中查看。".to_owned()
}

fn merge_process_step_detail(
    existing: Option<&ProcessStepDetail>,
    incoming: Option<ProcessStepDetail>,
) -> Option<ProcessStepDetail> {
    match (existing, incoming) {
        (
            Some(ProcessStepDetail::Command(existing_cmd)),
            Some(ProcessStepDetail::Command(incoming_cmd)),
        ) => {
            let command = if incoming_cmd.command == "命令内容已隐藏" {
                existing_cmd.command.clone()
            } else {
                incoming_cmd.command
            };
            Some(ProcessStepDetail::Command(CommandExecution {
                command,
                cwd: existing_cmd.cwd.clone().or(incoming_cmd.cwd),
                shell: existing_cmd.shell.clone().or(incoming_cmd.shell),
                sandbox: existing_cmd.sandbox.clone().or(incoming_cmd.sandbox),
                approval_request_id: existing_cmd
                    .approval_request_id
                    .clone()
                    .or(incoming_cmd.approval_request_id),
                exit_code: incoming_cmd.exit_code.or(existing_cmd.exit_code),
                duration_ms: incoming_cmd.duration_ms.or(existing_cmd.duration_ms),
                stdout_preview: incoming_cmd
                    .stdout_preview
                    .or_else(|| existing_cmd.stdout_preview.clone()),
                stderr_preview: incoming_cmd
                    .stderr_preview
                    .or_else(|| existing_cmd.stderr_preview.clone()),
                full_output_ref: incoming_cmd
                    .full_output_ref
                    .or_else(|| existing_cmd.full_output_ref.clone()),
                truncated: existing_cmd.truncated || incoming_cmd.truncated,
                redaction_state: incoming_cmd.redaction_state,
                risk_level: max_risk_level(existing_cmd.risk_level, incoming_cmd.risk_level),
            }))
        }
        (_, Some(incoming)) => Some(incoming),
        (Some(existing), None) => Some(existing.clone()),
        (None, None) => None,
    }
}

fn permission_request_state_from_payload(
    request_id: String,
    tool_use_id: String,
    payload: &Value,
    auto_resolved: bool,
) -> DecisionRequestState {
    let policy = DecisionPolicy {
        mode: string_field(payload, "effectiveMode")
            .or_else(|| string_field(payload, "effective_mode"))
            .unwrap_or_else(|| "default".to_owned()),
        rule: payload
            .get("review")
            .and_then(|review| string_field(review, "summary"))
            .filter(|summary| !summary.trim().is_empty()),
        sandbox: sandbox_policy_summary(payload),
    };
    DecisionRequestState {
        id: format!("permission:{request_id}"),
        request_id,
        tool_use_id: Some(tool_use_id),
        status: if auto_resolved {
            DecisionRequestStatus::Approved
        } else {
            DecisionRequestStatus::Pending
        },
        operation: permission_operation_from_payload(payload),
        target: permission_target_from_payload(payload),
        risk_level: risk_level_from_payload(payload),
        reason: string_field(payload, "reason").unwrap_or_default(),
        policy,
        decision_options: permission_decision_options(payload),
        evidence_refs: vec![],
        data_exposure: permission_data_exposure_from_payload(payload),
        confirmation: permission_confirmation_expected(payload).map(|text| DecisionConfirmation {
            expected_text: text,
            label: "Confirmation required".to_owned(),
        }),
    }
}

fn permission_operation_from_payload(payload: &Value) -> DecisionOperation {
    if let Some(operation) = string_field(payload, "operation") {
        let projected = operation_from_label(&operation);
        if projected != DecisionOperation::Unknown {
            return projected;
        }
        if let Some(target) = string_field(payload, "target") {
            let projected = operation_from_label(&target);
            if projected != DecisionOperation::Unknown {
                return projected;
            }
        }
    }
    let Some(subject) = payload.get("subject") else {
        return DecisionOperation::Unknown;
    };
    match subject_type(subject).as_deref() {
        Some("file_write" | "file_delete") => DecisionOperation::Write,
        Some("command_exec" | "dangerous_command") => DecisionOperation::Execute,
        Some("network_access") => DecisionOperation::Network,
        Some("mcp_tool_call") => DecisionOperation::Mcp,
        Some("tool_invocation") => permission_operation_from_tool_subject(subject),
        Some("custom") => string_field(subject_body(subject), "kind")
            .map(|kind| operation_from_label(&kind))
            .unwrap_or(DecisionOperation::Unknown),
        _ => DecisionOperation::Unknown,
    }
}

fn permission_operation_from_tool_subject(subject: &Value) -> DecisionOperation {
    let label = string_field(subject_body(subject), "tool").unwrap_or_default();
    operation_from_label(&label)
}

fn operation_from_label(label: &str) -> DecisionOperation {
    let normalized = label.to_ascii_lowercase();
    if normalized.contains("read") || normalized.contains("list") || normalized.contains("grep") {
        DecisionOperation::Read
    } else if normalized.contains("write")
        || normalized.contains("edit")
        || normalized.contains("patch")
        || normalized.contains("delete")
    {
        DecisionOperation::Write
    } else if normalized.contains("bash")
        || normalized.contains("shell")
        || normalized.contains("command")
        || normalized.contains("execute")
    {
        DecisionOperation::Execute
    } else if normalized.contains("network") || normalized.contains("http") {
        DecisionOperation::Network
    } else if normalized.contains("mcp") {
        DecisionOperation::Mcp
    } else if normalized.contains("artifact") {
        DecisionOperation::Artifact
    } else if normalized.contains("git") {
        DecisionOperation::Git
    } else {
        DecisionOperation::Unknown
    }
}

fn permission_target_from_payload(payload: &Value) -> DecisionTarget {
    if let Some(subject) = payload.get("subject") {
        if let Some(target) = permission_target_from_subject(subject) {
            return target;
        }
    }
    if let Some(label) = string_field(payload, "target") {
        let operation_kind = string_field(payload, "operation")
            .map(|operation| target_kind_from_operation_label(&operation))
            .unwrap_or(DecisionTargetKind::Unknown);
        return DecisionTarget {
            kind: if operation_kind == DecisionTargetKind::Unknown {
                target_kind_from_label(&label)
            } else {
                operation_kind
            },
            label,
            secondary_label: None,
        };
    }
    if let Some(scope_hint) = payload
        .get("scopeHint")
        .or_else(|| payload.get("scope_hint"))
        .and_then(permission_target_from_scope_hint)
    {
        return scope_hint;
    }
    DecisionTarget {
        kind: DecisionTargetKind::Unknown,
        label: String::new(),
        secondary_label: None,
    }
}

fn permission_target_from_subject(subject: &Value) -> Option<DecisionTarget> {
    let subject_type = subject_type(subject)?;
    let subject = subject_body(subject);
    match subject_type.as_str() {
        "file_write" | "file_delete" => string_field(subject, "path").map(|path| DecisionTarget {
            kind: DecisionTargetKind::File,
            label: path,
            secondary_label: None,
        }),
        "command_exec" => command_text_from_payload(subject).map(|command| DecisionTarget {
            kind: DecisionTargetKind::Command,
            label: command,
            secondary_label: command_cwd_from_payload(subject),
        }),
        "dangerous_command" => command_text_from_payload(subject).map(|command| DecisionTarget {
            kind: DecisionTargetKind::Command,
            label: command,
            secondary_label: string_field(subject, "patternId")
                .or_else(|| string_field(subject, "pattern_id")),
        }),
        "network_access" => string_field(subject, "host").map(|host| {
            let label = u64_field(subject, "port")
                .map(|port| format!("{host}:{port}"))
                .unwrap_or(host);
            DecisionTarget {
                kind: DecisionTargetKind::Url,
                label,
                secondary_label: None,
            }
        }),
        "mcp_tool_call" => {
            let server = string_field(subject, "server")?;
            let tool = string_field(subject, "tool")?;
            Some(DecisionTarget {
                kind: DecisionTargetKind::McpTool,
                label: format!("{server}/{tool}"),
                secondary_label: None,
            })
        }
        "tool_invocation" => string_field(subject, "tool").map(|tool| DecisionTarget {
            kind: target_kind_from_label(&tool),
            label: tool,
            secondary_label: None,
        }),
        "custom" => string_field(subject, "kind").map(|kind| DecisionTarget {
            kind: target_kind_from_label(&kind),
            label: kind,
            secondary_label: None,
        }),
        _ => None,
    }
}

fn permission_target_from_scope_hint(scope_hint: &Value) -> Option<DecisionTarget> {
    if let Some(path) =
        string_field(scope_hint, "path_prefix").or_else(|| string_field(scope_hint, "pathPrefix"))
    {
        return Some(DecisionTarget {
            kind: DecisionTargetKind::Directory,
            label: path,
            secondary_label: None,
        });
    }
    if let Some(command) = scope_hint
        .get("exact_command")
        .or_else(|| scope_hint.get("exactCommand"))
        .and_then(command_text_from_payload)
    {
        return Some(DecisionTarget {
            kind: DecisionTargetKind::Command,
            label: command,
            secondary_label: scope_hint
                .get("exact_command")
                .or_else(|| scope_hint.get("exactCommand"))
                .and_then(command_cwd_from_payload),
        });
    }
    string_field(scope_hint, "tool_name")
        .or_else(|| string_field(scope_hint, "toolName"))
        .map(|tool| DecisionTarget {
            kind: target_kind_from_label(&tool),
            label: tool,
            secondary_label: None,
        })
}

fn target_kind_from_label(label: &str) -> DecisionTargetKind {
    match operation_from_label(label) {
        DecisionOperation::Read | DecisionOperation::Write => DecisionTargetKind::File,
        DecisionOperation::Execute => DecisionTargetKind::Command,
        DecisionOperation::Network => DecisionTargetKind::Url,
        DecisionOperation::Mcp => DecisionTargetKind::McpTool,
        DecisionOperation::Artifact => DecisionTargetKind::Artifact,
        DecisionOperation::Git => DecisionTargetKind::GitRef,
        DecisionOperation::Unknown => DecisionTargetKind::Unknown,
    }
}

fn target_kind_from_operation_label(label: &str) -> DecisionTargetKind {
    let normalized = label.to_ascii_lowercase();
    if normalized.contains("file") || normalized.contains("write") || normalized.contains("delete")
    {
        DecisionTargetKind::File
    } else if normalized.contains("command") || normalized.contains("execute") {
        DecisionTargetKind::Command
    } else if normalized.contains("network") || normalized.contains("url") {
        DecisionTargetKind::Url
    } else if normalized.contains("mcp") {
        DecisionTargetKind::McpTool
    } else if normalized.contains("artifact") {
        DecisionTargetKind::Artifact
    } else if normalized.contains("git") {
        DecisionTargetKind::GitRef
    } else if normalized.contains("workspace") {
        DecisionTargetKind::Workspace
    } else {
        DecisionTargetKind::Unknown
    }
}

fn permission_data_exposure_from_payload(payload: &Value) -> DataExposure {
    let subject = payload.get("subject");
    let subject_kind = subject.and_then(subject_type);
    let operation = string_field(payload, "operation").unwrap_or_default();
    let exposure = string_field(payload, "exposure").unwrap_or_default();
    let path_like_values = subject
        .into_iter()
        .flat_map(|subject| {
            let subject = subject_body(subject);
            ["path", "cwd"]
                .into_iter()
                .filter_map(|field| string_field(subject, field))
        })
        .chain(string_field(payload, "target"))
        .collect::<Vec<_>>();
    DataExposure {
        sends_workspace_data: matches!(
            subject_kind.as_deref(),
            Some("file_write" | "file_delete" | "command_exec" | "tool_invocation")
        ) || exposure.to_ascii_lowercase().contains("workspace")
            || matches!(
                permission_operation_from_payload(payload),
                DecisionOperation::Read | DecisionOperation::Write | DecisionOperation::Execute
            ),
        sends_network_data: matches!(
            subject_kind.as_deref(),
            Some("network_access" | "mcp_tool_call")
        ) || operation.to_ascii_lowercase().contains("network")
            || exposure.to_ascii_lowercase().contains("network")
            || exposure.to_ascii_lowercase().contains("mcp"),
        touches_private_path: path_like_values
            .iter()
            .any(|value| looks_like_private_path(value)),
        secret_risk: if payload
            .get("review")
            .and_then(|review| bool_field(review, "redacted"))
            .unwrap_or(false)
        {
            DataExposureSecretRisk::Redacted
        } else {
            DataExposureSecretRisk::None
        },
    }
}

fn subject_type(subject: &Value) -> Option<String> {
    string_field(subject, "type")
        .or_else(|| string_field(subject, "kind"))
        .or_else(|| {
            subject.as_object().and_then(|object| {
                object
                    .keys()
                    .find(|key| is_permission_subject_variant(key.as_str()))
                    .cloned()
            })
        })
}

fn subject_body<'a>(subject: &'a Value) -> &'a Value {
    subject_type(subject)
        .and_then(|kind| subject.get(kind))
        .unwrap_or(subject)
}

fn is_permission_subject_variant(value: &str) -> bool {
    matches!(
        value,
        "tool_invocation"
            | "command_exec"
            | "file_write"
            | "file_delete"
            | "network_access"
            | "dangerous_command"
            | "mcp_tool_call"
            | "custom"
    )
}

fn looks_like_private_path(value: &str) -> bool {
    value.starts_with("/Users/")
        || value.starts_with("~/")
        || value.contains("/.ssh/")
        || value.contains("/.config/")
        || value.contains("/Library/Application Support/")
}

fn risk_level_from_payload(payload: &Value) -> RiskLevel {
    string_field(payload, "riskLevel")
        .or_else(|| string_field(payload, "risk_level"))
        .or_else(|| string_field(payload, "severity"))
        .or_else(|| {
            payload
                .get("subject")
                .and_then(|subject| string_field(subject_body(subject), "severity"))
        })
        .map(|value| risk_level_from_str(&value))
        .unwrap_or(RiskLevel::Low)
}

fn risk_level_from_str(value: &str) -> RiskLevel {
    match value {
        "critical" => RiskLevel::Critical,
        "high" => RiskLevel::High,
        "medium" => RiskLevel::Medium,
        "low" | "info" => RiskLevel::Low,
        _ => RiskLevel::Low,
    }
}

fn max_risk_level(left: RiskLevel, right: RiskLevel) -> RiskLevel {
    if risk_rank(right) > risk_rank(left) {
        right
    } else {
        left
    }
}

fn risk_rank(value: RiskLevel) -> u8 {
    match value {
        RiskLevel::Low => 0,
        RiskLevel::Medium => 1,
        RiskLevel::High => 2,
        RiskLevel::Critical => 3,
    }
}

fn command_text_from_payload(payload: &Value) -> Option<String> {
    string_field(payload, "command").or_else(|| {
        payload
            .get("subject")
            .and_then(|subject| command_text_from_payload(subject_body(subject)))
    })
}

fn command_cwd_from_payload(payload: &Value) -> Option<String> {
    string_field(payload, "cwd")
        .or_else(|| string_field(payload, "workingDirectory"))
        .or_else(|| string_field(payload, "working_directory"))
        .or_else(|| {
            payload
                .get("subject")
                .and_then(|subject| command_cwd_from_payload(subject_body(subject)))
        })
}

fn command_shell_from_payload(payload: &Value) -> Option<String> {
    string_field(payload, "shell").or_else(|| {
        payload
            .get("input")
            .and_then(|input| string_field(input, "shell"))
    })
}

fn command_output_is_truncated(payload: &Value) -> bool {
    if bool_field(payload, "truncated")
        .or_else(|| bool_field(payload, "stdoutTruncated"))
        .or_else(|| bool_field(payload, "stderrTruncated"))
        .unwrap_or(false)
    {
        return true;
    }
    if string_field(payload, "fullOutputRef")
        .or_else(|| string_field(payload, "full_output_ref"))
        .is_some()
    {
        return true;
    }
    let full_bytes = ["outputBytes", "stdoutBytes", "stderrBytes", "contentBytes"]
        .into_iter()
        .filter_map(|field| u64_field(payload, field))
        .max();
    let preview_bytes = ["returnedBytes", "previewBytes", "limitBytes"]
        .into_iter()
        .filter_map(|field| u64_field(payload, field))
        .max();
    matches!((full_bytes, preview_bytes), (Some(full), Some(preview)) if full > preview)
}

fn sandbox_policy_summary(payload: &Value) -> Option<String> {
    if let Some(sandbox) = string_field(payload, "sandbox") {
        return Some(sandbox);
    }
    let policy = payload
        .get("sandboxPolicy")
        .or_else(|| payload.get("sandbox_policy"))?;
    let mode = policy.get("mode").and_then(value_label)?;
    let scope = policy
        .get("scope")
        .and_then(value_label)
        .unwrap_or_else(|| "unknown_scope".to_owned());
    let network = policy
        .get("network")
        .and_then(value_label)
        .unwrap_or_else(|| "unknown".to_owned());
    Some(format!("{mode} / {scope} / network:{network}"))
}

fn value_label(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(value_label)
                .collect::<Vec<_>>()
                .join(","),
        ),
        Value::Object(object) if object.len() == 1 => {
            let (key, value) = object.iter().next()?;
            value_label(value).map(|value| format!("{key}:{value}"))
        }
        Value::Object(_) => serde_json::to_string(value).ok(),
        Value::Null => None,
    }
}

#[derive(Debug, Clone, Copy)]
enum ToolProcessPhase {
    Requested,
    Completed,
    Failed,
}

fn process_step_kind_for_tool_name(tool_name: &str) -> ProcessStepKind {
    let normalized = tool_name.to_ascii_lowercase();
    if normalized.contains("fileread")
        || normalized.contains("read_file")
        || normalized.contains("readfile")
        || normalized == "read"
        || normalized.contains("list_dir")
        || normalized.contains("listdir")
    {
        ProcessStepKind::FileRead
    } else if normalized.contains("grep")
        || normalized.contains("glob")
        || normalized.contains("search")
    {
        ProcessStepKind::FileSearch
    } else if normalized == "bash"
        || normalized.contains("shell")
        || normalized.contains("execute_code")
    {
        ProcessStepKind::Command
    } else if normalized.contains("filewrite")
        || normalized.contains("fileedit")
        || normalized.contains("apply_patch")
        || normalized == "write"
        || normalized == "edit"
    {
        ProcessStepKind::FileEdit
    } else {
        ProcessStepKind::Tool
    }
}

fn tool_process_step_id(event: &ConversationTimelineEvent, kind: ProcessStepKind) -> String {
    let tool_use_id = string_field(&event.payload, "toolUseId").unwrap_or_else(|| event.id.clone());
    format!(
        "process-step:{}:{}:{tool_use_id}",
        event.run_id,
        process_step_kind_id(kind)
    )
}

fn aggregate_process_step_id(run_id: &str, kind: ProcessStepKind) -> String {
    format!(
        "process-step:{run_id}:aggregate:{}",
        process_step_kind_id(kind)
    )
}

fn process_step_kind_id(kind: ProcessStepKind) -> &'static str {
    match kind {
        ProcessStepKind::Reasoning => "reasoning",
        ProcessStepKind::Activity => "activity",
        ProcessStepKind::Command => "command",
        ProcessStepKind::FileRead => "file-read",
        ProcessStepKind::FileSearch => "file-search",
        ProcessStepKind::FileEdit => "file-edit",
        ProcessStepKind::Diff => "diff",
        ProcessStepKind::Tool => "tool",
        ProcessStepKind::Artifact => "artifact",
        ProcessStepKind::Synthesis => "synthesis",
        ProcessStepKind::Withheld => "withheld",
    }
}

fn merged_activity_detail(
    title: &str,
    existing: Option<&ProcessStepDetail>,
    next_count: u32,
) -> ProcessStepDetail {
    let previous = match existing {
        Some(ProcessStepDetail::Activity { item_count, .. }) => item_count.unwrap_or(0),
        _ => 0,
    };
    ProcessStepDetail::Activity {
        summary: ui_text(title),
        item_count: Some(previous.saturating_add(next_count)),
    }
}

fn diff_process_detail_from_payload(payload: &Value) -> Option<ProcessStepDetail> {
    let files = payload
        .get("diff")?
        .get("files")?
        .as_array()?
        .iter()
        .filter_map(change_set_file_from_payload)
        .collect::<Vec<_>>();
    if files.is_empty() {
        return None;
    }
    let change_set_id = format!(
        "changeset:{}",
        payload
            .get("toolUseId")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
    );
    Some(ProcessStepDetail::Diff(ChangeSet {
        id: change_set_id,
        summary: format!("修改了 {} 个文件", files.len()),
        files,
    }))
}

fn change_set_file_from_payload(value: &Value) -> Option<ChangeSetFile> {
    let path = string_field(value, "path")?;
    let added_lines = u32_field(value, "addedLines").or_else(|| u32_field(value, "added_lines"))?;
    let removed_lines =
        u32_field(value, "removedLines").or_else(|| u32_field(value, "removed_lines"))?;
    let preview = string_field(value, "preview");
    Some(ChangeSetFile {
        path,
        old_path: None,
        status: ChangeSetFileStatus::Modified,
        added_lines,
        removed_lines,
        preview,
        full_patch_ref: string_field(value, "fullPatchRef").map(EvidenceRefId::new),
        risk_flags: vec![],
    })
}

fn tool_step_title(tool_name: &str, phase: ToolProcessPhase) -> String {
    match (process_step_kind_for_tool_name(tool_name), phase) {
        (ProcessStepKind::FileRead, ToolProcessPhase::Requested) => "准备读取文件".to_owned(),
        (ProcessStepKind::FileRead, ToolProcessPhase::Completed) => "已读取文件".to_owned(),
        (ProcessStepKind::FileRead, ToolProcessPhase::Failed) => "读取文件失败".to_owned(),
        (ProcessStepKind::FileSearch, ToolProcessPhase::Requested) => "准备搜索代码".to_owned(),
        (ProcessStepKind::FileSearch, ToolProcessPhase::Completed) => "已完成搜索".to_owned(),
        (ProcessStepKind::FileSearch, ToolProcessPhase::Failed) => "搜索失败".to_owned(),
        (ProcessStepKind::Command, ToolProcessPhase::Requested) => "准备运行命令".to_owned(),
        (ProcessStepKind::Command, ToolProcessPhase::Completed) => "命令已完成".to_owned(),
        (ProcessStepKind::Command, ToolProcessPhase::Failed) => "命令执行失败".to_owned(),
        (ProcessStepKind::FileEdit, ToolProcessPhase::Requested) => "准备编辑文件".to_owned(),
        (ProcessStepKind::FileEdit, ToolProcessPhase::Completed) => "已完成文件修改".to_owned(),
        (ProcessStepKind::FileEdit, ToolProcessPhase::Failed) => "文件修改失败".to_owned(),
        (_, ToolProcessPhase::Requested) => format!("准备使用 {tool_name}"),
        (_, ToolProcessPhase::Completed) => format!("{tool_name} 已完成"),
        (_, ToolProcessPhase::Failed) => format!("{tool_name} 执行失败"),
    }
}

fn process_step_detail_for_tool(
    event: &ConversationTimelineEvent,
    tool_name: &str,
    kind: ProcessStepKind,
    title: &str,
) -> Option<ProcessStepDetail> {
    match kind {
        ProcessStepKind::Command => Some(ProcessStepDetail::Command(CommandExecution {
            command: command_text_from_payload(&event.payload)
                .unwrap_or_else(|| "命令内容已隐藏".to_owned()),
            cwd: command_cwd_from_payload(&event.payload),
            shell: command_shell_from_payload(&event.payload),
            sandbox: sandbox_policy_summary(&event.payload),
            approval_request_id: string_field(&event.payload, "approvalRequestId")
                .or_else(|| string_field(&event.payload, "approval_request_id")),
            exit_code: i32_field(&event.payload, "exitCode"),
            duration_ms: u64_field(&event.payload, "durationMs"),
            stdout_preview: string_field(&event.payload, "stdoutPreview")
                .or_else(|| string_field(&event.payload, "outputSummary")),
            stderr_preview: string_field(&event.payload, "stderrPreview"),
            full_output_ref: string_field(&event.payload, "fullOutputRef").map(EvidenceRefId::new),
            truncated: command_output_is_truncated(&event.payload),
            redaction_state: redaction_state_from_event(event),
            risk_level: risk_level_from_payload(&event.payload),
        })),
        ProcessStepKind::FileRead | ProcessStepKind::FileSearch | ProcessStepKind::FileEdit => {
            Some(ProcessStepDetail::Activity {
                summary: ui_text(title),
                item_count: u32_field(&event.payload, "itemCount"),
            })
        }
        ProcessStepKind::Tool => Some(ProcessStepDetail::Tool {
            tool_name: ui_text(tool_name),
            output_summary: string_field(&event.payload, "outputSummary").and_then(|summary| {
                (summary != "Output withheld from conversation timeline.")
                    .then_some(ui_text(summary))
            }),
            duration_ms: u64_field(&event.payload, "durationMs"),
        }),
        ProcessStepKind::Reasoning
        | ProcessStepKind::Activity
        | ProcessStepKind::Diff
        | ProcessStepKind::Artifact
        | ProcessStepKind::Synthesis
        | ProcessStepKind::Withheld => None,
    }
}

#[must_use]
pub fn is_empty_assistant_body(value: &Value) -> bool {
    string_field(value, "body").is_none_or(|body| body.trim().is_empty())
}

fn event_ref(event: &ConversationTimelineEvent) -> ConversationEventRef {
    ConversationEventRef {
        event_id: event.id.clone(),
        cursor: event.cursor,
    }
}

fn user_message_from_event(
    event: &ConversationTimelineEvent,
    event_ref: ConversationEventRef,
) -> ConversationTurnUserMessage {
    let message_id =
        string_field(&event.payload, "messageId").unwrap_or_else(|| format!("event:{}", event.id));
    ConversationTurnUserMessage {
        id: format!("user:{message_id}"),
        message_id,
        body: ui_text(string_field(&event.payload, "body").unwrap_or_default()),
        client_message_id: string_field(&event.payload, "clientMessageId"),
        attachments: attachment_references_field(&event.payload, "attachments"),
        timestamp: event.timestamp,
        event_refs: vec![event_ref],
    }
}

fn attachment_references_field(value: &Value, field: &str) -> Vec<ConversationAttachmentReference> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(attachment_reference_from_value)
                .collect()
        })
        .unwrap_or_default()
}

fn attachment_reference_from_value(value: &Value) -> Option<ConversationAttachmentReference> {
    let blob_ref = value
        .get("blobRef")
        .or_else(|| value.get("blob_ref"))
        .and_then(blob_ref_from_value)?;
    Some(ConversationAttachmentReference {
        id: string_field(value, "id")?,
        name: string_field(value, "name")?,
        mime_type: string_field(value, "mimeType").or_else(|| string_field(value, "mime_type"))?,
        size_bytes: u64_field(value, "sizeBytes").or_else(|| u64_field(value, "size_bytes"))?,
        blob_ref,
    })
}

fn blob_ref_from_value(value: &Value) -> Option<BlobRef> {
    let id = BlobId::parse(&string_field(value, "id")?).ok()?;
    Some(BlobRef {
        id,
        size: u64_field(value, "size")?,
        content_hash: content_hash_field(value)?,
        content_type: value
            .get("contentType")
            .or_else(|| value.get("content_type"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

fn content_hash_field(value: &Value) -> Option<[u8; 32]> {
    let items = value
        .get("contentHash")
        .or_else(|| value.get("content_hash"))?
        .as_array()?;
    let bytes = items
        .iter()
        .map(|item| item.as_u64().and_then(|byte| u8::try_from(byte).ok()))
        .collect::<Option<Vec<_>>>()?;
    bytes.try_into().ok()
}

fn is_synthetic_user_message_for_run(user: &ConversationTurnUserMessage, run_id: &str) -> bool {
    user.message_id == synthetic_message_id(run_id)
}

fn synthetic_message_id(run_id: &str) -> String {
    format!("synthetic:{run_id}")
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn bool_field(value: &Value, field: &str) -> Option<bool> {
    value.get(field).and_then(Value::as_bool)
}

fn permission_decision_options(value: &Value) -> Vec<DecisionOption> {
    value
        .get("decisionOptions")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(permission_decision_option)
                .collect()
        })
        .unwrap_or_default()
}

fn permission_decision_option(value: &Value) -> Option<DecisionOption> {
    let matcher = value.get("matcher")?;
    Some(DecisionOption {
        id: string_field(value, "id")?,
        decision: permission_decision_kind(value)?,
        label: string_field(value, "label")?,
        lifetime: permission_decision_lifetime(value)?,
        matcher: DecisionMatcherSummary {
            kind: permission_decision_matcher_kind(matcher)?,
            label: string_field(matcher, "label")?,
        },
        requires_confirmation: bool_field(value, "requiresConfirmation").unwrap_or(false),
    })
}

fn permission_decision_kind(value: &Value) -> Option<DecisionKind> {
    match string_field(value, "decision").as_deref() {
        Some("approve") => Some(DecisionKind::Approve),
        Some("deny") => Some(DecisionKind::Deny),
        _ => None,
    }
}

fn permission_decision_lifetime(value: &Value) -> Option<DecisionLifetime> {
    match string_field(value, "lifetime").as_deref() {
        Some("once") => Some(DecisionLifetime::Once),
        Some("run") => Some(DecisionLifetime::Run),
        Some("session") => Some(DecisionLifetime::Session),
        Some("persisted") => Some(DecisionLifetime::Persisted),
        _ => None,
    }
}

fn permission_decision_matcher_kind(value: &Value) -> Option<DecisionMatcherKind> {
    match string_field(value, "kind").as_deref() {
        Some("exactCommand") => Some(DecisionMatcherKind::ExactCommand),
        Some("exactArgs") => Some(DecisionMatcherKind::ExactArgs),
        Some("toolName") => Some(DecisionMatcherKind::ToolName),
        Some("category") => Some(DecisionMatcherKind::Category),
        Some("pathPrefix") => Some(DecisionMatcherKind::PathPrefix),
        Some("globPattern") => Some(DecisionMatcherKind::GlobPattern),
        Some("executeCodeScript") => Some(DecisionMatcherKind::ExecuteCodeScript),
        Some("any") => Some(DecisionMatcherKind::Any),
        _ => None,
    }
}

fn notice_code_field(value: &Value, field: &str) -> Option<AssistantNoticeCode> {
    match string_field(value, field).as_deref() {
        Some("contextCompacted") => Some(AssistantNoticeCode::ContextCompacted),
        _ => None,
    }
}

fn u64_field(value: &Value, field: &str) -> Option<u64> {
    value.get(field).and_then(Value::as_u64)
}

fn u32_field(value: &Value, field: &str) -> Option<u32> {
    u64_field(value, field).and_then(|value| u32::try_from(value).ok())
}

fn i32_field(value: &Value, field: &str) -> Option<i32> {
    value
        .get(field)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn safe_summary_field(value: &Value) -> Option<UiSafeText> {
    string_field(value, "safeSummary").map(ui_text)
}

fn safe_summary_delta_field(value: &Value) -> Option<UiSafeText> {
    string_field(value, "safeSummaryDelta").map(ui_text)
}

fn artifact_summary(value: &Value) -> Option<UiSafeText> {
    string_field(value, "summary")
        .or_else(|| string_field(value, "preview"))
        .or_else(|| string_field(value, "status").map(|status| format!("Artifact {status}")))
        .map(ui_text)
}

fn maybe_artifact_kind(value: &Value) -> Option<String> {
    string_field(value, "kind")
        .or_else(|| string_field(value, "artifactKind"))
        .map(|kind| ui_text(kind).into_string())
}

fn maybe_artifact_status(value: &Value) -> Option<ArtifactStatus> {
    value.get("status")?;
    Some(artifact_status(value))
}

fn artifact_status(value: &Value) -> ArtifactStatus {
    match string_field(value, "status").as_deref() {
        Some("pending") => ArtifactStatus::Pending,
        Some("running") => ArtifactStatus::Running,
        Some("failed") => ArtifactStatus::Failed,
        Some("ready") | Some("complete") | Some("completed") => ArtifactStatus::Ready,
        _ => ArtifactStatus::Ready,
    }
}

fn maybe_artifact_source(value: &Value) -> Option<ArtifactSource> {
    value.get("source")?;
    Some(artifact_source(value))
}

fn artifact_source(value: &Value) -> ArtifactSource {
    match string_field(value, "source").as_deref() {
        Some("tool") => ArtifactSource::Tool,
        Some("file") => ArtifactSource::File,
        Some("modelService" | "model_service") => ArtifactSource::ModelService,
        _ => ArtifactSource::Assistant,
    }
}

fn artifact_media_preview(value: &Value, artifact_kind: &str) -> Option<ArtifactMediaPreview> {
    let media = value.get("media")?;
    let kind = string_field(media, "kind")
        .and_then(|kind| artifact_media_kind(&kind))
        .or_else(|| artifact_media_kind(artifact_kind))?;
    let safe_mime_type = string_field(media, "mimeType")
        .or_else(|| string_field(media, "mime_type"))
        .as_deref()
        .and_then(safe_artifact_mime_type);
    let mime_type = safe_mime_type
        .filter(|mime_type| {
            matches!(kind, ArtifactMediaKind::File)
                || artifact_media_kind_from_mime(mime_type)
                    .is_some_and(|mime_kind| mime_kind == kind)
        })
        .unwrap_or_else(|| default_artifact_mime_type(kind).to_owned());
    let size_bytes = media
        .get("sizeBytes")
        .or_else(|| media.get("size_bytes"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    Some(ArtifactMediaPreview {
        kind,
        mime_type,
        size_bytes,
    })
}

fn artifact_media_kind(value: &str) -> Option<ArtifactMediaKind> {
    match value {
        "image" => Some(ArtifactMediaKind::Image),
        "video" => Some(ArtifactMediaKind::Video),
        "audio" => Some(ArtifactMediaKind::Audio),
        "file" => Some(ArtifactMediaKind::File),
        _ => safe_artifact_mime_type(value)
            .as_deref()
            .and_then(artifact_media_kind_from_mime),
    }
}

fn artifact_media_kind_from_mime(value: &str) -> Option<ArtifactMediaKind> {
    if safe_artifact_image_mime_type(value).is_some() {
        Some(ArtifactMediaKind::Image)
    } else if value.starts_with("video/") {
        Some(ArtifactMediaKind::Video)
    } else if value.starts_with("audio/") {
        Some(ArtifactMediaKind::Audio)
    } else if safe_artifact_mime_type(value).is_some() {
        Some(ArtifactMediaKind::File)
    } else {
        None
    }
}

fn default_artifact_mime_type(kind: ArtifactMediaKind) -> &'static str {
    match kind {
        ArtifactMediaKind::Image => "image/png",
        ArtifactMediaKind::Video => "video/mp4",
        ArtifactMediaKind::Audio => "audio/mpeg",
        ArtifactMediaKind::File => "application/octet-stream",
    }
}

fn safe_artifact_mime_type(value: &str) -> Option<String> {
    let mime_type = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    match mime_type.as_str() {
        "image/png"
        | "image/jpeg"
        | "image/gif"
        | "image/webp"
        | "image/avif"
        | "video/mp4"
        | "video/webm"
        | "video/quicktime"
        | "audio/mpeg"
        | "audio/mp4"
        | "audio/ogg"
        | "audio/wav"
        | "audio/webm"
        | "text/plain"
        | "text/markdown"
        | "text/csv"
        | "application/json"
        | "application/pdf"
        | "application/zip"
        | "application/octet-stream" => Some(mime_type),
        _ => None,
    }
}

fn safe_artifact_image_mime_type(value: &str) -> Option<&'static str> {
    match value {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/avif" => Some("image/avif"),
        _ => None,
    }
}

fn artifact_media_kind_label(kind: ArtifactMediaKind) -> &'static str {
    match kind {
        ArtifactMediaKind::Image => "image",
        ArtifactMediaKind::Video => "video",
        ArtifactMediaKind::Audio => "audio",
        ArtifactMediaKind::File => "file",
    }
}

fn is_ready_image_artifact(status: ArtifactStatus, media: Option<&ArtifactMediaPreview>) -> bool {
    matches!(status, ArtifactStatus::Ready)
        && media.is_some_and(|media| matches!(media.kind, ArtifactMediaKind::Image))
}

fn assistant_completed_has_tool_uses(value: &Value) -> bool {
    value
        .get("toolUses")
        .and_then(Value::as_array)
        .is_some_and(|tool_uses| !tool_uses.is_empty())
}

fn is_redacted_only(value: &str) -> bool {
    value.trim() == "[REDACTED]"
}

fn ui_text(value: impl AsRef<str>) -> UiSafeText {
    UiSafeText::from_redacted_display(
        redact_obvious_secret_tokens(&redact_unsafe_display_text(value.as_ref())),
        &harness_contracts::NoopRedactor,
    )
}

fn redact_obvious_secret_tokens(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut pending_secret_value = false;

    for token in value.split_inclusive(char::is_whitespace) {
        let (body, trailing_ws) = token.split_at(token.trim_end_matches(char::is_whitespace).len());
        if body.is_empty() {
            output.push_str(trailing_ws);
            continue;
        }

        let lower = body.to_ascii_lowercase();
        let redact_current = pending_secret_value || is_obvious_secret_token(&lower);
        pending_secret_value = matches!(lower.as_str(), "bearer" | "basic")
            || lower.ends_with("authorization:")
            || lower.ends_with("authorization");

        if redact_current {
            output.push_str("[REDACTED]");
        } else {
            output.push_str(body);
        }
        output.push_str(trailing_ws);
    }

    output
}

fn is_obvious_secret_token(lower: &str) -> bool {
    lower.contains("authorization:")
        || lower == "bearer"
        || lower == "basic"
        || lower.contains("api_key")
        || lower.contains("api-key")
        || lower.contains("apikey")
        || lower.contains("token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.contains("sk-")
        || lower.contains("ghp_")
        || lower.contains("gho_")
        || lower.contains("ghu_")
        || lower.contains("ghs_")
        || lower.contains("ghr_")
        || lower.contains("akia")
        || lower.contains("aiza")
        || lower.contains("xoxb-")
        || lower.contains("xoxp-")
        || lower.contains("xoxa-")
        || lower.contains("xoxr-")
        || lower.contains("npm_")
        || lower.contains("lin_api_")
        || lower.contains("secret_")
        || lower.starts_with("eyj")
}

fn redact_unsafe_display_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if unsafe_url_starts_at(value, index) {
            output.push_str("[REDACTED]");
            index = unsafe_url_token_end(value, index);
            continue;
        }
        if local_unsafe_path_starts_at(value, index) {
            output.push_str("[REDACTED]");
            index = unsafe_token_end(value, index);
            continue;
        }
        let ch = value[index..]
            .chars()
            .next()
            .expect("index is within string bounds");
        output.push(ch);
        index += ch.len_utf8();
    }
    output
}

fn token_starts_at(value: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    value[..index]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace() || (!ch.is_alphanumeric() && ch != '_'))
}

fn unsafe_url_starts_at(value: &str, index: usize) -> bool {
    if unsafe_opaque_url_starts_at(value, index) {
        return true;
    }

    let tail = &value[index..];
    let Some(separator) = tail.find("://") else {
        return false;
    };
    if separator == 0 {
        return false;
    }
    tail[..separator]
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn unsafe_opaque_url_starts_at(value: &str, index: usize) -> bool {
    const SCHEMES: &[&str] = &["blob:", "data:", "file:", "javascript:", "mailto:"];
    let tail = &value[index..];
    ascii_token_starts_at(value, index)
        && SCHEMES.iter().any(|scheme| {
            tail.get(..scheme.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        })
}

fn ascii_token_starts_at(value: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    value[..index]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace() || (!ch.is_ascii_alphanumeric() && ch != '_'))
}

fn local_unsafe_path_starts_at(value: &str, index: usize) -> bool {
    let tail = &value[index..];
    if tail.starts_with("~/")
        || tail.starts_with("~\\")
        || starts_with_jyowo_path(tail)
        || starts_with_known_unix_absolute_root(tail)
    {
        return true;
    }
    token_starts_at(value, index) && (tail.starts_with('/') || is_windows_absolute_path(tail))
}

fn starts_with_jyowo_path(value: &str) -> bool {
    value
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(".jyowo"))
        && value
            .as_bytes()
            .get(6)
            .is_some_and(|byte| matches!(byte, b'/' | b'\\'))
}

fn is_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

fn starts_with_known_unix_absolute_root(value: &str) -> bool {
    const ROOTS: &[&str] = &[
        "/Applications",
        "/Library",
        "/System",
        "/Users",
        "/Volumes",
        "/dev",
        "/etc",
        "/home",
        "/media",
        "/mnt",
        "/opt",
        "/private",
        "/root",
        "/run",
        "/tmp",
        "/usr",
        "/var",
    ];

    ROOTS.iter().any(|root| {
        value
            .strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\'))
    })
}

fn unsafe_url_token_end(value: &str, start: usize) -> usize {
    if starts_with_unsafe_opaque_scheme(value, start, "data:")
        || starts_with_unsafe_opaque_scheme(value, start, "javascript:")
    {
        return unsafe_data_url_token_end(value, start);
    }

    unsafe_token_end(value, start)
}

fn starts_with_unsafe_opaque_scheme(value: &str, start: usize, scheme: &str) -> bool {
    ascii_token_starts_at(value, start)
        && value[start..]
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
}

fn unsafe_data_url_token_end(value: &str, start: usize) -> usize {
    value[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            (matches!(
                ch,
                '"' | '\''
                    | '`'
                    | '，'
                    | '。'
                    | '；'
                    | '、'
                    | '）'
                    | '】'
                    | '」'
                    | '》'
                    | '！'
                    | '？'
            ))
            .then_some(start + offset)
        })
        .unwrap_or(value.len())
}

fn unsafe_token_end(value: &str, start: usize) -> usize {
    value[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            (ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\''
                        | ')'
                        | ']'
                        | '}'
                        | ','
                        | ';'
                        | '<'
                        | '>'
                        | '，'
                        | '。'
                        | '；'
                        | '、'
                        | '）'
                        | '】'
                        | '」'
                        | '》'
                        | '！'
                        | '？'
                ))
            .then_some(start + offset)
        })
        .unwrap_or(value.len())
}

fn renumber_segments(assistant: &mut AssistantWork) {
    for (order, segment) in assistant.segments.iter_mut().enumerate() {
        match segment {
            AssistantSegment::Process(segment) => segment.order = order as u32,
            AssistantSegment::Text(segment) => segment.order = order as u32,
            AssistantSegment::ToolGroup(segment) => segment.order = order as u32,
            AssistantSegment::Artifact(segment) => segment.order = order as u32,
            AssistantSegment::ReviewRequest(segment) => segment.order = order as u32,
            AssistantSegment::ClarificationRequest(segment) => segment.order = order as u32,
            AssistantSegment::Notice(segment) => segment.order = order as u32,
            AssistantSegment::Error(segment) => segment.order = order as u32,
            AssistantSegment::AgentActivity(segment) => segment.order = order as u32,
        }
    }
}

fn renumber_process_steps(process: &mut ProcessSegment) {
    for (order, step) in process.steps.iter_mut().enumerate() {
        step.order = order as u32;
    }
}

fn subagent_announced_status(status: &str) -> AgentActivityStatus {
    match status {
        "completed" => AgentActivityStatus::Completed,
        "cancelled" => AgentActivityStatus::Cancelled,
        "failed" => AgentActivityStatus::Failed,
        "stalled" => AgentActivityStatus::Stalled,
        "maxIterationsReached" | "max_iterations_reached" => AgentActivityStatus::Failed,
        _ => AgentActivityStatus::Failed,
    }
}

fn subagent_termination_status(reason: &str) -> AgentActivityStatus {
    match reason {
        "naturalCompletion" | "natural_completion" => AgentActivityStatus::Completed,
        "parentCancelled" | "parent_cancelled" => AgentActivityStatus::Cancelled,
        "stalled" => AgentActivityStatus::Stalled,
        "bridgeBroken" | "bridge_broken" => AgentActivityStatus::Failed,
        "failed" => AgentActivityStatus::Failed,
        _ if reason.starts_with("adminInterrupted") || reason.starts_with("admin_interrupted") => {
            AgentActivityStatus::Cancelled
        }
        _ => AgentActivityStatus::Failed,
    }
}

fn permission_status_from_decision(payload: &Value) -> DecisionRequestStatus {
    match string_field(payload, "decision").as_deref() {
        Some("approve" | "approved" | "allow") => DecisionRequestStatus::Approved,
        Some("deny" | "denied") => DecisionRequestStatus::Denied,
        Some("failed") => DecisionRequestStatus::Failed,
        _ => DecisionRequestStatus::Denied,
    }
}

fn permission_confirmation_expected(value: &Value) -> Option<String> {
    string_field(value, "confirmationExpected")
}

fn background_agent_state_status(state: &str) -> AgentActivityStatus {
    match state {
        "queued" | "running" | "cancelling" => AgentActivityStatus::Running,
        "waiting_for_permission" => AgentActivityStatus::WaitingPermission,
        "waiting_for_input" => AgentActivityStatus::WaitingInput,
        "succeeded" => AgentActivityStatus::Completed,
        "failed" => AgentActivityStatus::Failed,
        "cancelled" | "archived" => AgentActivityStatus::Cancelled,
        "paused" | "interrupted" | "recoverable" => AgentActivityStatus::Stalled,
        _ => AgentActivityStatus::Stalled,
    }
}

fn is_terminal_agent_activity_status(status: AgentActivityStatus) -> bool {
    matches!(
        status,
        AgentActivityStatus::Completed
            | AgentActivityStatus::Failed
            | AgentActivityStatus::Cancelled
            | AgentActivityStatus::Redacted
    )
}

fn team_termination_status(reason: &str) -> AgentActivityStatus {
    match reason {
        "completed" => AgentActivityStatus::Completed,
        "cancelled" => AgentActivityStatus::Cancelled,
        "member_failed" | "memberFailed" | "timeout" | "idle_timeout" | "idleTimeout" => {
            AgentActivityStatus::Failed
        }
        _ if reason.starts_with("error") => AgentActivityStatus::Failed,
        _ => AgentActivityStatus::Failed,
    }
}

fn team_result_summary(reason: &str) -> &'static str {
    match team_termination_status(reason) {
        AgentActivityStatus::Completed => "Team completed.",
        AgentActivityStatus::Cancelled => "Team cancelled.",
        AgentActivityStatus::Failed => "Team failed.",
        _ => "Team stopped.",
    }
}

fn artifact_revision_kind_from_str(kind: &str) -> ArtifactRevisionKind {
    match kind {
        "code" => ArtifactRevisionKind::Code,
        "document" => ArtifactRevisionKind::Document,
        "image" => ArtifactRevisionKind::Image,
        "html" => ArtifactRevisionKind::Html,
        "data" => ArtifactRevisionKind::Data,
        "media" => ArtifactRevisionKind::Media,
        _ => ArtifactRevisionKind::File,
    }
}
