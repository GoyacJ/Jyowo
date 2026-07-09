use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures::stream;
use harness_contracts::{
    BudgetMetric, CacheImpact, DeferPolicy, Event, OverflowAction, ProviderRestriction,
    ResultBudget, ToolActionPlan, ToolCapability, ToolDescriptor, ToolError, ToolGroup, ToolOrigin,
    ToolProperties, ToolResult, ToolSchemaMaterializedEvent, ToolSearchQueriedEvent,
    ToolSearchQueryKind, TrustLevel,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, PermissionCheck, Tool, ToolContext,
    ToolEvent, ToolStream, ValidationError,
};

use crate::{
    AnthropicToolReferenceBackend, DefaultBackendSelector, InlineReinjectionBackend,
    MaterializationCoalescer, MaterializeOutcome, ScoringContext, ScoringTerms,
    ToolLoadingBackendSelector, ToolLoadingContext, ToolSearchPreHookOutcome, ToolSearchRuntimeCap,
    TOOL_SEARCH_RUNTIME_CAPABILITY,
};

pub const TOOL_SEARCH_PROMPT: &str = r#"Fetches full schema definitions for deferred tools so they can be called.

Deferred tools appear by name in <deferred-tools> messages. Until fetched,
only the name is known - there is no parameter schema, so the tool cannot
be invoked. This tool takes a query, matches it against the deferred tool
list, and returns the matched tools' complete JSONSchema definitions.

Query forms:
- "select:Read,Edit,Grep" - fetch these exact tools by name (comma separated)
- "notebook jupyter"      - keyword search, up to max_results best matches
- "+slack send"           - require "slack" in the name, rank by remaining terms
"#;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolSearchInput {
    pub query: String,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ToolSearchOutput {
    pub matches: Vec<String>,
    pub explanations: Vec<ToolSearchMatchExplanation>,
    pub query: String,
    pub total_deferred_tools: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub pending_mcp_servers: Vec<String>,
    pub materialization: ToolSearchMaterialization,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ToolSearchMatchExplanation {
    pub tool_name: String,
    pub score: u32,
    pub matched_fields: Vec<String>,
    pub materialization_reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolSearchMaterialization {
    ToolReference {
        tool_names: Vec<String>,
        reason: String,
    },
    InlineReinjected {
        tool_names: Vec<String>,
        cache_impact: CacheImpact,
        reason: String,
    },
    NoMatch {
        reason: String,
    },
    BackendFailed {
        tool_names: Vec<String>,
        backend: String,
        reason: String,
    },
}

#[derive(Clone)]
pub struct ToolSearchTool {
    descriptor: ToolDescriptor,
    scorer: Arc<dyn crate::ToolSearchScorer>,
    backend_selector: Arc<dyn ToolLoadingBackendSelector>,
    default_max_results: usize,
}

impl ToolSearchTool {
    #[must_use]
    pub fn builder() -> ToolSearchToolBuilder {
        ToolSearchToolBuilder::default()
    }
}

pub struct ToolSearchToolBuilder {
    scorer: Option<Arc<dyn crate::ToolSearchScorer>>,
    backend_selector: Option<Arc<dyn ToolLoadingBackendSelector>>,
    coalesce_window: Duration,
    max_coalesce_batch: usize,
    default_max_results: usize,
}

impl Default for ToolSearchToolBuilder {
    fn default() -> Self {
        Self {
            scorer: None,
            backend_selector: None,
            coalesce_window: Duration::from_millis(50),
            max_coalesce_batch: 32,
            default_max_results: 5,
        }
    }
}

impl ToolSearchToolBuilder {
    #[must_use]
    pub fn with_scorer(mut self, scorer: Arc<dyn crate::ToolSearchScorer>) -> Self {
        self.scorer = Some(scorer);
        self
    }

    #[must_use]
    pub fn with_backend_selector(mut self, selector: Arc<dyn ToolLoadingBackendSelector>) -> Self {
        self.backend_selector = Some(selector);
        self
    }

    #[must_use]
    pub fn with_coalesce_window(mut self, window: Duration) -> Self {
        self.coalesce_window = window;
        self
    }

    #[must_use]
    pub fn with_max_coalesce_batch(mut self, max: usize) -> Self {
        self.max_coalesce_batch = max.max(1);
        self
    }

    #[must_use]
    pub fn with_default_max_results(mut self, max_results: usize) -> Self {
        self.default_max_results = max_results.clamp(1, 50);
        self
    }

    #[must_use]
    pub fn build(self) -> ToolSearchTool {
        ToolSearchTool {
            descriptor: tool_search_descriptor(),
            scorer: self
                .scorer
                .unwrap_or_else(|| Arc::new(crate::DefaultScorer::default())),
            backend_selector: self.backend_selector.unwrap_or_else(|| {
                Arc::new(DefaultBackendSelector::new(
                    Arc::new(AnthropicToolReferenceBackend),
                    Arc::new(InlineReinjectionBackend::new(
                        MaterializationCoalescer::new(
                            self.coalesce_window,
                            self.max_coalesce_batch,
                        ),
                    )),
                ))
            }),
            default_max_results: self.default_max_results,
        }
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        parse_input(input, self.default_max_results).map(|_| ())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::Allowed,
            Vec::new(),
            harness_contracts::WorkspaceAccess::None,
            harness_contracts::NetworkAccess::None,
            harness_contracts::ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let mut input = parse_input(&input, self.default_max_results)
            .map_err(|error| ToolError::Validation(error.to_string()))?;
        let runtime = ctx.capability::<dyn ToolSearchRuntimeCap>(ToolCapability::Custom(
            TOOL_SEARCH_RUNTIME_CAPABILITY.to_owned(),
        ))?;
        let snapshot = runtime.snapshot().await?;
        let max_results = input
            .max_results
            .unwrap_or(self.default_max_results)
            .clamp(1, 50);
        let mut current_query_kind = query_kind(&input.query);

        let pre = runtime
            .dispatch_pre_tool_search_hook(&ctx, ctx.tool_use_id, &input.query, current_query_kind)
            .await?;
        match pre {
            ToolSearchPreHookOutcome::Continue => {}
            ToolSearchPreHookOutcome::Block { reason } => {
                return Err(ToolError::PermissionDenied(reason));
            }
            ToolSearchPreHookOutcome::RewriteInput(value) => {
                let before = input_value(&input);
                input = parse_rewritten_input(value, self.default_max_results)?;
                current_query_kind = query_kind(&input.query);
                runtime
                    .emit_event(Event::HookRewroteInput(
                        harness_contracts::HookRewroteInputEvent {
                            tool_use_id: ctx.tool_use_id,
                            before_hash: hash_value(&before),
                            after_hash: hash_value(&input_value(&input)),
                            causation_id: harness_contracts::EventId::new(),
                            at: harness_contracts::now(),
                        },
                    ))
                    .await?;
            }
        }

        let discovered = Arc::new(snapshot.discovered_tool_names.iter().cloned().collect());
        let mut scored = Vec::<(String, u32)>::new();
        let matches = if let Some(requested) = parse_select(&input.query) {
            select_matches(&requested, &snapshot)
        } else {
            let terms = ScoringTerms::parse(&input.query);
            let context = ScoringContext { discovered };
            for descriptor in &snapshot.deferred_tools {
                let score = self
                    .scorer
                    .score(descriptor, &descriptor.properties, &terms, &context)
                    .await;
                if score > 0 {
                    scored.push((descriptor.name.clone(), score));
                }
            }
            scored.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
            scored
                .iter()
                .take(max_results)
                .map(|(name, _)| name.clone())
                .collect()
        };

        let deferred_names = snapshot
            .deferred_tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<HashSet<_>>();
        let materialize_names = matches
            .iter()
            .filter(|name| deferred_names.contains(*name))
            .cloned()
            .collect::<Vec<_>>();
        let truncated = matches.len() >= max_results && scored.len() > max_results;

        runtime
            .emit_event(Event::ToolSearchQueried(ToolSearchQueriedEvent {
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                tool_use_id: ctx.tool_use_id,
                query: input.query.clone(),
                query_kind: current_query_kind,
                scored: scored.clone(),
                matched: matches.clone(),
                truncated_by_max_results: truncated,
                at: Utc::now(),
            }))
            .await?;

        let materialization = if materialize_names.is_empty() {
            ToolSearchMaterialization::NoMatch {
                reason: "query matched no deferred tools to materialize".to_owned(),
            }
        } else {
            let loading_ctx = ToolLoadingContext {
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                model_caps: snapshot.model_caps.clone(),
                reload_handle: snapshot.reload_handle.clone(),
            };
            let backend = self.backend_selector.select(&loading_ctx).await;
            let backend_name = backend.backend_name();
            match backend.materialize(&loading_ctx, &materialize_names).await {
                Ok(outcome) => {
                    let (
                        materialization,
                        cache_impact,
                        coalesced_count,
                        triggered_session_reload,
                    ) = match outcome {
                        MaterializeOutcome::ToolReferenceEmitted { refs } => (
                            ToolSearchMaterialization::ToolReference {
                                tool_names: refs
                                    .into_iter()
                                    .map(|reference| reference.tool_name)
                                    .collect(),
                                reason: format!(
                                    "{backend_name} selected because the model supports tool references"
                                ),
                            },
                            CacheImpact {
                                prompt_cache_invalidated: false,
                                reason: None,
                            },
                            0,
                            false,
                        ),
                        MaterializeOutcome::InlineReinjected {
                            tools,
                            cache_impact,
                        } => (
                            ToolSearchMaterialization::InlineReinjected {
                                tool_names: tools,
                                cache_impact: cache_impact.clone(),
                                reason: format!(
                                    "{backend_name} selected because schemas must be reinjected inline"
                                ),
                            },
                            cache_impact,
                            materialize_names.len() as u32,
                            true,
                        ),
                    };
                    let materialized_event = ToolSchemaMaterializedEvent {
                        session_id: ctx.session_id,
                        run_id: ctx.run_id,
                        tool_use_id: ctx.tool_use_id,
                        names: materialize_names,
                        backend: backend_name,
                        cache_impact,
                        triggered_session_reload,
                        coalesced_count,
                        at: Utc::now(),
                    };
                    runtime
                        .emit_event(Event::ToolSchemaMaterialized(materialized_event.clone()))
                        .await?;
                    runtime
                        .dispatch_post_tool_search_hook(
                            &ctx,
                            ctx.tool_use_id,
                            materialized_event.names,
                            materialized_event.backend,
                            materialized_event.cache_impact,
                        )
                        .await?;
                    materialization
                }
                Err(error) => ToolSearchMaterialization::BackendFailed {
                    tool_names: materialize_names,
                    backend: backend_name,
                    reason: error.to_string(),
                },
            }
        };

        let output = ToolSearchOutput {
            explanations: explain_matches(
                &matches,
                &scored,
                &snapshot.deferred_tools,
                &input.query,
            ),
            matches,
            query: input.query,
            total_deferred_tools: snapshot.deferred_tools.len(),
            pending_mcp_servers: snapshot.pending_mcp_servers,
            materialization,
        };
        let value =
            serde_json::to_value(output).map_err(|error| ToolError::Internal(error.to_string()))?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(value),
        )])))
    }
}

fn parse_rewritten_input(
    value: Value,
    default_max_results: usize,
) -> Result<ToolSearchInput, ToolError> {
    match value {
        Value::String(query) => parse_input(&json!({ "query": query }), default_max_results),
        object => parse_input(&object, default_max_results),
    }
    .map_err(|error| ToolError::Validation(error.to_string()))
}

fn input_value(input: &ToolSearchInput) -> Value {
    json!({
        "query": input.query,
        "max_results": input.max_results,
    })
}

fn hash_value(value: &Value) -> [u8; 32] {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    *blake3::hash(&bytes).as_bytes()
}

fn parse_input(
    input: &Value,
    default_max_results: usize,
) -> Result<ToolSearchInput, ValidationError> {
    let mut parsed: ToolSearchInput = serde_json::from_value(input.clone())
        .map_err(|error| ValidationError::from(error.to_string()))?;
    if parsed.query.trim().is_empty() {
        return Err(ValidationError::from("query is required"));
    }
    parsed.query = parsed.query.trim().to_owned();
    parsed.max_results = Some(
        parsed
            .max_results
            .unwrap_or(default_max_results)
            .clamp(1, 50),
    );
    Ok(parsed)
}

fn query_kind(query: &str) -> ToolSearchQueryKind {
    if parse_select(query).is_some() {
        ToolSearchQueryKind::Select
    } else {
        ToolSearchQueryKind::Keyword
    }
}

fn explain_matches(
    matches: &[String],
    scored: &[(String, u32)],
    descriptors: &[ToolDescriptor],
    query: &str,
) -> Vec<ToolSearchMatchExplanation> {
    let terms = ScoringTerms::parse(query);
    matches
        .iter()
        .map(|name| {
            let descriptor = descriptors
                .iter()
                .find(|descriptor| descriptor.name == *name);
            let score = scored
                .iter()
                .find_map(|(scored_name, score)| (scored_name == name).then_some(*score))
                .unwrap_or_default();
            let matched_fields = descriptor.map_or_else(
                || vec!["select".to_owned()],
                |descriptor| matched_fields(descriptor, &terms),
            );
            ToolSearchMatchExplanation {
                tool_name: name.clone(),
                score,
                matched_fields,
                materialization_reason: "matched deferred tool selected for materialization"
                    .to_owned(),
            }
        })
        .collect()
}

fn matched_fields(descriptor: &ToolDescriptor, terms: &ScoringTerms) -> Vec<String> {
    let all_terms = terms
        .required
        .iter()
        .chain(terms.optional.iter())
        .collect::<Vec<_>>();
    if all_terms.is_empty() {
        return vec!["select".to_owned()];
    }
    let name = descriptor.name.to_ascii_lowercase();
    let name_parts = crate::parse_tool_name_parts(&descriptor.name);
    let description = descriptor.description.to_ascii_lowercase();
    let search_hint = descriptor
        .search_hint
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let capabilities = descriptor
        .required_capabilities
        .iter()
        .map(|capability| format!("{capability:?}").to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let mut fields = Vec::new();
    if all_terms.iter().any(|term| {
        name.contains(term.as_str()) || name_parts.iter().any(|part| part.contains(term.as_str()))
    }) {
        fields.push("name".to_owned());
    }
    if all_terms
        .iter()
        .any(|term| description.contains(term.as_str()))
    {
        fields.push("description".to_owned());
    }
    if all_terms
        .iter()
        .any(|term| search_hint.contains(term.as_str()))
    {
        fields.push("search_hint".to_owned());
    }
    if all_terms
        .iter()
        .any(|term| capabilities.contains(term.as_str()))
    {
        fields.push("required_capabilities".to_owned());
    }
    if fields.is_empty() {
        fields.push("select".to_owned());
    }
    fields
}

fn parse_select(query: &str) -> Option<Vec<String>> {
    let rest = query.trim().strip_prefix("select:")?;
    Some(
        rest.split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

fn select_matches(
    requested: &[String],
    snapshot: &crate::ToolSearchRuntimeSnapshot,
) -> Vec<String> {
    let deferred = snapshot
        .deferred_tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<HashSet<_>>();
    requested
        .iter()
        .filter(|name| deferred.contains(*name) || snapshot.loaded_tool_names.contains(*name))
        .cloned()
        .collect()
}

fn tool_search_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "tool_search".to_owned(),
        display_name: "Tool Search".to_owned(),
        description: TOOL_SEARCH_PROMPT.to_owned(),
        category: "meta".to_owned(),
        group: ToolGroup::Meta,
        version: "1.0.0".to_owned(),
        input_schema: json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": { "type": "string" },
                "max_results": { "type": "integer", "minimum": 1, "maximum": 50 }
            },
            "additionalProperties": false
        }),
        output_schema: Some(json!({
            "type": "object",
            "required": ["matches", "explanations", "query", "total_deferred_tools", "materialization"],
            "properties": {
                "matches": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "explanations": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["tool_name", "score", "matched_fields", "materialization_reason"],
                        "properties": {
                            "tool_name": { "type": "string" },
                            "score": { "type": "integer", "minimum": 0 },
                            "matched_fields": {
                                "type": "array",
                                "items": { "type": "string" }
                            },
                            "materialization_reason": { "type": "string" }
                        }
                    }
                },
                "query": { "type": "string" },
                "total_deferred_tools": {
                    "type": "integer",
                    "minimum": 0
                },
                "pending_mcp_servers": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "materialization": {
                    "oneOf": [
                        {
                            "type": "object",
                            "required": ["kind", "tool_names", "reason"],
                            "properties": {
                                "kind": { "const": "tool_reference" },
                                "tool_names": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "reason": { "type": "string" }
                            }
                        },
                        {
                            "type": "object",
                            "required": ["kind", "tool_names", "cache_impact", "reason"],
                            "properties": {
                                "kind": { "const": "inline_reinjected" },
                                "tool_names": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "cache_impact": { "type": "object" },
                                "reason": { "type": "string" }
                            }
                        },
                        {
                            "type": "object",
                            "required": ["kind", "reason"],
                            "properties": {
                                "kind": { "const": "no_match" },
                                "reason": { "type": "string" }
                            }
                        },
                        {
                            "type": "object",
                            "required": ["kind", "tool_names", "backend", "reason"],
                            "properties": {
                                "kind": { "const": "backend_failed" },
                                "tool_names": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "backend": { "type": "string" },
                                "reason": { "type": "string" }
                            }
                        }
                    ]
                }
            }
        })),
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: None,
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities: Vec::new(),
        budget: ResultBudget {
            metric: BudgetMetric::Bytes,
            limit: 32 * 1024,
            on_overflow: OverflowAction::Offload,
            preview_head_chars: 2_000,
            preview_tail_chars: 2_000,
        },
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: None,
        service_binding: None,
        metadata: Default::default(),
    }
}
