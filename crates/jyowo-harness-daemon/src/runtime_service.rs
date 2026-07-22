use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{header, Response, StatusCode};
use axum::routing::get;
use axum::Router;
use harness_contracts::{
    BlobId, BrowserCommand, BrowserSessionState, BrowserSessionStatus, RuntimeCommand,
    RuntimeSessionKind, RuntimeSessionState, RuntimeSessionStatus, RuntimeSpec, RuntimeView,
    TaskId,
};
use harness_journal::{BlobRead, TaskBlobStore, TaskStore, TaskStoreError};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::{BrowserService, BrowserServiceError};

const HTML_CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; connect-src 'none'; object-src 'none'; frame-src 'none'; child-src 'none'; form-action 'none'; base-uri 'none'";
const MAX_RUNTIME_TITLE_CHARS: usize = 256;

#[derive(Debug, Error)]
pub enum RuntimeServiceError {
    #[error(transparent)]
    Browser(#[from] BrowserServiceError),
    #[error("runtime service I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid runtime request: {0}")]
    InvalidInput(String),
    #[error("runtime resource was not found")]
    NotFound,
    #[error(transparent)]
    Store(#[from] TaskStoreError),
}

#[derive(Clone)]
struct HtmlPreview {
    bytes: Arc<[u8]>,
    task_id: TaskId,
    title: String,
    token: String,
}

#[derive(Clone)]
struct PreviewState {
    sessions: Arc<RwLock<HashMap<String, HtmlPreview>>>,
}

struct PreviewServer {
    address: SocketAddr,
    join: JoinHandle<()>,
    shutdown: oneshot::Sender<()>,
}

pub struct RuntimeService {
    blob_root: PathBuf,
    browser: Arc<BrowserService>,
    preview_server: Mutex<Option<PreviewServer>>,
    preview_state: PreviewState,
    store: Arc<TaskStore>,
}

impl RuntimeService {
    #[must_use]
    pub fn new(
        store: Arc<TaskStore>,
        blob_root: impl Into<PathBuf>,
        browser: Arc<BrowserService>,
    ) -> Self {
        Self {
            blob_root: blob_root.into(),
            browser,
            preview_server: Mutex::new(None),
            preview_state: PreviewState {
                sessions: Arc::new(RwLock::new(HashMap::new())),
            },
            store,
        }
    }

    pub async fn handle(
        &self,
        task_id: TaskId,
        command: RuntimeCommand,
    ) -> Result<RuntimeSessionState, RuntimeServiceError> {
        match command {
            RuntimeCommand::Open { spec } => match spec {
                RuntimeSpec::Browser { url } => {
                    self.browser_runtime(task_id, BrowserCommand::Open { url })
                        .await
                }
                RuntimeSpec::Html { blob_id, title } => {
                    self.open_html(task_id, blob_id, &title).await
                }
            },
            RuntimeCommand::Status { session_id, kind } => match kind {
                RuntimeSessionKind::Browser => {
                    self.browser_runtime(task_id, BrowserCommand::Status).await
                }
                RuntimeSessionKind::Html => self.html_status(task_id, &session_id).await,
                _ => Err(RuntimeServiceError::InvalidInput(format!(
                    "runtime kind {kind:?} is not implemented"
                ))),
            },
            RuntimeCommand::Close { session_id, kind } => match kind {
                RuntimeSessionKind::Browser => {
                    self.browser_runtime(task_id, BrowserCommand::Close).await
                }
                RuntimeSessionKind::Html => self.close_html(task_id, &session_id).await,
                _ => Err(RuntimeServiceError::InvalidInput(format!(
                    "runtime kind {kind:?} is not implemented"
                ))),
            },
        }
    }

    pub async fn handle_browser(
        &self,
        task_id: TaskId,
        command: BrowserCommand,
    ) -> Result<BrowserSessionState, RuntimeServiceError> {
        Ok(self.browser.handle(task_id, command).await?)
    }

    pub async fn shutdown(&self) {
        self.preview_state.sessions.write().await.clear();
        let server = self.preview_server.lock().await.take();
        if let Some(server) = server {
            let _ = server.shutdown.send(());
            let _ = server.join.await;
        }
    }

    async fn browser_runtime(
        &self,
        task_id: TaskId,
        command: BrowserCommand,
    ) -> Result<RuntimeSessionState, RuntimeServiceError> {
        let state = self.handle_browser(task_id, command).await?;
        Ok(RuntimeSessionState {
            session_id: "browser".to_owned(),
            task_id,
            kind: RuntimeSessionKind::Browser,
            status: match state.status {
                BrowserSessionStatus::Unavailable => RuntimeSessionStatus::Unavailable,
                BrowserSessionStatus::Starting => RuntimeSessionStatus::Starting,
                BrowserSessionStatus::Ready => RuntimeSessionStatus::Ready,
                BrowserSessionStatus::Stopped => RuntimeSessionStatus::Stopped,
                BrowserSessionStatus::Failed => RuntimeSessionStatus::Failed,
            },
            title: state.title.unwrap_or_else(|| "Browser".to_owned()),
            view: state.dashboard_url.map(|url| RuntimeView::Url { url }),
            current_url: state.current_url,
            error: state.unavailable_reason,
        })
    }

    async fn open_html(
        &self,
        task_id: TaskId,
        blob_id: BlobId,
        title: &str,
    ) -> Result<RuntimeSessionState, RuntimeServiceError> {
        let title = validate_title(title)?;
        let blobs = TaskBlobStore::open(Arc::clone(&self.store), task_id, &self.blob_root)?;
        let BlobRead::Available { blob, bytes } = blobs.read(&blob_id)? else {
            return Err(RuntimeServiceError::NotFound);
        };
        let media_type = blob.content_type.unwrap_or_default();
        if media_type
            .split(';')
            .next()
            .is_none_or(|value| value.trim() != "text/html")
        {
            return Err(RuntimeServiceError::InvalidInput(
                "HTML runtime requires a text/html blob".to_owned(),
            ));
        }
        if std::str::from_utf8(&bytes).is_err() {
            return Err(RuntimeServiceError::InvalidInput(
                "HTML runtime requires UTF-8 content".to_owned(),
            ));
        }

        let address = self.ensure_preview_server().await?;
        let session_id = format!("html-{blob_id}");
        let token = Uuid::new_v4().simple().to_string();
        self.preview_state.sessions.write().await.insert(
            session_id.clone(),
            HtmlPreview {
                bytes: bytes.into(),
                task_id,
                title: title.clone(),
                token: token.clone(),
            },
        );
        Ok(RuntimeSessionState {
            session_id: session_id.clone(),
            task_id,
            kind: RuntimeSessionKind::Html,
            status: RuntimeSessionStatus::Ready,
            title,
            view: Some(RuntimeView::Url {
                url: format!("http://{address}/preview/{session_id}/{token}"),
            }),
            current_url: None,
            error: None,
        })
    }

    async fn html_status(
        &self,
        task_id: TaskId,
        session_id: &str,
    ) -> Result<RuntimeSessionState, RuntimeServiceError> {
        let preview = {
            let sessions = self.preview_state.sessions.read().await;
            let Some(preview) = sessions.get(session_id) else {
                return Ok(stopped_html_state(task_id, session_id));
            };
            if preview.task_id != task_id {
                return Err(RuntimeServiceError::NotFound);
            }
            preview.clone()
        };
        let Some(address) = self
            .preview_server
            .lock()
            .await
            .as_ref()
            .map(|value| value.address)
        else {
            return Ok(stopped_html_state(task_id, session_id));
        };
        Ok(RuntimeSessionState {
            session_id: session_id.to_owned(),
            task_id,
            kind: RuntimeSessionKind::Html,
            status: RuntimeSessionStatus::Ready,
            title: preview.title.clone(),
            view: Some(RuntimeView::Url {
                url: format!("http://{address}/preview/{session_id}/{}", preview.token),
            }),
            current_url: None,
            error: None,
        })
    }

    async fn close_html(
        &self,
        task_id: TaskId,
        session_id: &str,
    ) -> Result<RuntimeSessionState, RuntimeServiceError> {
        let mut sessions = self.preview_state.sessions.write().await;
        if sessions
            .get(session_id)
            .is_some_and(|preview| preview.task_id != task_id)
        {
            return Err(RuntimeServiceError::NotFound);
        }
        sessions.remove(session_id);
        Ok(stopped_html_state(task_id, session_id))
    }

    async fn ensure_preview_server(&self) -> Result<SocketAddr, RuntimeServiceError> {
        let mut current = self.preview_server.lock().await;
        if let Some(server) = current.as_ref() {
            return Ok(server.address);
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let app = Router::new()
            .route("/preview/{session_id}/{token}", get(serve_html_preview))
            .with_state(self.preview_state.clone());
        let (shutdown, shutdown_rx) = oneshot::channel();
        let join = tokio::spawn(async move {
            let result = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
            if let Err(error) = result {
                tracing::warn!(%error, "HTML runtime preview server stopped");
            }
        });
        *current = Some(PreviewServer {
            address,
            join,
            shutdown,
        });
        Ok(address)
    }
}

fn validate_title(title: &str) -> Result<String, RuntimeServiceError> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > MAX_RUNTIME_TITLE_CHARS {
        return Err(RuntimeServiceError::InvalidInput(
            "runtime title must contain 1-256 characters".to_owned(),
        ));
    }
    Ok(title.to_owned())
}

fn stopped_html_state(task_id: TaskId, session_id: &str) -> RuntimeSessionState {
    RuntimeSessionState {
        session_id: session_id.to_owned(),
        task_id,
        kind: RuntimeSessionKind::Html,
        status: RuntimeSessionStatus::Stopped,
        title: "HTML".to_owned(),
        view: None,
        current_url: None,
        error: None,
    }
}

async fn serve_html_preview(
    Path((session_id, token)): Path<(String, String)>,
    State(state): State<PreviewState>,
) -> Response<Body> {
    let preview = state.sessions.read().await.get(&session_id).cloned();
    let Some(preview) = preview.filter(|value| value.token == token) else {
        return response(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            "Not found",
        );
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .header("content-security-policy", HTML_CONTENT_SECURITY_POLICY)
        .header("x-content-type-options", "nosniff")
        .header("referrer-policy", "no-referrer")
        .body(Body::from(Bytes::from_owner(preview.bytes)))
        .expect("static HTML preview response headers are valid")
}

fn response(status: StatusCode, content_type: &'static str, body: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-content-type-options", "nosniff")
        .body(Body::from(body))
        .expect("static runtime response headers are valid")
}
