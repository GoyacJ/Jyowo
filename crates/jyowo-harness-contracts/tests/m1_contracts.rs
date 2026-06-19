use bytes::Bytes;
use futures::{future::BoxFuture, stream::BoxStream};
use harness_contracts::*;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn ids_roundtrip_and_tenant_sentinels_are_stable() {
    let session = SessionId::new();
    let encoded = session.to_string();
    let parsed = SessionId::parse(&encoded).expect("session id parses");

    assert_eq!(session, parsed);
    assert_eq!(
        serde_json::from_str::<SessionId>(&serde_json::to_string(&session).unwrap()).unwrap(),
        session
    );
    assert_ne!(TenantId::SINGLE, TenantId::SHARED);
    assert_eq!(
        TenantId::parse(&TenantId::SINGLE.to_string()).unwrap(),
        TenantId::SINGLE
    );
}

#[test]
fn key_events_serialize_with_type_tag() {
    let event = Event::RunEnded(RunEndedEvent {
        run_id: RunId::new(),
        reason: EndReason::Cancelled {
            initiator: CancelInitiator::User,
        },
        usage: None,
        ended_at: chrono::Utc::now(),
    });

    let value = serde_json::to_value(event).unwrap();
    assert_eq!(value["type"], "run_ended");

    let post_execution_failure =
        Event::SandboxPostExecutionFailed(SandboxPostExecutionFailedEvent {
            session_id: SessionId::new(),
            run_id: RunId::new(),
            tool_use_id: Some(ToolUseId::new()),
            backend_id: "ssh".to_owned(),
            error: SandboxError::Message("cleanup failed".to_owned()),
            at: chrono::Utc::now(),
        });
    let value = serde_json::to_value(post_execution_failure).unwrap();
    assert_eq!(value["type"], "sandbox_post_execution_failed");

    let backend_failure = Event::SandboxBackendFailed(SandboxBackendFailedEvent {
        session_id: SessionId::new(),
        run_id: RunId::new(),
        tool_use_id: Some(ToolUseId::new()),
        backend_id: "local".to_owned(),
        phase: SandboxBackendFailurePhase::Execute,
        error: SandboxError::Message("spawn failed".to_owned()),
        at: chrono::Utc::now(),
    });
    let value = serde_json::to_value(backend_failure).unwrap();
    assert_eq!(value["type"], "sandbox_backend_failed");
    assert_eq!(value["phase"], "execute");

    let grace = GraceCallTriggeredEvent {
        run_id: RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        current_iteration: 4,
        max_iterations: 3,
        usage_snapshot: UsageSnapshot::default(),
        at: chrono::Utc::now(),
        correlation_id: CorrelationId::new(),
    };
    assert_eq!(grace.current_iteration, 4);
}

#[test]
fn redactor_is_dyn_safe_and_noop_preserves_input() {
    let redactor: &dyn Redactor = &NoopRedactor;
    assert_eq!(redactor.redact("secret", &RedactRules::default()), "secret");
}

#[test]
fn schema_export_contains_required_surface() {
    let schemas = export_all_schemas();

    assert!(schemas.len() >= 60);
    assert!(schemas.contains_key("event"));
    assert!(schemas.contains_key("tool_descriptor"));
    assert!(schemas.contains_key("tool_use_requested"));
    assert!(schemas.contains_key("credential_pool_shared_across_tenants"));
    assert!(schemas.contains_key("manifest_validation_failed"));
    assert!(schemas.contains_key("hook_failed"));
    assert!(schemas.contains_key("clarify_prompt"));
    assert!(schemas.contains_key("user_message_delivery"));
    assert!(schemas.contains_key("skill_filter"));
    assert!(schemas.contains_key("skill_summary"));
    assert!(schemas.contains_key("skill_status"));
    assert!(schemas.contains_key("skill_view"));
    assert!(schemas.contains_key("skill_parameter_info"));
    assert!(schemas.contains_key("skill_injection_id"));
    assert!(schemas.contains_key("skill_invocation_receipt"));
    assert!(schemas.contains_key("rendered_skill"));
    assert!(schemas.contains_key("skill_shell_invocation"));
    assert!(schemas.contains_key("sandbox_post_execution_failed"));
    assert!(schemas.contains_key("sandbox_backend_failed"));

    for key in [
        "run_started",
        "run_ended",
        "assistant_delta_produced",
        "assistant_message_completed",
        "tool_use_requested",
        "tool_use_approved",
        "tool_use_denied",
        "tool_use_completed",
        "tool_use_failed",
        "permission_requested",
        "permission_resolved",
        "engine_failed",
    ] {
        assert!(schemas.contains_key(key), "missing MVP event schema: {key}");
    }
}

#[test]
fn tool_descriptor_is_contract_surface() {
    let descriptor = ToolDescriptor {
        name: "read_file".to_owned(),
        display_name: "Read file".to_owned(),
        description: "Read a workspace file".to_owned(),
        category: "filesystem".to_owned(),
        group: ToolGroup::FileSystem,
        version: "1.0.0".to_owned(),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: None,
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities: vec![ToolCapability::BlobReader],
        budget: ResultBudget {
            metric: BudgetMetric::Chars,
            limit: 8192,
            on_overflow: OverflowAction::Offload,
            preview_head_chars: 1024,
            preview_tail_chars: 1024,
        },
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: Some("read file path".to_owned()),
    };

    let value = serde_json::to_value(&descriptor).unwrap();
    assert_eq!(value["name"], "read_file");

    let roundtrip: ToolDescriptor = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, descriptor);
}

#[test]
fn model_error_variants_are_contract_surface() {
    let error = ModelError::ContextTooLong {
        tokens: 200_000,
        max: 128_000,
    };

    let value = serde_json::to_value(&error).unwrap();
    assert_eq!(value["context_too_long"]["tokens"], 200_000);

    let roundtrip: ModelError = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, error);
    assert_eq!(
        ModelError::AuxModelNotConfigured.to_string(),
        "aux model not configured"
    );
}

#[test]
fn sandbox_error_variants_are_contract_surface() {
    let cases = [
        SandboxError::Unavailable {
            backend: "docker".to_owned(),
            detail: "missing binary".to_owned(),
        },
        SandboxError::CapabilityMismatch {
            capability: "network".to_owned(),
            detail: "unsupported".to_owned(),
        },
        SandboxError::Timeout {
            detail: "wall clock exceeded".to_owned(),
        },
        SandboxError::InactivityTimeout {
            detail: "no output observed".to_owned(),
        },
        SandboxError::OutputBudgetExceeded { limit: 3 },
        SandboxError::HostPathDenied {
            path: "/secret".to_owned(),
        },
        SandboxError::ResourceLimitExceeded {
            limit: "memory".to_owned(),
            detail: "unsupported".to_owned(),
        },
        SandboxError::SnapshotUnsupported {
            kind: "shell_state".to_owned(),
        },
        SandboxError::ContainerLifecycleError {
            detail: "container failed".to_owned(),
        },
        SandboxError::CodeRuntime {
            detail: "forbidden symbol".to_owned(),
        },
    ];

    for error in cases {
        let value = serde_json::to_value(&error).unwrap();
        let roundtrip: SandboxError = serde_json::from_value(value).unwrap();
        assert_eq!(roundtrip, error);
    }
}

struct TestBlobReaderCap;

impl BlobReaderCap for TestBlobReaderCap {
    fn read_blob(
        &self,
        _tenant_id: TenantId,
        _blob: BlobRef,
    ) -> BoxFuture<'_, Result<BoxStream<'static, Bytes>, ToolError>> {
        Box::pin(async { Ok(Box::pin(futures::stream::empty()) as BoxStream<'static, Bytes>) })
    }
}

struct TestSkillRegistryCap;

impl SkillRegistryCap for TestSkillRegistryCap {
    fn list_summaries(&self, _agent: &AgentId, _filter: SkillFilter) -> Vec<SkillSummary> {
        Vec::new()
    }

    fn view(&self, _agent: &AgentId, _name: &str, _full: bool) -> Option<SkillView> {
        None
    }

    fn render(
        &self,
        _agent: &AgentId,
        name: String,
        _params: serde_json::Value,
    ) -> BoxFuture<'static, Result<RenderedSkill, ToolError>> {
        Box::pin(async move { Err(ToolError::Validation(format!("skill not found: {name}"))) })
    }
}

#[test]
fn capability_registry_stores_and_recovers_dyn_capabilities() {
    let mut registry = CapabilityRegistry::default();
    let cap: Arc<dyn BlobReaderCap> = Arc::new(TestBlobReaderCap);

    registry.install(ToolCapability::BlobReader, Arc::clone(&cap));

    let recovered = registry
        .get::<dyn BlobReaderCap>(&ToolCapability::BlobReader)
        .expect("installed capability is available");

    assert!(Arc::ptr_eq(&cap, &recovered));
    assert!(registry
        .get::<dyn BlobReaderCap>(&ToolCapability::SubagentRunner)
        .is_none());
}

#[test]
fn capability_registry_stores_and_recovers_skill_registry_capability() {
    let mut registry = CapabilityRegistry::default();
    let cap: Arc<dyn SkillRegistryCap> = Arc::new(TestSkillRegistryCap);

    registry.install(ToolCapability::SkillRegistry, Arc::clone(&cap));

    let recovered = registry
        .get::<dyn SkillRegistryCap>(&ToolCapability::SkillRegistry)
        .expect("installed skill registry capability is available");

    assert!(Arc::ptr_eq(&cap, &recovered));
}

#[test]
fn tool_error_variants_cover_m3_tool_surface() {
    let missing = ToolError::CapabilityMissing(ToolCapability::BlobReader);
    assert_eq!(
        missing.to_string(),
        "required capability missing: blob_reader"
    );

    let too_large = ToolError::ResultTooLarge {
        original: 4096,
        limit: 1024,
        metric: BudgetMetric::Bytes,
    };
    let value = serde_json::to_value(&too_large).unwrap();

    assert_eq!(value["result_too_large"]["original"], 4096);
    assert_eq!(value["result_too_large"]["metric"], "bytes");
}

#[test]
fn tool_result_part_uses_semantic_whitelist_shape() {
    let part = ToolResultPart::Structured {
        value: json!({ "ok": true }),
        schema_ref: Some("#/properties/ok".to_owned()),
    };

    let value = serde_json::to_value(part).unwrap();
    assert_eq!(value["kind"], "structured");
}

#[test]
fn tool_event_shape_matches_spec_and_rejects_legacy_fields() {
    let event = ToolUseRequestedEvent {
        run_id: RunId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: "read_file".to_owned(),
        input: json!({ "path": "README.md" }),
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: Some(LongRunningPolicy {
                stall_threshold: Duration::from_secs(30),
                hard_timeout: Duration::from_secs(120),
            }),
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        causation_id: EventId::new(),
        at: chrono::Utc::now(),
    };

    let value = serde_json::to_value(event).unwrap();
    assert!(value.get("properties").is_some());
    assert!(value.get("causation_id").is_some());
    assert!(value.get("session_id").is_none());
    assert!(value.get("origin").is_none());
}

#[test]
fn grace_call_does_not_default_required_fields() {
    let value = json!({
        "run_id": RunId::new(),
        "current_iteration": 4,
        "max_iterations": 5,
        "usage_snapshot": UsageSnapshot::default(),
        "at": chrono::Utc::now(),
        "correlation_id": CorrelationId::new(),
    });

    assert!(serde_json::from_value::<GraceCallTriggeredEvent>(value).is_err());
}

#[test]
fn message_and_reference_parts_keep_provider_native_contracts() {
    let thinking = ThinkingBlock {
        text: None,
        provider_id: "anthropic".to_owned(),
        provider_native: Some(json!({ "encrypted_content": "opaque" })),
        signature: Some("sig".to_owned()),
    };

    let part = MessagePart::Thinking(thinking.clone());
    let roundtrip: MessagePart =
        serde_json::from_value(serde_json::to_value(part).unwrap()).unwrap();
    assert_eq!(roundtrip, MessagePart::Thinking(thinking));

    let reference = ToolResultPart::Reference {
        reference_kind: ReferenceKind::Url {
            url: "https://example.test".to_owned(),
        },
        title: Some("example".to_owned()),
        summary: None,
    };
    let value = serde_json::to_value(reference).unwrap();
    assert_eq!(value["kind"], "reference");
    assert!(value.get("reference_kind").is_some());
}

#[test]
fn memory_lifecycle_views_are_public_contracts() {
    let _user = UserMessageView {
        text: "remember this preference",
        turn: 7,
        at: chrono::Utc::now(),
    };
    let _message = MessageView {
        role: MessageRole::Tool,
        text_snippet: "tool output",
        tool_use_id: Some(ToolUseId::new()),
    };
    let _summary = SessionSummaryView {
        end_reason: EndReason::Completed,
        turn_count: 3,
        tool_use_count: 1,
        usage: UsageSnapshot::default(),
        final_assistant_text: Some("done"),
    };
    let _ctx = MemorySessionCtx {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        workspace_id: Some(WorkspaceId::new()),
        user_id: Some("user-1"),
        team_id: Some(TeamId::new()),
    };
}

struct TestBlobStore;

#[async_trait::async_trait]
impl BlobStore for TestBlobStore {
    fn store_id(&self) -> &'static str {
        "test"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        _bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        Ok(BlobRef {
            id: BlobId::new(),
            size: meta.size,
            content_hash: meta.content_hash,
            content_type: meta.content_type,
        })
    }

    async fn get(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        Ok(Box::pin(futures::stream::once(async {
            Bytes::from_static(b"ok")
        })))
    }

    async fn head(&self, _tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError> {
        Ok(Some(BlobMeta {
            content_type: blob.content_type.clone(),
            size: blob.size,
            content_hash: blob.content_hash,
            created_at: chrono::Utc::now(),
            retention: BlobRetention::TenantScoped,
        }))
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Ok(())
    }
}

#[test]
fn blob_store_trait_is_async_and_object_safe() {
    let store: &dyn BlobStore = &TestBlobStore;
    let blob = futures::executor::block_on(store.put(
        TenantId::SINGLE,
        Bytes::from_static(b"ok"),
        BlobMeta {
            content_type: Some("text/plain".to_owned()),
            size: 2,
            content_hash: [7; 32],
            created_at: chrono::Utc::now(),
            retention: BlobRetention::TenantScoped,
        },
    ))
    .unwrap();

    assert_eq!(blob.size, 2);
    assert_eq!(store.store_id(), "test");
}
