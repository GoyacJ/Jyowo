use super::*;

impl Harness {
    #[cfg(feature = "memory-external-slot")]
    pub(super) async fn memory_manager_for_session(
        &self,
        options: &SessionOptions,
    ) -> Result<Option<Arc<harness_memory::MemoryManager>>, HarnessError> {
        let provider = self.effective_memory_provider();
        #[cfg(feature = "memory-consolidation")]
        let has_consolidation_hook = self.inner.consolidation_hook.is_some();
        #[cfg(not(feature = "memory-consolidation"))]
        let has_consolidation_hook = false;
        if provider.is_none() && !has_consolidation_hook {
            return Ok(None);
        }

        let mut manager = harness_memory::MemoryManager::new()
            .with_event_sink(Arc::new(SdkMemoryEventSink {
                event_store: Arc::clone(&self.inner.event_store),
                tenant_id: options.tenant_id,
                session_id: options.session_id,
            }))
            .with_threat_scanner(Arc::new(harness_memory::MemoryThreatScanner::default()));
        if let Some(metrics_sink) = self.memory_metrics_sink() {
            manager = manager.with_metrics_sink(metrics_sink);
        }
        #[cfg(feature = "memory-consolidation")]
        if let Some(hook) = &self.inner.consolidation_hook {
            manager = manager.with_consolidation_hook(Arc::clone(hook));
        }
        if let Some(provider) = provider {
            manager
                .set_external(provider)
                .map_err(HarnessError::Memory)?;
        }
        manager
            .initialize_session(&harness_contracts::MemorySessionCtx {
                tenant_id: options.tenant_id,
                session_id: options.session_id,
                workspace_id: None,
                user_id: options.user_id.as_deref(),
                team_id: options.team_id,
            })
            .await
            .map_err(HarnessError::Memory)?;
        Ok(Some(Arc::new(manager)))
    }

    #[cfg(feature = "memory-builtin")]
    pub(super) async fn builtin_system_prompt(
        &self,
        options: &SessionOptions,
    ) -> Result<Option<String>, HarnessError> {
        let Some(config) = &self.inner.builtin_memory else {
            return Ok(None);
        };
        let mut memory = config.for_session(options);
        if let Some(metrics_sink) = self.memory_metrics_sink() {
            memory = memory.with_metrics_sink(metrics_sink);
        }
        let snapshot = memory.read_all().await.map_err(HarnessError::Memory)?;
        let rendered =
            render_builtin_memory_system_prompt(&snapshot, options.tenant_id, options.session_id);
        if !rendered.overflows.is_empty() {
            let events = rendered
                .overflows
                .iter()
                .cloned()
                .map(Event::MemdirOverflow)
                .collect::<Vec<_>>();
            let _ = self
                .inner
                .event_store
                .append(options.tenant_id, options.session_id, &events)
                .await;
            if let Some(metrics_sink) = self.memory_metrics_sink() {
                for overflow in &rendered.overflows {
                    metrics_sink.record(harness_memory::MemoryMetric::MemdirOverflow {
                        file: overflow.file,
                        current_chars: overflow.current_chars,
                        threshold: overflow.threshold,
                    });
                }
            }
        }
        Ok(rendered.inner)
    }

    #[cfg(feature = "memory-external-slot")]
    pub async fn list_memory_items(
        &self,
        options: SessionOptions,
    ) -> Result<Vec<harness_memory::MemorySummary>, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .list_for_actor(memory_actor_from_options(&options))
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-external-slot")]
    pub async fn get_memory_item(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
    ) -> Result<harness_memory::MemoryRecord, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .get_for_actor(id, memory_actor_from_options(&options))
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-external-slot")]
    pub async fn update_memory_item_content(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
        content: impl Into<String>,
    ) -> Result<harness_memory::MemoryRecord, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .update_content_for_actor(id, memory_actor_from_options(&options), content, None)
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-external-slot")]
    pub async fn delete_memory_item(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
    ) -> Result<(), HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .forget_for_actor(id, memory_actor_from_options(&options), None)
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-external-slot")]
    pub async fn export_memory_items(
        &self,
        options: SessionOptions,
    ) -> Result<Vec<harness_memory::MemoryRecord>, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .export_for_actor(memory_actor_from_options(&options))
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-external-slot")]
    async fn memory_manager_for_browser(
        &self,
        options: &SessionOptions,
    ) -> Result<Arc<harness_memory::MemoryManager>, HarnessError> {
        self.memory_manager_for_session(options)
            .await?
            .ok_or_else(|| {
                HarnessError::Memory(harness_contracts::MemoryError::ExternalProviderNotConfigured)
            })
    }

    pub(super) fn effective_memory_provider(&self) -> Option<Arc<dyn MemoryProvider>> {
        self.inner
            .memory_provider
            .as_ref()
            .map(Arc::clone)
            .or_else(|| {
                self.inner
                    .plugin_registry
                    .as_ref()
                    .and_then(harness_plugin::PluginRegistry::registered_memory_provider)
            })
    }
}

#[cfg(feature = "memory-external-slot")]
fn memory_actor_from_options(options: &SessionOptions) -> harness_contracts::MemoryActor {
    harness_contracts::MemoryActor {
        tenant_id: options.tenant_id,
        user_id: options.user_id.clone(),
        team_id: options.team_id,
        session_id: Some(options.session_id),
    }
}

pub(super) fn record_memory_summary_event(state: &mut MemorySessionSummaryState, event: &Event) {
    match event {
        Event::UserMessageAppended(_) => {
            state.turn_count = state.turn_count.saturating_add(1);
        }
        Event::AssistantMessageCompleted(completed) => {
            state.final_assistant_text = message_content_text(&completed.content);
        }
        Event::ToolUseCompleted(_) | Event::ToolUseFailed(_) => {
            state.tool_use_count = state.tool_use_count.saturating_add(1);
        }
        _ => {}
    }
}

fn message_content_text(content: &MessageContent) -> Option<String> {
    match content {
        MessageContent::Text(text) => Some(text.clone()),
        MessageContent::Structured(value) => Some(value.to_string()),
        MessageContent::Multimodal(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| match part {
                    MessagePart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
    }
}

#[cfg(feature = "memory-external-slot")]
pub(super) struct SdkMemoryEventSink {
    pub(super) event_store: Arc<dyn EventStore>,
    pub(super) tenant_id: TenantId,
    pub(super) session_id: harness_contracts::SessionId,
}

#[cfg(feature = "memory-external-slot")]
#[async_trait]
impl harness_memory::MemoryEventSink for SdkMemoryEventSink {
    async fn emit(&self, event: Event) {
        let _ = self
            .event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await;
    }

    async fn emit_required(&self, event: Event) -> Result<(), harness_contracts::MemoryError> {
        self.event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await
            .map(|_| ())
            .map_err(|error| harness_contracts::MemoryError::Provider {
                provider: "journal".to_owned(),
                source_message: error.to_string(),
            })
    }
}
