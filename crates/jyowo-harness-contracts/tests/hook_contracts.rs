use harness_contracts::{export_all_schemas, HookEventKind, HookOutcomeDiscriminant};

#[test]
fn hook_event_kind_contract_covers_twenty_standard_events() {
    let kinds = [
        HookEventKind::UserPromptSubmit,
        HookEventKind::PreToolUse,
        HookEventKind::PostToolUse,
        HookEventKind::PostToolUseFailure,
        HookEventKind::PermissionRequest,
        HookEventKind::SessionStart,
        HookEventKind::Setup,
        HookEventKind::SessionEnd,
        HookEventKind::SubagentStart,
        HookEventKind::SubagentStop,
        HookEventKind::Notification,
        HookEventKind::PreLlmCall,
        HookEventKind::PostLlmCall,
        HookEventKind::PreApiRequest,
        HookEventKind::PostApiRequest,
        HookEventKind::TransformToolResult,
        HookEventKind::TransformTerminalOutput,
        HookEventKind::Elicitation,
        HookEventKind::PreToolSearch,
        HookEventKind::PostToolSearchMaterialize,
    ];

    assert_eq!(kinds.len(), 20);
}

#[test]
fn hook_outcome_discriminants_cover_public_hook_shapes() {
    let kinds = [
        HookOutcomeDiscriminant::Continue,
        HookOutcomeDiscriminant::Block,
        HookOutcomeDiscriminant::PreToolUse,
        HookOutcomeDiscriminant::RewriteInput,
        HookOutcomeDiscriminant::OverridePermission,
        HookOutcomeDiscriminant::AddContext,
        HookOutcomeDiscriminant::Transform,
    ];

    assert_eq!(kinds.len(), 7);
}

#[test]
fn hook_contract_schemas_are_exported() {
    let schemas = export_all_schemas();

    assert!(schemas.contains_key("hook_event_kind"));
    assert!(schemas.contains_key("hook_outcome_discriminant"));
}
