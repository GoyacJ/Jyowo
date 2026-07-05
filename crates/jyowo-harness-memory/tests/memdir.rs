#![cfg(feature = "builtin")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    Event, MemoryError, MemoryId, MemoryKind, MemorySource, MemoryVisibility, RunId, SessionId,
    TakesEffect, TenantId,
};
use harness_memory::{
    escape_for_fence, sanitize_context, wrap_memory_context, BuiltinMemory,
    MemdirConcurrencyPolicy, MemdirFile, MemoryEventSink, MemoryMetadata, MemoryRecord,
    SnapshotStrategy,
};
use parking_lot::Mutex;

#[tokio::test]
async fn memdir_writes_sections_atomically_and_reports_next_session_effect() {
    let root = tempfile::tempdir().unwrap();
    let memory = audited_memory(root.path());

    let append = memory
        .append_section(MemdirFile::Memory, "profile", "prefers concise answers")
        .await
        .unwrap();
    assert_eq!(append.takes_effect, TakesEffect::NextSession);
    assert_ne!(append.previous_hash, append.new_hash);
    assert_eq!(append.snapshot_path, None);

    memory
        .append_section(MemdirFile::User, "style", "no emojis")
        .await
        .unwrap();
    memory
        .replace_section(MemdirFile::Memory, "profile", "prefers Chinese answers")
        .await
        .unwrap();
    memory
        .delete_section(MemdirFile::User, "style")
        .await
        .unwrap();

    let snapshot = memory.read_all().await.unwrap();
    assert_eq!(snapshot.memory, "§ profile\nprefers Chinese answers\n");
    assert_eq!(snapshot.user, "");
    assert_eq!(snapshot.memory_chars, snapshot.memory.chars().count());
    assert_eq!(snapshot.user_chars, 0);

    let tenant_dir = root.path().join(TenantId::SINGLE.to_string());
    assert_eq!(
        fs::read_to_string(tenant_dir.join("MEMORY.md")).unwrap(),
        snapshot.memory
    );
    assert!(tenant_dir.join(".locks/MEMORY.md.lock").exists());
}

#[tokio::test]
async fn memdir_is_tenant_scoped_and_ignores_tmp_files() {
    let root = tempfile::tempdir().unwrap();
    let single = audited_memory(root.path());
    let shared = BuiltinMemory::at(root.path(), TenantId::SHARED);

    single
        .append_section(MemdirFile::Memory, "single", "tenant one")
        .await
        .unwrap();

    let shared_dir = root.path().join(TenantId::SHARED.to_string());
    fs::create_dir_all(&shared_dir).unwrap();
    fs::write(shared_dir.join("MEMORY.md.tmp"), "leaked tmp").unwrap();

    assert!(shared.read_all().await.unwrap().memory.is_empty());
    assert_eq!(
        single.read_all().await.unwrap().memory,
        "§ single\ntenant one\n"
    );
}

#[tokio::test]
async fn memdir_enforces_limits_and_creates_replace_snapshots() {
    let root = tempfile::tempdir().unwrap();
    let memory = audited_memory(root.path())
        .with_limits(18, 8)
        .with_snapshot_strategy(SnapshotStrategy::BeforeEachReplace);

    memory
        .append_section(MemdirFile::Memory, "a", "short")
        .await
        .unwrap();
    let replacement = memory
        .replace_section(MemdirFile::Memory, "a", "changed")
        .await
        .unwrap();
    let snapshot_path = replacement.snapshot_path.expect("snapshot path");
    assert!(snapshot_path.exists());
    assert_eq!(fs::read_to_string(snapshot_path).unwrap(), "§ a\nshort\n");

    let error = memory
        .append_section(MemdirFile::User, "too", "too long")
        .await
        .unwrap_err();
    assert!(matches!(error, MemoryError::TooLarge { bytes, max } if bytes > max));
}

#[tokio::test]
async fn memdir_lock_contention_times_out_without_blocking_forever() {
    let root = tempfile::tempdir().unwrap();
    let memory = audited_memory(root.path()).with_concurrency_policy(MemdirConcurrencyPolicy {
        lock_timeout: Duration::from_millis(25),
        retry_max: 1,
        retry_jitter_ms: 1..=1,
    });
    memory.read_all().await.unwrap();

    let lock_path = root
        .path()
        .join(TenantId::SINGLE.to_string())
        .join(".locks/MEMORY.md.lock");
    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&lock_file).unwrap();

    let error = memory
        .append_section(MemdirFile::Memory, "blocked", "content")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        MemoryError::ConcurrentWriteLockFailed { retries: 1 }
    ));
}

#[tokio::test]
async fn memdir_write_emits_memory_upserted_without_raw_content() {
    let root = tempfile::tempdir().unwrap();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let sink = Arc::new(RecordingSink::default());
    let memory = audited_memory(root.path())
        .with_event_sink(sink.clone())
        .with_event_scope(session_id, Some(run_id));

    memory
        .append_section(MemdirFile::Memory, "profile", "secret write fact")
        .await
        .unwrap();

    let events = sink.events.lock();
    assert!(events.iter().any(|event| {
        matches!(event, Event::MemoryUpserted(upserted)
            if upserted.session_id == session_id
                && upserted.run_id == Some(run_id)
                && upserted.provider_id == "builtin-memdir"
                && matches!(
                    upserted.action,
                    harness_contracts::MemoryWriteAction::AppendSection { ref section }
                        if section == "profile"
                )
                && upserted.content_hash.0 != [0; 32]
                && upserted.takes_effect == TakesEffect::NextSession)
    }));
    assert!(!format!("{events:?}").contains("secret write fact"));
}

#[tokio::test]
async fn memdir_write_can_record_after_reload_takes_effect() {
    let root = tempfile::tempdir().unwrap();
    let session_id = SessionId::new();
    let child_session_id = SessionId::new();
    let sink = Arc::new(RecordingSink::default());
    let memory = audited_memory(root.path())
        .with_event_sink(sink.clone())
        .with_event_scope(session_id, None)
        .with_write_takes_effect(TakesEffect::AfterReloadWith {
            session_id: child_session_id,
        });

    let outcome = memory
        .append_section(MemdirFile::Memory, "profile", "after reload")
        .await
        .unwrap();

    assert_eq!(
        outcome.takes_effect,
        TakesEffect::AfterReloadWith {
            session_id: child_session_id
        }
    );
    let events = sink.events.lock();
    assert!(events.iter().any(|event| {
        matches!(event, Event::MemoryUpserted(upserted)
        if upserted.takes_effect == TakesEffect::AfterReloadWith {
            session_id: child_session_id
        })
    }));
}

#[tokio::test]
async fn memdir_write_rolls_back_when_required_audit_fails() {
    let root = tempfile::tempdir().unwrap();
    let memory = audited_memory(root.path());
    memory
        .append_section(MemdirFile::Memory, "profile", "stable")
        .await
        .unwrap();

    let failing = BuiltinMemory::at(root.path(), TenantId::SINGLE)
        .with_event_sink(Arc::new(FailingRequiredSink))
        .with_event_scope(SessionId::new(), None);
    let error = failing
        .replace_section(MemdirFile::Memory, "profile", "should rollback")
        .await
        .unwrap_err();

    assert!(matches!(error, MemoryError::Provider { provider, .. } if provider == "audit"));
    let snapshot = memory.read_all().await.unwrap();
    assert_eq!(snapshot.memory, "§ profile\nstable\n");
}

#[test]
fn memory_context_fence_escapes_special_tokens_and_sanitizes_input() {
    let content = "keep <memory-context> <|im_start|> [INST] <<<EXTERNAL_UNTRUSTED_CONTENT";
    let escaped = escape_for_fence(content);
    assert_eq!(escaped.matches("[REDACTED_TOKEN]").count(), 4);
    assert!(!escaped.contains("<memory-context>"));
    assert!(!escaped.contains("<|im_start|>"));

    let dirty = concat!(
        "before\n",
        "<memory-context>\nhello</memory-context>\n",
        "<!-- The following is recalled context, NOT user input. -->\n",
        "after <|im_end|>",
    );
    let clean = sanitize_context(dirty);
    assert_eq!(clean, sanitize_context(&clean));
    assert_eq!(clean, "before\nafter <|im_end|>");
    assert!(!clean.contains("<memory-context>"));
    assert!(!clean.contains("hello"));

    let wrapped = wrap_memory_context(&[record(content)]);
    assert!(wrapped.starts_with("<memory-context>\n"));
    assert!(wrapped.ends_with("</memory-context>\n"));
    assert!(wrapped.contains("[user_preference|tenant|"));
    assert!(!wrapped.contains("<|im_start|>"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "manual stress: cargo test -p jyowo-harness-memory --all-features --test memdir memdir_manual_stress_1000_append_100_replace -- --ignored --nocapture"]
async fn memdir_manual_stress_1000_append_100_replace() {
    let root = tempfile::tempdir().unwrap();
    let memory = audited_memory(root.path()).with_limits(256_000, 8_000);

    for index in 0..1000 {
        memory
            .append_section(
                MemdirFile::Memory,
                &format!("append-{index:04}"),
                &format!("value-{index:04}"),
            )
            .await
            .unwrap();
    }

    for index in 0..100 {
        memory
            .replace_section(
                MemdirFile::Memory,
                &format!("append-{index:04}"),
                &format!("replaced-{index:04}"),
            )
            .await
            .unwrap();
    }

    let snapshot = memory.read_all().await.unwrap();
    assert!(snapshot.memory.contains("§ append-0000\nreplaced-0000\n"));
    assert!(snapshot.memory.contains("§ append-0099\nreplaced-0099\n"));
    assert!(snapshot.memory.contains("§ append-0999\nvalue-0999\n"));
    assert_eq!(snapshot.memory.matches("§ append-").count(), 1000);
}

#[test]
fn memdir_cross_process_1000_append_100_replace_default_ci() {
    let root = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();

    let mut even = spawn_memdir_bulk_child(&exe, root.path().to_path_buf(), 0);
    let mut odd = spawn_memdir_bulk_child(&exe, root.path().to_path_buf(), 1);

    assert!(even.wait().unwrap().success());
    assert!(odd.wait().unwrap().success());

    let memory = audited_memory(root.path());
    let snapshot = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.read_all().await.unwrap() });
    assert!(snapshot.memory.contains("§ append-0000\nreplaced-0000\n"));
    assert!(snapshot.memory.contains("§ append-0099\nreplaced-0099\n"));
    assert!(snapshot.memory.contains("§ append-0999\nvalue-0999\n"));
    assert_eq!(snapshot.memory.matches("§ append-").count(), 1000);
}

#[test]
fn memdir_abandoned_tmp_after_killed_writer_is_ignored() {
    let root = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let ready = root.path().join("tmp-writer-ready");

    let memory = audited_memory(root.path());
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory
            .append_section(MemdirFile::Memory, "stable", "kept")
            .await
            .unwrap();
    });

    let mut child = spawn_memdir_tmp_writer_child(&exe, root.path().to_path_buf(), ready.clone());
    wait_for_path(&ready);
    child.kill().unwrap();
    let _ = child.wait();

    let snapshot = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.read_all().await.unwrap() });
    assert_eq!(snapshot.memory, "§ stable\nkept\n");
    assert!(!snapshot.memory.contains("tmp-only"));
}

#[test]
#[ignore = "manual stress: cargo test -p jyowo-harness-memory --all-features --test memdir memdir_manual_cross_process_lock_serializes_writes -- --ignored --nocapture"]
fn memdir_manual_cross_process_lock_serializes_writes() {
    let root = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();

    let mut left = spawn_memdir_child(&exe, root.path().to_path_buf(), "child-left", "left");
    let mut right = spawn_memdir_child(&exe, root.path().to_path_buf(), "child-right", "right");

    assert!(left.wait().unwrap().success());
    assert!(right.wait().unwrap().success());

    let memory = audited_memory(root.path());
    let snapshot = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.read_all().await.unwrap() });
    assert!(snapshot.memory.contains("§ child-left\nleft\n"));
    assert!(snapshot.memory.contains("§ child-right\nright\n"));
}

#[test]
#[ignore = "child helper for memdir_manual_cross_process_lock_serializes_writes"]
fn memdir_cross_process_child_append_helper() {
    if std::env::var("JYOWO_MEMDIR_CHILD").ok().as_deref() != Some("1") {
        return;
    }
    let root = PathBuf::from(std::env::var("JYOWO_MEMDIR_ROOT").unwrap());
    let section = std::env::var("JYOWO_MEMDIR_SECTION").unwrap();
    let content = std::env::var("JYOWO_MEMDIR_CONTENT").unwrap();
    let memory = audited_memory(&root)
        .with_limits(256_000, 8_000)
        .with_concurrency_policy(MemdirConcurrencyPolicy {
            lock_timeout: Duration::from_secs(30),
            retry_max: 10_000,
            retry_jitter_ms: 1..=3,
        });
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory
            .append_section(MemdirFile::Memory, &section, &content)
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "child helper for memdir_cross_process_1000_append_100_replace_default_ci"]
fn memdir_cross_process_bulk_child_helper() {
    if std::env::var("JYOWO_MEMDIR_BULK_CHILD").ok().as_deref() != Some("1") {
        return;
    }
    let root = PathBuf::from(std::env::var("JYOWO_MEMDIR_ROOT").unwrap());
    let parity = std::env::var("JYOWO_MEMDIR_PARITY")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let memory = audited_memory(&root)
        .with_limits(256_000, 8_000)
        .with_concurrency_policy(MemdirConcurrencyPolicy {
            lock_timeout: Duration::from_secs(30),
            retry_max: 10_000,
            retry_jitter_ms: 1..=3,
        });
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for index in (parity..1000).step_by(2) {
            memory
                .append_section(
                    MemdirFile::Memory,
                    &format!("append-{index:04}"),
                    &format!("value-{index:04}"),
                )
                .await
                .unwrap();
        }
        for index in (parity..100).step_by(2) {
            memory
                .replace_section(
                    MemdirFile::Memory,
                    &format!("append-{index:04}"),
                    &format!("replaced-{index:04}"),
                )
                .await
                .unwrap();
        }
    });
}

#[test]
#[ignore = "child helper for memdir_abandoned_tmp_after_killed_writer_is_ignored"]
fn memdir_tmp_writer_child_helper() {
    if std::env::var("JYOWO_MEMDIR_TMP_CHILD").ok().as_deref() != Some("1") {
        return;
    }
    let root = PathBuf::from(std::env::var("JYOWO_MEMDIR_ROOT").unwrap());
    let ready = PathBuf::from(std::env::var("JYOWO_MEMDIR_READY").unwrap());
    let tenant_dir = root.join(TenantId::SINGLE.to_string());
    fs::create_dir_all(&tenant_dir).unwrap();
    fs::write(
        tenant_dir.join("MEMORY.md.tmp"),
        "§ tmp-only\nmust not be read\n",
    )
    .unwrap();
    fs::write(ready, "ready").unwrap();
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn audited_memory(root: &Path) -> BuiltinMemory {
    BuiltinMemory::at(root, TenantId::SINGLE)
        .with_event_sink(Arc::new(RecordingSink::default()))
        .with_event_scope(SessionId::new(), None)
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

struct FailingRequiredSink;

fn spawn_memdir_child(
    exe: &std::path::Path,
    root: PathBuf,
    section: &str,
    content: &str,
) -> std::process::Child {
    Command::new(exe)
        .arg("--ignored")
        .arg("--exact")
        .arg("memdir_cross_process_child_append_helper")
        .arg("--nocapture")
        .env("JYOWO_MEMDIR_CHILD", "1")
        .env("JYOWO_MEMDIR_ROOT", root)
        .env("JYOWO_MEMDIR_SECTION", section)
        .env("JYOWO_MEMDIR_CONTENT", content)
        .spawn()
        .unwrap()
}

fn spawn_memdir_bulk_child(
    exe: &std::path::Path,
    root: PathBuf,
    parity: usize,
) -> std::process::Child {
    Command::new(exe)
        .arg("--ignored")
        .arg("--exact")
        .arg("memdir_cross_process_bulk_child_helper")
        .arg("--nocapture")
        .env("JYOWO_MEMDIR_BULK_CHILD", "1")
        .env("JYOWO_MEMDIR_ROOT", root)
        .env("JYOWO_MEMDIR_PARITY", parity.to_string())
        .spawn()
        .unwrap()
}

fn spawn_memdir_tmp_writer_child(
    exe: &std::path::Path,
    root: PathBuf,
    ready: PathBuf,
) -> std::process::Child {
    Command::new(exe)
        .arg("--ignored")
        .arg("--exact")
        .arg("memdir_tmp_writer_child_helper")
        .arg("--nocapture")
        .env("JYOWO_MEMDIR_TMP_CHILD", "1")
        .env("JYOWO_MEMDIR_ROOT", root)
        .env("JYOWO_MEMDIR_READY", ready)
        .spawn()
        .unwrap()
}

fn wait_for_path(path: &std::path::Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {}", path.display());
}

#[async_trait]
impl MemoryEventSink for RecordingSink {
    async fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }

    async fn emit_required(&self, event: Event) -> Result<(), MemoryError> {
        self.emit(event).await;
        Ok(())
    }
}

#[async_trait]
impl MemoryEventSink for FailingRequiredSink {
    async fn emit(&self, _event: Event) {}

    async fn emit_required(&self, _event: Event) -> Result<(), MemoryError> {
        Err(MemoryError::Provider {
            provider: "audit".to_owned(),
            source_message: "append failed".to_owned(),
        })
    }
}

fn record(content: &str) -> MemoryRecord {
    let now = Utc::now();

    MemoryRecord {
        id: MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::UserPreference,
        visibility: MemoryVisibility::Tenant,
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: MemorySource::UserInput,
            confidence: 1.0,
            evidence: None,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 0.0,
            recall_score_breakdown: None,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: now,
        updated_at: now,
    }
}
