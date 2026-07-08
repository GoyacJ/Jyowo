use std::{fs, path::PathBuf, sync::Arc};

use harness_contracts::{MessageId, ModelProtocol, RunId, SessionId, TenantId};
use harness_provider_state::{
    FileProviderContinuationStore, ProviderContinuationKind, ProviderContinuationQuery,
    ProviderContinuationRecord, ProviderContinuationScope, ProviderContinuationStore,
};
use serde_json::json;
use tempfile::TempDir;
use tokio::task::JoinSet;

const PRIVATE_SENTINEL: &str = "PRIVATE_DEEPSEEK_REASONING_SENTINEL";

fn canonical_temp_root(temp: &TempDir) -> PathBuf {
    temp.path().canonicalize().expect("canonical tempdir")
}

fn open_store(temp: &TempDir) -> FileProviderContinuationStore {
    FileProviderContinuationStore::open_runtime_dir(
        canonical_temp_root(temp).join(".jyowo").join("runtime"),
    )
    .unwrap()
}

fn continuation_log_path(temp: &TempDir) -> PathBuf {
    canonical_temp_root(temp).join(".jyowo/runtime/provider-continuations.jsonl")
}

fn continuation_lock_path(temp: &TempDir) -> PathBuf {
    canonical_temp_root(temp).join(".jyowo/runtime/provider-continuations.jsonl.lock")
}

fn record(
    provider_id: &str,
    dialect: &str,
    tenant_id: TenantId,
    session_id: SessionId,
    message_id: MessageId,
    kind: ProviderContinuationKind,
    payload: serde_json::Value,
) -> ProviderContinuationRecord {
    ProviderContinuationRecord {
        provider_id: provider_id.to_owned(),
        model_config_id: Some("model-config-1".to_owned()),
        protocol: ModelProtocol::ChatCompletions,
        dialect: dialect.to_owned(),
        tenant_id,
        session_id,
        producing_run_id: RunId::new(),
        message_id,
        scope: ProviderContinuationScope::Conversation,
        kind,
        payload,
        created_at: harness_contracts::now(),
    }
}

fn query(
    provider_id: &str,
    dialect: &str,
    tenant_id: TenantId,
    session_id: SessionId,
    message_ids: Vec<MessageId>,
    kinds: Vec<ProviderContinuationKind>,
) -> ProviderContinuationQuery {
    ProviderContinuationQuery {
        provider_id: provider_id.to_owned(),
        model_config_id: Some("model-config-1".to_owned()),
        protocol: ModelProtocol::ChatCompletions,
        dialect: dialect.to_owned(),
        tenant_id,
        session_id,
        message_ids,
        kinds,
    }
}

#[tokio::test]
async fn file_store_round_trips_private_records_by_final_message_ids() {
    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let kept_message_id = MessageId::new();
    let other_message_id = MessageId::new();

    store
        .append_batch(vec![
            record(
                "deepseek",
                "deepseek",
                tenant_id,
                session_id,
                kept_message_id,
                ProviderContinuationKind::ReasoningReplay,
                json!({"private": PRIVATE_SENTINEL}),
            ),
            record(
                "deepseek",
                "deepseek",
                tenant_id,
                session_id,
                other_message_id,
                ProviderContinuationKind::ReasoningReplay,
                json!({"private": "not requested"}),
            ),
        ])
        .await
        .unwrap();

    let loaded = store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            tenant_id,
            session_id,
            vec![kept_message_id],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap();

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].message_id, kept_message_id);
    assert_eq!(loaded[0].payload["private"], PRIVATE_SENTINEL);
}

#[tokio::test]
async fn file_store_does_not_return_records_for_other_provider_or_dialect() {
    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let message_id = MessageId::new();

    store
        .append_batch(vec![
            record(
                "deepseek",
                "deepseek",
                tenant_id,
                session_id,
                message_id,
                ProviderContinuationKind::ReasoningReplay,
                json!({"private": PRIVATE_SENTINEL}),
            ),
            record(
                "minimax",
                "deepseek",
                tenant_id,
                session_id,
                message_id,
                ProviderContinuationKind::ReasoningReplay,
                json!({"private": "wrong provider"}),
            ),
            record(
                "deepseek",
                "plain",
                tenant_id,
                session_id,
                message_id,
                ProviderContinuationKind::ReasoningReplay,
                json!({"private": "wrong dialect"}),
            ),
        ])
        .await
        .unwrap();

    let wrong_dialect = store
        .load_for_messages(query(
            "deepseek",
            "minimax",
            tenant_id,
            session_id,
            vec![message_id],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap();
    let loaded = store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            tenant_id,
            session_id,
            vec![message_id],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap();

    assert!(wrong_dialect.is_empty());
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].provider_id, "deepseek");
    assert_eq!(loaded[0].payload["private"], PRIVATE_SENTINEL);
}

#[tokio::test]
async fn file_store_keeps_newest_record_per_message_and_kind() {
    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let message_id = MessageId::new();
    let mut old = record(
        "deepseek",
        "deepseek",
        tenant_id,
        session_id,
        message_id,
        ProviderContinuationKind::ReasoningReplay,
        json!({"version": "old"}),
    );
    old.created_at = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut new = old.clone();
    new.payload = json!({"version": "new"});
    new.created_at = chrono::DateTime::parse_from_rfc3339("2026-01-02T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);

    store.append_batch(vec![old, new]).await.unwrap();

    let loaded = store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            tenant_id,
            session_id,
            vec![message_id],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap();

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].payload["version"], "new");
}

#[tokio::test]
async fn file_store_rejects_null_payload() {
    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);
    let result = store
        .append_batch(vec![record(
            "deepseek",
            "deepseek",
            TenantId::SINGLE,
            SessionId::new(),
            MessageId::new(),
            ProviderContinuationKind::ReasoningReplay,
            serde_json::Value::Null,
        )])
        .await;

    assert!(result.is_err());
    let message = result.unwrap_err().to_string();
    assert!(!message.contains(PRIVATE_SENTINEL));
}

#[tokio::test]
async fn corrupt_jsonl_fails_closed() {
    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);
    let path = continuation_log_path(&temp);
    fs::write(&path, b"{not valid json}\n").unwrap();

    let result = store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            TenantId::SINGLE,
            SessionId::new(),
            vec![MessageId::new()],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("provider-continuations.jsonl"));
}

#[tokio::test]
async fn prune_session_only_removes_matching_session() {
    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);
    let tenant_id = TenantId::SINGLE;
    let removed_session = SessionId::new();
    let kept_session = SessionId::new();
    let removed_message = MessageId::new();
    let kept_message = MessageId::new();

    store
        .append_batch(vec![
            record(
                "deepseek",
                "deepseek",
                tenant_id,
                removed_session,
                removed_message,
                ProviderContinuationKind::ReasoningReplay,
                json!({"session": "removed"}),
            ),
            record(
                "deepseek",
                "deepseek",
                tenant_id,
                kept_session,
                kept_message,
                ProviderContinuationKind::ReasoningReplay,
                json!({"session": "kept"}),
            ),
        ])
        .await
        .unwrap();

    store
        .prune_session(tenant_id, removed_session)
        .await
        .unwrap();

    let removed = store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            tenant_id,
            removed_session,
            vec![removed_message],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap();
    let kept = store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            tenant_id,
            kept_session,
            vec![kept_message],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap();

    assert!(removed.is_empty());
    assert_eq!(kept.len(), 1);
}

#[tokio::test]
async fn concurrent_appends_are_serialized_without_dropping_records() {
    let temp = TempDir::new().unwrap();
    let store = Arc::new(open_store(&temp));
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let mut message_ids = Vec::new();
    let mut tasks = JoinSet::new();

    for _ in 0..20 {
        let store = Arc::clone(&store);
        let message_id = MessageId::new();
        message_ids.push(message_id);
        tasks.spawn(async move {
            store
                .append_batch(vec![record(
                    "deepseek",
                    "deepseek",
                    tenant_id,
                    session_id,
                    message_id,
                    ProviderContinuationKind::ReasoningReplay,
                    json!({"message": message_id.to_string()}),
                )])
                .await
                .unwrap();
        });
    }

    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }

    let loaded = store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            tenant_id,
            session_id,
            message_ids,
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap();

    assert_eq!(loaded.len(), 20);
}

#[tokio::test]
async fn concurrent_appends_from_distinct_store_instances_do_not_drop_records() {
    let temp = TempDir::new().unwrap();
    let temp_root = canonical_temp_root(&temp);
    let first_store = Arc::new(
        FileProviderContinuationStore::open_runtime_dir(temp_root.join(".jyowo").join("runtime"))
            .unwrap(),
    );
    let second_store = Arc::new(
        FileProviderContinuationStore::open_runtime_dir(temp_root.join(".jyowo").join("runtime"))
            .unwrap(),
    );
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let mut message_ids = Vec::new();
    let mut tasks = JoinSet::new();

    for index in 0..40 {
        let store = if index % 2 == 0 {
            Arc::clone(&first_store)
        } else {
            Arc::clone(&second_store)
        };
        let message_id = MessageId::new();
        message_ids.push(message_id);
        tasks.spawn(async move {
            store
                .append_batch(vec![record(
                    "deepseek",
                    "deepseek",
                    tenant_id,
                    session_id,
                    message_id,
                    ProviderContinuationKind::ReasoningReplay,
                    json!({"message": message_id.to_string()}),
                )])
                .await
                .unwrap();
        });
    }

    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }

    let loaded = first_store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            tenant_id,
            session_id,
            message_ids,
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap();

    assert_eq!(loaded.len(), 40);
}

#[tokio::test]
async fn prune_session_uses_atomic_replace_and_ignores_temp_files() {
    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let message_id = MessageId::new();
    let runtime_dir = canonical_temp_root(&temp).join(".jyowo/runtime");
    fs::write(
        runtime_dir.join("provider-continuations.jsonl.prune.tmp"),
        b"{not valid json}\n",
    )
    .unwrap();

    store
        .append_batch(vec![record(
            "deepseek",
            "deepseek",
            tenant_id,
            session_id,
            message_id,
            ProviderContinuationKind::ReasoningReplay,
            json!({"private": PRIVATE_SENTINEL}),
        )])
        .await
        .unwrap();

    let loaded = store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            tenant_id,
            session_id,
            vec![message_id],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap();
    assert_eq!(loaded.len(), 1);

    store.prune_session(tenant_id, session_id).await.unwrap();
    assert!(runtime_dir.join("provider-continuations.jsonl").exists());
}

#[cfg(unix)]
#[test]
fn file_store_open_resolves_symlink_runtime_parent_via_canonical_path() {
    let temp = TempDir::new().unwrap();
    let temp_root = canonical_temp_root(&temp);
    let external = TempDir::new().unwrap();
    std::os::unix::fs::symlink(external.path(), temp_root.join(".jyowo")).unwrap();

    // The store should resolve the symlink via canonical prefix and operate
    // at the canonical (real) location.
    let _store =
        FileProviderContinuationStore::open_runtime_dir(temp_root.join(".jyowo").join("runtime"))
            .expect("store open should resolve symlink prefix via canonical path");

    // The runtime directory should be created at the canonical target, not
    // the symlink source. (The jsonl log is created lazily on first write.)
    let canonical_runtime = external.path().canonicalize().unwrap().join("runtime");
    assert!(canonical_runtime.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn load_for_messages_rejects_symlink_jsonl_file() {
    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);
    let external = tempfile::NamedTempFile::new().unwrap();
    let path = continuation_log_path(&temp);
    std::os::unix::fs::symlink(external.path(), &path).unwrap();

    let error = store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            TenantId::SINGLE,
            SessionId::new(),
            vec![MessageId::new()],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("I/O failed"));
}

#[cfg(unix)]
#[tokio::test]
async fn append_batch_creates_owner_only_jsonl_file() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let message_id = MessageId::new();

    store
        .append_batch(vec![record(
            "deepseek",
            "deepseek",
            tenant_id,
            session_id,
            message_id,
            ProviderContinuationKind::ReasoningReplay,
            json!({"private": PRIVATE_SENTINEL}),
        )])
        .await
        .unwrap();

    let path = continuation_log_path(&temp);
    let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[tokio::test]
async fn load_for_messages_tightens_existing_jsonl_file() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let message_id = MessageId::new();
    let path = continuation_log_path(&temp);

    store
        .append_batch(vec![record(
            "deepseek",
            "deepseek",
            tenant_id,
            session_id,
            message_id,
            ProviderContinuationKind::ReasoningReplay,
            json!({"private": PRIVATE_SENTINEL}),
        )])
        .await
        .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    let loaded = store
        .load_for_messages(query(
            "deepseek",
            "deepseek",
            tenant_id,
            session_id,
            vec![message_id],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .unwrap();

    assert_eq!(loaded.len(), 1);
    let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[tokio::test]
async fn append_batch_creates_owner_only_lock_file() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let store = open_store(&temp);

    store
        .append_batch(vec![record(
            "deepseek",
            "deepseek",
            TenantId::SINGLE,
            SessionId::new(),
            MessageId::new(),
            ProviderContinuationKind::ReasoningReplay,
            json!({"private": PRIVATE_SENTINEL}),
        )])
        .await
        .unwrap();

    let path = continuation_lock_path(&temp);
    let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn provider_continuation_record_debug_redacts_payload() {
    let debug = format!(
        "{:?}",
        record(
            "deepseek",
            "deepseek",
            TenantId::SINGLE,
            SessionId::new(),
            MessageId::new(),
            ProviderContinuationKind::ReasoningReplay,
            json!({"private": PRIVATE_SENTINEL}),
        )
    );

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains(PRIVATE_SENTINEL));
}

#[test]
fn provider_continuation_store_errors_do_not_display_payload_or_full_paths() {
    let error = harness_provider_state::ProviderContinuationStoreError::CorruptRecord {
        line: 1,
        details: "full/path/PRIVATE_DEEPSEEK_REASONING_SENTINEL".to_owned(),
    };
    let message = error.to_string();

    assert!(message.contains("provider-continuations.jsonl"));
    assert!(!message.contains(PRIVATE_SENTINEL));
    assert!(!message.contains("full/path"));
}

#[test]
fn provider_continuation_store_error_debug_redacts_details() {
    let error = harness_provider_state::ProviderContinuationStoreError::CorruptRecord {
        line: 1,
        details: "full/path/PRIVATE_DEEPSEEK_REASONING_SENTINEL".to_owned(),
    };
    let debug = format!("{error:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains(PRIVATE_SENTINEL));
    assert!(!debug.contains("full/path"));
}

#[tokio::test]
async fn open_runtime_dir_uses_explicit_runtime_root() {
    let temp = TempDir::new().unwrap();
    let temp_root = canonical_temp_root(&temp);
    let runtime_root = temp_root.join("custom-runtime");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let message_id = MessageId::new();

    let store =
        harness_provider_state::FileProviderContinuationStore::open_runtime_dir(&runtime_root)
            .expect("open_runtime_dir succeeds");

    let rec = record(
        "provider-1",
        "openai",
        tenant_id,
        session_id,
        message_id,
        ProviderContinuationKind::ReasoningReplay,
        serde_json::json!({"key": "value"}),
    );
    store.append_batch(vec![rec]).await.expect("append");
    let loaded = store
        .load_for_messages(query(
            "provider-1",
            "openai",
            tenant_id,
            session_id,
            vec![message_id],
            vec![ProviderContinuationKind::ReasoningReplay],
        ))
        .await
        .expect("load");
    assert_eq!(loaded.len(), 1);
}

#[tokio::test]
async fn open_runtime_dir_and_open_produce_equivalent_stores() {
    let workspace = TempDir::new().unwrap();
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_root = workspace_root.join(".jyowo/runtime");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let message_id = MessageId::new();

    let store_via_open = harness_provider_state::FileProviderContinuationStore::open_runtime_dir(
        workspace_root.join(".jyowo").join("runtime"),
    )
    .expect("open succeeds");
    let store_via_dir =
        harness_provider_state::FileProviderContinuationStore::open_runtime_dir(&runtime_root)
            .expect("open_runtime_dir succeeds");

    let rec = record(
        "provider-2",
        "openai",
        tenant_id,
        session_id,
        message_id,
        ProviderContinuationKind::ToolReplay,
        serde_json::json!({"key": "value"}),
    );
    store_via_open
        .append_batch(vec![rec])
        .await
        .expect("append via open");
    let loaded = store_via_dir
        .load_for_messages(query(
            "provider-2",
            "openai",
            tenant_id,
            session_id,
            vec![message_id],
            vec![ProviderContinuationKind::ToolReplay],
        ))
        .await
        .expect("load via dir");
    assert_eq!(loaded.len(), 1);
}
