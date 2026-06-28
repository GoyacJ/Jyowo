//! Pure conversation worktree projection.

use std::collections::{HashMap, HashSet};

use harness_contracts::{
    ArtifactMediaKind, ArtifactMediaPreview, ArtifactSegment, ArtifactSource, ArtifactStatus,
    AssistantNoticeCode, AssistantSegment, AssistantWork, AssistantWorkStatus, BlobId, BlobRef,
    ClarificationRequestSegment, ConversationAttachmentReference, ConversationCursor,
    ConversationEventRef, ConversationTimelineEvent, ConversationTurn, ConversationTurnUserMessage,
    ConversationWorktreePage, ErrorSegment, NoticeSegment, ProcessDiffFile, ProcessSegment,
    ProcessSegmentStatus, ProcessStep, ProcessStepDetail, ProcessStepKind, ProcessStepStatus,
    ReviewRequestSegment, TextSegment, ThinkingSegmentStatus, ThinkingSummary, ToolAttempt,
    ToolAttemptStatus, ToolGroupSegment, ToolPermissionState, ToolPermissionStatus, UiSafeText,
};
use serde_json::Value;

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
                    state.prune_completed_tool_attempt(&event.run_id);
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
            _ => {}
        }
    }

    ConversationWorktreeProjection {
        turns: state.turns,
        event_cursor: state.event_cursor,
        event_refs: state.event_refs,
    }
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

struct ProjectionState<'a> {
    conversation_id: &'a str,
    turns: Vec<ConversationTurn>,
    run_turns: HashMap<String, usize>,
    request_tools: HashMap<String, String>,
    seen_event_ids: HashSet<String>,
    event_cursor: Option<ConversationCursor>,
    event_refs: Vec<ConversationEventRef>,
}

impl ProjectionState<'_> {
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
        let status = thinking_status_from_event(event);
        let summary = process_summary_from_thinking(status, safe_summary_field(&event.payload));
        let safe_summary_delta = safe_summary_delta_field(&event.payload);
        self.ensure_process_segment(
            event,
            event_ref.clone(),
            process_status_from_thinking(status),
            summary,
        );
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
            string_field(&event.payload, "toolName").unwrap_or_else(|| "Tool".to_owned());
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
            tool_name: ui_text(&tool_name),
            status: ToolAttemptStatus::Running,
            permission: None,
            failure_summary: None,
            event_refs: vec![event_ref.clone()],
        });
        self.append_tool_process_step(
            event,
            event_ref,
            ProcessStepStatus::Running,
            tool_step_title(&tool_name, ToolProcessPhase::Requested),
            tool_name,
        );
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
        event_ref: ConversationEventRef,
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
        if let Some(attempt) = self.tool_attempt_mut(&event.run_id, &tool_use_id) {
            attempt.status = ToolAttemptStatus::WaitingPermission;
            attempt.permission = Some(ToolPermissionState {
                id: format!("permission:{request_id}"),
                request_id,
                tool_use_id,
                status: ToolPermissionStatus::Pending,
                summary: string_field(&event.payload, "reason").map(ui_text),
                event_refs: vec![event_ref],
            });
        }
    }

    fn project_permission_resolved(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        let Some(request_id) = string_field(&event.payload, "requestId") else {
            return;
        };
        let Some(tool_use_id) = self.request_tools.get(&request_id).cloned() else {
            return;
        };
        let status = match string_field(&event.payload, "decision").as_deref() {
            Some("approve" | "approved" | "allow") => ToolPermissionStatus::Approved,
            Some("deny" | "denied") => ToolPermissionStatus::Denied,
            Some("failed") => ToolPermissionStatus::Failed,
            _ => ToolPermissionStatus::Denied,
        };
        if let Some(attempt) = self.tool_attempt_mut(&event.run_id, &tool_use_id) {
            if matches!(attempt.status, ToolAttemptStatus::WaitingPermission) {
                attempt.status = ToolAttemptStatus::Running;
            }
            if let Some(permission) = attempt.permission.as_mut() {
                permission.status = status;
                permission.event_refs.push(event_ref);
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
                    .and_then(|artifact| artifact.media.clone())
            })
            .or_else(|| {
                existing_process_snapshot
                    .as_ref()
                    .map(|(_, media)| media.clone())
            });
        if is_ready_image_artifact(status, media.as_ref()) {
            self.remove_artifact_segment(&event.run_id, &artifact_id);
            self.remove_redacted_text_segments(&event.run_id);
            self.append_artifact_process_step(event, artifact_id, title, media);
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
            existing.media = media.clone();
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
                title: ui_text(title.unwrap_or_else(|| "Artifact".to_owned())),
                summary,
                media: media.clone(),
                event_refs: vec![event_ref],
            }));
    }

    fn append_artifact_process_step(
        &mut self,
        event: &ConversationTimelineEvent,
        artifact_id: String,
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
            Some(ProcessStepDetail::Artifact { artifact_id, media }),
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
                    process.status = ProcessSegmentStatus::Complete;
                    process.summary = ui_text("已完成工作过程");
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
                body: ui_text("执行失败。详情可在 Activity 中查看。"),
                event_refs: vec![event_ref],
            }));
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
                    if is_ready_image_artifact(artifact.status, artifact.media.as_ref())
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

    fn prune_completed_tool_attempt(&mut self, run_id: &str) {
        let Some(index) = self.run_turns.get(run_id).copied() else {
            return;
        };
        let Some(assistant) = self.turns[index].assistant.as_mut() else {
            return;
        };
        assistant.segments.retain_mut(|segment| {
            let AssistantSegment::ToolGroup(group) = segment else {
                return true;
            };
            group
                .attempts
                .retain(|attempt| !matches!(attempt.status, ToolAttemptStatus::Completed));
            !group.attempts.is_empty()
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
}

#[must_use]
pub fn safe_tool_failure_summary(_event: &ConversationTimelineEvent) -> UiSafeText {
    ui_text("工具执行失败。详情可在 Activity 中查看。")
}

fn merge_process_step_detail(
    existing: Option<&ProcessStepDetail>,
    incoming: Option<ProcessStepDetail>,
) -> Option<ProcessStepDetail> {
    match (existing, incoming) {
        (
            Some(ProcessStepDetail::Command {
                command: existing_command,
                output: existing_output,
                exit_code: existing_exit_code,
                duration_ms: existing_duration_ms,
            }),
            Some(ProcessStepDetail::Command {
                command,
                output,
                exit_code,
                duration_ms,
            }),
        ) => Some(ProcessStepDetail::Command {
            command: if command.as_str() == "命令内容已隐藏" {
                existing_command.clone()
            } else {
                command
            },
            output: output.or_else(|| existing_output.clone()),
            exit_code: exit_code.or(*existing_exit_code),
            duration_ms: duration_ms.or(*existing_duration_ms),
        }),
        (_, Some(incoming)) => Some(incoming),
        (Some(existing), None) => Some(existing.clone()),
        (None, None) => None,
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
        .filter_map(process_diff_file_from_payload)
        .collect::<Vec<_>>();
    (!files.is_empty()).then_some(ProcessStepDetail::Diff { files })
}

fn process_diff_file_from_payload(value: &Value) -> Option<ProcessDiffFile> {
    let path = string_field(value, "path").map(ui_text)?;
    let added_lines = u32_field(value, "addedLines").or_else(|| u32_field(value, "added_lines"))?;
    let removed_lines =
        u32_field(value, "removedLines").or_else(|| u32_field(value, "removed_lines"))?;
    let preview = string_field(value, "preview").map(ui_text);
    Some(ProcessDiffFile {
        path,
        added_lines,
        removed_lines,
        preview,
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
        ProcessStepKind::Command => Some(ProcessStepDetail::Command {
            command: ui_text(
                string_field(&event.payload, "command")
                    .unwrap_or_else(|| "命令内容已隐藏".to_owned()),
            ),
            output: string_field(&event.payload, "outputSummary").and_then(|summary| {
                (summary != "Output withheld from conversation timeline.")
                    .then_some(ui_text(summary))
            }),
            exit_code: i32_field(&event.payload, "exitCode"),
            duration_ms: u64_field(&event.payload, "durationMs"),
        }),
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
pub fn thinking_status_from_event(event: &ConversationTimelineEvent) -> ThinkingSegmentStatus {
    if event.visibility == "withheld" {
        return ThinkingSegmentStatus::Withheld;
    }
    match string_field(&event.payload, "status").as_deref() {
        Some("complete" | "completed") => ThinkingSegmentStatus::Complete,
        Some("withheld") => ThinkingSegmentStatus::Withheld,
        _ => ThinkingSegmentStatus::Running,
    }
}

#[must_use]
pub fn safe_thinking_display(
    status: ThinkingSegmentStatus,
    explicit_safe_summary: Option<UiSafeText>,
) -> ThinkingSummary {
    if let Some(text) = explicit_safe_summary {
        return ThinkingSummary { text };
    }
    let text = match status {
        ThinkingSegmentStatus::Running => "正在处理请求",
        ThinkingSegmentStatus::Complete => "已完成思考摘要",
        ThinkingSegmentStatus::Withheld => "思考内容已折叠",
    };
    ThinkingSummary {
        text: ui_text(text),
    }
}

#[must_use]
pub fn process_summary_from_thinking(
    status: ThinkingSegmentStatus,
    explicit_safe_summary: Option<UiSafeText>,
) -> UiSafeText {
    if let Some(text) = explicit_safe_summary {
        return text;
    }
    let text = match status {
        ThinkingSegmentStatus::Running => "正在处理请求",
        ThinkingSegmentStatus::Complete => "已完成工作过程",
        ThinkingSegmentStatus::Withheld => "过程内容已折叠",
    };
    ui_text(text)
}

#[must_use]
pub fn process_status_from_thinking(status: ThinkingSegmentStatus) -> ProcessSegmentStatus {
    match status {
        ThinkingSegmentStatus::Running => ProcessSegmentStatus::Running,
        ThinkingSegmentStatus::Complete => ProcessSegmentStatus::Complete,
        ThinkingSegmentStatus::Withheld => ProcessSegmentStatus::Withheld,
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
            AssistantSegment::Thinking(segment) => segment.order = order as u32,
            AssistantSegment::Text(segment) => segment.order = order as u32,
            AssistantSegment::ToolGroup(segment) => segment.order = order as u32,
            AssistantSegment::Artifact(segment) => segment.order = order as u32,
            AssistantSegment::ReviewRequest(segment) => segment.order = order as u32,
            AssistantSegment::ClarificationRequest(segment) => segment.order = order as u32,
            AssistantSegment::Notice(segment) => segment.order = order as u32,
            AssistantSegment::Error(segment) => segment.order = order as u32,
        }
    }
}

fn renumber_process_steps(process: &mut ProcessSegment) {
    for (order, step) in process.steps.iter_mut().enumerate() {
        step.order = order as u32;
    }
}
