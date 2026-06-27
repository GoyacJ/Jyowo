#![cfg(feature = "builtin-toolset")]

use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{future::BoxFuture, stream, StreamExt};
use harness_contracts::{
    BlobError, BlobMeta, BlobReaderCap, BlobReaderCapAdapter, BlobRef, BlobRetention, BlobStore,
    CapabilityRegistry, Decision, DecisionScope, OffloadedBlobAuthorizerCap, PermissionError,
    PermissionSubject, Severity, TenantId, ToolCapability, ToolError, ToolResult, ToolUseId,
};
use harness_permission::{PermissionBroker, PermissionCheck, PermissionContext, PermissionRequest};
use harness_tool::{
    builtin::{FileReadTool, FileWriteTool, GrepTool, ListDirTool, ReadBlobTool},
    BuiltinToolset, InterruptToken, Tool, ToolContext, ToolRegistry,
};
use serde_json::{json, Value};
use tempfile::tempdir;

#[tokio::test]
async fn file_read_reads_utf8_and_line_ranges() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let tool = FileReadTool::default();

    assert_asks_for_permission(&tool, json!({ "path": file })).await;

    let result = execute_final(
        &tool,
        json!({
            "path": file,
            "start_line": 2,
            "end_line": 3
        }),
        tool_ctx_at(dir.path(), CapabilityRegistry::default()),
    )
    .await;

    assert_eq!(result, ToolResult::Text("two\nthree\n".to_owned()));
}

#[tokio::test]
async fn file_write_overwrites_file_and_asks_for_path_permission() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("out.txt");
    let tool = FileWriteTool::default();

    let check = tool
        .check_permission(
            &json!({ "path": file, "content": "new" }),
            &tool_ctx(CapabilityRegistry::default()),
        )
        .await;
    assert!(matches!(
        check,
        PermissionCheck::AskUser {
            subject: PermissionSubject::FileWrite { .. },
            scope: DecisionScope::PathPrefix(_)
        }
    ));

    let result = execute_final(
        &tool,
        json!({ "path": file, "content": "new" }),
        tool_ctx_at(dir.path(), CapabilityRegistry::default()),
    )
    .await;

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "new");
    assert_eq!(
        result,
        ToolResult::Structured(json!({ "path": file, "bytes": 3 }))
    );
}

#[tokio::test]
async fn file_tools_report_dangerous_path_patterns_before_normal_permission() {
    let ctx = tool_ctx(CapabilityRegistry::default());

    let read_check = FileReadTool::default()
        .check_permission(&json!({ "path": "/etc/passwd" }), &ctx)
        .await;
    assert!(matches!(
        read_check,
        PermissionCheck::DangerousPattern {
            ref kind,
            ref pattern,
            severity: Severity::Critical,
            ..
        } if kind == "path" && pattern == "path-unix-system-auth-db"
    ));

    let write_check = FileWriteTool::default()
        .check_permission(
            &json!({ "path": "/tmp/workspace/.ssh/id_rsa", "content": "secret" }),
            &ctx,
        )
        .await;
    assert!(matches!(
        write_check,
        PermissionCheck::DangerousPattern {
            ref kind,
            ref pattern,
            severity: Severity::High,
            ..
        } if kind == "path" && pattern == "path-unix-ssh-credential"
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn file_read_workspace_escape_is_denied_before_broker() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let outside_file = root.path().join("outside.txt");
    std::fs::write(&outside_file, "secret\n").unwrap();
    std::os::unix::fs::symlink(&outside_file, workspace.join("link.txt")).unwrap();
    let tool = FileReadTool::default();
    let ctx = tool_ctx_at(&workspace, CapabilityRegistry::default());

    for input in [
        json!({ "path": "../outside.txt" }),
        json!({ "path": outside_file }),
        json!({ "path": "link.txt" }),
    ] {
        let check = tool.check_permission(&input, &ctx).await;
        assert!(matches!(check, PermissionCheck::Denied { .. }));
    }
}

#[tokio::test]
async fn list_dir_is_stable_and_hides_dotfiles_by_default() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();
    std::fs::write(dir.path().join(".hidden"), "hidden").unwrap();
    std::fs::create_dir(dir.path().join("a_dir")).unwrap();
    let tool = ListDirTool::default();

    let result = execute_final(
        &tool,
        json!({ "path": dir.path() }),
        tool_ctx_at(dir.path(), CapabilityRegistry::default()),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured list result");
    };
    let names: Vec<_> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["a_dir", "b.txt"]);
}

#[tokio::test]
async fn list_dir_honors_max_depth_and_reports_modified_timestamp() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("nested").join("deep.txt"), "deep").unwrap();
    std::fs::write(dir.path().join("root.txt"), "root").unwrap();
    let tool = ListDirTool::default();

    let result = execute_final(
        &tool,
        json!({ "path": dir.path(), "max_depth": 2 }),
        tool_ctx_at(dir.path(), CapabilityRegistry::default()),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured list result");
    };
    let entries = value.as_array().unwrap();
    let paths: Vec<_> = entries
        .iter()
        .map(|entry| entry["path"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(paths, ["nested", "nested/deep.txt", "root.txt"]);
    assert!(entries
        .iter()
        .all(|entry| entry["modified"].as_str().is_some()));
}

#[tokio::test]
async fn grep_uses_rg_and_returns_matches() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "alpha\nneedle\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "other\nneedle two\n").unwrap();
    let tool = GrepTool::default();

    let result = execute_final(
        &tool,
        json!({ "path": dir.path(), "pattern": "needle" }),
        tool_ctx_at(dir.path(), CapabilityRegistry::default()),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured grep result");
    };
    let matches = value.as_array().unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0]["line"], 2);
    assert!(matches[0]["path"].as_str().unwrap().ends_with("a.txt"));
    assert_eq!(matches[1]["line"], 2);
    assert!(matches[1]["path"].as_str().unwrap().ends_with("b.txt"));
}

#[tokio::test]
async fn file_read_rejects_invalid_line_window() {
    let tool = FileReadTool::default();
    let ctx = tool_ctx(CapabilityRegistry::default());

    let negative = tool
        .validate(&json!({ "path": "notes.txt", "start_line": -1 }), &ctx)
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(negative, "start_line must be a positive integer");

    let zero = tool
        .validate(&json!({ "path": "notes.txt", "end_line": 0 }), &ctx)
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(zero, "end_line must be greater than 0");
}

#[tokio::test]
async fn read_blob_reads_from_capability_registry_and_reports_missing_store() {
    let blob_ref = BlobRef {
        id: harness_contracts::BlobId::new(),
        size: 5,
        content_hash: [9; 32],
        content_type: Some("text/plain".to_owned()),
    };
    let store: Arc<dyn BlobStore> = Arc::new(TestBlobStore {
        blob_ref: blob_ref.clone(),
        bytes: Bytes::from_static(b"hello"),
    });
    let reader: Arc<dyn BlobReaderCap> = Arc::new(BlobReaderCapAdapter::new(store));
    let mut caps = CapabilityRegistry::default();
    caps.install(ToolCapability::BlobReader, reader);
    caps.install::<dyn OffloadedBlobAuthorizerCap>(
        ToolCapability::OffloadedBlobAuthorizer,
        Arc::new(AllowOffloadedBlobAuthorizer),
    );
    let tool = ReadBlobTool::default();

    let result = execute_final(&tool, json!({ "blob_ref": blob_ref }), tool_ctx(caps)).await;
    assert_eq!(result, ToolResult::Text("hello".to_owned()));

    let mut missing_reader_caps = CapabilityRegistry::default();
    missing_reader_caps.install::<dyn OffloadedBlobAuthorizerCap>(
        ToolCapability::OffloadedBlobAuthorizer,
        Arc::new(AllowOffloadedBlobAuthorizer),
    );
    let error = execute_error(
        &tool,
        json!({ "blob_ref": blob_ref }),
        tool_ctx(missing_reader_caps),
    )
    .await;
    assert!(matches!(
        error,
        ToolError::CapabilityMissing(harness_contracts::ToolCapability::BlobReader)
    ));
}

#[tokio::test]
async fn read_blob_rejects_invalid_read_window() {
    let blob_ref = test_blob_ref(5, [8; 32]);
    let tool = ReadBlobTool::default();
    let ctx = tool_ctx(CapabilityRegistry::default());

    let negative_offset = tool
        .validate(&json!({ "blob_ref": blob_ref, "offset": -1 }), &ctx)
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(negative_offset, "offset must be a non-negative integer");

    let invalid_limit = tool
        .validate(&json!({ "blob_ref": blob_ref, "limit": "bad" }), &ctx)
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(invalid_limit, "limit must be a positive integer");
}

#[tokio::test]
async fn read_blob_requires_offloaded_blob_authorizer() {
    let blob_ref = test_blob_ref(5, [7; 32]);
    let reader: Arc<dyn BlobReaderCap> = Arc::new(CountingBlobReader {
        blob_ref: blob_ref.clone(),
        reads: Arc::new(AtomicUsize::new(0)),
    });
    let mut caps = CapabilityRegistry::default();
    caps.install(ToolCapability::BlobReader, reader);
    let tool = ReadBlobTool::default();

    let error = execute_error(&tool, json!({ "blob_ref": blob_ref }), tool_ctx(caps)).await;

    assert!(matches!(
        error,
        ToolError::CapabilityMissing(ToolCapability::OffloadedBlobAuthorizer)
    ));
}

#[tokio::test]
async fn read_blob_rejects_unauthorized_blob_before_reading_store() {
    let blob_ref = test_blob_ref(5, [6; 32]);
    let reads = Arc::new(AtomicUsize::new(0));
    let reader: Arc<dyn BlobReaderCap> = Arc::new(CountingBlobReader {
        blob_ref: blob_ref.clone(),
        reads: Arc::clone(&reads),
    });
    let mut caps = CapabilityRegistry::default();
    caps.install(ToolCapability::BlobReader, reader);
    caps.install::<dyn OffloadedBlobAuthorizerCap>(
        ToolCapability::OffloadedBlobAuthorizer,
        Arc::new(DenyOffloadedBlobAuthorizer),
    );
    let tool = ReadBlobTool::default();

    let error = execute_error(&tool, json!({ "blob_ref": blob_ref }), tool_ctx(caps)).await;

    assert!(matches!(error, ToolError::PermissionDenied(_)));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn read_blob_respects_offset_and_limit_without_consuming_full_blob() {
    let blob_ref = BlobRef {
        id: harness_contracts::BlobId::new(),
        size: 10,
        content_hash: [8; 32],
        content_type: Some("text/plain".to_owned()),
    };
    let yielded = Arc::new(AtomicUsize::new(0));
    let store: Arc<dyn BlobStore> = Arc::new(ChunkedBlobStore {
        blob_ref: blob_ref.clone(),
        chunks: vec![Bytes::from_static(b"hello"), Bytes::from_static(b"world")],
        yielded: Arc::clone(&yielded),
    });
    let reader: Arc<dyn BlobReaderCap> = Arc::new(BlobReaderCapAdapter::new(store));
    let mut caps = CapabilityRegistry::default();
    caps.install(ToolCapability::BlobReader, reader);
    caps.install::<dyn OffloadedBlobAuthorizerCap>(
        ToolCapability::OffloadedBlobAuthorizer,
        Arc::new(AllowOffloadedBlobAuthorizer),
    );
    let tool = ReadBlobTool::default();

    let result = execute_final(
        &tool,
        json!({
            "blob_ref": blob_ref,
            "offset": 1,
            "limit": 3
        }),
        tool_ctx(caps),
    )
    .await;

    assert_eq!(result, ToolResult::Text("ell".to_owned()));
    assert_eq!(yielded.load(Ordering::SeqCst), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn file_io_tools_reject_workspace_escape_paths_before_fs_access() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let outside_file = root.path().join("outside.txt");
    let outside_write = root.path().join("write.txt");
    let outside_dir = root.path().join("outside_dir");
    std::fs::create_dir(&outside_dir).unwrap();
    std::fs::write(&outside_file, "secret\n").unwrap();
    std::fs::write(outside_dir.join("a.txt"), "needle\n").unwrap();
    std::os::unix::fs::symlink(&outside_file, workspace.join("link.txt")).unwrap();
    let ctx = || tool_ctx_at(&workspace, CapabilityRegistry::default());

    assert!(matches!(
        execute_error(
            &FileReadTool::default(),
            json!({ "path": "../outside.txt" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &FileReadTool::default(),
            json!({ "path": outside_file }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &FileReadTool::default(),
            json!({ "path": "link.txt" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &FileWriteTool::default(),
            json!({ "path": "../write.txt", "content": "nope" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(!outside_write.exists());
    assert!(matches!(
        execute_error(
            &ListDirTool::default(),
            json!({ "path": outside_dir }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &GrepTool::default(),
            json!({ "path": outside_dir, "pattern": "needle" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
}

#[test]
fn default_builtin_toolset_registers_m3_t04a_tools_without_model_or_journal_deps() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    for name in ["FileRead", "FileWrite", "ListDir", "Grep", "ReadBlob"] {
        assert!(registry.get(name).is_some(), "{name} should be registered");
    }

    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    #[cfg(not(feature = "minimax-tools"))]
    assert!(!manifest.contains("jyowo-harness-model"));
    assert!(!manifest.contains("jyowo-harness-journal"));
}

async fn assert_asks_for_permission(tool: &dyn Tool, input: Value) {
    let check = tool
        .check_permission(&input, &tool_ctx(CapabilityRegistry::default()))
        .await;
    assert!(matches!(check, PermissionCheck::AskUser { .. }));
}

async fn execute_final(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolResult {
    tool.validate(&input, &ctx).await.unwrap();
    let mut stream = tool.execute(input, ctx).await.unwrap();
    match stream.next().await {
        Some(harness_tool::ToolEvent::Final(result)) => result,
        other => panic!("expected final result, got {other:?}"),
    }
}

async fn execute_error(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolError {
    tool.validate(&input, &ctx).await.unwrap();
    match tool.execute(input, ctx).await {
        Ok(_) => panic!("expected tool error"),
        Err(error) => error,
    }
}

fn tool_ctx(cap_registry: CapabilityRegistry) -> ToolContext {
    tool_ctx_at(std::env::temp_dir(), cap_registry)
}

fn tool_ctx_at(workspace_root: impl AsRef<Path>, cap_registry: CapabilityRegistry) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: workspace_root.as_ref().to_path_buf(),
        sandbox: None,
        permission_broker: Arc::new(AllowBroker),
        cap_registry: Arc::new(cap_registry),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
    }
}

#[derive(Debug)]
struct AllowBroker;

#[async_trait]
impl PermissionBroker for AllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

#[derive(Clone)]
struct TestBlobStore {
    blob_ref: BlobRef,
    bytes: Bytes,
}

#[derive(Clone)]
struct ChunkedBlobStore {
    blob_ref: BlobRef,
    chunks: Vec<Bytes>,
    yielded: Arc<AtomicUsize>,
}

struct CountingBlobReader {
    blob_ref: BlobRef,
    reads: Arc<AtomicUsize>,
}

struct AllowOffloadedBlobAuthorizer;

struct DenyOffloadedBlobAuthorizer;

fn test_blob_ref(size: u64, content_hash: [u8; 32]) -> BlobRef {
    BlobRef {
        id: harness_contracts::BlobId::new(),
        size,
        content_hash,
        content_type: Some("text/plain".to_owned()),
    }
}

impl BlobReaderCap for CountingBlobReader {
    fn read_blob(
        &self,
        _tenant_id: TenantId,
        blob: BlobRef,
    ) -> BoxFuture<'_, Result<futures::stream::BoxStream<'static, Bytes>, ToolError>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if blob.id != self.blob_ref.id {
                return Err(ToolError::Message("unexpected blob".to_owned()));
            }
            let stream: futures::stream::BoxStream<'static, Bytes> =
                Box::pin(stream::once(async { Bytes::from_static(b"hello") }));
            Ok(stream)
        })
    }
}

impl OffloadedBlobAuthorizerCap for AllowOffloadedBlobAuthorizer {
    fn authorize_offloaded_blob(
        &self,
        _tenant_id: TenantId,
        _session_id: harness_contracts::SessionId,
        _run_id: harness_contracts::RunId,
        _blob: BlobRef,
    ) -> BoxFuture<'_, Result<(), ToolError>> {
        Box::pin(async { Ok(()) })
    }
}

impl OffloadedBlobAuthorizerCap for DenyOffloadedBlobAuthorizer {
    fn authorize_offloaded_blob(
        &self,
        _tenant_id: TenantId,
        _session_id: harness_contracts::SessionId,
        _run_id: harness_contracts::RunId,
        _blob: BlobRef,
    ) -> BoxFuture<'_, Result<(), ToolError>> {
        Box::pin(async {
            Err(ToolError::PermissionDenied(
                "blob not offloaded in run".to_owned(),
            ))
        })
    }
}

#[async_trait]
impl BlobStore for ChunkedBlobStore {
    fn store_id(&self) -> &'static str {
        "chunked"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        _bytes: Bytes,
        _meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        Ok(self.blob_ref.clone())
    }

    async fn get(
        &self,
        _tenant: TenantId,
        blob: &BlobRef,
    ) -> Result<futures::stream::BoxStream<'static, Bytes>, BlobError> {
        if blob.id != self.blob_ref.id {
            return Err(BlobError::NotFound(blob.id));
        }
        let chunks = self.chunks.clone();
        let yielded = Arc::clone(&self.yielded);
        Ok(Box::pin(stream::unfold(0, move |index| {
            let chunks = chunks.clone();
            let yielded = Arc::clone(&yielded);
            async move {
                let chunk = chunks.get(index).cloned()?;
                yielded.fetch_add(1, Ordering::SeqCst);
                Some((chunk, index + 1))
            }
        })))
    }

    async fn head(&self, _tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError> {
        Ok((blob.id == self.blob_ref.id).then(|| BlobMeta {
            content_type: self.blob_ref.content_type.clone(),
            size: self.blob_ref.size,
            content_hash: self.blob_ref.content_hash,
            created_at: chrono::Utc::now(),
            retention: BlobRetention::TenantScoped,
        }))
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Ok(())
    }
}

#[async_trait]
impl BlobStore for TestBlobStore {
    fn store_id(&self) -> &'static str {
        "test"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        _bytes: Bytes,
        _meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        Ok(self.blob_ref.clone())
    }

    async fn get(
        &self,
        _tenant: TenantId,
        blob: &BlobRef,
    ) -> Result<futures::stream::BoxStream<'static, Bytes>, BlobError> {
        if blob.id != self.blob_ref.id {
            return Err(BlobError::NotFound(blob.id));
        }
        Ok(Box::pin(stream::once({
            let bytes = self.bytes.clone();
            async move { bytes }
        })))
    }

    async fn head(&self, _tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError> {
        Ok((blob.id == self.blob_ref.id).then(|| BlobMeta {
            content_type: self.blob_ref.content_type.clone(),
            size: self.blob_ref.size,
            content_hash: self.blob_ref.content_hash,
            created_at: chrono::Utc::now(),
            retention: BlobRetention::TenantScoped,
        }))
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Ok(())
    }
}
