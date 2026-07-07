use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args = supervisor_args();
    let scope = match args.runtime_root {
        Some(runtime_root) => match args.workspace_root {
            Some(workspace_root) => {
                jyowo_desktop_shell::agent_supervisor::AgentSupervisorScope::project(workspace_root)
            }
            None => {
                let conversation_id = args.conversation_id.expect(
                    "--conversation-id is required with --runtime-root and no --workspace-root",
                );
                jyowo_desktop_shell::agent_supervisor::AgentSupervisorScope::runtime_conversation(
                    runtime_root,
                    conversation_id,
                )
            }
        },
        None => {
            let workspace_root = args
                .workspace_root
                .or_else(|| std::env::var_os("JYOWO_WORKSPACE_ROOT").map(PathBuf::from))
                .unwrap_or_else(|| {
                    std::env::current_dir().expect("current dir should be available")
                });
            jyowo_desktop_shell::agent_supervisor::AgentSupervisorScope::project(workspace_root)
        }
    };
    if let Err(error) =
        jyowo_desktop_shell::agent_supervisor::run_supervisor_process_for_scope(scope).await
    {
        eprintln!("jyowo-agent-supervisor failed: {error}");
        std::process::exit(1);
    }
}

struct SupervisorArgs {
    runtime_root: Option<PathBuf>,
    workspace_root: Option<PathBuf>,
    conversation_id: Option<harness_contracts::SessionId>,
}

fn supervisor_args() -> SupervisorArgs {
    let mut args = std::env::args_os().skip(1);
    let mut parsed = SupervisorArgs {
        runtime_root: None,
        workspace_root: None,
        conversation_id: None,
    };
    while let Some(arg) = args.next() {
        if arg == "--runtime-root" {
            parsed.runtime_root = args.next().map(PathBuf::from);
            continue;
        }
        if arg == "--workspace-root" {
            parsed.workspace_root = args.next().map(PathBuf::from);
            continue;
        }
        if arg == "--conversation-id" {
            parsed.conversation_id = args.next().map(|value| {
                let value = value.to_string_lossy();
                harness_contracts::SessionId::parse(&value)
                    .expect("--conversation-id must be a valid session id")
            });
        }
    }
    parsed
}
