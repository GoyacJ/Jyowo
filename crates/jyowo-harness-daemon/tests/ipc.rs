use std::sync::Arc;

use harness_contracts::{
    now, AssistantDeltaProducedEvent, AssistantMessageCompletedEvent, ClientFrame, ClientId,
    ClientRequest, CommandId, CommandMetadata, CommandRejectionReason, ContinueTaskCommand,
    CreateTaskCommand, DaemonPermissionKind, DeltaChunk, EndReason, Event, HandshakeRequest,
    MessageContent, MessageId, NoopRedactor, PermissionOption, QueueItemId, RequestId,
    ResolvePermissionCommand, RunEndedEvent, RunId, RunSegmentId, RunState, RunTerminalReason,
    ServerMessage, SessionId, StopMode, StopReason, StopRunCommand, TaskId, TaskState, TenantId,
    ToolUseId, UsageSnapshot, WorkspaceMode, WorkspaceSelection, MAX_DAEMON_BLOB_BYTES,
    PROTOCOL_VERSION,
};
use harness_daemon::{
    encode_frame, IpcConnection, IpcServerConfig, JsonFrameDecoder, LocalIpcServer, MemoryService,
    PermissionRequestDraft, RecoveryService, RunCoordinatorFactory, RunningSegment,
    RuntimeConfigResolver, SkillReferenceCandidateService, StartSegmentRequest, Supervisor,
    SupervisorQuotas, MAX_FRAME_BYTES,
};
use harness_engine::{RunControlHandle, SafePointDecision, TurnOutcome};
use harness_journal::{
    AcceptedCommand, AppendMetadata, EventStore, NewTaskEvent, TaskBlobStore,
    TaskEventStoreAdapter, TaskStore,
};
use serde_json::json;
use std::sync::Mutex;

struct IdleRunFactory;

impl RunCoordinatorFactory for IdleRunFactory {
    fn spawn_idempotent(
        &self,
        _request: StartSegmentRequest,
        _workspace_tools: harness_daemon::WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn harness_subagent::SubagentRunner>,
        _agent_starters: harness_daemon::AgentStarterCapabilities,
    ) -> RunningSegment {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        std::mem::forget(sender);
        RunningSegment::new(receiver)
    }
}

#[derive(Default)]
struct ControlledRunFactory {
    starts: Mutex<Vec<StartSegmentRequest>>,
    controls: Mutex<Vec<(RunSegmentId, RunControlHandle)>>,
    senders: Mutex<Vec<tokio::sync::mpsc::UnboundedSender<harness_daemon::RunCoordinatorEvent>>>,
}

mod memory_cases {
    include!("ipc/memory_cases.rs");
}

impl ControlledRunFactory {
    fn control(&self, segment_id: RunSegmentId) -> RunControlHandle {
        self.controls
            .lock()
            .unwrap()
            .iter()
            .find(|(candidate, _)| *candidate == segment_id)
            .unwrap()
            .1
            .clone()
    }
}

impl RunCoordinatorFactory for ControlledRunFactory {
    fn spawn_idempotent(
        &self,
        request: StartSegmentRequest,
        _workspace_tools: harness_daemon::WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn harness_subagent::SubagentRunner>,
        _agent_starters: harness_daemon::AgentStarterCapabilities,
    ) -> RunningSegment {
        let control = RunControlHandle::new();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.starts.lock().unwrap().push(request.clone());
        self.controls
            .lock()
            .unwrap()
            .push((request.segment_id, control.clone()));
        self.senders.lock().unwrap().push(sender);
        RunningSegment::with_control(request.segment_id, receiver, control)
    }
}

fn config() -> IpcServerConfig {
    IpcServerConfig {
        daemon_version: "0.1.0".into(),
        user_instance_id: "user-a".into(),
        connection_token: "token-a".into(),
        event_batch_capacity: 2,
        blob_root: std::env::temp_dir().join("jyowo-daemon-ipc-unused-blobs"),
    }
}

fn frame(request_id: &str, request: ClientRequest) -> ClientFrame {
    ClientFrame {
        request_id: request_id.into(),
        protocol_version: PROTOCOL_VERSION,
        request,
    }
}

fn handshake(token: &str) -> ClientFrame {
    handshake_for_client(token, ClientId::new())
}

fn handshake_for_client(token: &str, client_id: ClientId) -> ClientFrame {
    frame(
        "handshake",
        ClientRequest::Handshake(HandshakeRequest {
            client_id,
            client_version: "0.1.0".into(),
            user_instance_id: "user-a".into(),
            connection_token: token.into(),
            last_acknowledged_offset: 0,
        }),
    )
}

fn create(request_id: &str, command_id: CommandId, key: &str) -> ClientFrame {
    std::fs::create_dir_all("/tmp/workspace").unwrap();
    frame(
        request_id,
        ClientRequest::CreateTask(CreateTaskCommand {
            metadata: CommandMetadata {
                command_id,
                idempotency_key: key.into(),
                expected_stream_version: 0,
            },
            title: "task".into(),
            workspace: WorkspaceSelection {
                mode: WorkspaceMode::Current,
                root: "/tmp/workspace".into(),
            },
        }),
    )
}

#[path = "ipc/ipc_cases.rs"]
mod ipc_cases;
#[path = "ipc/ipc_task_and_transport_cases.rs"]
mod ipc_task_and_transport_cases;
