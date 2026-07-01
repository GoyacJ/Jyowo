use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use harness_contracts::{
    validate_agent_profile, AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope,
    AgentProfileSandboxInheritance, AgentProfileScope, AgentWorkspaceIsolationMode,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::store::{AgentRuntimeStore, AgentRuntimeStoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfilesFile {
    pub profiles: Vec<AgentProfile>,
}

#[derive(Debug, Error)]
pub enum AgentProfileRegistryError {
    #[error("agent profile registry io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("agent profile registry sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("agent profile registry json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("agent profile validation error: {0}")]
    Validation(String),
    #[error("builtin agent profiles are read-only")]
    BuiltinReadOnly,
    #[error("agent profile not found: {0}")]
    NotFound(String),
    #[error("agent runtime store error: {0}")]
    Store(#[from] AgentRuntimeStoreError),
}

pub struct AgentProfileRegistry<'store> {
    store: &'store AgentRuntimeStore,
}

impl<'store> AgentProfileRegistry<'store> {
    #[must_use]
    pub fn new(store: &'store AgentRuntimeStore) -> Self {
        Self { store }
    }

    pub fn list(&self) -> Result<Vec<AgentProfile>, AgentProfileRegistryError> {
        let mut profiles = builtin_agent_profiles();
        profiles.extend(self.load_user_profiles()?);
        self.sync_profile_cache(&profiles)?;
        Ok(profiles)
    }

    pub fn save(&self, profile: AgentProfile) -> Result<AgentProfile, AgentProfileRegistryError> {
        if profile.scope == AgentProfileScope::Builtin {
            return Err(AgentProfileRegistryError::BuiltinReadOnly);
        }
        validate_agent_profile(&profile)
            .map_err(|error| AgentProfileRegistryError::Validation(error.to_string()))?;

        let mut profiles = self.load_user_profiles()?;
        if let Some(existing) = profiles.iter_mut().find(|entry| entry.id == profile.id) {
            *existing = profile.clone();
        } else {
            profiles.push(profile.clone());
        }
        self.save_user_profiles(&profiles)?;
        self.sync_profile_cache(&self.list()?)?;
        Ok(profile)
    }

    pub fn delete(&self, profile_id: &str) -> Result<(), AgentProfileRegistryError> {
        if builtin_agent_profiles()
            .iter()
            .any(|profile| profile.id == profile_id)
        {
            return Err(AgentProfileRegistryError::BuiltinReadOnly);
        }

        let mut profiles = self.load_user_profiles()?;
        let original_len = profiles.len();
        profiles.retain(|profile| profile.id != profile_id);
        if profiles.len() == original_len {
            return Err(AgentProfileRegistryError::NotFound(profile_id.to_owned()));
        }
        self.save_user_profiles(&profiles)?;
        self.store.with_connection(|connection| {
            connection.execute(
                "DELETE FROM agent_profile_cache WHERE profile_id = ?1",
                [profile_id],
            )
        })?;
        Ok(())
    }

    fn load_user_profiles(&self) -> Result<Vec<AgentProfile>, AgentProfileRegistryError> {
        let path = self.store.profiles_file_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let bytes = fs::read(&path)?;
        let parsed = match serde_json::from_slice::<AgentProfilesFile>(&bytes) {
            Ok(file) => file,
            Err(error) => {
                quarantine_invalid_profile_file(&path)?;
                return Err(AgentProfileRegistryError::Json(error));
            }
        };

        for profile in &parsed.profiles {
            if let Err(validation) = validate_agent_profile(profile) {
                quarantine_invalid_profile_file(&path)?;
                return Err(AgentProfileRegistryError::Validation(
                    validation.to_string(),
                ));
            }
        }

        Ok(parsed.profiles)
    }

    fn save_user_profiles(
        &self,
        profiles: &[AgentProfile],
    ) -> Result<(), AgentProfileRegistryError> {
        let path = self.store.profiles_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let payload = AgentProfilesFile {
            profiles: profiles.to_vec(),
        };
        let bytes = serde_json::to_vec_pretty(&payload)?;
        write_atomic_file(&path, &bytes)?;
        Ok(())
    }

    fn sync_profile_cache(
        &self,
        profiles: &[AgentProfile],
    ) -> Result<(), AgentProfileRegistryError> {
        let updated_at = Utc::now().to_rfc3339();
        self.store.with_connection(|connection| {
            let tx = connection.unchecked_transaction()?;
            tx.execute("DELETE FROM agent_profile_cache", [])?;
            for profile in profiles {
                let payload_json = serde_json::to_string(profile).map_err(|error| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                })?;
                tx.execute(
                    "INSERT INTO agent_profile_cache(profile_id, scope, role, updated_at, payload_json)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        profile.id,
                        scope_label(profile.scope),
                        profile.role,
                        updated_at,
                        payload_json,
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })?;
        Ok(())
    }
}

pub fn quarantine_invalid_profile_file(path: &Path) -> Result<PathBuf, AgentProfileRegistryError> {
    let quarantine_path = path.with_extension("json.invalid");
    if path.exists() {
        fs::rename(path, &quarantine_path)?;
    }
    Ok(quarantine_path)
}

fn write_atomic_file(path: &Path, bytes: &[u8]) -> Result<(), AgentProfileRegistryError> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("profile file path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp_path = path.with_extension(format!("json.tmp-{}", std::process::id()));
    {
        let mut temp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
    }
    fs::rename(temp_path, path)?;
    Ok(())
}

#[must_use]
fn scope_label(scope: AgentProfileScope) -> &'static str {
    match scope {
        AgentProfileScope::Builtin => "builtin",
        AgentProfileScope::User => "user",
        AgentProfileScope::Project => "project",
    }
}

#[must_use]
pub fn builtin_agent_profiles() -> Vec<AgentProfile> {
    vec![
        AgentProfile {
            id: "reviewer".to_owned(),
            scope: AgentProfileScope::Builtin,
            role: "Reviewer".to_owned(),
            description: "Read-only review subagent with narrow tool scope.".to_owned(),
            model_config_override: None,
            tool_allowlist: Some(vec!["read".to_owned(), "grep".to_owned()]),
            tool_blocklist: vec!["bash".to_owned(), "write".to_owned()],
            sandbox_inheritance: AgentProfileSandboxInheritance::NarrowOnly,
            memory_scope: AgentProfileMemoryScope::ReadOnly,
            context_mode: AgentProfileContextMode::Focused,
            max_turns: 8,
            max_depth: 1,
            default_workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
        },
        AgentProfile {
            id: "worker".to_owned(),
            scope: AgentProfileScope::Builtin,
            role: "Worker".to_owned(),
            description: "General worker subagent for bounded delegation.".to_owned(),
            model_config_override: None,
            tool_allowlist: None,
            tool_blocklist: vec![],
            sandbox_inheritance: AgentProfileSandboxInheritance::InheritParent,
            memory_scope: AgentProfileMemoryScope::ReadOnly,
            context_mode: AgentProfileContextMode::FullWorkspace,
            max_turns: 12,
            max_depth: 2,
            default_workspace_isolation: AgentWorkspaceIsolationMode::PatchOnly,
        },
    ]
}
