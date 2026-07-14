//! `jyowo-harness-sdk`
//!
//! Business-facing facade for the Jyowo Agent Harness SDK.
//!
//! Status: M7 facade.
//!
//! ```compile_fail
//! # async fn demo() {
//! let _ = jyowo_harness_sdk::Harness::builder().build().await;
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod builder;
pub mod builtin;
pub mod error;
pub mod ext;
pub mod harness;
pub mod options;
pub mod prelude;
pub mod session;
mod settings_runtime;
pub mod skill_config;
pub mod skill_pack_loader;
mod system_prompt;
#[cfg(feature = "testing")]
pub mod testing;

pub use builder::{HarnessBuilder, Set, Unset};
pub use error::HarnessError;
#[cfg(feature = "stream-permission")]
pub use harness::StreamPermissionRuntime;
pub use harness::{
    filter_unrouted_service_tools, ConversationEventsPage, ConversationEventsPageRequest,
    ConversationRunOptions, ConversationSession, ConversationSessionSummary,
    ConversationTurnReceipt, ConversationTurnRequest, Harness, HarnessOptions,
    HarnessSamplingProvider, McpConfig, RuntimeSkillConfig, RuntimeSkillParameter,
    RuntimeSkillScript, RuntimeSkillScriptEnv, RuntimeSkillSummary, RuntimeSkillView, TenantPolicy,
    WorkspaceCreateRequest,
};
pub use harness_agent_runtime::builtin_agent_profiles;
pub use harness_engine::{RunControl, RunControlHandle, TurnOutcome};
#[cfg(feature = "sqlite-store")]
pub use harness_journal::SqliteEvidenceRefRegistry;
pub use harness_journal::{
    AuditFilter, AuditOrder, AuditPage, AuditQuery, AuditRecord, AuditScope, EvidenceRefStore,
};
pub use harness_session::{BootstrapFileSpec, Workspace, WorkspaceBootstrap, WorkspaceSpec};
pub use harness_skill::{parse_skill_markdown, SkillSource};
pub use options::{
    ConfigError, ConfigSource, ConfigWarning, LastKnownGoodConfig, OptionsParseMode,
    ParsedHarnessOptions,
};
pub use session::{EventStream, RunContext, Session, SessionHandle, SessionOptions};
pub use settings_runtime::DesktopSettingsRuntime;
pub use skill_config::{
    apply_skill_config_statuses, validate_required_skill_config, KeyringSkillSecretStore,
    SkillConfigError, SkillConfigSnapshot, SkillConfigSnapshotResolver, SkillConfigStoreError,
    SkillSecretStore,
};
pub use skill_pack_loader::{
    LockedSkillPackFile, LockedSkillVersionSnapshot, SkillPackLoaderAdapter, SkillPackLoaderError,
};

pub use harness_contracts::{
    AgentId, ConversationAttachmentReference, ConversationContextReference, ConversationTurnInput,
    Event, MessageId, RunId, SessionId, TeamId, TenantId, ToolExecutionChannel, ToolUseId,
    TurnInput, WorkspaceId,
};
