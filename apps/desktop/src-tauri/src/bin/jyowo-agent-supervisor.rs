use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let workspace_root = workspace_root_arg()
        .or_else(|| std::env::var_os("JYOWO_WORKSPACE_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().expect("current dir should be available"));
    if let Err(error) =
        jyowo_desktop_shell::agent_supervisor::run_supervisor_process(workspace_root).await
    {
        eprintln!("jyowo-agent-supervisor failed: {error}");
        std::process::exit(1);
    }
}

fn workspace_root_arg() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--workspace-root" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}
