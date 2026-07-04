use super::support::*;
use super::*;

const VALID_PERMISSION_OPTION_ID: &str = "01HZ0000000000000000000002";

#[tokio::test]
async fn resolve_permission_deny_ignores_confirmation_text() {
    let workspace = unique_workspace("confirmation-deny");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace)
        .await
        .expect("runtime state should initialize");

    // Resolve a non-existent request with deny and a confirmation text.
    // Deny decisions must not require confirmation text validation.
    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: SessionId::new().to_string(),
            decision: PermissionDecision::Deny,
            option_id: VALID_PERMISSION_OPTION_ID.to_owned(),
            request_id: "01HZ0000000000000000000001".to_owned(),
            confirmation_text: Some("DELETE".to_owned()),
        },
        &state,
    )
    .await
    .unwrap_err();

    // The request doesn't exist, so we get NOT_FOUND (not a confirmation error).
    assert_eq!(error.code, "NOT_FOUND");
    assert!(error.message.contains("permission request not found"));
}

#[tokio::test]
async fn production_runtime_wires_full_permission_authority() {
    let workspace = unique_workspace("production-authority");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .expect("runtime state should initialize");

    let harness = state
        .harness()
        .expect("production runtime should have a harness");

    // Production runtime must use the full PermissionAuthority, not stream broker alone.
    assert!(
        harness.permission_authority().is_some(),
        "production runtime must use full PermissionAuthority, not stream broker alone"
    );

    // Production runtime must also have the authorization service wired.
    let _authorization_service = harness.authorization_service();
}

#[tokio::test]
async fn resolve_permission_approve_with_correct_confirmation_text_succeeds() {
    let workspace = unique_workspace("confirmation-approve");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .expect("runtime state should initialize");
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let session_id = state.default_conversation_id();
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let mut request = permission_request();
    request.session_id = session_id;
    let expected = "DELETE".to_owned();
    request.confirmation_expected = Some(expected.clone());
    let request_id = request.request_id;
    let ctx = permission_context_for_request(&request, None);

    let decision_task = tokio::spawn(async move { broker.decide(request, ctx).await });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let payload = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id: approve_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: Some(expected),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.status, "resolved");
    assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn resolve_permission_approve_requires_confirmation_text_when_expected() {
    let workspace = unique_workspace("confirmation-missing");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .expect("runtime state should initialize");
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let session_id = state.default_conversation_id();
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let mut request = permission_request();
    request.session_id = session_id;
    request.confirmation_expected = Some("DELETE".to_owned());
    let request_id = request.request_id;
    let ctx = permission_context_for_request(&request, None);

    let decision_task = tokio::spawn(async move { broker.decide(request, ctx).await });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id: approve_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "PERMISSION_RESOLVE_FAILED");
    assert!(error.message.contains("confirmation text is required"));
    assert!(state
        .pending_permission_requests()
        .iter()
        .any(|pending| pending.request.request_id == request_id));

    // Clean up with a deny (deny does not require confirmation text).
    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            option_id: deny_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
    let _ = decision_task.await;
}

#[tokio::test]
async fn resolve_permission_approve_rejects_mismatched_confirmation_text() {
    let workspace = unique_workspace("confirmation-mismatch");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .expect("runtime state should initialize");
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let session_id = state.default_conversation_id();
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let mut request = permission_request();
    request.session_id = session_id;
    request.confirmation_expected = Some("DELETE".to_owned());
    let request_id = request.request_id;
    let ctx = permission_context_for_request(&request, None);

    let decision_task = tokio::spawn(async move { broker.decide(request, ctx).await });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id: approve_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: Some("OVERWRITE".to_owned()),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "PERMISSION_RESOLVE_FAILED");
    assert!(error.message.contains("confirmation text does not match"));
    assert!(state
        .pending_permission_requests()
        .iter()
        .any(|pending| pending.request.request_id == request_id));

    // Clean up.
    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            option_id: deny_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
    let _ = decision_task.await;
}
