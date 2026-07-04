//! Contract tests for the conversation workbench projection.
//!
//! These tests assert the typed workbench projection shape required by
//! docs/superpowers/plans/2026-07-04-agent-workbench-conversation-redesign.md.

use harness_contracts::*;
use serde_json::json;

#[test]
fn conversation_worktree_page_contains_typed_decision_tool_command_diff_and_artifact_shapes() {
    let page: ConversationWorktreePage =
        serde_json::from_str(include_str!("fixtures/conversation_worktree_page.json")).unwrap();
    let assistant = page.turns[0].assistant.as_ref().unwrap();
    assert!(assistant.projection_version > 0);

    let decision = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::ToolGroup(group) => group
                .attempts
                .iter()
                .find_map(|attempt| attempt.permission.as_ref()),
            _ => None,
        })
        .expect("fixture must include a backend-authored decision request");
    assert!(!decision.decision_options.is_empty());
    assert!(decision
        .decision_options
        .iter()
        .all(|option| !option.id.is_empty()));
    assert!(decision
        .decision_options
        .iter()
        .all(|option| option.id != "approve" && option.id != "deny"));

    let command = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Process(process) => process.steps.iter().find_map(|step| match step
                .detail
                .as_ref()?
            {
                ProcessStepDetail::Command(command) => Some(command),
                _ => None,
            }),
            _ => None,
        })
        .expect("fixture must include command execution evidence");
    assert!(command.truncated);
    assert!(
        command.full_output_ref.is_none(),
        "Task 1 must not mint refs before EvidenceRefStore exists"
    );

    let change_set = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Process(process) => process.steps.iter().find_map(|step| match step
                .detail
                .as_ref()?
            {
                ProcessStepDetail::Diff(change_set) => Some(change_set),
                _ => None,
            }),
            _ => None,
        })
        .expect("fixture must include changeset evidence");
    assert!(change_set
        .files
        .iter()
        .all(|file| file.full_patch_ref.is_none()));

    let artifact = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Artifact(artifact) => Some(&artifact.revision),
            _ => None,
        })
        .expect("fixture must include an artifact revision summary");
    assert!(!artifact.revision_id.as_str().is_empty());
}

#[test]
fn conversation_worktree_page_rejects_legacy_thinking_segment() {
    let raw = r#"{
      "turns":[{
        "id":"turn-1","conversationId":"conversation-1","position":1,
        "user":{"id":"user-1","messageId":"message-1","body":"hi","timestamp":"1970-01-01T00:00:00Z"},
        "assistant":{"id":"assistant-1","runId":"run-1","status":"running","projectionVersion":1,"segments":[
          {"kind":"thinking","id":"thinking-1","order":0,"status":"running","summary":{"text":"raw thought"}}
        ]}
      }],
      "pageCursor":null,"eventCursor":null,"hasMoreBefore":false,"hasMoreAfter":false,"gap":false
    }"#;
    assert!(serde_json::from_str::<ConversationWorktreePage>(raw).is_err());
}

#[test]
fn decision_request_state_has_backend_authored_decision_options() {
    let option = DecisionOption {
        id: "opt_01HZ0000000000000000000001".to_owned(),
        decision: DecisionKind::Approve,
        label: "Allow this command once".to_owned(),
        lifetime: DecisionLifetime::Once,
        matcher: DecisionMatcherSummary {
            kind: DecisionMatcherKind::ExactCommand,
            label: "cargo test".to_owned(),
        },
        requires_confirmation: false,
    };

    let value = serde_json::to_value(&option).unwrap();
    assert_eq!(value["id"], "opt_01HZ0000000000000000000001");
    assert_eq!(value["decision"], "approve");
    assert_eq!(value["lifetime"], "once");
    assert_eq!(value["matcher"]["kind"], "exactCommand");

    let roundtrip: DecisionOption = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, option);
}

#[test]
fn command_execution_contract_uses_stable_wire_shape() {
    let cmd = CommandExecution {
        command: "cargo test".to_owned(),
        cwd: Some("/workspace".to_owned()),
        shell: None,
        sandbox: Some("local".to_owned()),
        approval_request_id: Some("request-1".to_owned()),
        exit_code: Some(0),
        duration_ms: Some(1200),
        stdout_preview: Some("test result: ok".to_owned()),
        stderr_preview: None,
        full_output_ref: None,
        truncated: true,
        redaction_state: EvidenceRedactionState::Clean,
        risk_level: RiskLevel::Low,
    };

    let value = serde_json::to_value(&cmd).unwrap();
    assert_eq!(value["command"], "cargo test");
    assert_eq!(value["cwd"], "/workspace");
    assert_eq!(value["exitCode"], 0);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["redactionState"], "clean");

    let roundtrip: CommandExecution = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, cmd);
}

#[test]
fn change_set_contract_uses_stable_wire_shape() {
    let cs = ChangeSet {
        id: "changeset-1".to_owned(),
        summary: "Updated 2 files".to_owned(),
        files: vec![
            ChangeSetFile {
                path: "src/main.rs".to_owned(),
                old_path: None,
                status: ChangeSetFileStatus::Modified,
                added_lines: 5,
                removed_lines: 2,
                preview: Some("+ fn main() {".to_owned()),
                full_patch_ref: None,
                risk_flags: vec![],
            },
            ChangeSetFile {
                path: "src/old.rs".to_owned(),
                old_path: None,
                status: ChangeSetFileStatus::Deleted,
                added_lines: 0,
                removed_lines: 10,
                preview: None,
                full_patch_ref: None,
                risk_flags: vec![ChangeSetRiskFlag::Delete],
            },
        ],
    };

    let value = serde_json::to_value(&cs).unwrap();
    assert_eq!(value["files"][0]["path"], "src/main.rs");
    assert_eq!(value["files"][0]["status"], "modified");
    assert_eq!(value["files"][0]["addedLines"], 5);
    assert_eq!(value["files"][1]["status"], "deleted");
    assert_eq!(value["files"][1]["riskFlags"][0], "delete");

    let roundtrip: ChangeSet = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, cs);
}

#[test]
fn artifact_revision_summary_has_required_revision_id() {
    let revision = ArtifactRevisionSummary {
        artifact_id: "artifact-1".to_owned(),
        revision_id: "rev-01HZ0000000000000000000001".to_owned(),
        kind: ArtifactRevisionKind::Image,
        status: ArtifactRevisionStatus::Ready,
        source_run_id: "run-1".to_owned(),
        title: "Generated image".to_owned(),
        summary: Some("Image artifact ready".to_owned()),
        preview_ref: None,
        content_ref: None,
        media: Some(ArtifactMediaPreview {
            kind: ArtifactMediaKind::Image,
            mime_type: "image/png".to_owned(),
            size_bytes: 128,
        }),
    };

    let value = serde_json::to_value(&revision).unwrap();
    assert_eq!(value["artifactId"], "artifact-1");
    assert_eq!(value["revisionId"], "rev-01HZ0000000000000000000001");
    assert_eq!(value["kind"], "image");
    assert_eq!(value["status"], "ready");

    let roundtrip: ArtifactRevisionSummary = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, revision);
}

#[test]
fn evidence_ref_summary_contract_uses_stable_wire_shape() {
    let summary = EvidenceRefSummary {
        id: EvidenceRefId::new("ev-ref-1"),
        kind: EvidenceRefKind::CommandOutput,
        content_type: "text/plain".to_owned(),
        byte_length: 1024,
        truncated: true,
        redaction_state: EvidenceRedactionState::Redacted,
        source_event_refs: vec![],
    };

    let value = serde_json::to_value(&summary).unwrap();
    assert_eq!(value["kind"], "commandOutput");
    assert_eq!(value["contentType"], "text/plain");
    assert_eq!(value["byteLength"], 1024);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["redactionState"], "redacted");

    let roundtrip: EvidenceRefSummary = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, summary);
}

#[test]
fn assistant_work_requires_projection_version() {
    let raw_no_version = json!({
        "id": "assistant-1",
        "runId": "run-1",
        "status": "running",
        "segments": []
    });
    assert!(serde_json::from_value::<AssistantWork>(raw_no_version).is_err());

    let raw_with_version = json!({
        "id": "assistant-1",
        "runId": "run-1",
        "status": "running",
        "projectionVersion": 1,
        "segments": []
    });
    assert!(serde_json::from_value::<AssistantWork>(raw_with_version).is_ok());
}

#[test]
fn tool_attempt_has_expanded_fields() {
    let attempt = ToolAttempt {
        id: "tool-1".to_owned(),
        order: 0,
        tool_use_id: "tool-use-1".to_owned(),
        tool_name: "shell".to_owned(),
        origin: ToolAttemptOrigin::Builtin,
        status: ToolAttemptStatus::Completed,
        arguments_preview: Some(r#"{"command":"cargo test"}"#.to_owned()),
        output_summary: Some("test result: ok".to_owned()),
        affected_targets: vec!["src/main.rs".to_owned()],
        started_at: None,
        ended_at: None,
        duration_ms: Some(1200),
        retry_of: None,
        failure_phase: None,
        failure_summary: None,
        permission: None,
        event_refs: vec![],
    };

    let value = serde_json::to_value(&attempt).unwrap();
    assert_eq!(value["toolName"], "shell");
    assert_eq!(value["origin"], "builtin");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["argumentsPreview"], r#"{"command":"cargo test"}"#);
    assert_eq!(value["outputSummary"], "test result: ok");
    assert_eq!(value["affectedTargets"][0], "src/main.rs");

    let roundtrip: ToolAttempt = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, attempt);
}

#[test]
fn permission_decision_option_has_opaque_backend_id() {
    let option = PermissionDecisionOption {
        option_id: PermissionOptionId::new(),
        decision: Decision::AllowOnce,
        scope: DecisionScope::ExactCommand {
            command: "cargo test".to_owned(),
            cwd: None,
        },
        lifetime: DecisionLifetime::Once,
        matcher_summary: DecisionMatcherSummary {
            kind: DecisionMatcherKind::ExactCommand,
            label: "cargo test".to_owned(),
        },
        label: "Allow this command once".to_owned(),
        requires_confirmation: false,
        action_plan_hash: ActionPlanHash::default(),
        fingerprint: None,
    };

    let value = serde_json::to_value(&option).unwrap();
    assert!(!value["option_id"].as_str().unwrap().is_empty());
    assert_eq!(value["decision"], "allow_once");
    assert_eq!(value["lifetime"], "once");
    assert_eq!(value["matcher_summary"]["kind"], "exactCommand");

    let roundtrip: PermissionDecisionOption = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, option);
}

#[test]
fn process_step_has_ui_visibility() {
    let step = ProcessStep {
        id: "step-1".to_owned(),
        order: 0,
        kind: ProcessStepKind::Reasoning,
        status: ProcessStepStatus::Complete,
        title: UiSafeText::from_trusted_redacted("分析请求"),
        body: Some(UiSafeText::from_trusted_redacted("确认需要生成图片")),
        detail: None,
        visibility: UiVisibility::UserSafe,
        event_refs: vec![],
    };

    let value = serde_json::to_value(&step).unwrap();
    assert_eq!(value["visibility"], "userSafe");
    assert!(serde_json::from_value::<ProcessStep>(value).is_ok());
}

#[test]
fn schema_export_excludes_legacy_thinking_types() {
    let schemas = export_all_schemas();

    // New workbench types must be present
    for key in [
        "decision_request_state",
        "decision_option",
        "decision_lifetime",
        "decision_matcher_summary",
        "command_execution",
        "change_set",
        "change_set_file",
        "artifact_revision_summary",
        "evidence_ref_summary",
        "evidence_ref_kind",
        "evidence_redaction_state",
    ] {
        assert!(schemas.contains_key(key), "missing workbench schema: {key}");
    }

    // Legacy thinking types must be absent
    for key in [
        "thinking_segment",
        "thinking_summary",
        "thinking_step",
        "thinking_step_kind",
        "thinking_step_status",
    ] {
        assert!(
            !schemas.contains_key(key),
            "legacy thinking schema must not be exported: {key}"
        );
    }

    // Legacy permission state must be absent
    assert!(
        !schemas.contains_key("tool_permission_state"),
        "legacy tool_permission_state must not be exported"
    );
}
