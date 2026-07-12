use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

pub use harness_contracts::McpServerScope;
use harness_contracts::{ManifestOriginRef, McpServerId, McpServerSource, SessionId, TrustLevel};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::SamplingPolicy;

#[derive(Debug, Clone, PartialEq)]
pub struct McpServerSpec {
    pub server_id: McpServerId,
    pub display_name: String,
    pub transport: TransportChoice,
    pub auth: McpClientAuth,
    pub capabilities_expected: McpExpectedCapabilities,
    pub source: McpServerSource,
    pub manifest_origin: ManifestOriginRef,
    pub trust: TrustLevel,
    pub timeouts: McpTimeouts,
    pub reconnect: ReconnectPolicy,
    pub tool_filter: McpToolFilter,
    pub sampling: SamplingPolicy,
    pub resource_update_policy: McpResourceUpdatePolicy,
}

impl McpServerSpec {
    pub fn new(
        server_id: McpServerId,
        display_name: impl Into<String>,
        transport: TransportChoice,
        source: McpServerSource,
    ) -> Self {
        let trust = trust_level_for_source(&source);
        let manifest_origin = manifest_origin_for_source(&source);
        Self {
            server_id,
            display_name: display_name.into(),
            transport,
            auth: McpClientAuth::None,
            capabilities_expected: McpExpectedCapabilities::default(),
            source,
            manifest_origin,
            trust,
            timeouts: McpTimeouts::default(),
            reconnect: ReconnectPolicy::default(),
            tool_filter: McpToolFilter::default(),
            sampling: SamplingPolicy::denied(),
            resource_update_policy: McpResourceUpdatePolicy::default(),
        }
    }

    #[must_use]
    pub fn with_manifest_origin(mut self, manifest_origin: ManifestOriginRef) -> Self {
        self.manifest_origin = manifest_origin;
        self
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum McpServerRef {
    Shared(McpServerId),
    Inline(McpServerSpec),
    Required(McpServerId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerPattern {
    pub pattern: String,
    pub require_ready: bool,
    pub allow_inline: bool,
}

impl McpServerPattern {
    pub fn exact(server_id: McpServerId) -> Self {
        Self {
            pattern: server_id.0,
            require_ready: true,
            allow_inline: true,
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequiredEvaluation {
    Satisfied,
    Missing {
        pattern: String,
    },
    NotReady {
        server_id: McpServerId,
        state: crate::McpConnectionState,
    },
    InlineDisallowed {
        pattern: String,
        server_id: McpServerId,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum TransportChoice {
    Stdio {
        command: String,
        args: Vec<String>,
        env: StdioEnv,
        policy: StdioPolicy,
    },
    Http {
        url: String,
        headers: BTreeMap<String, String>,
    },
    WebSocket {
        url: String,
        headers: BTreeMap<String, String>,
    },
    Sse {
        url: String,
        headers: BTreeMap<String, String>,
    },
    InProcess,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpClientAuth {
    None,
    Bearer(String),
    OAuth {
        authorize_url: String,
        token_url: String,
        client_id: String,
        client_secret: String,
        scopes: Vec<String>,
        refresh_token: Option<String>,
    },
    Xaa {
        parent_session: SessionId,
        scopes: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpExpectedCapabilities {
    pub tools: bool,
    pub resources: bool,
    pub prompts: bool,
    pub logging: bool,
    pub completions: bool,
    pub tasks: bool,
}

impl Default for McpExpectedCapabilities {
    fn default() -> Self {
        Self {
            tools: true,
            resources: false,
            prompts: false,
            logging: false,
            completions: false,
            tasks: false,
        }
    }
}

impl McpExpectedCapabilities {
    pub(crate) fn missing_from(&self, offered: &crate::McpServerCapabilities) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.tools && offered.tools.is_none() {
            missing.push("tools");
        }
        if self.resources && offered.resources.is_none() {
            missing.push("resources");
        }
        if self.prompts && offered.prompts.is_none() {
            missing.push("prompts");
        }
        if self.logging && offered.logging.is_none() {
            missing.push("logging");
        }
        if self.completions && offered.completions.is_none() {
            missing.push("completions");
        }
        if self.tasks && offered.tasks.is_none() {
            missing.push("tasks");
        }
        missing
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpTimeouts {
    pub handshake: Duration,
    pub call_default: Duration,
    pub sampling: Duration,
    pub idle: Duration,
    pub cancel_ack: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McpResourceUpdatePolicy {
    pub max_updates_per_window: u32,
    pub window: Duration,
}

impl Default for McpResourceUpdatePolicy {
    fn default() -> Self {
        Self {
            max_updates_per_window: 120,
            window: Duration::from_secs(60),
        }
    }
}

impl Default for McpTimeouts {
    fn default() -> Self {
        Self {
            handshake: Duration::from_secs(5),
            call_default: Duration::from_secs(30),
            sampling: Duration::from_secs(60),
            idle: Duration::from_secs(300),
            cancel_ack: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReconnectPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub backoff_jitter: f32,
    pub success_reset_after: Duration,
    pub keep_deferred_during_reconnect: bool,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 0,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            backoff_jitter: 0.2,
            success_reset_after: Duration::from_secs(300),
            keep_deferred_during_reconnect: true,
        }
    }
}

impl ReconnectPolicy {
    pub fn validate(&self) -> Result<(), crate::McpError> {
        if self.initial_backoff.is_zero() {
            return Err(crate::McpError::Protocol(
                "reconnect initial_backoff must be greater than zero".into(),
            ));
        }
        if self.max_backoff.is_zero() {
            return Err(crate::McpError::Protocol(
                "reconnect max_backoff must be greater than zero".into(),
            ));
        }
        if self.initial_backoff > self.max_backoff {
            return Err(crate::McpError::Protocol(
                "reconnect initial_backoff must not exceed max_backoff".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.backoff_jitter) {
            return Err(crate::McpError::Protocol(
                "reconnect backoff_jitter must be in [0.0, 1.0]".into(),
            ));
        }
        Ok(())
    }

    pub fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let multiplier = 1_u32
            .checked_shl(attempt.saturating_sub(1).min(31))
            .unwrap_or(1);
        self.initial_backoff
            .saturating_mul(multiplier)
            .min(self.max_backoff)
    }

    pub fn is_exhausted(&self, attempts_so_far: u32) -> bool {
        self.max_attempts != 0 && attempts_so_far >= self.max_attempts
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdioEnv {
    Allowlist {
        inherit: BTreeSet<String>,
        extra: BTreeMap<String, String>,
    },
    InheritWithDeny {
        deny: BTreeSet<String>,
        extra: BTreeMap<String, String>,
    },
    Empty {
        extra: BTreeMap<String, String>,
    },
}

impl StdioEnv {
    pub fn default_deny_envs() -> BTreeSet<String> {
        [
            "OPENAI_API_KEY",
            "OPENAI_ORG",
            "ANTHROPIC_API_KEY",
            "GOOGLE_API_KEY",
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "AZURE_OPENAI_KEY",
            "AZURE_CLIENT_SECRET",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "KUBECONFIG",
            "KUBE_TOKEN",
            "GITHUB_TOKEN",
            "GITLAB_TOKEN",
            "DOCKER_AUTH_CONFIG",
            "NPM_TOKEN",
            "CARGO_REGISTRY_TOKEN",
            "HARNESS_*",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
}

impl Default for StdioEnv {
    fn default() -> Self {
        Self::InheritWithDeny {
            deny: Self::default_deny_envs(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdioPolicy {
    pub stderr_line_max_bytes: u32,
    pub redact_stderr: bool,
    pub graceful_kill_after: Duration,
    pub working_dir: Option<std::path::PathBuf>,
}

impl Default for StdioPolicy {
    fn default() -> Self {
        Self {
            stderr_line_max_bytes: 4096,
            redact_stderr: true,
            graceful_kill_after: Duration::from_secs(5),
            working_dir: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolFilter {
    pub allow: Vec<McpToolGlob>,
    pub deny: Vec<McpToolGlob>,
    pub on_conflict: FilterConflict,
}

impl Default for McpToolFilter {
    fn default() -> Self {
        Self {
            allow: Vec::new(),
            deny: Vec::new(),
            on_conflict: FilterConflict::DenyWins,
        }
    }
}

impl McpToolFilter {
    pub fn evaluate(&self, canonical_name: &str) -> FilterDecision {
        let allow_match = self.allow.iter().any(|glob| glob.matches(canonical_name));
        let deny_match = self.deny.iter().any(|glob| glob.matches(canonical_name));

        if allow_match && deny_match {
            return match self.on_conflict {
                FilterConflict::DenyWins => FilterDecision::Skip {
                    reason: "allow and deny matched; deny wins".to_owned(),
                },
                FilterConflict::AllowWins => FilterDecision::Inject,
                FilterConflict::Reject => FilterDecision::Reject {
                    reason: "allow and deny matched; reject configured".to_owned(),
                },
            };
        }

        if !self.allow.is_empty() && !allow_match {
            return FilterDecision::Skip {
                reason: "no allow glob matched".to_owned(),
            };
        }

        if deny_match {
            return FilterDecision::Skip {
                reason: "deny glob matched".to_owned(),
            };
        }

        FilterDecision::Inject
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolGlob(pub String);

impl McpToolGlob {
    pub fn matches(&self, candidate: &str) -> bool {
        glob_matches(&self.0, candidate)
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterConflict {
    DenyWins,
    AllowWins,
    Reject,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterDecision {
    Inject,
    Skip { reason: String },
    Reject { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolDescriptor {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<Vec<crate::McpIcon>>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<McpToolExecution>,
    #[serde(
        rename = "outputSchema",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub output_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<McpToolAnnotations>,
    #[serde(rename = "_meta", default)]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(
        rename = "readOnlyHint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub read_only_hint: Option<bool>,
    #[serde(
        rename = "destructiveHint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub destructive_hint: Option<bool>,
    #[serde(
        rename = "idempotentHint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub idempotent_hint: Option<bool>,
    #[serde(
        rename = "openWorldHint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub open_world_hint: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolExecution {
    #[serde(
        rename = "taskSupport",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub task_support: Option<McpTaskSupport>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTaskSupport {
    #[default]
    Forbidden,
    Optional,
    Required,
}

impl std::fmt::Display for McpTaskSupport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Forbidden => "forbidden",
            Self::Optional => "optional",
            Self::Required => "required",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolResult {
    pub content: Vec<McpContent>,
    #[serde(
        rename = "structuredContent",
        default,
        deserialize_with = "deserialize_optional_object",
        skip_serializing_if = "Option::is_none"
    )]
    pub structured_content: Option<Map<String, Value>>,
    #[serde(rename = "isError", default)]
    pub is_error: bool,
    #[serde(rename = "_meta", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, Value>,
}

fn deserialize_optional_object<'de, D>(
    deserializer: D,
) -> Result<Option<Map<String, Value>>, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::Object(object) => Ok(Some(object)),
        _ => Err(D::Error::custom("structuredContent must be a JSON object")),
    }
}

impl McpToolResult {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![McpContent::text(text)],
            structured_content: None,
            is_error: false,
            meta: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum McpContent {
    Text {
        text: String,
        annotations: Option<McpAnnotations>,
        meta: BTreeMap<String, Value>,
    },
    Image {
        data: String,
        mime_type: String,
        annotations: Option<McpAnnotations>,
        meta: BTreeMap<String, Value>,
    },
    Audio {
        data: String,
        mime_type: String,
        annotations: Option<McpAnnotations>,
        meta: BTreeMap<String, Value>,
    },
    ResourceLink {
        resource: Box<McpResource>,
    },
    Resource {
        resource: McpResourceContents,
        annotations: Option<McpAnnotations>,
        meta: BTreeMap<String, Value>,
    },
    Unknown(Value),
}

impl McpContent {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            annotations: None,
            meta: BTreeMap::new(),
        }
    }

    pub fn text_value(&self) -> Option<&str> {
        match self {
            Self::Text { text, .. } => Some(text),
            _ => None,
        }
    }
}

impl Serialize for McpContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = match self {
            Self::Text {
                text,
                annotations,
                meta,
            } => content_value(
                "text",
                [Some(("text", Value::String(text.clone())))],
                annotations,
                meta,
            ),
            Self::Image {
                data,
                mime_type,
                annotations,
                meta,
            } => content_value(
                "image",
                [
                    Some(("data", Value::String(data.clone()))),
                    Some(("mimeType", Value::String(mime_type.clone()))),
                ],
                annotations,
                meta,
            ),
            Self::Audio {
                data,
                mime_type,
                annotations,
                meta,
            } => content_value(
                "audio",
                [
                    Some(("data", Value::String(data.clone()))),
                    Some(("mimeType", Value::String(mime_type.clone()))),
                ],
                annotations,
                meta,
            ),
            Self::ResourceLink { resource } => {
                let mut value =
                    serde_json::to_value(resource).map_err(serde::ser::Error::custom)?;
                value
                    .as_object_mut()
                    .expect("MCP resource serializes as an object")
                    .insert("type".to_owned(), Value::String("resource_link".to_owned()));
                value
            }
            Self::Resource {
                resource,
                annotations,
                meta,
            } => content_value(
                "resource",
                [Some((
                    "resource",
                    serde_json::to_value(resource).map_err(serde::ser::Error::custom)?,
                ))],
                annotations,
                meta,
            ),
            Self::Unknown(value) => {
                let object = value.as_object().ok_or_else(|| {
                    serde::ser::Error::custom("unknown MCP content must be a JSON object")
                })?;
                let kind = object
                    .get("type")
                    .ok_or_else(|| serde::ser::Error::custom("unknown MCP content missing type"))?
                    .as_str()
                    .ok_or_else(|| {
                        serde::ser::Error::custom("unknown MCP content type must be a string")
                    })?;
                if matches!(
                    kind,
                    "text" | "image" | "audio" | "resource_link" | "resource"
                ) {
                    return Err(serde::ser::Error::custom(format!(
                        "known MCP content type {kind:?} must use its typed variant"
                    )));
                }
                value.clone()
            }
        };
        value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for McpContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("MCP content must be a JSON object"))?;
        let kind = object
            .get("type")
            .ok_or_else(|| D::Error::custom("MCP content missing type"))?
            .as_str()
            .ok_or_else(|| D::Error::custom("MCP content type must be a string"))?
            .to_owned();
        match kind.as_str() {
            "text" => {
                let fields: ContentFields =
                    serde_json::from_value(value).map_err(D::Error::custom)?;
                Ok(Self::Text {
                    text: fields
                        .text
                        .ok_or_else(|| D::Error::custom("text content missing text"))?,
                    annotations: fields.annotations,
                    meta: fields.meta,
                })
            }
            "image" | "audio" => {
                let fields: ContentFields =
                    serde_json::from_value(value).map_err(D::Error::custom)?;
                let data = fields
                    .data
                    .ok_or_else(|| D::Error::custom(format!("{kind} content missing data")))?;
                let mime_type = fields
                    .mime_type
                    .ok_or_else(|| D::Error::custom(format!("{kind} content missing mimeType")))?;
                if kind == "image" {
                    Ok(Self::Image {
                        data,
                        mime_type,
                        annotations: fields.annotations,
                        meta: fields.meta,
                    })
                } else {
                    Ok(Self::Audio {
                        data,
                        mime_type,
                        annotations: fields.annotations,
                        meta: fields.meta,
                    })
                }
            }
            "resource_link" => {
                let mut resource = value;
                resource
                    .as_object_mut()
                    .expect("checked object")
                    .remove("type");
                Ok(Self::ResourceLink {
                    resource: Box::new(serde_json::from_value(resource).map_err(D::Error::custom)?),
                })
            }
            "resource" => {
                let fields: ContentFields =
                    serde_json::from_value(value).map_err(D::Error::custom)?;
                Ok(Self::Resource {
                    resource: fields
                        .resource
                        .ok_or_else(|| D::Error::custom("embedded resource missing resource"))?,
                    annotations: fields.annotations,
                    meta: fields.meta,
                })
            }
            _ => Ok(Self::Unknown(value)),
        }
    }
}

fn content_value<const N: usize>(
    kind: &str,
    fields: [Option<(&str, Value)>; N],
    annotations: &Option<McpAnnotations>,
    meta: &BTreeMap<String, Value>,
) -> Value {
    let mut object = Map::new();
    object.insert("type".to_owned(), Value::String(kind.to_owned()));
    for (name, value) in fields.into_iter().flatten() {
        object.insert(name.to_owned(), value);
    }
    if let Some(annotations) = annotations {
        object.insert(
            "annotations".to_owned(),
            serde_json::to_value(annotations).expect("MCP annotations serialize"),
        );
    }
    if !meta.is_empty() {
        object.insert(
            "_meta".to_owned(),
            serde_json::to_value(meta).expect("MCP metadata serialize"),
        );
    }
    Value::Object(object)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContentFields {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    resource: Option<McpResourceContents>,
    #[serde(default)]
    annotations: Option<McpAnnotations>,
    #[serde(rename = "_meta", default)]
    meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<McpRole>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    #[serde(
        rename = "lastModified",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<Vec<crate::McpIcon>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<McpAnnotations>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(rename = "_meta", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum McpResourceContents {
    Text {
        uri: String,
        #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        text: String,
        #[serde(rename = "_meta", default, skip_serializing_if = "BTreeMap::is_empty")]
        meta: BTreeMap<String, Value>,
    },
    Blob {
        uri: String,
        #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        blob: String,
        #[serde(rename = "_meta", default, skip_serializing_if = "BTreeMap::is_empty")]
        meta: BTreeMap<String, Value>,
    },
}

impl<'de> Deserialize<'de> for McpResourceContents {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("resource contents must be a JSON object"))?;
        match (object.contains_key("text"), object.contains_key("blob")) {
            (true, true) => {
                return Err(D::Error::custom(
                    "resource contents must not contain both text and blob",
                ));
            }
            (false, false) => {
                return Err(D::Error::custom(
                    "resource contents must contain exactly one of text or blob",
                ));
            }
            _ => {}
        }
        let fields: ResourceContentsFields =
            serde_json::from_value(value).map_err(D::Error::custom)?;
        match (fields.text, fields.blob) {
            (Some(text), None) => Ok(Self::Text {
                uri: fields.uri,
                mime_type: fields.mime_type,
                text,
                meta: fields.meta,
            }),
            (None, Some(blob)) => Ok(Self::Blob {
                uri: fields.uri,
                mime_type: fields.mime_type,
                blob,
                meta: fields.meta,
            }),
            (None, None) | (Some(_), Some(_)) => Err(D::Error::custom(
                "resource text and blob values must be non-null strings",
            )),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResourceContentsFields {
    uri: String,
    #[serde(rename = "mimeType", default)]
    mime_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    blob: Option<String>,
    #[serde(rename = "_meta", default)]
    meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpReadResourceResult {
    pub contents: Vec<McpResourceContents>,
    #[serde(rename = "_meta", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<Vec<crate::McpIcon>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<McpPromptArgument>>,
    #[serde(rename = "_meta", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpPromptMessages {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<McpPromptMessage>,
    #[serde(rename = "_meta", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpPromptMessage {
    pub role: McpRole,
    pub content: McpContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPaginationLimits {
    pub max_pages: usize,
    pub max_items: usize,
}

impl Default for McpPaginationLimits {
    fn default() -> Self {
        Self {
            max_pages: 100,
            max_items: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpListPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

pub fn trust_level_for_source(source: &McpServerSource) -> TrustLevel {
    if matches!(
        source,
        McpServerSource::Workspace | McpServerSource::Policy | McpServerSource::Managed { .. }
    ) {
        TrustLevel::AdminTrusted
    } else {
        // Plugin source only carries PluginId here, not plugin trust. Fail closed until the
        // plugin registry supplies that trust during composition.
        TrustLevel::UserControlled
    }
}

fn manifest_origin_for_source(source: &McpServerSource) -> ManifestOriginRef {
    match source {
        McpServerSource::Plugin(plugin_id) => ManifestOriginRef::CargoExtension {
            binary: plugin_id.0.clone(),
        },
        McpServerSource::Managed { registry_url } => ManifestOriginRef::RemoteRegistry {
            endpoint: registry_url.clone(),
        },
        McpServerSource::Workspace => ManifestOriginRef::File {
            path: "workspace-mcp-config".to_owned(),
        },
        McpServerSource::Project => ManifestOriginRef::File {
            path: "project-mcp-config".to_owned(),
        },
        McpServerSource::User => ManifestOriginRef::File {
            path: "user-mcp-config".to_owned(),
        },
        McpServerSource::Policy => ManifestOriginRef::File {
            path: "policy-mcp-config".to_owned(),
        },
        McpServerSource::Dynamic { registered_by } => ManifestOriginRef::File {
            path: format!("dynamic-mcp-config:{registered_by}"),
        },
        _ => ManifestOriginRef::File {
            path: "mcp-config".to_owned(),
        },
    }
}

fn glob_matches(pattern: &str, candidate: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == candidate;
    }

    let mut remaining = candidate;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if index == 0 {
            let Some(stripped) = remaining.strip_prefix(part) else {
                return false;
            };
            remaining = stripped;
            continue;
        }

        let Some(position) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[position + part.len()..];
    }

    pattern.ends_with('*')
        || parts
            .last()
            .is_some_and(|last| remaining.is_empty() || last.is_empty())
}
