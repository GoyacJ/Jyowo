use std::time::Duration;

use harness_sandbox::{
    execute_skill_script, SkillScriptPackFile, SkillScriptSandboxRequest, SkillScriptStatus,
};
use serde_json::json;

#[tokio::test]
async fn skill_script_runs_in_temp_workspace_and_captures_outputs_and_artifacts() {
    let result = execute_skill_script(SkillScriptSandboxRequest {
        script_path: "scripts/run.sh".to_owned(),
        input: json!({ "name": "jyowo" }),
        files: vec![
            SkillScriptPackFile {
                path: "scripts/run.sh".to_owned(),
                content: "printf 'hello '; cat \"$JYOWO_SKILL_INPUT\"; printf 'warn' >&2; printf artifact > output.txt\n".to_owned(),
            },
            SkillScriptPackFile {
                path: "README.md".to_owned(),
                content: "mounted".to_owned(),
            },
        ],
        timeout: Duration::from_secs(2),
        max_stdout_bytes: 256,
        max_stderr_bytes: 256,
        network_allowed: false,
        memory_limit_mb: Some(64),
    })
    .await
    .expect("script should execute");

    assert_eq!(result.status, SkillScriptStatus::Succeeded);
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.contains("hello"));
    assert!(result.stdout.contains("\"name\":\"jyowo\""));
    assert_eq!(result.stderr, "warn");
    assert!(!result.network_enabled);
    assert_eq!(result.memory_limit_mb, Some(64));
    assert!(result.workspace_path.starts_with(std::env::temp_dir()));
    assert!(result
        .artifacts
        .iter()
        .any(|artifact| artifact.path == "output.txt" && artifact.content == "artifact"));
    assert!(!result
        .mounted_files
        .iter()
        .any(|path| path == "../secret.txt" || path.starts_with('/')));
}

#[tokio::test]
async fn skill_script_enforces_timeout_and_output_budget() {
    let timed_out = execute_skill_script(SkillScriptSandboxRequest {
        script_path: "run.sh".to_owned(),
        input: json!({}),
        files: vec![SkillScriptPackFile {
            path: "run.sh".to_owned(),
            content: "sleep 2\n".to_owned(),
        }],
        timeout: Duration::from_millis(50),
        max_stdout_bytes: 64,
        max_stderr_bytes: 64,
        network_allowed: false,
        memory_limit_mb: Some(32),
    })
    .await
    .expect("timeout should be reported as a script result");
    assert_eq!(timed_out.status, SkillScriptStatus::TimedOut);
    assert_eq!(timed_out.exit_code, None);

    let limited = execute_skill_script(SkillScriptSandboxRequest {
        script_path: "run.sh".to_owned(),
        input: json!({}),
        files: vec![SkillScriptPackFile {
            path: "run.sh".to_owned(),
            content: "printf abcdef\n".to_owned(),
        }],
        timeout: Duration::from_secs(2),
        max_stdout_bytes: 3,
        max_stderr_bytes: 64,
        network_allowed: false,
        memory_limit_mb: Some(32),
    })
    .await
    .expect("output budget should be reported as a script result");
    assert_eq!(limited.status, SkillScriptStatus::OutputLimitExceeded);
    assert_eq!(limited.stdout, "abc");
}
