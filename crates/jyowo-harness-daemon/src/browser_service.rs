use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use harness_contracts::{
    BrowserCommand, BrowserSessionState, BrowserSessionStatus, TaskId, ToolError,
};
use harness_tool::builtin::{BrokeredPlatformRuntimeCap, BrokeredPlatformRuntimeRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

const HOST_RESPONSE_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Error)]
pub enum BrowserServiceError {
    #[error("browser runtime is unavailable: {0}")]
    Unavailable(String),
    #[error("browser runtime I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("browser runtime protocol failed: {0}")]
    Protocol(String),
    #[error("browser runtime request timed out")]
    Timeout,
}

#[derive(Debug, Clone)]
struct BrowserRuntimeConfig {
    node: PathBuf,
    script: PathBuf,
    chrome: Option<PathBuf>,
    session_root: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserRuntimeManifest {
    schema_version: u16,
    node_path: String,
    script_path: String,
    chrome_executable: String,
}

impl BrowserRuntimeConfig {
    fn discover(session_root: PathBuf) -> Result<Self, String> {
        let runtime_root = std::env::var_os("JYOWO_BROWSER_RUNTIME_DIR").map(PathBuf::from);
        let manifest = runtime_root
            .as_ref()
            .map(|root| read_runtime_manifest(root))
            .transpose()?;

        let node = std::env::var_os("JYOWO_BROWSER_NODE")
            .map(PathBuf::from)
            .or_else(|| {
                runtime_root
                    .as_ref()
                    .zip(manifest.as_ref())
                    .map(|(root, value)| resolve_runtime_path(root, &value.node_path))
            })
            .ok_or_else(|| "bundled Node.js executable was not found".to_owned())?;
        let script = std::env::var_os("JYOWO_BROWSER_RUNTIME_SCRIPT")
            .map(PathBuf::from)
            .or_else(|| {
                runtime_root
                    .as_ref()
                    .zip(manifest.as_ref())
                    .map(|(root, value)| resolve_runtime_path(root, &value.script_path))
            })
            .ok_or_else(|| "browser host script was not found".to_owned())?;
        let chrome = std::env::var_os("JYOWO_BROWSER_EXECUTABLE")
            .map(PathBuf::from)
            .or_else(|| {
                runtime_root
                    .as_ref()
                    .zip(manifest.as_ref())
                    .map(|(root, value)| resolve_runtime_path(root, &value.chrome_executable))
            });

        if node.components().count() > 1 && !node.is_file() {
            return Err(format!(
                "Node.js executable does not exist: {}",
                node.display()
            ));
        }
        if !script.is_file() {
            return Err(format!(
                "browser host script does not exist: {}",
                script.display()
            ));
        }
        if chrome.as_ref().is_some_and(|path| !path.is_file()) {
            return Err(format!(
                "Chrome for Testing executable does not exist: {}",
                chrome.as_ref().expect("checked").display()
            ));
        }

        Ok(Self {
            node,
            script,
            chrome,
            session_root,
        })
    }
}

fn read_runtime_manifest(root: &Path) -> Result<BrowserRuntimeManifest, String> {
    let path = root.join("runtime-manifest.json");
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let manifest: BrowserRuntimeManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported browser runtime manifest schema version {}",
            manifest.schema_version
        ));
    }
    Ok(manifest)
}

fn resolve_runtime_path(root: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

pub struct BrowserService {
    config: Result<BrowserRuntimeConfig, String>,
    host: Mutex<Option<BrowserHostProcess>>,
}

impl BrowserService {
    #[must_use]
    pub fn from_environment(session_root: impl Into<PathBuf>) -> Self {
        Self {
            config: BrowserRuntimeConfig::discover(session_root.into()),
            host: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            config: Err(reason.into()),
            host: Mutex::new(None),
        }
    }

    pub async fn handle(
        &self,
        task_id: TaskId,
        command: BrowserCommand,
    ) -> Result<BrowserSessionState, BrowserServiceError> {
        let unavailable_reason = match &self.config {
            Ok(_) => None,
            Err(reason) => Some(reason.clone()),
        };
        if let Some(reason) = unavailable_reason {
            return Ok(BrowserSessionState {
                task_id,
                status: BrowserSessionStatus::Unavailable,
                dashboard_url: None,
                current_url: None,
                title: None,
                unavailable_reason: Some(reason),
            });
        }

        if matches!(command, BrowserCommand::Status) && self.host.lock().await.is_none() {
            return Ok(stopped_state(task_id));
        }

        let (method, params) = match command {
            BrowserCommand::Open { url } => ("open", json!({ "url": url })),
            BrowserCommand::Status => ("status", json!({})),
            BrowserCommand::Close => ("close", json!({})),
            BrowserCommand::Show => ("show", json!({})),
        };
        let value = self.call(task_id, method, params).await?;
        serde_json::from_value(value).map_err(|error| {
            BrowserServiceError::Protocol(format!("invalid browser session state: {error}"))
        })
    }

    pub async fn execute_tool(
        &self,
        task_id: TaskId,
        request: BrokeredPlatformRuntimeRequest,
    ) -> Result<Value, ToolError> {
        if !matches!(request.tool_name.as_str(), "BrowserUse" | "BrowserDevTools") {
            return Err(ToolError::Message(format!(
                "the browser runtime does not implement {}",
                request.tool_name
            )));
        }
        if let Err(reason) = &self.config {
            return Err(ToolError::Message(format!(
                "browser runtime is unavailable: {reason}"
            )));
        }
        let workspace_root = request
            .project_workspace_root
            .as_deref()
            .unwrap_or(request.workspace_root.as_path());
        self.call(
            task_id,
            "tool",
            json!({
                "toolName": request.tool_name,
                "input": request.input,
                "workspaceRoot": workspace_root,
            }),
        )
        .await
        .map_err(|error| ToolError::Message(error.to_string()))
    }

    pub async fn shutdown(&self) {
        let mut host = self.host.lock().await;
        let Some(process) = host.as_mut() else {
            return;
        };
        let _ = tokio::time::timeout(
            Duration::from_secs(10),
            process.call(TaskId::new(), "shutdown", json!({})),
        )
        .await;
        if tokio::time::timeout(Duration::from_secs(5), process.child.wait())
            .await
            .is_err()
        {
            let _ = process.child.start_kill();
        }
        *host = None;
    }

    async fn call(
        &self,
        task_id: TaskId,
        method: &str,
        params: Value,
    ) -> Result<Value, BrowserServiceError> {
        let config = self
            .config
            .as_ref()
            .map_err(|reason| BrowserServiceError::Unavailable(reason.clone()))?;
        let mut host = self.host.lock().await;
        if host.is_none() {
            *host = Some(BrowserHostProcess::spawn(config).await?);
        }
        let result = tokio::time::timeout(
            HOST_RESPONSE_TIMEOUT,
            host.as_mut()
                .expect("browser host was initialized")
                .call(task_id, method, params),
        )
        .await;
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                if let Some(process) = host.as_mut() {
                    let _ = process.child.start_kill();
                }
                *host = None;
                Err(error)
            }
            Err(_) => {
                if let Some(process) = host.as_mut() {
                    let _ = process.child.start_kill();
                }
                *host = None;
                Err(BrowserServiceError::Timeout)
            }
        }
    }
}

fn stopped_state(task_id: TaskId) -> BrowserSessionState {
    BrowserSessionState {
        task_id,
        status: BrowserSessionStatus::Stopped,
        dashboard_url: None,
        current_url: None,
        title: None,
        unavailable_reason: None,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserHostRequest<'a> {
    id: u64,
    task_id: String,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserHostResponse {
    id: u64,
    ok: bool,
    result: Option<Value>,
    error: Option<String>,
}

struct BrowserHostProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl BrowserHostProcess {
    async fn spawn(config: &BrowserRuntimeConfig) -> Result<Self, BrowserServiceError> {
        tokio::fs::create_dir_all(&config.session_root).await?;
        let mut command = Command::new(&config.node);
        command
            .arg(&config.script)
            .env("JYOWO_BROWSER_SESSION_ROOT", &config.session_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if let Some(chrome) = &config.chrome {
            command.env("JYOWO_BROWSER_EXECUTABLE", chrome);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| {
            BrowserServiceError::Protocol("browser host stdin was not created".to_owned())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            BrowserServiceError::Protocol("browser host stdout was not created".to_owned())
        })?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            next_id: 1,
        })
    }

    async fn call(
        &mut self,
        task_id: TaskId,
        method: &str,
        params: Value,
    ) -> Result<Value, BrowserServiceError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let request = BrowserHostRequest {
            id,
            task_id: task_id.to_string(),
            method,
            params,
        };
        let mut line = serde_json::to_vec(&request)
            .map_err(|error| BrowserServiceError::Protocol(error.to_string()))?;
        line.push(b'\n');
        self.stdin.write_all(&line).await?;
        self.stdin.flush().await?;

        let response_line = self.stdout.next_line().await?.ok_or_else(|| {
            BrowserServiceError::Protocol("browser host stopped without a response".to_owned())
        })?;
        let response: BrowserHostResponse =
            serde_json::from_str(&response_line).map_err(|error| {
                BrowserServiceError::Protocol(format!("invalid browser host response: {error}"))
            })?;
        if response.id != id {
            return Err(BrowserServiceError::Protocol(format!(
                "browser host response ID {} did not match {id}",
                response.id
            )));
        }
        if !response.ok {
            return Err(BrowserServiceError::Protocol(
                response
                    .error
                    .unwrap_or_else(|| "browser host request failed".to_owned()),
            ));
        }
        Ok(response.result.unwrap_or(Value::Null))
    }
}

pub struct TaskBrowserRuntime {
    service: Arc<BrowserService>,
    task_id: TaskId,
}

impl TaskBrowserRuntime {
    #[must_use]
    pub fn new(service: Arc<BrowserService>, task_id: TaskId) -> Self {
        Self { service, task_id }
    }
}

impl BrokeredPlatformRuntimeCap for TaskBrowserRuntime {
    fn execute(
        &self,
        request: BrokeredPlatformRuntimeRequest,
    ) -> BoxFuture<'static, Result<Value, ToolError>> {
        let service = Arc::clone(&self.service);
        let task_id = self.task_id;
        Box::pin(async move { service.execute_tool(task_id, request).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unavailable_runtime_reports_a_stable_session_state() {
        let task_id = TaskId::new();
        let service = BrowserService::unavailable("browser resources are missing");

        let state = service
            .handle(task_id, BrowserCommand::Status)
            .await
            .unwrap();

        assert_eq!(state.task_id, task_id);
        assert_eq!(state.status, BrowserSessionStatus::Unavailable);
        assert_eq!(
            state.unavailable_reason.as_deref(),
            Some("browser resources are missing")
        );
    }

    #[test]
    fn runtime_manifest_requires_the_bundled_chrome_executable() {
        let runtime = tempfile::tempdir().unwrap();
        std::fs::write(
            runtime.path().join("runtime-manifest.json"),
            r#"{"schemaVersion":1,"nodePath":"node/node","scriptPath":"src/runtime.mjs"}"#,
        )
        .unwrap();

        let error = read_runtime_manifest(runtime.path()).unwrap_err();

        assert!(error.contains("chromeExecutable"));
    }
}
