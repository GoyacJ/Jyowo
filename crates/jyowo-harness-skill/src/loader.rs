use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    ContentHash, Event, McpServerId, PluginId, RunId, SessionId, SkillLoadedEvent,
    SkillPrerequisiteAdvisoryEvent, SkillPrerequisiteMissingEvent, SkillRejectedEvent,
    SkillRejectionReason as EventSkillRejectionReason, SkillThreatDetectedEvent, ThreatAction,
    TrustLevel,
};
#[cfg(feature = "threat-scanner")]
use harness_memory::MemoryThreatScanner;

use crate::{
    parse_skill_markdown_with_options, McpSkillRecord, McpSource, Skill, SkillCompatMode,
    SkillError, SkillHookTransport, SkillPlatform, SkillRegistration, SkillRejectReason,
    SkillRejection, SkillSource,
};

pub const DEFAULT_SHELL_ALLOWLIST: &[&str] = &["pwd", "date", "whoami", "hostname", "uname"];

#[derive(Clone)]
pub struct SkillLoader {
    sources: Vec<SkillSourceConfig>,
    runtime_platform: SkillPlatform,
    shell_allowlist: Vec<String>,
    max_shell_output: usize,
    compat_mode: SkillCompatMode,
    event_sink: Option<Arc<dyn SkillEventSink>>,
    event_scope: SkillThreatEventScope,
    metrics_sink: Option<Arc<dyn SkillMetricsSink>>,
    #[cfg(feature = "threat-scanner")]
    threat_scanner: Option<Arc<MemoryThreatScanner>>,
}

#[derive(Clone)]
pub struct SkillValidator {
    runtime_platform: SkillPlatform,
    event_sink: Option<Arc<dyn SkillEventSink>>,
    event_scope: SkillThreatEventScope,
    metrics_sink: Option<Arc<dyn SkillMetricsSink>>,
    #[cfg(feature = "threat-scanner")]
    threat_scanner: Option<Arc<MemoryThreatScanner>>,
}

#[derive(Debug, Clone)]
pub enum SkillSourceConfig {
    BundledRecords {
        records: Vec<BundledSkillRecord>,
    },
    Directory {
        path: PathBuf,
        source_kind: DirectorySourceKind,
    },
    DirectoryPackages {
        path: PathBuf,
        source_kind: DirectorySourceKind,
    },
    McpRecords {
        server_id: McpServerId,
        records: Vec<McpSkillRecord>,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BundledSkillRecord {
    pub name: String,
    pub description: String,
    pub body: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DirectorySourceKind {
    Workspace,
    User,
    Plugin {
        plugin_id: PluginId,
        trust: TrustLevel,
    },
}

#[derive(Debug, Clone)]
pub struct LoadReport {
    pub loaded: Vec<Skill>,
    pub rejected: Vec<SkillRejection>,
}

#[derive(Debug, Clone)]
enum PrefetchLoadUnit {
    Bundled {
        record: BundledSkillRecord,
    },
    Mcp {
        server_id: McpServerId,
        record: McpSkillRecord,
    },
    Directory {
        raw_path: PathBuf,
        source: SkillSource,
    },
}

enum PrefetchUnitOutcome {
    Loaded(Skill),
    Rejected(SkillRejection),
    Skipped,
}

#[async_trait]
pub trait SkillEventSink: Send + Sync + 'static {
    async fn emit(&self, event: Event);
}

pub trait SkillMetricsSink: Send + Sync + 'static {
    fn skill_loaded(&self, _source: &str) {}
    fn skill_rejected(&self, _reason: &str) {}
    fn skill_render_duration_ms(&self, _duration_ms: u64) {}
    fn skill_invocation(&self, _skill_name: &str) {}
    fn skill_view(&self, _skill_name: &str) {}
    fn skill_shell_invocation(&self, _command: &str) {}
    fn skill_shell_blocked(&self, _command: &str) {}
    fn skill_threat_detected(&self, _category: &str) {}
    fn skill_prerequisite_missing(&self, _skill_name: &str) {}
    fn skill_prerequisite_advisory(&self, _skill_name: &str) {}
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SkillThreatEventScope {
    pub session_id: Option<SessionId>,
    pub run_id: Option<RunId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRenderPolicy {
    pub shell_allowlist: Vec<String>,
    pub max_shell_output: usize,
}

impl Default for SkillRenderPolicy {
    fn default() -> Self {
        Self {
            shell_allowlist: DEFAULT_SHELL_ALLOWLIST
                .iter()
                .map(ToString::to_string)
                .collect(),
            max_shell_output: 4_000,
        }
    }
}

impl Default for SkillLoader {
    fn default() -> Self {
        let policy = SkillRenderPolicy::default();
        Self {
            sources: Vec::new(),
            runtime_platform: current_platform(),
            shell_allowlist: policy.shell_allowlist,
            max_shell_output: policy.max_shell_output,
            compat_mode: SkillCompatMode::Lenient,
            event_sink: None,
            event_scope: SkillThreatEventScope::default(),
            metrics_sink: None,
            #[cfg(feature = "threat-scanner")]
            threat_scanner: Some(Arc::new(MemoryThreatScanner::default())),
        }
    }
}

impl Default for SkillValidator {
    fn default() -> Self {
        Self {
            runtime_platform: current_platform(),
            event_sink: None,
            event_scope: SkillThreatEventScope::default(),
            metrics_sink: None,
            #[cfg(feature = "threat-scanner")]
            threat_scanner: Some(Arc::new(MemoryThreatScanner::default())),
        }
    }
}

impl SkillLoader {
    #[must_use]
    pub fn with_source(mut self, source: SkillSourceConfig) -> Self {
        self.sources.push(source);
        self
    }

    #[must_use]
    pub fn with_runtime_platform(mut self, platform: SkillPlatform) -> Self {
        self.runtime_platform = platform;
        self
    }

    #[must_use]
    pub fn with_shell_allowlist(mut self, cmds: impl IntoIterator<Item = String>) -> Self {
        self.shell_allowlist = cmds.into_iter().collect();
        self
    }

    #[must_use]
    pub fn with_max_shell_output(mut self, max_shell_output: usize) -> Self {
        self.max_shell_output = max_shell_output;
        self
    }

    #[must_use]
    pub fn with_compat_mode(mut self, compat_mode: SkillCompatMode) -> Self {
        self.compat_mode = compat_mode;
        self
    }

    #[cfg(feature = "threat-scanner")]
    #[must_use]
    pub fn with_threat_scanner(mut self, scanner: Arc<MemoryThreatScanner>) -> Self {
        self.threat_scanner = Some(scanner);
        self
    }

    #[must_use]
    pub fn with_event_sink(mut self, event_sink: Arc<dyn SkillEventSink>) -> Self {
        self.event_sink = Some(event_sink);
        self
    }

    #[must_use]
    pub fn with_event_scope(mut self, scope: SkillThreatEventScope) -> Self {
        self.event_scope = scope;
        self
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, metrics_sink: Arc<dyn SkillMetricsSink>) -> Self {
        self.metrics_sink = Some(metrics_sink);
        self
    }

    #[must_use]
    pub fn render_policy(&self) -> SkillRenderPolicy {
        SkillRenderPolicy {
            shell_allowlist: self.shell_allowlist.clone(),
            max_shell_output: self.max_shell_output,
        }
    }

    #[must_use]
    pub fn validator(&self) -> SkillValidator {
        let mut validator = SkillValidator::default().with_runtime_platform(self.runtime_platform);
        if let Some(event_sink) = &self.event_sink {
            validator = validator.with_event_sink(Arc::clone(event_sink));
        }
        validator = validator.with_event_scope(self.event_scope);
        if let Some(metrics_sink) = &self.metrics_sink {
            validator = validator.with_metrics_sink(Arc::clone(metrics_sink));
        }
        #[cfg(feature = "threat-scanner")]
        if let Some(scanner) = &self.threat_scanner {
            validator = validator.with_threat_scanner(Arc::clone(scanner));
        }
        validator
    }

    pub async fn load_all(&self) -> Result<LoadReport, SkillError> {
        let mut loaded = Vec::new();
        let mut rejected = Vec::new();

        for source in &self.sources {
            match source {
                SkillSourceConfig::BundledRecords { records } => {
                    for record in records {
                        let source = SkillSource::Bundled;
                        let skill = parse_skill_markdown_with_options(
                            &record.to_markdown(),
                            source,
                            None,
                            self.runtime_platform,
                            self.compat_mode,
                        )?;
                        let skill = self
                            .validate_loaded_skill(skill, None)
                            .await
                            .map_err(skill_error_from_rejection)?;
                        self.emit_loaded(&skill).await;
                        loaded.push(skill);
                    }
                }
                SkillSourceConfig::McpRecords { server_id, records } => {
                    let report = McpSource::new(server_id.clone(), records.clone())
                        .load_with_options(self.runtime_platform, self.compat_mode)
                        .await?;
                    for rejection in report.rejected {
                        self.emit_rejected(&rejection).await;
                        rejected.push(rejection);
                    }
                    for skill in report.loaded {
                        match self.validate_loaded_skill(skill, None).await {
                            Ok(skill) => {
                                self.emit_loaded(&skill).await;
                                loaded.push(skill);
                            }
                            Err(rejection) => {
                                self.emit_rejected(&rejection).await;
                                rejected.push(rejection);
                            }
                        }
                    }
                }
                SkillSourceConfig::Directory { path, source_kind } => {
                    if !path.exists() {
                        continue;
                    }
                    for raw_path in directory_skill_paths(path, true)? {
                        let source = source_from_directory(path.clone(), source_kind);
                        let markdown = std::fs::read_to_string(&raw_path)?;
                        match parse_skill_markdown_with_options(
                            &markdown,
                            source.clone(),
                            Some(raw_path.clone()),
                            self.runtime_platform,
                            self.compat_mode,
                        ) {
                            Ok(skill) => {
                                match self.validate_loaded_skill(skill, Some(&raw_path)).await {
                                    Ok(skill) => {
                                        self.emit_loaded(&skill).await;
                                        loaded.push(skill);
                                    }
                                    Err(rejection) => {
                                        self.emit_rejected(&rejection).await;
                                        rejected.push(rejection);
                                    }
                                }
                            }
                            Err(error) => {
                                let rejection = SkillRejection {
                                    source,
                                    raw_path: Some(raw_path),
                                    reason: SkillRejectReason::from_error(&error),
                                };
                                self.emit_rejected(&rejection).await;
                                rejected.push(rejection);
                            }
                        }
                    }
                }
                SkillSourceConfig::DirectoryPackages { path, source_kind } => {
                    if !path.exists() {
                        continue;
                    }
                    for raw_path in directory_skill_paths(path, false)? {
                        let source = source_from_directory(path.clone(), source_kind);
                        let markdown = std::fs::read_to_string(&raw_path)?;
                        match parse_skill_markdown_with_options(
                            &markdown,
                            source.clone(),
                            Some(raw_path.clone()),
                            self.runtime_platform,
                            self.compat_mode,
                        ) {
                            Ok(skill) => {
                                match self.validate_loaded_skill(skill, Some(&raw_path)).await {
                                    Ok(skill) => {
                                        self.emit_loaded(&skill).await;
                                        loaded.push(skill);
                                    }
                                    Err(rejection) => {
                                        self.emit_rejected(&rejection).await;
                                        rejected.push(rejection);
                                    }
                                }
                            }
                            Err(error) => {
                                let rejection = SkillRejection {
                                    source,
                                    raw_path: Some(raw_path),
                                    reason: SkillRejectReason::from_error(&error),
                                };
                                self.emit_rejected(&rejection).await;
                                rejected.push(rejection);
                            }
                        }
                    }
                }
            }
        }

        Ok(LoadReport { loaded, rejected })
    }

    pub async fn load_prefetch_batch(
        &self,
        hints: Option<&[String]>,
        limit: Option<usize>,
    ) -> Result<LoadReport, SkillError> {
        if let Some(limit) = limit {
            return self.load_prefetch_batch_concurrent(hints, limit).await;
        }

        let mut loaded = Vec::new();
        let mut rejected = Vec::new();

        for source in &self.sources {
            if reached_limit(loaded.len(), limit) {
                break;
            }
            match source {
                SkillSourceConfig::BundledRecords { records } => {
                    for record in records {
                        if reached_limit(loaded.len(), limit) {
                            break;
                        }
                        if !matches_hint(&record.name, hints) {
                            continue;
                        }
                        let skill = parse_skill_markdown_with_options(
                            &record.to_markdown(),
                            SkillSource::Bundled,
                            None,
                            self.runtime_platform,
                            self.compat_mode,
                        )?;
                        let skill = self
                            .validate_loaded_skill(skill, None)
                            .await
                            .map_err(skill_error_from_rejection)?;
                        self.emit_loaded(&skill).await;
                        loaded.push(skill);
                    }
                }
                SkillSourceConfig::McpRecords { server_id, records } => {
                    for record in records {
                        if reached_limit(loaded.len(), limit) {
                            break;
                        }
                        let name = format!("mcp__{}__{}", server_id.0, record.name);
                        if !matches_hint(&name, hints) {
                            continue;
                        }
                        let source = SkillSource::Mcp(server_id.clone());
                        let markdown = format!(
                            "---\nname: {}\ndescription: {}\n---\n{}",
                            yaml_quoted_scalar(&name),
                            yaml_quoted_scalar(&record.description),
                            record.body
                        );
                        match parse_skill_markdown_with_options(
                            &markdown,
                            source.clone(),
                            None,
                            self.runtime_platform,
                            self.compat_mode,
                        ) {
                            Ok(skill) => match self.validate_loaded_skill(skill, None).await {
                                Ok(skill) => {
                                    self.emit_loaded(&skill).await;
                                    loaded.push(skill);
                                }
                                Err(rejection) => {
                                    self.emit_rejected(&rejection).await;
                                    rejected.push(rejection);
                                }
                            },
                            Err(error) => {
                                let rejection = SkillRejection {
                                    source,
                                    raw_path: None,
                                    reason: SkillRejectReason::from_error(&error),
                                };
                                self.emit_rejected(&rejection).await;
                                rejected.push(rejection);
                            }
                        }
                    }
                }
                SkillSourceConfig::Directory { path, source_kind } => {
                    if !path.exists() {
                        continue;
                    }
                    for raw_path in directory_skill_paths(path, true)? {
                        if reached_limit(loaded.len(), limit) {
                            break;
                        }
                        let source = source_from_directory(path.clone(), source_kind);
                        let markdown = std::fs::read_to_string(&raw_path)?;
                        match parse_skill_markdown_with_options(
                            &markdown,
                            source.clone(),
                            Some(raw_path.clone()),
                            self.runtime_platform,
                            self.compat_mode,
                        ) {
                            Ok(skill) => {
                                if !matches_hint(&skill.name, hints) {
                                    continue;
                                }
                                match self.validate_loaded_skill(skill, Some(&raw_path)).await {
                                    Ok(skill) => {
                                        self.emit_loaded(&skill).await;
                                        loaded.push(skill);
                                    }
                                    Err(rejection) => {
                                        self.emit_rejected(&rejection).await;
                                        rejected.push(rejection);
                                    }
                                }
                            }
                            Err(error) => {
                                let rejection = SkillRejection {
                                    source,
                                    raw_path: Some(raw_path),
                                    reason: SkillRejectReason::from_error(&error),
                                };
                                self.emit_rejected(&rejection).await;
                                rejected.push(rejection);
                            }
                        }
                    }
                }
                SkillSourceConfig::DirectoryPackages { path, source_kind } => {
                    if !path.exists() {
                        continue;
                    }
                    for raw_path in directory_skill_paths(path, false)? {
                        if reached_limit(loaded.len(), limit) {
                            break;
                        }
                        let source = source_from_directory(path.clone(), source_kind);
                        let markdown = std::fs::read_to_string(&raw_path)?;
                        match parse_skill_markdown_with_options(
                            &markdown,
                            source.clone(),
                            Some(raw_path.clone()),
                            self.runtime_platform,
                            self.compat_mode,
                        ) {
                            Ok(skill) => {
                                if !matches_hint(&skill.name, hints) {
                                    continue;
                                }
                                match self.validate_loaded_skill(skill, Some(&raw_path)).await {
                                    Ok(skill) => {
                                        self.emit_loaded(&skill).await;
                                        loaded.push(skill);
                                    }
                                    Err(rejection) => {
                                        self.emit_rejected(&rejection).await;
                                        rejected.push(rejection);
                                    }
                                }
                            }
                            Err(error) => {
                                let rejection = SkillRejection {
                                    source,
                                    raw_path: Some(raw_path),
                                    reason: SkillRejectReason::from_error(&error),
                                };
                                self.emit_rejected(&rejection).await;
                                rejected.push(rejection);
                            }
                        }
                    }
                }
            }
        }

        Ok(LoadReport { loaded, rejected })
    }

    async fn load_prefetch_batch_concurrent(
        &self,
        hints: Option<&[String]>,
        concurrency: usize,
    ) -> Result<LoadReport, SkillError> {
        if concurrency == 0 {
            return Ok(LoadReport {
                loaded: Vec::new(),
                rejected: Vec::new(),
            });
        }

        let units = self.collect_prefetch_units(hints, concurrency)?;
        let outcomes = futures::stream::iter(units)
            .map(|unit| async move { self.load_prefetch_unit(unit, hints).await })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;

        let mut loaded = Vec::new();
        let mut rejected = Vec::new();
        for outcome in outcomes {
            match outcome? {
                PrefetchUnitOutcome::Loaded(skill) => loaded.push(skill),
                PrefetchUnitOutcome::Rejected(rejection) => rejected.push(rejection),
                PrefetchUnitOutcome::Skipped => {}
            }
        }

        Ok(LoadReport { loaded, rejected })
    }

    fn collect_prefetch_units(
        &self,
        hints: Option<&[String]>,
        max_units: usize,
    ) -> Result<Vec<PrefetchLoadUnit>, SkillError> {
        let mut units = Vec::new();
        for source in &self.sources {
            if units.len() >= max_units {
                break;
            }
            match source {
                SkillSourceConfig::BundledRecords { records } => {
                    for record in records {
                        if units.len() >= max_units {
                            break;
                        }
                        if matches_hint(&record.name, hints) {
                            units.push(PrefetchLoadUnit::Bundled {
                                record: record.clone(),
                            });
                        }
                    }
                }
                SkillSourceConfig::McpRecords { server_id, records } => {
                    for record in records {
                        if units.len() >= max_units {
                            break;
                        }
                        let name = format!("mcp__{}__{}", server_id.0, record.name);
                        if matches_hint(&name, hints) {
                            units.push(PrefetchLoadUnit::Mcp {
                                server_id: server_id.clone(),
                                record: record.clone(),
                            });
                        }
                    }
                }
                SkillSourceConfig::Directory { path, source_kind } => {
                    if !path.exists() {
                        continue;
                    }
                    for raw_path in directory_skill_paths(path, true)? {
                        if units.len() >= max_units {
                            break;
                        }
                        units.push(PrefetchLoadUnit::Directory {
                            raw_path,
                            source: source_from_directory(path.clone(), source_kind),
                        });
                    }
                }
                SkillSourceConfig::DirectoryPackages { path, source_kind } => {
                    if !path.exists() {
                        continue;
                    }
                    for raw_path in directory_skill_paths(path, false)? {
                        if units.len() >= max_units {
                            break;
                        }
                        units.push(PrefetchLoadUnit::Directory {
                            raw_path,
                            source: source_from_directory(path.clone(), source_kind),
                        });
                    }
                }
            }
        }
        Ok(units)
    }

    async fn load_prefetch_unit(
        &self,
        unit: PrefetchLoadUnit,
        hints: Option<&[String]>,
    ) -> Result<PrefetchUnitOutcome, SkillError> {
        match unit {
            PrefetchLoadUnit::Bundled { record } => {
                let skill = parse_skill_markdown_with_options(
                    &record.to_markdown(),
                    SkillSource::Bundled,
                    None,
                    self.runtime_platform,
                    self.compat_mode,
                )?;
                let skill = self
                    .validate_loaded_skill(skill, None)
                    .await
                    .map_err(skill_error_from_rejection)?;
                self.emit_loaded(&skill).await;
                Ok(PrefetchUnitOutcome::Loaded(skill))
            }
            PrefetchLoadUnit::Mcp { server_id, record } => {
                let source = SkillSource::Mcp(server_id.clone());
                let name = format!("mcp__{}__{}", server_id.0, record.name);
                let markdown = format!(
                    "---\nname: {}\ndescription: {}\n---\n{}",
                    yaml_quoted_scalar(&name),
                    yaml_quoted_scalar(&record.description),
                    record.body
                );
                match parse_skill_markdown_with_options(
                    &markdown,
                    source.clone(),
                    None,
                    self.runtime_platform,
                    self.compat_mode,
                ) {
                    Ok(skill) => match self.validate_loaded_skill(skill, None).await {
                        Ok(skill) => {
                            self.emit_loaded(&skill).await;
                            Ok(PrefetchUnitOutcome::Loaded(skill))
                        }
                        Err(rejection) => {
                            self.emit_rejected(&rejection).await;
                            Ok(PrefetchUnitOutcome::Rejected(rejection))
                        }
                    },
                    Err(error) => {
                        let rejection = SkillRejection {
                            source,
                            raw_path: None,
                            reason: SkillRejectReason::from_error(&error),
                        };
                        self.emit_rejected(&rejection).await;
                        Ok(PrefetchUnitOutcome::Rejected(rejection))
                    }
                }
            }
            PrefetchLoadUnit::Directory { raw_path, source } => {
                let markdown = std::fs::read_to_string(&raw_path)?;
                match parse_skill_markdown_with_options(
                    &markdown,
                    source.clone(),
                    Some(raw_path.clone()),
                    self.runtime_platform,
                    self.compat_mode,
                ) {
                    Ok(skill) => {
                        if !matches_hint(&skill.name, hints) {
                            return Ok(PrefetchUnitOutcome::Skipped);
                        }
                        match self.validate_loaded_skill(skill, Some(&raw_path)).await {
                            Ok(skill) => {
                                self.emit_loaded(&skill).await;
                                Ok(PrefetchUnitOutcome::Loaded(skill))
                            }
                            Err(rejection) => {
                                self.emit_rejected(&rejection).await;
                                Ok(PrefetchUnitOutcome::Rejected(rejection))
                            }
                        }
                    }
                    Err(error) => {
                        let rejection = SkillRejection {
                            source,
                            raw_path: Some(raw_path),
                            reason: SkillRejectReason::from_error(&error),
                        };
                        self.emit_rejected(&rejection).await;
                        Ok(PrefetchUnitOutcome::Rejected(rejection))
                    }
                }
            }
        }
    }

    pub async fn load_by_name(&self, name: &str) -> Result<Skill, SkillError> {
        let report = self.load_all().await?;
        report
            .loaded
            .into_iter()
            .find(|skill| skill.name == name)
            .ok_or_else(|| SkillError::ParseFrontmatter(format!("skill not found: {name}")))
    }

    async fn validate_loaded_skill(
        &self,
        skill: Skill,
        raw_path: Option<&Path>,
    ) -> Result<Skill, SkillRejection> {
        self.validator()
            .validate_loaded_skill_as_rejection(skill, raw_path)
            .await
    }

    async fn emit_loaded(&self, skill: &Skill) {
        if let Some(metrics) = &self.metrics_sink {
            metrics.skill_loaded(source_metric_label(&skill.source));
        }
        let Some(sink) = &self.event_sink else {
            return;
        };
        sink.emit(Event::SkillLoaded(SkillLoadedEvent {
            session_id: self.event_scope.session_id,
            skill_id: skill.id.clone(),
            skill_name: skill.name.clone(),
            source: skill.source.to_kind(),
            at: harness_contracts::now(),
        }))
        .await;
    }

    async fn emit_rejected(&self, rejection: &SkillRejection) {
        if let Some(metrics) = &self.metrics_sink {
            metrics.skill_rejected(rejection.reason.label());
        }
        let Some(sink) = &self.event_sink else {
            return;
        };
        sink.emit(Event::SkillRejected(SkillRejectedEvent {
            session_id: self.event_scope.session_id,
            skill_name: rejection
                .raw_path
                .as_deref()
                .and_then(Path::file_stem)
                .and_then(std::ffi::OsStr::to_str)
                .map(ToOwned::to_owned),
            source: rejection.source.to_kind(),
            reason: event_rejection_reason(&rejection.reason),
            detail: Some(format!("{:?}", rejection.reason)),
            at: harness_contracts::now(),
        }))
        .await;
    }
}

impl SkillValidator {
    #[must_use]
    pub fn with_runtime_platform(mut self, platform: SkillPlatform) -> Self {
        self.runtime_platform = platform;
        self
    }

    #[cfg(feature = "threat-scanner")]
    #[must_use]
    pub fn with_threat_scanner(mut self, scanner: Arc<MemoryThreatScanner>) -> Self {
        self.threat_scanner = Some(scanner);
        self
    }

    #[must_use]
    pub fn with_event_sink(mut self, event_sink: Arc<dyn SkillEventSink>) -> Self {
        self.event_sink = Some(event_sink);
        self
    }

    #[must_use]
    pub fn with_event_scope(mut self, scope: SkillThreatEventScope) -> Self {
        self.event_scope = scope;
        self
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, metrics_sink: Arc<dyn SkillMetricsSink>) -> Self {
        self.metrics_sink = Some(metrics_sink);
        self
    }

    pub async fn validate_registration(
        &self,
        registration: &SkillRegistration,
    ) -> Result<Skill, SkillError> {
        let mut skill = registration.skill.clone();
        if let Some(allowlist) = &registration.force_allowlist {
            skill.frontmatter.allowlist_agents =
                Some(allowlist.iter().map(ToString::to_string).collect());
        }
        self.validate_skill(skill).await
    }

    pub async fn validate_skill(&self, skill: Skill) -> Result<Skill, SkillError> {
        self.validate_loaded_skill_as_rejection(skill, None)
            .await
            .map_err(skill_error_from_rejection)
    }

    async fn validate_loaded_skill_as_rejection(
        &self,
        skill: Skill,
        raw_path: Option<&Path>,
    ) -> Result<Skill, SkillRejection> {
        let source = skill.source.clone();
        if !skill.frontmatter.platforms.is_empty()
            && !skill.frontmatter.platforms.contains(&self.runtime_platform)
        {
            return Err(SkillRejection {
                source,
                raw_path: raw_path.map(Path::to_path_buf),
                reason: SkillRejectReason::from_error(&SkillError::PlatformMismatch {
                    required: skill.frontmatter.platforms.clone(),
                }),
            });
        }
        if let Err(error) = validate_hook_trust(&skill) {
            return Err(SkillRejection {
                source,
                raw_path: raw_path.map(Path::to_path_buf),
                reason: SkillRejectReason::from_error(&error),
            });
        }
        let skill = self.apply_threat_scan(skill, raw_path).await?;
        self.emit_prerequisite_events(&skill).await;
        Ok(skill)
    }

    #[cfg(feature = "threat-scanner")]
    async fn apply_threat_scan(
        &self,
        mut skill: Skill,
        raw_path: Option<&Path>,
    ) -> Result<Skill, SkillRejection> {
        if let Some(scanner) = &self.threat_scanner {
            if let Err(error) = self.scan_skill(&mut skill, scanner).await {
                return Err(SkillRejection {
                    source: skill.source.clone(),
                    raw_path: raw_path.map(Path::to_path_buf),
                    reason: SkillRejectReason::from_error(&error),
                });
            }
        }
        Ok(skill)
    }

    #[cfg(feature = "threat-scanner")]
    async fn scan_skill(
        &self,
        skill: &mut Skill,
        scanner: &MemoryThreatScanner,
    ) -> Result<(), SkillError> {
        if matches!(skill.source, SkillSource::Bundled) {
            return Ok(());
        }

        let mut description = skill.description.clone();
        self.scan_skill_text(skill, &mut description, scanner)
            .await?;
        if description != skill.description {
            skill.description = description.clone();
            skill.frontmatter.description = description;
        }

        let mut body = skill.body.clone();
        self.scan_skill_text(skill, &mut body, scanner).await?;
        skill.body = body;

        Ok(())
    }

    #[cfg(feature = "threat-scanner")]
    async fn scan_skill_text(
        &self,
        skill: &Skill,
        content: &mut String,
        scanner: &MemoryThreatScanner,
    ) -> Result<(), SkillError> {
        let content_hash = harness_memory::threat_content_hash(content);
        let report = scanner.scan(content);
        self.emit_skill_threat_events(skill, content_hash.clone(), &report)
            .await;
        if report.action == ThreatAction::Block {
            if let Some(hit) = report.hits.first() {
                return Err(SkillError::ThreatDetected {
                    pattern_id: hit.pattern_id.clone(),
                    category: hit.category,
                });
            }
        }

        if report.action == ThreatAction::Redact {
            if let Some(redacted) = report.redacted_content {
                *content = redacted;
            }
        }

        Ok(())
    }

    #[cfg(feature = "threat-scanner")]
    async fn emit_skill_threat_events(
        &self,
        skill: &Skill,
        content_hash: ContentHash,
        report: &harness_memory::ThreatScanReport,
    ) {
        if report.hits.is_empty() {
            return;
        }
        for hit in &report.hits {
            if let Some(metrics) = &self.metrics_sink {
                metrics.skill_threat_detected(&format!("{:?}", hit.category));
            }
            if let Some(sink) = &self.event_sink {
                sink.emit(Event::SkillThreatDetected(SkillThreatDetectedEvent {
                    session_id: self.event_scope.session_id,
                    run_id: self.event_scope.run_id,
                    skill_id: Some(skill.id.clone()),
                    skill_name: Some(skill.name.clone()),
                    pattern_id: hit.pattern_id.clone(),
                    category: hit.category,
                    severity: hit.severity,
                    action: hit.action,
                    content_hash: content_hash.clone(),
                    at: harness_contracts::now(),
                }))
                .await;
            }
        }
    }

    #[cfg(not(feature = "threat-scanner"))]
    async fn apply_threat_scan(
        &self,
        skill: Skill,
        _raw_path: Option<&Path>,
    ) -> Result<Skill, SkillRejection> {
        Ok(skill)
    }

    async fn emit_prerequisite_events(&self, skill: &Skill) {
        let missing_env_vars = missing_env_vars(skill);
        if !missing_env_vars.is_empty() {
            if let Some(metrics) = &self.metrics_sink {
                metrics.skill_prerequisite_missing(&skill.name);
            }
            if let Some(sink) = &self.event_sink {
                sink.emit(Event::SkillPrerequisiteMissing(
                    SkillPrerequisiteMissingEvent {
                        session_id: self.event_scope.session_id,
                        skill_id: skill.id.clone(),
                        skill_name: skill.name.clone(),
                        env_vars: missing_env_vars,
                        at: harness_contracts::now(),
                    },
                ))
                .await;
            }
        }

        let missing_commands = missing_commands(skill);
        if !missing_commands.is_empty() {
            if let Some(metrics) = &self.metrics_sink {
                metrics.skill_prerequisite_advisory(&skill.name);
            }
            if let Some(sink) = &self.event_sink {
                sink.emit(Event::SkillPrerequisiteAdvisory(
                    SkillPrerequisiteAdvisoryEvent {
                        session_id: self.event_scope.session_id,
                        skill_id: skill.id.clone(),
                        skill_name: skill.name.clone(),
                        commands: missing_commands,
                        at: harness_contracts::now(),
                    },
                ))
                .await;
            }
        }
    }
}

impl BundledSkillRecord {
    fn to_markdown(&self) -> String {
        format!(
            "---\nname: {}\ndescription: {}\n---\n{}",
            self.name, self.description, self.body
        )
    }
}

fn source_from_directory(path: PathBuf, source_kind: &DirectorySourceKind) -> SkillSource {
    match source_kind {
        DirectorySourceKind::Workspace => SkillSource::Workspace(path),
        DirectorySourceKind::User => SkillSource::User(path),
        DirectorySourceKind::Plugin { plugin_id, trust } => SkillSource::Plugin {
            plugin_id: plugin_id.clone(),
            trust: *trust,
        },
    }
}

fn directory_skill_paths(
    path: &Path,
    include_markdown_files: bool,
) -> Result<Vec<PathBuf>, SkillError> {
    let mut raw_paths = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let raw_path = entry?.path();
        if include_markdown_files && raw_path.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            raw_paths.push(raw_path);
            continue;
        }
        if raw_path.is_dir() {
            let package_entry = raw_path.join("SKILL.md");
            if package_entry.is_file() {
                raw_paths.push(package_entry);
            }
        }
    }
    raw_paths.sort();
    Ok(raw_paths)
}

fn reached_limit(loaded: usize, limit: Option<usize>) -> bool {
    limit.is_some_and(|limit| loaded >= limit)
}

fn matches_hint(name: &str, hints: Option<&[String]>) -> bool {
    hints
        .map(|hints| hints.iter().any(|hint| hint == name))
        .unwrap_or(true)
}

fn yaml_quoted_scalar(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

fn validate_hook_trust(skill: &Skill) -> Result<(), SkillError> {
    for hook in &skill.frontmatter.hooks {
        match (&skill.source, &hook.transport) {
            (SkillSource::Mcp(_), _) => {
                return Err(SkillError::HookTransportNotPermitted {
                    trust: skill.source.trust_level(),
                });
            }
            (_, SkillHookTransport::Builtin(_)) => {}
            (SkillSource::Bundled, SkillHookTransport::Exec(_) | SkillHookTransport::Http(_)) => {}
            (
                SkillSource::Plugin {
                    trust: TrustLevel::AdminTrusted,
                    ..
                },
                SkillHookTransport::Exec(_) | SkillHookTransport::Http(_),
            ) => {}
            (_, SkillHookTransport::Exec(_) | SkillHookTransport::Http(_)) => {
                return Err(SkillError::HookTransportNotPermitted {
                    trust: skill.source.trust_level(),
                });
            }
        }
    }
    Ok(())
}

fn missing_env_vars(skill: &Skill) -> Vec<String> {
    skill
        .frontmatter
        .prerequisites
        .env_vars
        .iter()
        .filter(|name| std::env::var_os(name).is_none())
        .cloned()
        .collect()
}

fn missing_commands(skill: &Skill) -> Vec<String> {
    skill
        .frontmatter
        .prerequisites
        .commands
        .iter()
        .filter(|command| !command_in_path(command))
        .cloned()
        .collect()
}

fn command_in_path(command: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(command).is_file())
}

fn event_rejection_reason(reason: &SkillRejectReason) -> EventSkillRejectionReason {
    match reason {
        SkillRejectReason::ParseFrontmatter(_) | SkillRejectReason::Io(_) => {
            EventSkillRejectionReason::ParseFrontmatter
        }
        SkillRejectReason::PlatformMismatch { .. } => EventSkillRejectionReason::PlatformMismatch,
        SkillRejectReason::ThreatDetected { .. } => EventSkillRejectionReason::ThreatDetected,
        SkillRejectReason::NameTooLong(_) => EventSkillRejectionReason::NameTooLong,
        SkillRejectReason::DescriptionTooLong(_) => EventSkillRejectionReason::DescriptionTooLong,
        SkillRejectReason::HookTransportNotPermitted { .. } => {
            EventSkillRejectionReason::HookTransportNotPermitted
        }
        SkillRejectReason::Duplicate => EventSkillRejectionReason::Duplicate,
    }
}

fn source_metric_label(source: &SkillSource) -> &'static str {
    match source {
        SkillSource::Bundled => "bundled",
        SkillSource::Workspace(_) => "workspace",
        SkillSource::User(_) => "user",
        SkillSource::Plugin { .. } => "plugin",
        SkillSource::Mcp(_) => "mcp",
    }
}

fn skill_error_from_rejection(rejection: SkillRejection) -> SkillError {
    match rejection.reason {
        SkillRejectReason::ParseFrontmatter(message) => SkillError::ParseFrontmatter(message),
        SkillRejectReason::PlatformMismatch { required } => {
            SkillError::PlatformMismatch { required }
        }
        SkillRejectReason::ThreatDetected {
            pattern_id,
            category,
        } => SkillError::ThreatDetected {
            pattern_id,
            category,
        },
        SkillRejectReason::NameTooLong(size) => SkillError::NameTooLong(size),
        SkillRejectReason::DescriptionTooLong(size) => SkillError::DescriptionTooLong(size),
        SkillRejectReason::HookTransportNotPermitted { trust } => {
            SkillError::HookTransportNotPermitted { trust }
        }
        SkillRejectReason::Duplicate => SkillError::Duplicate("bundled skill".to_owned()),
        SkillRejectReason::Io(message) => SkillError::ParseFrontmatter(message),
    }
}

fn current_platform() -> SkillPlatform {
    if cfg!(target_os = "macos") {
        SkillPlatform::Macos
    } else if cfg!(target_os = "windows") {
        SkillPlatform::Windows
    } else {
        SkillPlatform::Linux
    }
}
