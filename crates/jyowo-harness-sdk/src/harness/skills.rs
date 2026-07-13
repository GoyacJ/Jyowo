use super::*;

impl Harness {
    pub(super) async fn skill_registry_service(
        &self,
        options: &SessionOptions,
        pending_session_events: Option<Arc<PendingSessionEvents>>,
    ) -> Result<Option<SkillRegistryService>, HarnessError> {
        let registry = self.inner.skill_registry.clone();
        let metrics_sink = self.skill_metrics_sink();
        if let Some(loader) = &self.inner.skill_loader {
            let event_sink: Arc<dyn harness_skill::SkillEventSink> =
                if let Some(pending_session_events) = pending_session_events {
                    Arc::new(BufferedSkillEventSink {
                        pending_session_events,
                    })
                } else {
                    Arc::new(SdkSkillEventSink {
                        event_store: Arc::clone(&self.inner.event_store),
                        tenant_id: options.tenant_id,
                        session_id: options.session_id,
                    })
                };
            let mut loader = loader.clone().with_event_sink(event_sink).with_event_scope(
                SkillThreatEventScope {
                    session_id: Some(options.session_id),
                    run_id: None,
                },
            );
            if let Some(metrics_sink) = &metrics_sink {
                loader = loader.with_metrics_sink(Arc::clone(metrics_sink));
            }
            let report = loader
                .load_all()
                .await
                .map_err(|error| HarnessError::Other(format!("load skills failed: {error}")))?;
            let snapshot = registry.snapshot();
            let new_skills = report
                .loaded
                .into_iter()
                .filter(|skill| {
                    !snapshot
                        .entries
                        .get(&skill.name)
                        .is_some_and(|existing| existing.source == skill.source)
                })
                .collect::<Vec<_>>();
            if !new_skills.is_empty() {
                registry.register_batch(new_skills).map_err(|error| {
                    HarnessError::Other(format!("register skill failed: {error}"))
                })?;
            }
        }
        self.register_skill_hooks(&registry)?;
        let snapshot = registry.snapshot();
        validate_required_skill_config(&snapshot, &self.inner.skill_config_snapshot)
            .map_err(|error| HarnessError::Other(error.to_string()))?;
        let mut renderer = SkillRenderer::new(Arc::new(
            SkillConfigSnapshotResolver::from_registry_snapshot(
                &snapshot,
                self.inner.skill_config_snapshot.clone(),
            ),
        ))
        .with_policy(self.skill_render_policy());
        if let Some(metrics_sink) = &metrics_sink {
            renderer = renderer.with_metrics_sink(Arc::clone(metrics_sink));
        }
        let mut service = SkillRegistryService::new(registry, renderer);
        if let Some(metrics_sink) = metrics_sink {
            service = service.with_metrics_sink(metrics_sink);
        }
        Ok(Some(service))
    }

    pub(super) fn skill_metrics_sink(&self) -> Option<Arc<dyn SkillMetricsSink>> {
        self.inner.observer.as_ref().map(|observer| {
            Arc::new(SdkSkillMetricsSink {
                observer: Arc::clone(observer),
            }) as Arc<dyn SkillMetricsSink>
        })
    }

    fn register_skill_hooks(&self, registry: &SkillRegistry) -> Result<(), HarnessError> {
        for binding in registry.hook_bindings() {
            validate_skill_hook_binding(&binding)?;
            if self
                .inner
                .hook_registry
                .origin_for(&binding.handler_id)
                .is_some()
            {
                continue;
            }
            self.inner
                .hook_registry
                .register(skill_hook_handler(binding)?)
                .map_err(|error| {
                    HarnessError::Hook(harness_contracts::HookError::Message(error.to_string()))
                })?;
        }
        Ok(())
    }

    pub(super) fn skill_render_policy(&self) -> SkillRenderPolicy {
        self.inner
            .skill_loader
            .as_ref()
            .map(SkillLoader::render_policy)
            .unwrap_or_default()
    }

    pub fn skill_loader(&self) -> Option<&SkillLoader> {
        self.inner.skill_loader.as_ref()
    }

    #[must_use]
    pub fn skill_registry(&self) -> &SkillRegistry {
        &self.inner.skill_registry
    }

    pub async fn validate_workspace_skill_markdown(
        &self,
        markdown: &str,
        source_path: Option<PathBuf>,
    ) -> Result<RuntimeSkillView, HarnessError> {
        let source = SkillSource::Workspace(PathBuf::new());
        let skill =
            parse_skill_markdown(markdown, source, source_path, sdk_current_skill_platform())
                .map_err(|error| HarnessError::Other(format!("parse skill failed: {error}")))?;
        let validator = self
            .inner
            .skill_loader
            .as_ref()
            .map(SkillLoader::validator)
            .unwrap_or_default();
        let skill = validator
            .validate_skill(skill)
            .await
            .map_err(|error| HarnessError::Other(format!("validate skill failed: {error}")))?;
        Ok(runtime_skill_view(
            &skill,
            harness_contracts::SkillStatus::Ready,
            true,
        ))
    }

    pub async fn reload_workspace_managed_skills(
        &self,
        enabled_dir: impl AsRef<Path>,
    ) -> Result<(), HarnessError> {
        self.reload_workspace_managed_skills_with_allowed_package_ids(enabled_dir, None)
            .await
    }

    pub async fn reload_workspace_managed_skills_with_allowed_package_ids(
        &self,
        enabled_dir: impl AsRef<Path>,
        allowed_package_ids: Option<std::collections::BTreeSet<String>>,
    ) -> Result<(), HarnessError> {
        let enabled_dir = enabled_dir.as_ref().to_path_buf();
        let source = SkillSource::Workspace(enabled_dir.clone());
        let loader = SkillLoader::default().with_source(SkillSourceConfig::DirectoryPackages {
            path: enabled_dir,
            source_kind: DirectorySourceKind::Workspace,
            allowed_package_ids,
        });
        let report = loader.load_all().await.map_err(|error| {
            HarnessError::Other(format!("load workspace skills failed: {error}"))
        })?;
        self.replace_workspace_managed_skills(source, report.loaded)
    }

    /// Reload user-managed (global) skills from the given directory.
    ///
    /// Skills are loaded with [`DirectorySourceKind::User`] and stored under
    /// [`SkillSource::User`] so they coexist with workspace-managed skills.
    pub async fn reload_user_managed_skills(
        &self,
        enabled_dir: impl AsRef<Path>,
    ) -> Result<(), HarnessError> {
        self.reload_user_managed_skills_with_allowed_package_ids(enabled_dir, None)
            .await
    }

    pub async fn reload_user_managed_skills_with_allowed_package_ids(
        &self,
        enabled_dir: impl AsRef<Path>,
        allowed_package_ids: Option<std::collections::BTreeSet<String>>,
    ) -> Result<(), HarnessError> {
        let enabled_dir = enabled_dir.as_ref().to_path_buf();
        let source = SkillSource::User(enabled_dir.clone());
        let loader = SkillLoader::default().with_source(SkillSourceConfig::DirectoryPackages {
            path: enabled_dir,
            source_kind: DirectorySourceKind::User,
            allowed_package_ids,
        });
        let report = loader
            .load_all()
            .await
            .map_err(|error| HarnessError::Other(format!("load user skills failed: {error}")))?;
        self.replace_workspace_managed_skills(source, report.loaded)
    }

    pub fn list_runtime_skills(&self) -> Vec<RuntimeSkillSummary> {
        let snapshot = self.inner.skill_registry.snapshot();
        snapshot
            .entries
            .values()
            .map(|skill| {
                let status = snapshot
                    .status
                    .get(&skill.id)
                    .cloned()
                    .unwrap_or(harness_contracts::SkillStatus::Ready);
                runtime_skill_summary(skill, status)
            })
            .collect()
    }

    pub fn view_runtime_skill(&self, name: &str, full: bool) -> Option<RuntimeSkillView> {
        let snapshot = self.inner.skill_registry.snapshot();
        let skill = snapshot.entries.get(name)?;
        let status = snapshot
            .status
            .get(&skill.id)
            .cloned()
            .unwrap_or(harness_contracts::SkillStatus::Ready);
        Some(runtime_skill_view(skill, status, full))
    }

    fn replace_workspace_managed_skills(
        &self,
        source: SkillSource,
        skills: Vec<Skill>,
    ) -> Result<(), HarnessError> {
        match self
            .inner
            .skill_registry
            .try_replace_source(source, skills, |current, candidate| {
                self.reconcile_skill_hooks(current, candidate)
            }) {
            Ok(_) => Ok(()),
            Err(SkillRegistryUpdateError::Registry(error)) => Err(HarnessError::Other(format!(
                "replace skill source failed: {error}"
            ))),
            Err(SkillRegistryUpdateError::Reconcile(error)) => Err(error),
        }
    }

    fn reconcile_skill_hooks(
        &self,
        current: &SkillRegistrySnapshot,
        candidate: &SkillRegistrySnapshot,
    ) -> Result<(), HarnessError> {
        let old_bindings = self.inner.skill_registry.hook_bindings_in_snapshot(current);
        let next_bindings = self
            .inner
            .skill_registry
            .hook_bindings_in_snapshot(candidate);
        let next_handler_ids = next_bindings
            .iter()
            .map(|binding| binding.handler_id.clone())
            .collect::<HashSet<_>>();

        let mut registered = Vec::<String>::new();
        for binding in next_bindings {
            if let Err(error) = validate_skill_hook_binding(&binding) {
                for registered_id in registered {
                    self.inner.hook_registry.deregister(&registered_id);
                }
                return Err(error);
            }
            if self
                .inner
                .hook_registry
                .origin_for(&binding.handler_id)
                .is_some()
            {
                continue;
            }
            let handler_id = binding.handler_id.clone();
            let handler = match skill_hook_handler(binding) {
                Ok(handler) => handler,
                Err(error) => {
                    for registered_id in registered {
                        self.inner.hook_registry.deregister(&registered_id);
                    }
                    return Err(error);
                }
            };
            if let Err(error) = self.inner.hook_registry.register(handler) {
                for registered_id in registered {
                    self.inner.hook_registry.deregister(&registered_id);
                }
                return Err(HarnessError::Hook(harness_contracts::HookError::Message(
                    error.to_string(),
                )));
            }
            registered.push(handler_id);
        }

        for binding in old_bindings {
            if !next_handler_ids.contains(&binding.handler_id) {
                self.inner.hook_registry.deregister(&binding.handler_id);
            }
        }
        Ok(())
    }

    pub fn register_locked_skill_versions(
        &self,
        snapshots: &[LockedSkillVersionSnapshot],
    ) -> Result<(), SkillPackLoaderError> {
        let skills = SkillPackLoaderAdapter::default().load_skills(snapshots)?;
        let skill_count = skills.len();
        self.inner
            .skill_registry
            .register_batch(skills)
            .map_err(|error| SkillPackLoaderError::Registry(error.to_string()))?;
        if let Some(observer) = &self.inner.observer {
            let mut span = observer.start_span(
                "skill.runtime_injection",
                SpanAttributes::new().with(
                    "skill_count",
                    AttributeValue::Int(skill_count.min(i64::MAX as usize) as i64),
                ),
            );
            span.set_status(SpanStatus::Ok);
            span.end();
        }
        Ok(())
    }
}

struct BufferedSkillEventSink {
    pending_session_events: Arc<PendingSessionEvents>,
}

#[async_trait]
impl harness_skill::SkillEventSink for BufferedSkillEventSink {
    async fn emit(&self, event: Event) {
        self.pending_session_events.push(event);
    }
}

struct SdkSkillEventSink {
    event_store: Arc<dyn EventStore>,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
}

#[async_trait]
impl harness_skill::SkillEventSink for SdkSkillEventSink {
    async fn emit(&self, event: Event) {
        let _ = self
            .event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await;
    }
}

struct SdkSkillMetricsSink {
    observer: Arc<Observer>,
}

impl SkillMetricsSink for SdkSkillMetricsSink {
    fn skill_loaded(&self, source: &str) {
        self.record("skill.loaded", "source", source);
    }

    fn skill_rejected(&self, reason: &str) {
        self.record("skill.rejected", "reason", reason);
    }

    fn skill_render_duration_ms(&self, duration_ms: u64) {
        let mut span = self.observer.start_span(
            "skill.render",
            SpanAttributes::new().with(
                "duration_ms",
                AttributeValue::Int(duration_ms.min(i64::MAX as u64) as i64),
            ),
        );
        span.set_status(SpanStatus::Ok);
        span.end();
    }

    fn skill_invocation(&self, skill_name: &str) {
        self.record(
            "skill.invocation",
            "skill_ref",
            &safe_skill_metric_label(skill_name),
        );
    }

    fn skill_view(&self, skill_name: &str) {
        self.record(
            "skill.view",
            "skill_ref",
            &safe_skill_metric_label(skill_name),
        );
    }

    fn skill_shell_invocation(&self, command: &str) {
        self.record(
            "skill.shell.invocation",
            "command_kind",
            &safe_skill_metric_label(command),
        );
    }

    fn skill_shell_blocked(&self, command: &str) {
        self.record(
            "skill.shell.blocked",
            "command_kind",
            &safe_skill_metric_label(command),
        );
    }

    fn skill_threat_detected(&self, category: &str) {
        self.record("skill.threat.detected", "category", category);
    }

    fn skill_prerequisite_missing(&self, skill_name: &str) {
        self.record(
            "skill.prerequisite.missing",
            "skill_ref",
            &safe_skill_metric_label(skill_name),
        );
    }

    fn skill_prerequisite_advisory(&self, skill_name: &str) {
        self.record(
            "skill.prerequisite.advisory",
            "skill_ref",
            &safe_skill_metric_label(skill_name),
        );
    }
}

impl SdkSkillMetricsSink {
    fn record(&self, name: &str, key: &str, value: &str) {
        let mut span = self.observer.start_span(
            name,
            SpanAttributes::new().with(key, AttributeValue::String(value.to_owned())),
        );
        span.set_status(SpanStatus::Ok);
        span.end();
    }
}

fn safe_skill_metric_label(value: &str) -> String {
    let mut label = value
        .chars()
        .map(|character| match character {
            'a'..='z' | '0'..='9' => character,
            'A'..='Z' => character.to_ascii_lowercase(),
            '-' | '.' | '/' | ':' | ' ' => '_',
            '_' => '_',
            _ => '_',
        })
        .take(48)
        .collect::<String>();
    while label.contains("__") {
        label = label.replace("__", "_");
    }
    let label = label.trim_matches('_').to_owned();
    if label.is_empty() {
        "unknown".to_owned()
    } else {
        label
    }
}

pub(super) struct SdkSkillReloadCap {
    pub(super) inner: Arc<HarnessInner>,
}

fn runtime_skill_summary(
    skill: &Skill,
    status: harness_contracts::SkillStatus,
) -> RuntimeSkillSummary {
    RuntimeSkillSummary {
        name: skill.name.clone(),
        description: skill.description.clone(),
        tags: skill.frontmatter.tags.clone(),
        category: skill.frontmatter.category.clone(),
        source: skill.source.to_kind(),
        status,
    }
}

fn runtime_skill_view(
    skill: &Skill,
    status: harness_contracts::SkillStatus,
    full: bool,
) -> RuntimeSkillView {
    RuntimeSkillView {
        summary: runtime_skill_summary(skill, status),
        parameters: skill
            .frontmatter
            .parameters
            .iter()
            .map(|parameter| RuntimeSkillParameter {
                name: parameter.name.clone(),
                param_type: skill_param_type_name(parameter.param_type).to_owned(),
                required: parameter.required,
                default: parameter.default.clone(),
                description: parameter.description.clone(),
            })
            .collect(),
        config_keys: skill
            .frontmatter
            .config
            .iter()
            .map(|config| config.key.clone())
            .collect(),
        body_preview: skill.body.chars().take(1024).collect(),
        body_full: full.then(|| skill.body.clone()),
    }
}

fn skill_param_type_name(param_type: SkillParamType) -> &'static str {
    match param_type {
        SkillParamType::String => "string",
        SkillParamType::Number => "number",
        SkillParamType::Boolean => "boolean",
        SkillParamType::Path => "path",
        SkillParamType::Url => "url",
    }
}

fn sdk_current_skill_platform() -> SkillPlatform {
    #[cfg(target_os = "macos")]
    {
        SkillPlatform::Macos
    }
    #[cfg(target_os = "linux")]
    {
        SkillPlatform::Linux
    }
    #[cfg(target_os = "windows")]
    {
        SkillPlatform::Windows
    }
}

#[async_trait]
impl SkillReloadCap for SdkSkillReloadCap {
    async fn reload_skills(&self, registrations: &[SkillRegistration]) -> Result<(), String> {
        let validator = self.skill_validator();
        let mut validated = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let skill = validator
                .validate_registration(registration)
                .await
                .map_err(|error| error.to_string())?;
            validated.push(SkillRegistration {
                skill,
                force_allowlist: registration.force_allowlist.clone(),
            });
        }

        match self.inner.skill_registry.try_replace_registrations(
            &validated,
            |current, candidate| {
                Harness {
                    inner: Arc::clone(&self.inner),
                }
                .reconcile_skill_hooks(current, candidate)
            },
        ) {
            Ok(_) => Ok(()),
            Err(SkillRegistryUpdateError::Registry(error)) => Err(error.to_string()),
            Err(SkillRegistryUpdateError::Reconcile(error)) => Err(error.to_string()),
        }
    }
}

impl SdkSkillReloadCap {
    fn skill_validator(&self) -> SkillValidator {
        let mut validator = self
            .inner
            .skill_loader
            .as_ref()
            .map(SkillLoader::validator)
            .unwrap_or_default();
        if let Some(observer) = &self.inner.observer {
            validator = validator.with_metrics_sink(Arc::new(SdkSkillMetricsSink {
                observer: Arc::clone(observer),
            }));
        }
        validator
    }
}

struct SkillDeclaredHookHandler {
    handler_id: String,
    events: Vec<HookEventKind>,
}

#[async_trait]
impl HookHandler for SkillDeclaredHookHandler {
    fn handler_id(&self) -> &str {
        &self.handler_id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &self.events
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::Continue)
    }
}

fn skill_hook_handler(binding: SkillHookBinding) -> Result<Box<dyn HookHandler>, HarnessError> {
    validate_skill_hook_binding(&binding)?;
    match binding.transport {
        SkillHookTransport::Builtin(BuiltinHookKind::AuditLog) => {
            Ok(Box::new(SkillDeclaredHookHandler {
                handler_id: binding.handler_id,
                events: binding.events,
            }))
        }
        SkillHookTransport::Exec(spec) => {
            let handler = ExecHookTransport::new(HookExecSpec {
                handler_id: binding.handler_id,
                interested_events: binding.events,
                failure_mode: spec.failure_mode,
                command: spec.command,
                args: spec.args,
                env: Default::default(),
                working_dir: WorkingDir::SessionWorkspace,
                timeout: Duration::from_millis(spec.timeout_ms),
                resource_limits: HookExecResourceLimits::default(),
                signal_policy: HookExecSignalPolicy::default(),
                protocol_version: HookProtocolVersion::V1,
                trust: binding.source.trust_level(),
            })
            .map_err(HarnessError::Hook)?;
            Ok(Box::new(handler))
        }
        SkillHookTransport::Http(spec) => {
            let url = spec
                .url
                .parse()
                .map_err(|error| harness_contracts::HookError::Message(format!("{error}")))?;
            let handler = HttpHookTransport::new(HookHttpSpec {
                handler_id: binding.handler_id,
                interested_events: binding.events,
                failure_mode: spec.failure_mode,
                url,
                auth: HookHttpAuth::None,
                timeout: Duration::from_millis(spec.timeout_ms),
                security: HookHttpSecurityPolicy {
                    allowlist: HostAllowlist::from_hosts(spec.security.allowlist),
                    ssrf_guard: skill_ssrf_guard_policy(spec.security.ssrf_guard),
                    max_redirects: spec.security.max_redirects,
                    max_body_bytes: spec.security.max_body_bytes,
                    mtls: None,
                },
                protocol_version: HookProtocolVersion::V1,
                trust: binding.source.trust_level(),
            })
            .map_err(HarnessError::Hook)?;
            Ok(Box::new(handler))
        }
    }
}

fn skill_ssrf_guard_policy(enabled: bool) -> SsrfGuardPolicy {
    if enabled {
        return SsrfGuardPolicy::default();
    }
    SsrfGuardPolicy {
        deny_loopback: false,
        deny_private: false,
        deny_link_local: false,
        deny_metadata: false,
    }
}

fn validate_skill_hook_binding(binding: &SkillHookBinding) -> Result<(), HarnessError> {
    if matches!(
        &binding.transport,
        SkillHookTransport::Http(spec) if spec.security.mtls_required
    ) {
        return Err(HarnessError::Hook(
            harness_contracts::HookError::Unauthorized(format!(
                "skill hook `{}` requires mTLS, but no client certificate source is configured",
                binding.logical_id
            )),
        ));
    }
    let denied = match (&binding.source, &binding.transport) {
        (SkillSource::Mcp(_), _) => true,
        (_, SkillHookTransport::Builtin(_)) => false,
        (SkillSource::Bundled, SkillHookTransport::Exec(_) | SkillHookTransport::Http(_)) => false,
        (
            SkillSource::Plugin {
                trust: TrustLevel::AdminTrusted,
                ..
            },
            SkillHookTransport::Exec(_) | SkillHookTransport::Http(_),
        ) => false,
        (_, SkillHookTransport::Exec(_) | SkillHookTransport::Http(_)) => true,
    };
    if denied {
        return Err(HarnessError::Hook(
            harness_contracts::HookError::Unauthorized(format!(
                "skill hook transport not permitted for trust={:?}",
                binding.source.trust_level()
            )),
        ));
    }
    Ok(())
}
