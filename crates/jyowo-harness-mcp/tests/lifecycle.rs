use harness_mcp::{
    InitializeResult, McpClientCapabilities, McpExpectedCapabilities, McpImplementation,
    McpLifecycleState, McpServerCapabilities, McpSession, PromptsServerCapability,
    ResourcesServerCapability, SamplingClientCapability, ToolsServerCapability,
    LATEST_PROTOCOL_VERSION,
};

fn implementation(name: &str) -> McpImplementation {
    McpImplementation::new(name, "1.0.0")
}

fn result(protocol_version: &str, capabilities: McpServerCapabilities) -> InitializeResult {
    InitializeResult {
        protocol_version: protocol_version.to_owned(),
        capabilities,
        server_info: implementation("fixture-server"),
        instructions: Some("Use fixture tools carefully".to_owned()),
        extra: Default::default(),
    }
}

#[test]
fn initialization_requests_latest_version_and_offers_only_client_capabilities() {
    let offered = McpClientCapabilities {
        sampling: Some(SamplingClientCapability::default()),
        ..Default::default()
    };
    let mut session = McpSession::new(
        McpExpectedCapabilities::default(),
        offered.clone(),
        implementation("jyowo"),
    );

    let params = session.begin_initialization().unwrap();

    assert_eq!(params.protocol_version, LATEST_PROTOCOL_VERSION);
    assert_eq!(params.capabilities, offered);
    assert!(params.capabilities.tasks.is_none());
    assert_eq!(params.client_info.name, "jyowo");
    assert_eq!(session.state(), McpLifecycleState::Initializing);
}

#[test]
fn accepts_all_supported_protocol_revisions_and_saves_negotiated_state() {
    for version in ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"] {
        let capabilities = McpServerCapabilities {
            tools: Some(ToolsServerCapability::default()),
            ..Default::default()
        };
        let mut session = McpSession::new(
            McpExpectedCapabilities::default(),
            McpClientCapabilities::default(),
            implementation("jyowo"),
        );
        session.begin_initialization().unwrap();
        session
            .accept_initialize_result(result(version, capabilities.clone()))
            .unwrap();

        assert_eq!(session.state(), McpLifecycleState::Negotiated);
        assert_eq!(session.negotiated_protocol_version(), Some(version));
        assert_eq!(session.server_capabilities(), Some(&capabilities));
        assert_eq!(session.server_info().unwrap().name, "fixture-server");
        assert_eq!(session.instructions(), Some("Use fixture tools carefully"));
    }
}

#[test]
fn rejects_unknown_protocol_revision_without_saving_negotiated_state() {
    let mut session = McpSession::new(
        McpExpectedCapabilities::default(),
        McpClientCapabilities::default(),
        implementation("jyowo"),
    );
    session.begin_initialization().unwrap();

    let error = session
        .accept_initialize_result(result("2099-01-01", McpServerCapabilities::default()))
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("unsupported MCP protocol version"));
    assert_eq!(session.state(), McpLifecycleState::Failed);
    assert_eq!(session.negotiated_protocol_version(), None);
    assert!(session.initialized_notification().is_err());
}

#[test]
fn rejects_missing_required_server_capability() {
    let required = McpExpectedCapabilities {
        tools: true,
        resources: true,
        prompts: true,
        logging: true,
        completions: true,
        tasks: true,
    };
    let mut session = McpSession::new(
        required,
        McpClientCapabilities::default(),
        implementation("jyowo"),
    );
    session.begin_initialization().unwrap();

    let error = session
        .accept_initialize_result(result(
            LATEST_PROTOCOL_VERSION,
            McpServerCapabilities {
                tools: Some(ToolsServerCapability::default()),
                resources: Some(ResourcesServerCapability::default()),
                prompts: Some(PromptsServerCapability::default()),
                ..Default::default()
            },
        ))
        .unwrap_err();

    assert!(error.to_string().contains("logging"));
    assert!(error.to_string().contains("completions"));
    assert!(error.to_string().contains("tasks"));
    assert_eq!(session.state(), McpLifecycleState::Failed);
    assert!(session.initialized_notification().is_err());
}

#[test]
fn initialized_notification_can_only_be_sent_after_successful_validation() {
    let mut session = McpSession::new(
        McpExpectedCapabilities::default(),
        McpClientCapabilities::default(),
        implementation("jyowo"),
    );

    assert!(session.initialized_notification().is_err());
    session.begin_initialization().unwrap();
    assert!(session.initialized_notification().is_err());
    session
        .accept_initialize_result(result(
            LATEST_PROTOCOL_VERSION,
            McpServerCapabilities {
                tools: Some(ToolsServerCapability::default()),
                ..Default::default()
            },
        ))
        .unwrap();

    let notification = session.initialized_notification().unwrap();
    assert_eq!(notification.method, "notifications/initialized");
    assert_eq!(session.state(), McpLifecycleState::Negotiated);

    session.mark_initialized_notification_sent().unwrap();
    assert_eq!(session.state(), McpLifecycleState::Ready);
    assert!(session.mark_initialized_notification_sent().is_err());
}
