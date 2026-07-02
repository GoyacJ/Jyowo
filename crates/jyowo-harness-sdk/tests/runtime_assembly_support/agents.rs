use super::*;

#[cfg(feature = "agents-team")]
pub fn agent_team_tool_use_events(goal: &str, max_turns_per_goal: u32) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "agent_team".to_owned(),
                input: json!({
                    "goal": goal,
                    "maxTurnsPerGoal": max_turns_per_goal,
                }),
            },
        },
        ModelStreamEvent::MessageStop,
    ]
}

#[cfg(feature = "agents-team")]
#[derive(Default)]
pub struct BlockingTeamMemberProvider {
    calls: AtomicUsize,
    pub member_started: Arc<Notify>,
    release: Arc<Notify>,
}

#[cfg(feature = "agents-team")]
#[async_trait]
impl ModelProvider for BlockingTeamMemberProvider {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        TestModelProvider::default().supported_models()
    }

    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        let system = req.system.clone().unwrap_or_default();
        if system.contains("You are team member") {
            self.member_started.notify_waiters();
            let release = Arc::clone(&self.release);
            return Ok(Box::pin(stream::once(async move {
                release.notified().await;
                ModelStreamEvent::MessageStop
            })));
        }

        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            return Ok(Box::pin(stream::iter(agent_team_tool_use_events(
                "Run a cancellable team review",
                2,
            ))));
        }

        Ok(Box::pin(stream::iter(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("parent accepted".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ])))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}
