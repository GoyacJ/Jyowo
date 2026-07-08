use super::*;
use crate::commands::validation::{
    mcp_env_key_looks_secret_bearing, mcp_header_value_looks_secret_bearing,
    mcp_http_header_is_sensitive,
};
use crate::storage_layout::StorageLayout;

/// Project-scoped MCP server store.
///
/// Stores enabled/custom MCP server configurations under
/// `<workspace>/.jyowo/config/mcp-servers.json`.
///
/// Secret-bearing inline material (bearer tokens, API keys, etc.) must be
/// detected during migration and raised as `SecretMaterialRequiresUserAction`.
/// New saves through this store are validated by the command layer and should
/// not contain raw inline secrets.
#[derive(Clone)]
pub(crate) struct DesktopMcpServerStore {
    layout: StorageLayout,
    workspace_root: PathBuf,
}

impl DesktopMcpServerStore {
    pub(crate) fn new(layout: StorageLayout, workspace_root: PathBuf) -> Self {
        Self {
            layout,
            workspace_root,
        }
    }

    fn settings_path(&self) -> PathBuf {
        self.layout.project_mcp_servers_file(&self.workspace_root)
    }
}

impl McpServerStore for DesktopMcpServerStore {
    fn load_records(&self) -> Result<Vec<McpServerConfigRecord>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        read_secret_json_file(&settings_path, "mcp server settings")
            .map(|records| records.unwrap_or_default())
    }

    fn save_record(&self, record: &McpServerConfigRecord) -> Result<(), CommandErrorPayload> {
        let mut records = self.load_records()?;
        records.retain(|existing| existing.id != record.id);
        records.push(record.clone());
        records.sort_by(|left, right| left.id.cmp(&right.id));
        write_mcp_server_records(&self.settings_path(), &records)
    }

    fn delete_record(&self, id: &str) -> Result<(), CommandErrorPayload> {
        let mut records = self.load_records()?;
        records.retain(|existing| existing.id != id);
        write_mcp_server_records(&self.settings_path(), &records)
    }
}

#[derive(Clone, Default)]
pub(crate) struct NoWorkspaceMcpServerStore;

impl McpServerStore for NoWorkspaceMcpServerStore {
    fn load_records(&self) -> Result<Vec<McpServerConfigRecord>, CommandErrorPayload> {
        Ok(Vec::new())
    }

    fn save_record(&self, _record: &McpServerConfigRecord) -> Result<(), CommandErrorPayload> {
        Err(invalid_payload(
            "custom MCP servers require an active project workspace".to_owned(),
        ))
    }

    fn delete_record(&self, _id: &str) -> Result<(), CommandErrorPayload> {
        Err(invalid_payload(
            "custom MCP servers require an active project workspace".to_owned(),
        ))
    }
}

// ── Migration ──────────────────────────────────────────────────────────

/// Migrate MCP server configuration from the old runtime path to the new
/// project config path.
///
/// Old path: `<workspace>/.jyowo/runtime/mcp-servers.json`
/// New path: `<workspace>/.jyowo/config/mcp-servers.json`
///
/// Migration rules:
/// - Non-sensitive inline values migrate as-is.
/// - Records with inline secret-bearing env values or header values produce
///   `SecretMaterialRequiresUserAction` and do NOT migrate automatically.
/// - Old workspace MCP servers are NOT promoted to global presets.
/// - Diagnostics remain runtime JSONL and are not touched.
pub(crate) fn migrate_mcp_servers_from_runtime(
    layout: &StorageLayout,
    workspace_root: &Path,
) -> Result<MigrationResult, CommandErrorPayload> {
    let old_path = workspace_root
        .join(".jyowo")
        .join("runtime")
        .join("mcp-servers.json");
    let new_path = layout.project_mcp_servers_file(workspace_root);

    // Pre-scan old records for inline secret-bearing material before migrating.
    // If any record contains secrets, fail closed with SecretMaterialRequiresUserAction.
    if old_path.exists() {
        if let Some(old_records) = read_secret_json_file::<Vec<McpServerConfigRecord>>(
            &old_path,
            "old mcp server settings",
        )? {
            let secret_ids = find_mcp_records_with_inline_secrets(&old_records, workspace_root);
            if !secret_ids.is_empty() {
                return Ok(MigrationResult::Conflict(MigrationConflict {
                    kind: MigrationConflictKind::SecretMaterialRequiresUserAction,
                    old_path: old_path.clone(),
                    new_path: new_path.clone(),
                    detail: format!(
                        "MCP server records contain inline secret material: {}. \
                         Remove inline secrets or convert to environment-variable references before migration.",
                        secret_ids.join(", ")
                    ),
                }));
            }
        }
    }

    migrate_secret_json_file_with::<Vec<McpServerConfigRecord>, _>(
        &old_path,
        &new_path,
        "mcp server settings",
        true,
        |old_records, new_records| mcp_records_content_eq(old_records, new_records),
    )
}

/// Returns `true` when two MCP record sets contain the same logical content,
/// ignoring only non-material formatting differences.
fn mcp_records_content_eq(old: &[McpServerConfigRecord], new: &[McpServerConfigRecord]) -> bool {
    if old.len() != new.len() {
        return false;
    }
    let old_map: std::collections::BTreeMap<&str, &McpServerConfigRecord> =
        old.iter().map(|r| (r.id.as_str(), r)).collect();
    let new_map: std::collections::BTreeMap<&str, &McpServerConfigRecord> =
        new.iter().map(|r| (r.id.as_str(), r)).collect();
    if old_map.len() != new_map.len() {
        return false;
    }
    for (id, old_record) in &old_map {
        let Some(new_record) = new_map.get(id) else {
            return false;
        };
        if old_record != new_record {
            return false;
        }
    }
    true
}

/// Inspect MCP server records for inline secret-bearing material.
///
/// Returns a list of record ids that contain potentially sensitive inline data
/// (bearer tokens, API keys, secret env values, sensitive headers with
/// secret-like values).
pub(crate) fn find_mcp_records_with_inline_secrets(
    records: &[McpServerConfigRecord],
    workspace_root: &Path,
) -> Vec<String> {
    let mut secret_bearing_ids = Vec::new();
    let workspace_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());

    for record in records {
        match &record.transport {
            McpServerTransportConfig::Stdio {
                env, working_dir, ..
            } => {
                let has_secret_env = env.iter().any(|nv| {
                    mcp_env_key_looks_secret_bearing(&nv.key)
                        || nv.value.len() >= 16
                            && crate::commands::validation::looks_like_raw_secret(&nv.value)
                });
                let has_private_working_dir = working_dir.as_deref().is_some_and(|path| {
                    mcp_working_dir_is_private_absolute_path(path, &workspace_root)
                });
                if has_secret_env || has_private_working_dir {
                    secret_bearing_ids.push(record.id.clone());
                    continue;
                }
            }
            McpServerTransportConfig::Http {
                bearer_token_env_var,
                headers,
                ..
            } => {
                // bearer_token_env_var is a reference — always safe.
                let _ = bearer_token_env_var;

                // Check inline headers for secret-bearing values.
                let has_sensitive_header = headers.iter().any(|nv| {
                    mcp_http_header_is_sensitive(&nv.key)
                        || mcp_header_value_looks_secret_bearing(&nv.value)
                        || crate::commands::validation::looks_like_raw_secret(&nv.value)
                });
                if has_sensitive_header {
                    secret_bearing_ids.push(record.id.clone());
                }
            }
            McpServerTransportConfig::InProcess => {}
        }
    }

    secret_bearing_ids
}

fn mcp_working_dir_is_private_absolute_path(path: &str, workspace_root: &Path) -> bool {
    let candidate = Path::new(path);
    if !candidate.is_absolute() {
        return false;
    }
    if candidate.strip_prefix(workspace_root).is_ok() {
        return false;
    }
    match candidate.canonicalize() {
        Ok(canonical) => canonical.strip_prefix(workspace_root).is_err(),
        Err(_) => true,
    }
}

#[derive(Clone)]
pub struct DesktopMcpDiagnosticStore {
    retention_limit: usize,
    runtime_root: PathBuf,
}

impl DesktopMcpDiagnosticStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::new_with_limit(workspace_root, MCP_DIAGNOSTIC_RETENTION_LIMIT)
    }

    pub fn new_with_limit(workspace_root: PathBuf, retention_limit: usize) -> Self {
        let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
        Self {
            retention_limit,
            runtime_root: workspace_root.join(".jyowo").join("runtime"),
        }
    }

    pub fn new_runtime_root(runtime_root: PathBuf) -> Self {
        Self::new_runtime_root_with_limit(runtime_root, MCP_DIAGNOSTIC_RETENTION_LIMIT)
    }

    pub fn new_runtime_root_with_limit(runtime_root: PathBuf, retention_limit: usize) -> Self {
        Self {
            retention_limit,
            runtime_root,
        }
    }

    fn diagnostics_path(&self) -> PathBuf {
        self.runtime_root.join("mcp-diagnostics.jsonl")
    }
}

impl McpDiagnosticStore for DesktopMcpDiagnosticStore {
    fn load_records(&self) -> Result<Vec<McpDiagnosticRecord>, CommandErrorPayload> {
        let diagnostics_path = self.diagnostics_path();
        read_jsonl_records_locked(
            &diagnostics_path,
            "mcp diagnostics",
            |error| runtime_operation_failed(format!("mcp diagnostics parse failed: {error}")),
            |_| Ok(()),
        )
    }

    fn append_record(&self, record: &McpDiagnosticRecord) -> Result<(), CommandErrorPayload> {
        append_jsonl_record_with_retention_locked(
            &self.diagnostics_path(),
            "mcp diagnostics",
            record,
            self.retention_limit,
            |error| runtime_operation_failed(format!("mcp diagnostics parse failed: {error}")),
            |_| Ok(()),
        )
    }

    fn clear_records(&self, server_id: Option<&str>) -> Result<(), CommandErrorPayload> {
        update_jsonl_records_locked(
            &self.diagnostics_path(),
            "mcp diagnostics",
            |records: &mut Vec<McpDiagnosticRecord>| match server_id {
                Some(server_id) => records.retain(|record| record.server_id != server_id),
                None => records.clear(),
            },
            |error| runtime_operation_failed(format!("mcp diagnostics parse failed: {error}")),
            |_| Ok(()),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::commands::stores::migration::{MigrationConflictKind, MigrationResult};
    use crate::storage_layout::{JyowoHome, StorageLayout};

    fn make_layout() -> StorageLayout {
        StorageLayout::new(JyowoHome::new(Path::new("/home/alice/.jyowo")))
    }

    fn make_workspace(temp: &tempfile::TempDir) -> PathBuf {
        temp.path().canonicalize().expect("canonical")
    }

    fn make_stdio_record(id: &str, env: Vec<(String, String)>) -> McpServerConfigRecord {
        make_stdio_record_with_working_dir(id, env, None)
    }

    fn make_stdio_record_with_working_dir(
        id: &str,
        env: Vec<(String, String)>,
        working_dir: Option<String>,
    ) -> McpServerConfigRecord {
        McpServerConfigRecord {
            id: id.to_owned(),
            enabled: true,
            display_name: format!("Server {id}"),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec!["server.js".to_owned()],
                env: env
                    .into_iter()
                    .map(|(key, value)| McpNameValueRecord { key, value })
                    .collect(),
                inherit_env: vec![],
                working_dir,
            },
        }
    }

    fn make_http_record(id: &str, headers: Vec<(String, String)>) -> McpServerConfigRecord {
        McpServerConfigRecord {
            id: id.to_owned(),
            enabled: true,
            display_name: format!("Server {id}"),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Http {
                url: "http://localhost:8080".to_owned(),
                headers: headers
                    .into_iter()
                    .map(|(key, value)| McpNameValueRecord { key, value })
                    .collect(),
                headers_from_env: vec![],
                bearer_token_env_var: None,
            },
        }
    }

    #[test]
    fn find_mcp_records_with_inline_secrets_detects_stdio_api_key_env() {
        let records = vec![make_stdio_record(
            "s1",
            vec![("API_KEY".to_owned(), "sk-secret-value-1234".to_owned())],
        )];
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = make_workspace(&temp);
        let ids = find_mcp_records_with_inline_secrets(&records, &workspace);
        assert_eq!(ids, vec!["s1"]);
    }

    #[test]
    fn find_mcp_records_with_inline_secrets_detects_http_bearer_header() {
        let records = vec![make_http_record(
            "h1",
            vec![(
                "Authorization".to_owned(),
                "Bearer secret-token-1234".to_owned(),
            )],
        )];
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = make_workspace(&temp);
        let ids = find_mcp_records_with_inline_secrets(&records, &workspace);
        assert_eq!(ids, vec!["h1"]);
    }

    #[test]
    fn find_mcp_records_with_inline_secrets_detects_sensitive_header_key() {
        let records = vec![make_http_record(
            "h1",
            vec![("Cookie".to_owned(), "sessionid".to_owned())],
        )];
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = make_workspace(&temp);
        let ids = find_mcp_records_with_inline_secrets(&records, &workspace);
        assert_eq!(ids, vec!["h1"]);
    }

    #[test]
    fn find_mcp_records_with_inline_secrets_detects_oauth_header_value() {
        let records = vec![make_http_record(
            "h1",
            vec![("X-Auth".to_owned(), "OAuth abc123".to_owned())],
        )];
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = make_workspace(&temp);
        let ids = find_mcp_records_with_inline_secrets(&records, &workspace);
        assert_eq!(ids, vec!["h1"]);
    }

    #[test]
    fn find_mcp_records_with_inline_secrets_detects_private_absolute_working_dir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = make_workspace(&temp);
        let records = vec![make_stdio_record_with_working_dir(
            "s1",
            Vec::new(),
            Some(
                workspace
                    .parent()
                    .unwrap()
                    .join("private")
                    .display()
                    .to_string(),
            ),
        )];
        let ids = find_mcp_records_with_inline_secrets(&records, &workspace);
        assert_eq!(ids, vec!["s1"]);
    }

    #[test]
    fn find_mcp_records_with_inline_secrets_allows_safe_env_values() {
        let records = vec![make_stdio_record(
            "s1",
            vec![
                ("NODE_ENV".to_owned(), "production".to_owned()),
                ("DEBUG".to_owned(), "true".to_owned()),
            ],
        )];
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = make_workspace(&temp);
        let ids = find_mcp_records_with_inline_secrets(&records, &workspace);
        assert!(ids.is_empty());
    }

    #[test]
    fn migrate_mcp_servers_rejects_inline_secrets() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = make_workspace(&temp);
        let layout = make_layout();

        let old_servers = workspace
            .join(".jyowo")
            .join("runtime")
            .join("mcp-servers.json");
        let old_parent = old_servers.parent().expect("parent");
        std::fs::create_dir_all(old_parent).expect("create old dir");

        let records = vec![make_stdio_record(
            "s1",
            vec![("API_KEY".to_owned(), "sk-secret-1234".to_owned())],
        )];
        std::fs::write(
            &old_servers,
            serde_json::to_vec_pretty(&records).expect("serialize"),
        )
        .expect("write old");

        let result = migrate_mcp_servers_from_runtime(&layout, &workspace).expect("migrate");

        match &result {
            MigrationResult::Conflict(conflict) => {
                assert_eq!(
                    conflict.kind,
                    MigrationConflictKind::SecretMaterialRequiresUserAction
                );
                assert!(conflict.detail.contains("s1"));
            }
            other => panic!("expected Conflict, got {other:?}"),
        }

        // New file must NOT exist.
        let new_path = layout.project_mcp_servers_file(&workspace);
        assert!(
            !new_path.exists(),
            "new path must not be written on secret conflict"
        );
    }

    #[test]
    fn migrate_mcp_servers_migrates_safe_records() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = make_workspace(&temp);
        let layout = make_layout();

        let old_servers = workspace
            .join(".jyowo")
            .join("runtime")
            .join("mcp-servers.json");
        let old_parent = old_servers.parent().expect("parent");
        std::fs::create_dir_all(old_parent).expect("create old dir");

        let records = vec![make_stdio_record(
            "s1",
            vec![
                ("NODE_ENV".to_owned(), "production".to_owned()),
                ("DEBUG".to_owned(), "1".to_owned()),
            ],
        )];
        std::fs::write(
            &old_servers,
            serde_json::to_vec_pretty(&records).expect("serialize"),
        )
        .expect("write old");

        let result = migrate_mcp_servers_from_runtime(&layout, &workspace).expect("migrate");

        assert_eq!(result, MigrationResult::Migrated);

        let new_path = layout.project_mcp_servers_file(&workspace);
        assert!(new_path.exists(), "new path should exist after migration");
    }
}
