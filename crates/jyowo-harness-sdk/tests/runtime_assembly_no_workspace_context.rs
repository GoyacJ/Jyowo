#![cfg(feature = "testing")]

mod runtime_assembly_support;
use runtime_assembly_support::*;

#[test]
fn no_workspace_tool_context_does_not_treat_execution_cwd_as_project_workspace() {
    block_on(async {
        let runtime_root = unique_workspace("sdk-no-workspace-tool-context-runtime");
        let execution_cwd = unique_workspace("sdk-no-workspace-tool-context");
        std::fs::create_dir_all(&runtime_root).unwrap();
        std::fs::create_dir_all(&execution_cwd).unwrap();
        let session_id = SessionId::new();
        let tool_use_id = ToolUseId::new();
        let captured = Arc::new(Mutex::new(None));
        let mut caps = ConversationModelCapability::default();
        caps.tool_calling = true;
        let model = Arc::new(CapabilityScriptedProvider::new(
            caps,
            vec![
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::ToolUseComplete {
                            id: tool_use_id,
                            name: "capture_project_workspace".to_owned(),
                            input: json!({}),
                        },
                    },
                    ModelStreamEvent::MessageStop,
                ],
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::Text("done".to_owned()),
                    },
                    ModelStreamEvent::MessageStop,
                ],
            ],
        ));
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .with_tool(Box::new(CaptureProjectWorkspaceTool {
                captured: Arc::clone(&captured),
                descriptor: SdkPluginTool::new("capture_project_workspace").descriptor,
            }))
            .build()
            .expect("tool registry should build");
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let harness = Harness::builder()
            .with_workspace_root(&runtime_root)
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_tool_registry(registry)
            .build()
            .await
            .expect("harness should build");
        let options = SessionOptions::new(&execution_cwd)
            .with_session_id(session_id)
            .with_model_id("test-model")
            .with_tool_profile(ToolProfile::Full);
        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .expect("session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                options,
                ConversationTurnInput::ask("capture project workspace"),
                Some(PermissionMode::BypassPermissions),
                None,
                None,
            ))
            .await
            .expect("turn should run");

        let captured = captured.lock().unwrap().clone();
        let captured = match captured {
            Some(captured) => captured,
            None => {
                let events: Vec<_> = store
                    .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
                    .await
                    .expect("events should be readable")
                    .collect()
                    .await;
                panic!("captured context; events: {events:#?}");
            }
        };
        assert_eq!(captured.workspace_root, execution_cwd);
        assert_eq!(captured.project_workspace_root, None);
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapturedProjectWorkspaceContext {
    workspace_root: std::path::PathBuf,
    project_workspace_root: Option<std::path::PathBuf>,
}

struct CaptureProjectWorkspaceTool {
    descriptor: ToolDescriptor,
    captured: Arc<Mutex<Option<CapturedProjectWorkspaceContext>>>,
}

#[async_trait]
impl Tool for CaptureProjectWorkspaceTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::Allowed,
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        *self.captured.lock().unwrap() = Some(CapturedProjectWorkspaceContext {
            workspace_root: ctx.workspace_root,
            project_workspace_root: ctx.project_workspace_root,
        });
        Ok(Box::pin(futures::stream::iter([ToolEvent::Final(
            ToolResult::Text("captured".to_owned()),
        )])))
    }
}
