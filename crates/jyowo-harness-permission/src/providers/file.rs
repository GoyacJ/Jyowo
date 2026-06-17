use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::BoxStream;
use futures::StreamExt;
use harness_contracts::{PermissionError, RuleSource, TenantId};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::{PermissionRule, RuleProvider, RulesUpdated};

#[derive(Debug)]
pub struct FileRuleProvider {
    provider_id: String,
    source: RuleSource,
    path: PathBuf,
    updates: broadcast::Sender<RulesUpdated>,
    _watcher: Arc<Mutex<RecommendedWatcher>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RuleFile {
    Rules(Vec<PermissionRule>),
    Wrapped { rules: Vec<PermissionRule> },
}

impl FileRuleProvider {
    pub fn new(
        provider_id: impl Into<String>,
        source: RuleSource,
        path: PathBuf,
    ) -> Result<Self, PermissionError> {
        let provider_id = provider_id.into();
        let (updates, _receiver) = broadcast::channel(16);
        let watch_path = path.clone();
        let watch_provider_id = provider_id.clone();
        let watch_updates = updates.clone();
        let mut watcher = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else {
                    return;
                };
                if !is_update_event(event.kind)
                    || !event
                        .paths
                        .iter()
                        .any(|event_path| same_file_path(event_path, &watch_path))
                {
                    return;
                }

                let Ok(new_rules) = load_rules(&watch_path) else {
                    return;
                };
                let _ = watch_updates.send(RulesUpdated {
                    provider_id: watch_provider_id.clone(),
                    tenant_id: TenantId::SHARED,
                    new_rules,
                    at: Utc::now(),
                });
            },
            Config::default(),
        )
        .map_err(to_permission_error)?;

        let watch_root = path.parent().unwrap_or_else(|| Path::new("."));
        watcher
            .watch(watch_root, RecursiveMode::NonRecursive)
            .map_err(to_permission_error)?;

        Ok(Self {
            provider_id,
            source,
            path,
            updates,
            _watcher: Arc::new(Mutex::new(watcher)),
        })
    }
}

#[async_trait]
impl RuleProvider for FileRuleProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn source(&self) -> RuleSource {
        self.source
    }

    async fn resolve_rules(
        &self,
        _tenant: TenantId,
    ) -> Result<Vec<PermissionRule>, PermissionError> {
        load_rules(&self.path)
    }

    fn watch(&self) -> Option<BoxStream<'static, RulesUpdated>> {
        Some(
            BroadcastStream::new(self.updates.subscribe())
                .filter_map(|update| async move { update.ok() })
                .boxed(),
        )
    }
}

fn load_rules(path: &Path) -> Result<Vec<PermissionRule>, PermissionError> {
    let content = std::fs::read_to_string(path).map_err(to_permission_error)?;
    let parsed = match path.extension().and_then(|extension| extension.to_str()) {
        Some("toml") => toml::from_str::<RuleFile>(&content).map_err(to_permission_error)?,
        _ => serde_json::from_str::<RuleFile>(&content).map_err(to_permission_error)?,
    };

    Ok(match parsed {
        RuleFile::Rules(rules) | RuleFile::Wrapped { rules } => rules,
    })
}

fn is_update_event(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Any
    )
}

fn same_file_path(event_path: &Path, watched_path: &Path) -> bool {
    if event_path == watched_path {
        return true;
    }

    event_path.file_name() == watched_path.file_name()
}

fn to_permission_error(error: impl std::fmt::Display) -> PermissionError {
    PermissionError::Message(error.to_string())
}
