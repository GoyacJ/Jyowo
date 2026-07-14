use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::future::BoxFuture;
use harness_contracts::{
    AgentTeamStarterCap, AgentTeamToolStartRequest, AgentTeamToolStartResponse,
    BackgroundAgentStarterCap, BackgroundAgentToolStartRequest, BackgroundAgentToolStartResponse,
    ClientId, CommandId, ConversationContextReference, ModelProtocol, PermissionMode,
    ProviderProfileConversationCapability, ProviderProfileDefinition,
    ProviderProfileModelDescriptor, ProviderProfileModelLifecycle, ProviderSecretEntry,
    ProviderSecretsRecord, ProviderSelectionRecord, QueueItemId, RunId, RunSegmentId, SessionId,
    SkillId, SkillSourceKind, TaskId, ToolError, TurnInput, WorkspaceMode,
    CURRENT_CONTEXT_REFERENCE_VERSION,
};
use harness_daemon::{
    AgentStarterCapabilities, PermissionBroker, RunCoordinatorFactory, RuntimeConfigResolver,
    SdkRunCoordinatorFactory, StartSegmentRequest, WorkspaceAccess, WorkspaceAcquireOutcome,
    WorkspaceCoordinator, WorkspaceExecutionKind, WorkspaceLeaseRequest, WorkspaceToolDispatcher,
};
use harness_journal::{AcceptedCommand, CommandOutcome, NewTaskEvent, SegmentRunInput, TaskStore};
use harness_subagent::{
    ParentContext, SubagentError, SubagentHandle, SubagentRunner, SubagentSpec,
};
use jyowo_harness_sdk::ext::{hash_skill_package, SkillConfigStoreError, SkillSecretStore};
use jyowo_harness_sdk::skill_config::SecretString;
use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

const SKILL_ID: &str = "user:daemon-review";
const PACKAGE_ID: &str = "daemon-review";
const SKILL_BODY: &str = "DAEMON_SKILL_BODY";
const SECRET_VALUE: &str = "daemon-secret-must-not-leak";

#[tokio::test]
async fn selected_skill_is_delivered_once_and_consumed_recovery_skips_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"id\":\"chatcmpl-skill\",\"choices\":[{\"index\":0,",
                        "\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}],",
                        "\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
                        "data: [DONE]\n\n"
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let root = tempfile::tempdir().unwrap();
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    let config_root = root.path().join("jyowo/config");
    std::fs::create_dir_all(&config_root).unwrap();
    write_provider_config(&config_root, &server);
    write_skill_config(root.path(), &config_root);

    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let actor_id = store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    let workspace = Arc::new(
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap(),
    );
    let lease = match workspace
        .acquire(WorkspaceLeaseRequest {
            task_id,
            actor_id,
            root: workspace_root,
            mode: Some(WorkspaceMode::Current),
            access: WorkspaceAccess::Write,
            execution_kind: WorkspaceExecutionKind::Foreground,
            expires_at: None,
        })
        .unwrap()
    {
        WorkspaceAcquireOutcome::Acquired(lease) => lease,
        WorkspaceAcquireOutcome::Waiting(_) => panic!("fixture workspace lease must be active"),
    };
    let redactor = Arc::new(harness_contracts::NoopRedactor);
    let permissions = Arc::new(PermissionBroker::new(Arc::clone(&store), redactor.clone()));
    let resolver = RuntimeConfigResolver::new(config_root)
        .with_skill_secret_store(Arc::new(FixtureSecretStore));
    let factory = SdkRunCoordinatorFactory::new(
        Arc::clone(&store),
        resolver,
        root.path().join("blobs"),
        permissions,
        redactor.clone(),
    );
    let workspace_tools = WorkspaceToolDispatcher::new(workspace);
    let queue_item_id = QueueItemId::new();
    let session_id = SessionId::new();
    let references = vec![
        ConversationContextReference::WorkspaceFile {
            path: "Cargo.toml".into(),
            label: "manifest".into(),
        },
        ConversationContextReference::Skill {
            version: CURRENT_CONTEXT_REFERENCE_VERSION,
            skill_id: SkillId(SKILL_ID.into()),
            label: "Daemon review".into(),
            parameters: BTreeMap::from([("topic".into(), json!("durability"))]),
            source: Some(SkillSourceKind::User),
        },
    ];
    let first = segment_request(
        task_id,
        RunSegmentId::new(),
        session_id,
        RunId::new(),
        queue_item_id,
        lease.lease_id,
        references.clone(),
    );
    let delivery_key = first
        .skill_context_delivery_key(1)
        .expect("typed skill reference should receive a durable key");
    assert_eq!(first.skill_context_delivery_key(0), None);

    let first_segment_id = first.segment_id;
    let _first_running = factory.spawn_idempotent(
        first,
        workspace_tools.clone(),
        Arc::new(UnusedSubagentRunner),
        unused_agent_starters(),
    );
    wait_for_segment_completion(&store, first_segment_id).await;

    let events_after_first = all_task_events(&store, task_id);
    let serialized_events = serde_json::to_string(&events_after_first).unwrap();
    assert!(serialized_events.contains("skill_context_consumed"));
    assert!(serialized_events.contains(&delivery_key));
    assert!(!serialized_events.contains(SKILL_BODY));
    assert!(!serialized_events.contains(SECRET_VALUE));

    let recovered = segment_request(
        task_id,
        RunSegmentId::new(),
        session_id,
        RunId::new(),
        queue_item_id,
        lease.lease_id,
        references,
    );
    assert_eq!(
        recovered.skill_context_delivery_key(1).as_deref(),
        Some(delivery_key.as_str())
    );
    let recovered_segment_id = recovered.segment_id;
    let _recovered_running = factory.spawn_idempotent(
        recovered,
        workspace_tools,
        Arc::new(UnusedSubagentRunner),
        unused_agent_starters(),
    );
    wait_for_segment_completion(&store, recovered_segment_id).await;

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let first_request = serde_json::to_string(&requests[0].body_json::<Value>().unwrap()).unwrap();
    let recovered_request =
        serde_json::to_string(&requests[1].body_json::<Value>().unwrap()).unwrap();
    assert_eq!(first_request.matches(SKILL_BODY).count(), 1);
    assert!(first_request.contains("durability"));
    assert!(!first_request.contains("skill: Daemon review"));
    assert!(!first_request.contains(SECRET_VALUE));
    assert!(!recovered_request.contains(SKILL_BODY));
    assert!(!recovered_request.contains(SECRET_VALUE));

    let all_events = serde_json::to_string(&all_task_events(&store, task_id)).unwrap();
    assert!(!all_events.contains(SKILL_BODY));
    assert!(!all_events.contains(SECRET_VALUE));
}

fn segment_request(
    task_id: TaskId,
    segment_id: RunSegmentId,
    session_id: SessionId,
    run_id: RunId,
    queue_item_id: QueueItemId,
    workspace_lease_id: harness_contracts::WorkspaceLeaseId,
    context_references: Vec<ConversationContextReference>,
) -> StartSegmentRequest {
    StartSegmentRequest {
        task_id,
        segment_id,
        input: SegmentRunInput {
            queue_item_id: Some(queue_item_id),
            queue_item_revision: Some(7),
            content: "apply selected context".into(),
            attachments: Vec::new(),
            context_references,
            model_config_id: Some("skill-provider".into()),
            permission_mode: PermissionMode::BypassPermissions,
            workspace: None,
            session_id,
            run_id,
            workspace_lease_id: Some(workspace_lease_id),
        },
        indeterminate_tools: Vec::new(),
    }
}

fn write_provider_config(config_root: &std::path::Path, server: &MockServer) {
    write_json(
        &config_root.join("provider-profiles.json"),
        &[ProviderProfileDefinition {
            id: "skill-provider".into(),
            display_name: "skill-provider".into(),
            provider_id: "openai".into(),
            model_id: "gpt-5.4-mini".into(),
            protocol: ModelProtocol::ChatCompletions,
            model_options: Default::default(),
            base_url: Some(server.uri()),
            provider_defaults: None,
            model_descriptor: ProviderProfileModelDescriptor {
                protocol: ModelProtocol::ChatCompletions,
                context_window: 32_000,
                display_name: "gpt-5.4-mini".into(),
                lifecycle: ProviderProfileModelLifecycle::Stable,
                max_output_tokens: 4_096,
                model_id: "gpt-5.4-mini".into(),
                provider_id: "openai".into(),
                conversation_capability: ProviderProfileConversationCapability {
                    input_modalities: vec!["text".into()],
                    output_modalities: vec!["text".into()],
                    context_window: 32_000,
                    max_output_tokens: 4_096,
                    streaming: true,
                    tool_calling: true,
                    reasoning: false,
                    prompt_cache: false,
                    structured_output: false,
                },
                runtime_semantics: None,
            },
        }],
    );
    write_json(
        &config_root.join("provider-secrets.json"),
        &ProviderSecretsRecord {
            entries: vec![ProviderSecretEntry {
                config_id: "skill-provider".into(),
                api_key: "test-key".into(),
                official_quota_api_key: None,
            }],
        },
    );
    write_json(
        &config_root.join("provider-selection.json"),
        &ProviderSelectionRecord {
            default_config_id: Some("skill-provider".into()),
        },
    );
}

fn write_skill_config(root: &std::path::Path, config_root: &std::path::Path) {
    let package = root.join("jyowo/skills/packages").join(PACKAGE_ID);
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("SKILL.md"),
        format!(
            r#"---
name: {PACKAGE_ID}
description: Verify durable daemon skill delivery.
parameters:
  - name: topic
    type: string
    required: true
config:
  - key: api_token
    type: string
    secret: true
    required: true
---
{SKILL_BODY}: review ${{topic}}.
"#
        ),
    )
    .unwrap();
    let package_hash = hash_skill_package(&package).unwrap();
    write_json(
        &root.join("jyowo/skills/index.json"),
        &json!([{ "id": PACKAGE_ID, "contentHash": package_hash }]),
    );
    write_json(
        &config_root.join("skills.json"),
        &json!({ "enabled": [PACKAGE_ID] }),
    );
    write_json(
        &config_root.join("skill-config.json"),
        &json!({
            "version": 1,
            "skills": {
                SKILL_ID: {
                    "values": {},
                    "secrets": { "api_token": { "configured": true } }
                }
            }
        }),
    );
}

fn create_task(store: &TaskStore) -> TaskId {
    let task_id = TaskId::new();
    let outcome = store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("create-{task_id}"),
                expected_stream_version: 0,
                authority: TaskStore::user_authority(ClientId::new()),
                payload: json!({ "type": "create_task" }),
            },
            |_| Ok(vec![NewTaskEvent::task_created("skill context flow")]),
        )
        .unwrap();
    assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
    task_id
}

async fn wait_for_segment_completion(store: &TaskStore, segment_id: RunSegmentId) {
    let database = store.database_path().to_owned();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let completed = rusqlite::Connection::open(&database)
                .unwrap()
                .query_row(
                    "SELECT status FROM segment_execution WHERE run_segment_id = ?1",
                    [segment_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .is_ok_and(|status| status == "completed");
            if completed {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("segment should complete");
}

fn all_task_events(
    store: &TaskStore,
    task_id: TaskId,
) -> Vec<harness_contracts::TaskEventEnvelope> {
    let mut after_stream_sequence = 0;
    let mut events = Vec::new();
    loop {
        let page = store
            .task_events_after(task_id, after_stream_sequence, usize::MAX)
            .unwrap();
        let Some(last) = page.last() else {
            break;
        };
        after_stream_sequence = last.stream_sequence;
        events.extend(page);
    }
    events
}

fn write_json(path: &std::path::Path, value: &(impl serde::Serialize + ?Sized)) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

struct FixtureSecretStore;

impl SkillSecretStore for FixtureSecretStore {
    fn get(
        &self,
        skill_id: &str,
        key: &str,
    ) -> Result<Option<SecretString>, SkillConfigStoreError> {
        Ok((skill_id == SKILL_ID && key == "api_token")
            .then(|| SecretString::from(SECRET_VALUE.to_owned())))
    }

    fn set(
        &self,
        _skill_id: &str,
        _key: &str,
        _value: SecretString,
    ) -> Result<(), SkillConfigStoreError> {
        unreachable!("fixture secret store is read-only")
    }

    fn delete(&self, _skill_id: &str, _key: &str) -> Result<(), SkillConfigStoreError> {
        unreachable!("fixture secret store is read-only")
    }
}

struct UnusedSubagentRunner;

#[async_trait]
impl SubagentRunner for UnusedSubagentRunner {
    async fn spawn(
        &self,
        _spec: SubagentSpec,
        _input: TurnInput,
        _parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        Err(SubagentError::Engine(
            "subagents are not used by this test".into(),
        ))
    }
}

struct UnusedAgentStarter;

impl BackgroundAgentStarterCap for UnusedAgentStarter {
    fn start_background_agent(
        &self,
        _request: BackgroundAgentToolStartRequest,
    ) -> BoxFuture<'static, Result<BackgroundAgentToolStartResponse, ToolError>> {
        Box::pin(async { Err(ToolError::Internal("background agents are not used".into())) })
    }
}

impl AgentTeamStarterCap for UnusedAgentStarter {
    fn start_agent_team(
        &self,
        _request: AgentTeamToolStartRequest,
    ) -> BoxFuture<'static, Result<AgentTeamToolStartResponse, ToolError>> {
        Box::pin(async { Err(ToolError::Internal("agent teams are not used".into())) })
    }
}

fn unused_agent_starters() -> AgentStarterCapabilities {
    AgentStarterCapabilities {
        background: Arc::new(UnusedAgentStarter),
        team: Arc::new(UnusedAgentStarter),
    }
}
