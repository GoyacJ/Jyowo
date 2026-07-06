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
    #[error("agent profile registry fs error: {0}")]
    Fs(#[from] harness_fs::FsError),
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
        harness_fs::ensure_no_symlink_components(&path)?;
        if !path.exists() {
            return Ok(Vec::new());
        }

        let Some(bytes) = harness_fs::read_file_no_follow(&path)? else {
            return Ok(Vec::new());
        };
        harness_fs::set_owner_only_file_if_unix(
            &std::fs::OpenOptions::new().read(true).open(&path)?,
        )?;

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
        let payload = AgentProfilesFile {
            profiles: profiles.to_vec(),
        };
        harness_fs::write_json_file_atomic(&path, &payload, true)?;
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
    Ok(harness_fs::quarantine_invalid_json_file(path)?)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn agent_profile_json_write_rejects_symlink_parent_components() {
        let temp = tempfile::tempdir().expect("tempdir");
        let external = tempfile::tempdir().expect("external tempdir");
        let temp_root = temp.path().canonicalize().expect("canonical tempdir");
        let external_root = external.path().canonicalize().expect("canonical external");
        let symlinked_parent = temp_root.join("runtime");
        std::os::unix::fs::symlink(&external_root, &symlinked_parent).expect("symlink");
        let path = symlinked_parent.join("agent-profiles.json");

        let error = harness_fs::write_json_file_atomic(
            &path,
            &AgentProfilesFile {
                profiles: Vec::new(),
            },
            true,
        )
        .expect_err("profile write should reject symlink parent");

        assert!(matches!(error, harness_fs::FsError::Symlink(_)));
        assert!(!external_root.join("agent-profiles.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn agent_profile_json_read_rejects_symlink_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let external = tempfile::NamedTempFile::new().expect("external file");
        let temp_root = temp.path().canonicalize().expect("canonical tempdir");
        let path = temp_root.join("agent-profiles.json");
        std::os::unix::fs::symlink(external.path(), &path).expect("symlink");

        let error = harness_fs::ensure_no_symlink_components(&path)
            .expect_err("read should reject symlink");

        assert!(matches!(error, harness_fs::FsError::Symlink(_)));
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_invalid_profile_file_moves_real_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = temp.path().canonicalize().expect("canonical tempdir");
        let path = temp_root.join("agent-profiles.json");
        std::fs::write(&path, b"{not-json").expect("invalid profile file");

        let quarantine_path =
            quarantine_invalid_profile_file(&path).expect("quarantine real profile file");

        assert!(!path.exists());
        assert!(quarantine_path.exists());
        assert_eq!(quarantine_path.extension().unwrap(), "invalid");
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_invalid_profile_file_rejects_symlink_file_without_moving_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = temp.path().canonicalize().expect("canonical tempdir");
        let target = temp_root.join("target.json");
        let path = temp_root.join("agent-profiles.json");
        std::fs::write(&target, b"{not-json").expect("invalid target");
        std::os::unix::fs::symlink(&target, &path).expect("symlink");

        let error = quarantine_invalid_profile_file(&path)
            .expect_err("profile quarantine should reject final symlink");

        assert!(matches!(error, AgentProfileRegistryError::Fs(_)));
        assert!(target.exists());
        assert!(std::fs::symlink_metadata(&path)
            .expect("link metadata")
            .file_type()
            .is_symlink());
    }
}
