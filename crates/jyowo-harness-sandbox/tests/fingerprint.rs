use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use harness_sandbox::{
    ExecSpec, OutputOverflowPolicy, OutputPolicy, SandboxBaseConfig, StdioSpec, WorkspaceAccess,
};

fn base_config() -> SandboxBaseConfig {
    SandboxBaseConfig {
        passthrough_env_keys: BTreeSet::from(["LANG".to_owned(), "PATH".to_owned()]),
        ..SandboxBaseConfig::default()
    }
}

fn exec_spec() -> ExecSpec {
    ExecSpec {
        command: "cargo".to_owned(),
        args: vec!["test".to_owned()],
        env: BTreeMap::from([
            ("LANG".to_owned(), "C".to_owned()),
            ("PATH".to_owned(), "/bin".to_owned()),
            ("SECRET".to_owned(), "ignored".to_owned()),
        ]),
        cwd: Some(PathBuf::from("/workspace/./crates/../crates/jyowo")),
        workspace_access: WorkspaceAccess::ReadOnly,
        ..ExecSpec::default()
    }
}

#[test]
fn fingerprint_is_stable_for_env_order_and_lexical_cwd() {
    let mut reordered = exec_spec();
    reordered.env = BTreeMap::from([
        ("SECRET".to_owned(), "changed-but-ignored".to_owned()),
        ("PATH".to_owned(), "/bin".to_owned()),
        ("LANG".to_owned(), "C".to_owned()),
    ]);
    reordered.cwd = Some(PathBuf::from("/workspace/crates/jyowo"));

    assert_eq!(
        exec_spec().canonical_fingerprint(&base_config()),
        reordered.canonical_fingerprint(&base_config())
    );
}

#[test]
fn fingerprint_ignores_runtime_and_io_options() {
    let mut changed = exec_spec();
    changed.timeout = Some(Duration::from_secs(5));
    changed.activity_timeout = Some(Duration::from_secs(1));
    changed.stdin = StdioSpec::Null;
    changed.stdout = StdioSpec::File(PathBuf::from("out.log"));
    changed.output_policy = OutputPolicy {
        max_inline_bytes: 8,
        overflow: OutputOverflowPolicy::AbortExec,
        redact_secrets: true,
    };

    assert_eq!(
        exec_spec().canonical_fingerprint(&base_config()),
        changed.canonical_fingerprint(&base_config())
    );
}

#[test]
fn fingerprint_changes_when_workspace_access_changes() {
    let mut changed = exec_spec();
    changed.workspace_access = WorkspaceAccess::ReadWrite {
        allowed_writable_subpaths: vec![PathBuf::from("tmp")],
    };

    assert_ne!(
        exec_spec().canonical_fingerprint(&base_config()),
        changed.canonical_fingerprint(&base_config())
    );
}

#[test]
fn fingerprint_binds_secret_presence_without_hashing_its_value() {
    let mut first = exec_spec();
    first
        .env
        .insert("TOKEN".to_owned(), "first-secret".to_owned());
    first.authorized_env_keys.insert("TOKEN".to_owned());
    first.secret_env_keys.insert("TOKEN".to_owned());

    let mut changed_secret = first.clone();
    changed_secret
        .env
        .insert("TOKEN".to_owned(), "second-secret".to_owned());
    assert_eq!(
        first.canonical_fingerprint(&base_config()),
        changed_secret.canonical_fingerprint(&base_config())
    );

    let mut missing_secret = first.clone();
    missing_secret.env.remove("TOKEN");
    assert_ne!(
        first.canonical_fingerprint(&base_config()),
        missing_secret.canonical_fingerprint(&base_config())
    );
}

#[test]
fn fingerprint_still_binds_public_environment_values() {
    let mut first = exec_spec();
    first
        .env
        .insert("PUBLIC_CONFIG".to_owned(), "first".to_owned());
    first.authorized_env_keys.insert("PUBLIC_CONFIG".to_owned());

    let mut changed = first.clone();
    changed
        .env
        .insert("PUBLIC_CONFIG".to_owned(), "second".to_owned());

    assert_ne!(
        first.canonical_fingerprint(&base_config()),
        changed.canonical_fingerprint(&base_config())
    );
}
