//! Pure conversation worktree projection.

use std::collections::{HashMap, HashSet};

use harness_contracts::{
    AssistantSegment, AssistantWork, AssistantWorkStatus, ConversationCursor, ConversationEventRef,
    ConversationTimelineEvent, ConversationTurn, ConversationTurnUserMessage,
    ConversationWorktreePage, ErrorSegment, TextSegment, ThinkingSegment, ThinkingSegmentStatus,
    ThinkingSummary, ToolAttempt, ToolAttemptStatus, ToolGroupSegment, ToolPermissionState,
    ToolPermissionStatus, UiSafeText,
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
                state.update_tool_status(&event, event_ref, ToolAttemptStatus::Completed)
            }
            "tool.failed" => state.project_tool_failed(&event, event_ref),
            "tool.denied" => state.update_tool_status(&event, event_ref, ToolAttemptStatus::Denied),
            "permission.requested" => state.project_permission_requested(&event, event_ref),
            "permission.resolved" => state.project_permission_resolved(&event, event_ref),
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
        let message_id = string_field(&event.payload, "messageId")
            .unwrap_or_else(|| format!("event:{}", event.id));
        let position = event.cursor.conversation_sequence;
        let turn = ConversationTurn {
            id: format!("turn:{message_id}"),
            conversation_id: self.conversation_id.to_owned(),
            position,
            user: ConversationTurnUserMessage {
                id: format!("user:{message_id}"),
                message_id,
                body: ui_text(string_field(&event.payload, "body").unwrap_or_default()),
                client_message_id: string_field(&event.payload, "clientMessageId"),
                timestamp: event.timestamp,
                event_refs: vec![event_ref],
            },
            assistant: None,
        };
        let index = self.turns.len();
        self.run_turns.insert(event.run_id.clone(), index);
        self.turns.push(turn);
    }

    fn project_assistant_completed(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        self.assistant_work(event, event_ref.clone());
        if is_empty_assistant_body(&event.payload) {
            return;
        }
        let body = string_field(&event.payload, "body").unwrap_or_default();
        let message_id =
            string_field(&event.payload, "messageId").unwrap_or_else(|| event.id.clone());
        let text = TextSegment {
            id: format!("segment:text:{message_id}"),
            order: self.next_segment_order(&event.run_id),
            message_id,
            body: ui_text(body),
            event_refs: vec![event_ref],
        };
        self.push_segment(&event.run_id, AssistantSegment::Text(text));
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
        self.assistant_work(event, event_ref.clone());
        let segment = TextSegment {
            id: format!("segment:text:{}", event.id),
            order: self.next_segment_order(&event.run_id),
            message_id: event.id.clone(),
            body: ui_text(text),
            event_refs: vec![event_ref],
        };
        self.push_segment(&event.run_id, AssistantSegment::Text(segment));
    }

    fn project_thinking(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
    ) {
        self.assistant_work(event, event_ref.clone());
        let status = thinking_status_from_event(event);
        let summary = safe_thinking_display(status, safe_summary_field(&event.payload));
        let order = self.next_segment_order(&event.run_id);
        let assistant = self.assistant_work(event, event_ref.clone());
        if let Some(existing) = assistant
            .segments
            .iter_mut()
            .find_map(|segment| match segment {
                AssistantSegment::Thinking(thinking) => Some(thinking),
                _ => None,
            })
        {
            existing.status = status;
            existing.summary = summary;
            existing.event_refs.push(event_ref);
            return;
        }
        assistant.segments.insert(
            0,
            AssistantSegment::Thinking(ThinkingSegment {
                id: format!("segment:thinking:{}", event.run_id),
                order,
                status,
                summary,
                event_refs: vec![event_ref],
            }),
        );
        renumber_segments(assistant);
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
            tool_use_id,
            tool_name: ui_text(tool_name),
            status: ToolAttemptStatus::Running,
            permission: None,
            failure_summary: None,
            event_refs: vec![event_ref],
        });
    }

    fn update_tool_status(
        &mut self,
        event: &ConversationTimelineEvent,
        event_ref: ConversationEventRef,
        status: ToolAttemptStatus,
    ) {
        let Some(tool_use_id) = string_field(&event.payload, "toolUseId") else {
            return;
        };
        if let Some(attempt) = self.tool_attempt_mut(&event.run_id, &tool_use_id) {
            attempt.status = status;
            attempt.event_refs.push(event_ref);
        }
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
            attempt.event_refs.push(event_ref);
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
            .or_else(|| self.latest_tool_use_id(&event.run_id));
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
        let message_id = format!("synthetic:{}", event.run_id);
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
                timestamp: event.timestamp,
                event_refs: Vec::new(),
            },
            assistant: None,
        });
        self.run_turns.insert(event.run_id.clone(), index);
        index
    }

    fn push_segment(&mut self, run_id: &str, segment: AssistantSegment) {
        if let Some(index) = self.run_turns.get(run_id).copied() {
            if let Some(assistant) = self.turns[index].assistant.as_mut() {
                assistant.segments.push(segment);
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

    fn latest_tool_use_id(&self, run_id: &str) -> Option<String> {
        let index = self.run_turns.get(run_id).copied()?;
        let assistant = self.turns[index].assistant.as_ref()?;
        assistant
            .segments
            .iter()
            .rev()
            .find_map(|segment| match segment {
                AssistantSegment::ToolGroup(group) => group
                    .attempts
                    .iter()
                    .rev()
                    .find(|attempt| attempt.permission.is_none())
                    .map(|attempt| attempt.tool_use_id.clone()),
                _ => None,
            })
    }
}

#[must_use]
pub fn safe_tool_failure_summary(_event: &ConversationTimelineEvent) -> UiSafeText {
    ui_text("工具执行失败。详情可在 Activity 中查看。")
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
pub fn is_empty_assistant_body(value: &Value) -> bool {
    string_field(value, "body").is_none_or(|body| body.trim().is_empty())
}

fn event_ref(event: &ConversationTimelineEvent) -> ConversationEventRef {
    ConversationEventRef {
        event_id: event.id.clone(),
        cursor: event.cursor,
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn safe_summary_field(value: &Value) -> Option<UiSafeText> {
    string_field(value, "safeSummary").map(ui_text)
}

fn ui_text(value: impl AsRef<str>) -> UiSafeText {
    UiSafeText::from_trusted_redacted(value.as_ref())
}

fn renumber_segments(assistant: &mut AssistantWork) {
    for (order, segment) in assistant.segments.iter_mut().enumerate() {
        match segment {
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
