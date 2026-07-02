use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionError,
    PermissionSubject, RuleSource, ShellKind, TenantId,
};
use tokio::task::JoinHandle;

use crate::{
    policy_scope_matches_request, DangerousPatternLibrary, DecisionPersistence, InlineRuleProvider,
    NoopDecisionPersistence, PermissionBroker, PermissionContext, PermissionRequest,
    PermissionRule, PersistedDecision, RuleAction, RuleProvider, RuleSnapshot,
};

pub struct RuleEngineBroker {
    snapshot: Arc<ArcSwap<RuleSnapshot>>,
    rule_providers: Vec<Arc<dyn RuleProvider>>,
    fallback: FallbackPolicy,
    tenant: TenantId,
    persistence: Arc<dyn DecisionPersistence>,
    dangerous_patterns: Option<DangerousPatternLibrary>,
    policy_deny_only: bool,
    watch_task: Option<JoinHandle<()>>,
}

pub struct RuleEngineBrokerBuilder {
    tenant: TenantId,
    rule_providers: Vec<Arc<dyn RuleProvider>>,
    fallback: FallbackPolicy,
    dangerous_patterns: Option<DangerousPatternLibrary>,
    persistence: Option<Arc<dyn DecisionPersistence>>,
    policy_deny_only: bool,
}

impl RuleEngineBroker {
    pub fn builder() -> RuleEngineBrokerBuilder {
        RuleEngineBrokerBuilder {
            tenant: TenantId::SHARED,
            rule_providers: Vec::new(),
            fallback: FallbackPolicy::AskUser,
            dangerous_patterns: None,
            persistence: None,
            policy_deny_only: false,
        }
    }

    pub async fn reload(&self) -> Result<(), PermissionError> {
        let generation = self.snapshot.load().generation + 1;
        let snapshot = build_snapshot(&self.rule_providers, self.tenant, generation).await?;
        self.snapshot.store(Arc::new(snapshot));
        Ok(())
    }

    pub fn current_snapshot(&self) -> Arc<RuleSnapshot> {
        self.snapshot.load_full()
    }
}

impl RuleEngineBrokerBuilder {
    #[must_use]
    pub fn with_tenant(mut self, tenant: TenantId) -> Self {
        self.tenant = tenant;
        self
    }

    #[must_use]
    pub fn with_rule_provider(mut self, provider: Arc<dyn RuleProvider>) -> Self {
        self.rule_providers.push(provider);
        self
    }

    #[must_use]
    pub fn with_rules(mut self, rules: Vec<PermissionRule>) -> Self {
        self.rule_providers.push(Arc::new(InlineRuleProvider::new(
            "inline",
            RuleSource::Session,
            rules,
        )));
        self
    }

    #[must_use]
    pub fn with_fallback(mut self, fallback: FallbackPolicy) -> Self {
        self.fallback = fallback;
        self
    }

    #[must_use]
    pub fn with_dangerous_library(mut self, library: DangerousPatternLibrary) -> Self {
        self.dangerous_patterns = Some(library);
        self
    }

    #[must_use]
    pub fn with_persistence(mut self, persistence: Arc<dyn DecisionPersistence>) -> Self {
        self.persistence = Some(persistence);
        self
    }

    #[must_use]
    pub fn policy_deny_only(mut self) -> Self {
        self.policy_deny_only = true;
        self
    }

    #[must_use]
    pub fn with_platform_dangerous_library(mut self, shell_kind: ShellKind) -> Self {
        self.dangerous_patterns = Some(DangerousPatternLibrary::for_shell_kind(shell_kind));
        self
    }

    pub async fn build(self) -> Result<RuleEngineBroker, PermissionError> {
        if matches!(self.persistence.as_ref(), Some(persistence) if !persistence.supports_integrity())
        {
            return Err(PermissionError::Message(
                "decision persistence must support integrity verification".to_owned(),
            ));
        }
        let snapshot = build_snapshot(&self.rule_providers, self.tenant, 1).await?;
        let snapshot = Arc::new(ArcSwap::from_pointee(snapshot));
        let watch_task =
            spawn_watch_task(self.rule_providers.clone(), self.tenant, snapshot.clone());
        Ok(RuleEngineBroker {
            snapshot,
            rule_providers: self.rule_providers,
            fallback: self.fallback,
            tenant: self.tenant,
            persistence: self
                .persistence
                .unwrap_or_else(|| Arc::new(NoopDecisionPersistence)),
            dangerous_patterns: self.dangerous_patterns,
            policy_deny_only: self.policy_deny_only,
            watch_task,
        })
    }
}

#[async_trait]
impl PermissionBroker for RuleEngineBroker {
    fn can_anchor_authority(&self) -> bool {
        true
    }

    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        let snapshot = self.current_snapshot();
        let rule = select_rule(&snapshot.rules, &request.scope_hint);
        if policy_rule_denies(rule) {
            return Decision::DenyOnce;
        }

        if self.policy_deny_only {
            return Decision::Escalate;
        }

        let is_dangerous_command = self.is_dangerous_command(&request);
        if is_dangerous_command {
            return match ctx.interactivity {
                InteractivityLevel::NoInteractive => Decision::DenyOnce,
                InteractivityLevel::FullyInteractive
                | InteractivityLevel::DeferredInteractive
                | _ => Decision::Escalate,
            };
        }

        let Some(rule) = rule else {
            return fallback_decision(self.fallback, &request, &ctx);
        };

        match &rule.action {
            RuleAction::Allow => Decision::AllowOnce,
            RuleAction::Deny => Decision::DenyOnce,
            RuleAction::AskWithDefault(default) => match ctx.interactivity {
                InteractivityLevel::NoInteractive => default.clone(),
                InteractivityLevel::FullyInteractive
                | InteractivityLevel::DeferredInteractive
                | _ => Decision::Escalate,
            },
        }
    }

    async fn hard_policy_denies(
        &self,
        request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        let snapshot = self.current_snapshot();
        let rule = select_rule(&snapshot.rules, &request.scope_hint);
        policy_rule_denies(rule)
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        if decision.source == RuleSource::Policy {
            return Err(PermissionError::Message(
                "runtime learned decisions cannot be persisted as Policy rules".to_owned(),
            ));
        }

        self.persistence.persist(decision).await
    }
}

fn policy_rule_denies(rule: Option<&PermissionRule>) -> bool {
    matches!(rule, Some(rule) if rule.source == RuleSource::Policy && matches!(rule.action, RuleAction::Deny))
}

impl RuleEngineBroker {
    fn is_dangerous_command(&self, request: &PermissionRequest) -> bool {
        let Some(library) = &self.dangerous_patterns else {
            return false;
        };
        let PermissionSubject::CommandExec { command, .. } = &request.subject else {
            return false;
        };

        library.detect(command).is_some()
    }
}

impl Drop for RuleEngineBroker {
    fn drop(&mut self) {
        if let Some(watch_task) = &self.watch_task {
            watch_task.abort();
        }
    }
}

fn spawn_watch_task(
    providers: Vec<Arc<dyn RuleProvider>>,
    tenant: TenantId,
    snapshot: Arc<ArcSwap<RuleSnapshot>>,
) -> Option<JoinHandle<()>> {
    let watches = providers
        .iter()
        .filter_map(|provider| provider.watch())
        .collect::<Vec<_>>();
    if watches.is_empty() {
        return None;
    }

    Some(tokio::spawn(async move {
        let mut updates = futures::stream::select_all(watches);
        while updates.next().await.is_some() {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let generation = snapshot.load().generation + 1;
            if let Ok(next_snapshot) = build_snapshot(&providers, tenant, generation).await {
                snapshot.store(Arc::new(next_snapshot));
            }
        }
    }))
}

async fn build_snapshot(
    providers: &[Arc<dyn RuleProvider>],
    tenant: TenantId,
    generation: u64,
) -> Result<RuleSnapshot, PermissionError> {
    let mut rules = Vec::new();
    for provider in providers {
        let provider_rules = provider.resolve_rules(tenant).await?;
        if provider.source() == RuleSource::Policy {
            validate_policy_provider(provider.provider_id(), &provider_rules)?;
        }
        rules.extend(provider_rules);
    }

    rules.sort_by(compare_rules);
    Ok(RuleSnapshot {
        rules,
        generation,
        built_at: Utc::now(),
    })
}

fn validate_policy_provider(
    provider_id: &str,
    rules: &[PermissionRule],
) -> Result<(), PermissionError> {
    if let Some(rule) = rules.iter().find(|rule| rule.source != RuleSource::Policy) {
        return Err(PermissionError::Message(format!(
            "Policy provider `{provider_id}` returned non-Policy rule `{}`",
            rule.id
        )));
    }
    Ok(())
}

fn select_rule<'a>(
    rules: &'a [PermissionRule],
    scope: &DecisionScope,
) -> Option<&'a PermissionRule> {
    if let Some(policy_deny) = rules.iter().find(|rule| {
        rule.source == RuleSource::Policy
            && policy_scope_matches_request(&rule.scope, scope)
            && matches!(rule.action, RuleAction::Deny)
    }) {
        return Some(policy_deny);
    }

    rules.iter().find(|rule| rule.scope == *scope)
}

fn compare_rules(left: &PermissionRule, right: &PermissionRule) -> std::cmp::Ordering {
    source_rank(right.source)
        .cmp(&source_rank(left.source))
        .then_with(|| right.priority.cmp(&left.priority))
        .then_with(|| left.id.cmp(&right.id))
}

fn source_rank(source: RuleSource) -> u8 {
    const USER_RANK: u8 = 0;
    const UNKNOWN_RANK: u8 = 0;

    match source {
        RuleSource::User => USER_RANK,
        RuleSource::Workspace => 1,
        RuleSource::Project => 2,
        RuleSource::Local => 3,
        RuleSource::Flag => 4,
        RuleSource::Policy => 5,
        RuleSource::CliArg => 6,
        RuleSource::Command => 7,
        RuleSource::Session => 8,
        _ => UNKNOWN_RANK,
    }
}

fn fallback_decision(
    fallback: FallbackPolicy,
    request: &PermissionRequest,
    _ctx: &PermissionContext,
) -> Decision {
    match fallback {
        FallbackPolicy::AskUser => Decision::Escalate,
        FallbackPolicy::AllowReadOnly => {
            if is_read_only_subject(&request.subject) {
                Decision::AllowOnce
            } else {
                Decision::DenyOnce
            }
        }
        _ => Decision::DenyOnce,
    }
}

fn is_read_only_subject(subject: &PermissionSubject) -> bool {
    match subject {
        PermissionSubject::CommandExec { command, argv, .. } => {
            is_read_only_command(command) && argv.iter().all(|arg| !is_mutating_arg(arg))
        }
        PermissionSubject::ToolInvocation { input, .. }
        | PermissionSubject::McpToolCall { input, .. } => input
            .get("read_only")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        _ => false,
    }
}

fn is_read_only_command(command: &str) -> bool {
    matches!(
        command.split_whitespace().next(),
        Some(
            "cat"
                | "cd"
                | "find"
                | "grep"
                | "head"
                | "ls"
                | "pwd"
                | "rg"
                | "sed"
                | "tail"
                | "test"
                | "wc"
        )
    )
}

fn is_mutating_arg(arg: &str) -> bool {
    matches!(
        arg,
        "-delete"
            | "-exec"
            | "-i"
            | "--in-place"
            | "--delete"
            | "--remove"
            | "--write"
            | "--output"
            | "-o"
    )
}
